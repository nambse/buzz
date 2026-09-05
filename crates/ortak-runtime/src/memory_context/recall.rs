//! Bounded external recall; no database transaction crosses this module.

use std::time::Duration;

use ortak_control::adapter::truncate_at_char_boundary;
use ortak_control::memory::{
    MemoryAdapter, MemoryBudget, MemoryRecall, MemoryRecallRequest, MemoryScope,
};
use ortak_control::run_event::{strip_control_characters, RedactionPolicy};
use uuid::Uuid;

use super::{FrozenRunSnapshot, MAX_CONTEXT_BYTES, MAX_CONTEXT_RECORDS};
use crate::authority::{DispatchAuthority, DispatchRefusal};

/// Memory provider used by dispatch. Implementations may not broaden run scope.
#[allow(async_fn_in_trait)]
pub trait RunMemory {
    /// Checks the exact binding's current health without writing memory.
    async fn check(&self, authority: &DispatchAuthority) -> Result<(), DispatchRefusal>;
    /// Recalls only this run's scratch scope and returns a validated snapshot.
    async fn snapshot(
        &self,
        authority: &DispatchAuthority,
        run_id: Uuid,
        redaction: &RedactionPolicy,
    ) -> Result<FrozenRunSnapshot, DispatchRefusal>;
}

/// Default provider: memory-free employees work; required memory fails closed.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoRunMemory;

impl RunMemory for NoRunMemory {
    async fn check(&self, authority: &DispatchAuthority) -> Result<(), DispatchRefusal> {
        authority.require_validated_memory()?;
        if authority.memory_binding().is_some() {
            return Err(DispatchRefusal::MemoryAdapterUnavailable);
        }
        Ok(())
    }

    async fn snapshot(
        &self,
        authority: &DispatchAuthority,
        run_id: Uuid,
        _: &RedactionPolicy,
    ) -> Result<FrozenRunSnapshot, DispatchRefusal> {
        self.check(authority).await?;
        FrozenRunSnapshot::from_recall(authority, run_id, MemoryRecall::default())
            .map_err(|_| DispatchRefusal::MemoryContextRejected)
    }
}

/// Borrowed adapter keeps runtime start and post-run writes on one validated
/// deployment instance without requiring a clone of secret configuration.
pub struct AdapterRunMemory<'a, M> {
    pub(super) adapter: &'a M,
}

impl<M> std::fmt::Debug for AdapterRunMemory<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AdapterRunMemory(..)")
    }
}

impl<M> Clone for AdapterRunMemory<'_, M> {
    fn clone(&self) -> Self {
        Self {
            adapter: self.adapter,
        }
    }
}

impl<'a, M: MemoryAdapter> AdapterRunMemory<'a, M> {
    pub(crate) fn new(adapter: &'a M) -> Self {
        Self { adapter }
    }
}

impl<M: MemoryAdapter> RunMemory for AdapterRunMemory<'_, M> {
    async fn check(&self, authority: &DispatchAuthority) -> Result<(), DispatchRefusal> {
        authority.require_validated_memory()?;
        let Some(binding) = authority.memory_binding() else {
            return Ok(());
        };
        if binding.adapter != self.adapter.adapter_name() {
            return Err(DispatchRefusal::MemoryAdapterUnavailable);
        }
        let health = tokio::time::timeout(Duration::from_secs(10), self.adapter.health(binding))
            .await
            .map_err(|_| DispatchRefusal::MemoryUnavailable)?
            .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
        if !health.is_healthy() {
            return Err(DispatchRefusal::MemoryUnavailable);
        }
        Ok(())
    }

    async fn snapshot(
        &self,
        authority: &DispatchAuthority,
        run_id: Uuid,
        redaction: &RedactionPolicy,
    ) -> Result<FrozenRunSnapshot, DispatchRefusal> {
        let Some(binding) = authority.memory_binding() else {
            return NoRunMemory.snapshot(authority, run_id, redaction).await;
        };
        let request = MemoryRecallRequest {
            employee_id: authority.employee_id().clone(),
            binding: binding.clone(),
            scope: MemoryScope::RunScratch { run_id },
            query: truncate_at_char_boundary(&authority.input().body, 4096).to_owned(),
            budget: MemoryBudget {
                max_records: MAX_CONTEXT_RECORDS,
                max_bytes: MAX_CONTEXT_BYTES,
            },
        };
        let mut recall =
            tokio::time::timeout(Duration::from_secs(10), self.adapter.recall(&request))
                .await
                .map_err(|_| DispatchRefusal::MemoryUnavailable)?
                .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
        // Validate before redaction/serialization to reject oversized or foreign
        // responses before copying them. Redaction cannot turn a scope violation
        // into an acceptable record.
        super::validate_recall(authority, run_id, &recall)
            .map_err(|_| DispatchRefusal::MemoryContextRejected)?;
        for record in &mut recall.records {
            record.content = redaction.redact(&strip_control_characters(&record.content));
            record.provenance.source = redaction.redact(&record.provenance.source);
            if redaction.redact(&record.record_ref) != record.record_ref {
                return Err(DispatchRefusal::MemoryContextRejected);
            }
        }
        FrozenRunSnapshot::from_recall(authority, run_id, recall)
            .map_err(|_| DispatchRefusal::MemoryContextRejected)
    }
}
