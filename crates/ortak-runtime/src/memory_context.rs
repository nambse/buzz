//! Immutable pre-start context; persisted input is never concurrency authority.

mod recall;
pub use recall::{AdapterRunMemory, NoRunMemory, RunMemory};

use ortak_control::adapter::Detail;
use ortak_control::memory::{MemoryRecall, MemoryScope};
use ortak_control::outbox::OutboxLease;
use ortak_control::runtime::{RunSpec, RuntimeError};
use ortak_control::CompanyScope;
use ortak_domain::MemoryBinding;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::authority::{DispatchAuthority, DispatchRefusal};
use crate::Result;

/// Hard ceiling on an encoded specification and its provenance.
pub const MAX_SNAPSHOT_BYTES: usize = 256 * 1024;
/// Recall record ceiling for the first, run-scratch-only slice.
pub const MAX_CONTEXT_RECORDS: usize = 8;
/// Aggregate recalled content ceiling.
pub const MAX_CONTEXT_BYTES: usize = 16 * 1024;

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SnapshotWire {
    version: u8,
    company_id: Uuid,
    routing_decision_id: Uuid,
    message_id: String,
    root_message_id: String,
    event_kind: i32,
    input_truncated: bool,
    memory_binding: Option<MemoryBinding>,
    recall: MemoryRecall,
    spec: RunSpec,
}

/// Bounded, immutable full RunSpec and provenance. Decoding retains the exact
/// original bytes. Lease tokens and Office generations are intentionally absent.
#[derive(Clone, Eq, PartialEq)]
pub struct FrozenRunSnapshot {
    wire: SnapshotWire,
    bytes: Vec<u8>,
}

impl std::fmt::Debug for FrozenRunSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrozenRunSnapshot")
            .field("run_id", &self.wire.spec.run_id)
            .finish_non_exhaustive()
    }
}

fn rejected() -> crate::RunSupervisionError {
    RuntimeError::InvalidSpec {
        detail: Detail::new("run memory snapshot rejected"),
    }
    .into()
}

impl FrozenRunSnapshot {
    pub(crate) fn from_recall(
        authority: &DispatchAuthority,
        run_id: Uuid,
        recall: MemoryRecall,
    ) -> Result<Self> {
        validate_recall(authority, run_id, &recall)?;
        let mut spec = authority.run_spec(run_id)?;
        spec.context.memory_context = rendered(&recall)?;
        let wire = SnapshotWire {
            version: 1,
            company_id: authority.company_id(),
            routing_decision_id: authority.routing_decision_id(),
            message_id: authority.message_id().to_hex(),
            root_message_id: authority.root_message_id().to_hex(),
            event_kind: authority.input().event_kind,
            input_truncated: authority.input().truncated,
            memory_binding: authority.memory_binding().cloned(),
            recall,
            spec,
        };
        let bytes = serde_json::to_vec(&wire).map_err(|_| rejected())?;
        Self::decode(&bytes, authority, run_id)
    }

    /// Loads bounded bytes and validates all source/configuration pins. The DB
    /// repository separately verifies the stored SHA-256 digest.
    pub fn decode(bytes: &[u8], authority: &DispatchAuthority, run_id: Uuid) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(rejected());
        }
        let wire = serde_json::from_slice(bytes).map_err(|_| rejected())?;
        let value = Self {
            wire,
            bytes: bytes.to_vec(),
        };
        value.validate_for(authority, run_id)?;
        Ok(value)
    }

    /// Returns an unchanged copy of the original persisted bytes.
    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(self.bytes.clone())
    }

    /// Full specification to reuse on every external start attempt.
    pub fn spec(&self) -> &RunSpec {
        &self.wire.spec
    }

    /// Verifies fresh canonical pins and same-run provenance without trusting an
    /// old admission witness. Transactional authority must still be re-derived.
    pub fn validate_for(&self, authority: &DispatchAuthority, run_id: Uuid) -> Result<()> {
        authority
            .require_validated_memory()
            .map_err(|_| rejected())?;
        let wire = &self.wire;
        if run_id.is_nil()
            || wire.version != 1
            || wire.company_id != authority.company_id()
            || wire.routing_decision_id != authority.routing_decision_id()
            || wire.message_id != authority.message_id().to_hex()
            || wire.root_message_id != authority.root_message_id().to_hex()
            || wire.event_kind != authority.input().event_kind
            || wire.input_truncated != authority.input().truncated
            || wire.memory_binding.as_ref() != authority.memory_binding()
            || wire.recall.records.len() > MAX_CONTEXT_RECORDS
            || (authority.memory_binding().is_none()
                && (!wire.recall.records.is_empty() || wire.recall.truncated))
        {
            return Err(rejected());
        }
        validate_recall(authority, run_id, &wire.recall)?;
        let mut expected = authority.run_spec(run_id)?;
        expected.context.memory_context = rendered(&wire.recall)?;
        if expected != wire.spec || wire.spec.validate().is_err() {
            return Err(rejected());
        }
        Ok(())
    }
}

pub(super) fn validate_recall(
    authority: &DispatchAuthority,
    run_id: Uuid,
    recall: &MemoryRecall,
) -> Result<()> {
    if recall.records.len() > MAX_CONTEXT_RECORDS {
        return Err(rejected());
    }
    let mut refs = std::collections::BTreeSet::new();
    let mut total = 0usize;
    for record in &recall.records {
        let provenance = &record.provenance;
        if record.scope != (MemoryScope::RunScratch { run_id })
            || &provenance.employee_id != authority.employee_id()
            || provenance.run_id != Some(run_id)
            || record.record_ref.is_empty()
            || record.record_ref.len() > 256
            || record.record_ref.chars().any(char::is_control)
            || !refs.insert(&record.record_ref)
            || provenance.source.is_empty()
            || provenance.source.len() > 128
            || provenance.source.chars().any(char::is_control)
            || record.content.trim().is_empty()
            || record.content.len() > 4096
        {
            return Err(rejected());
        }
        total = total
            .checked_add(record.content.len())
            .ok_or_else(rejected)?;
        if total > MAX_CONTEXT_BYTES {
            return Err(rejected());
        }
    }
    Ok(())
}

fn rendered(recall: &MemoryRecall) -> Result<Vec<String>> {
    if recall.records.len() > MAX_CONTEXT_RECORDS {
        return Err(rejected());
    }
    recall
        .records
        .iter()
        .map(|record| {
            if record.content.len() > 4096
                || record.record_ref.len() > 256
                || record.provenance.source.len() > 128
            {
                return Err(rejected());
            }
            serde_json::to_string(&serde_json::json!({
                "type": "run_scratch_memory", "trust": "untrusted_data", "record": record
            }))
            .map_err(|_| rejected())
        })
        .collect()
}

/// Result of immutable persistence and final fenced admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FreezeSnapshotOutcome {
    /// Durable winner after fresh authority verification.
    Ready(Box<FrozenRunSnapshot>),
    /// Current durable facts prohibit starting.
    Refused(DispatchRefusal),
    /// Expired or replaced lease; nothing committed under it.
    StaleLease,
}

/// Durable snapshot boundary. No external recall occurs inside these methods.
#[allow(async_fn_in_trait)]
pub trait RunContextRepository: Send + Sync {
    /// Load only this company/run, verifying stored digest and source pins.
    async fn load_run_snapshot(
        &self,
        scope: &CompanyScope,
        authority: &DispatchAuthority,
        run_id: Uuid,
    ) -> Result<Option<FrozenRunSnapshot>>;

    /// Acquire shared authority before row locks; rederive current authority,
    /// validate candidate and existing bytes, check run/lease/cancellation, insert
    /// once, and renew admission atomically. Always return the durable winner.
    async fn freeze_run_snapshot(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        authority: &DispatchAuthority,
        run_id: Uuid,
        candidate: &FrozenRunSnapshot,
    ) -> Result<FreezeSnapshotOutcome>;
}

#[cfg(test)]
mod tests;
