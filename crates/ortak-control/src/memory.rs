//! Memory adapter port (Architecture v0 §4.5), Honcho-oriented.
//!
//! A [`MemoryBinding`] names an endpoint by opaque reference plus a
//! workspace and two peers. The port covers the minimum read/write boundary
//! (recall and remember), resource create-or-adopt, and per-resource health.
//! Inspect, forget, and retention arrive after the deployed API semantics are
//! verified (Implementation Plan Milestone 5).

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use ortak_domain::{EmployeeId, MemoryBinding, ProvisioningMode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::adapter::{Detail, HealthReport, ResourceOutcome};

/// Memory operations an adapter may support.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCapability {
    /// Report workspace and peer health.
    HealthProbe,
    /// Inspect existing workspaces and peers without modifying them.
    ResourceInspect,
    /// Create workspaces and peers.
    ResourceCreate,
    /// Delete resources Ortak created.
    ResourceDelete,
    /// Bounded recall.
    Recall,
    /// Provenance-tagged writes.
    Remember,
}

/// Capabilities every activated memory binding must support.
pub const ACTIVATION_REQUIRED_MEMORY_CAPABILITIES: [MemoryCapability; 4] = [
    MemoryCapability::HealthProbe,
    MemoryCapability::ResourceInspect,
    MemoryCapability::Recall,
    MemoryCapability::Remember,
];

/// Probed capability set for one memory adapter deployment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryCapabilities {
    /// Adapter name, e.g. `honcho`.
    pub adapter: String,
    /// Adapter-versioned API shape that was probed.
    pub api_version: String,
    /// Supported operations.
    pub capabilities: BTreeSet<MemoryCapability>,
}

impl MemoryCapabilities {
    /// Required capabilities that are missing, in stable order.
    pub fn missing(&self, required: &[MemoryCapability]) -> Vec<MemoryCapability> {
        required
            .iter()
            .copied()
            .filter(|capability| !self.capabilities.contains(capability))
            .collect()
    }
}

/// Health of each resource a binding depends on.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryHealthReport {
    /// Workspace/namespace reachability.
    pub workspace: HealthReport,
    /// Human operator peer identity.
    pub user_peer: HealthReport,
    /// Employee peer identity.
    pub employee_peer: HealthReport,
}

impl MemoryHealthReport {
    /// True only when every resource is healthy.
    pub fn is_healthy(&self) -> bool {
        self.workspace.is_healthy()
            && self.user_peer.is_healthy()
            && self.employee_peer.is_healthy()
    }
}

/// Create-or-adopt request for an employee's memory resources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryResourceRequest {
    /// Employee whose memory is being bound.
    pub employee_id: EmployeeId,
    /// Create new resources or adopt existing ones.
    pub mode: ProvisioningMode,
    /// Secret-free binding.
    pub binding: MemoryBinding,
    /// Step idempotency key.
    pub idempotency_key: String,
}

/// Outcome per resource; each may be independently created or adopted.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryResourceOutcome {
    /// Workspace outcome.
    pub workspace: ResourceOutcome,
    /// User peer outcome.
    pub user_peer: ResourceOutcome,
    /// Employee peer outcome.
    pub employee_peer: ResourceOutcome,
}

impl MemoryResourceOutcome {
    /// Every outcome in stable order.
    pub fn outcomes(&self) -> [&ResourceOutcome; 3] {
        [&self.workspace, &self.user_peer, &self.employee_peer]
    }

    /// True when at least one resource was adopted.
    pub fn any_adopted(&self) -> bool {
        self.outcomes()
            .iter()
            .any(|outcome| outcome.ownership.is_adopted())
    }
}

/// Explicit memory namespace (Architecture v0 §4.5).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "scope")]
pub enum MemoryScope {
    /// Reviewed company truth.
    CompanyTruth,
    /// Project context.
    ProjectContext {
        /// Project boundary.
        project_id: Uuid,
    },
    /// Employee experiential memory.
    EmployeeExperience,
    /// Human/employee relationship memory.
    Relationship,
    /// Run/session scratch context.
    RunScratch {
        /// Owning run.
        run_id: Uuid,
    },
}

/// Bounds for one recall.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryBudget {
    /// Maximum records returned.
    pub max_records: usize,
    /// Maximum total content bytes returned.
    pub max_bytes: usize,
}

impl Default for MemoryBudget {
    fn default() -> Self {
        Self {
            max_records: 32,
            max_bytes: 16 * 1024,
        }
    }
}

/// Hard ceilings a recall budget may not exceed.
pub const HARD_MAX_RECALL_RECORDS: usize = 256;
/// Hard ceiling for recall content bytes.
pub const HARD_MAX_RECALL_BYTES: usize = 128 * 1024;

/// Bounded recall request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryRecallRequest {
    /// Employee recalling.
    pub employee_id: EmployeeId,
    /// Binding to recall through.
    pub binding: MemoryBinding,
    /// Namespace.
    pub scope: MemoryScope,
    /// Bounded query text.
    pub query: String,
    /// Budget.
    pub budget: MemoryBudget,
}

/// Where a recalled or written record came from.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryProvenance {
    /// Employee that produced the record.
    pub employee_id: EmployeeId,
    /// Run that produced it, if any.
    pub run_id: Option<Uuid>,
    /// Stable source label, e.g. `office_message`, `tool_result`, `review`.
    pub source: String,
    /// When it was recorded.
    pub recorded_at: DateTime<Utc>,
}

/// One recalled record with provenance and an adapter-side reference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryRecord {
    /// Adapter-side record reference.
    pub record_ref: String,
    /// Namespace.
    pub scope: MemoryScope,
    /// Bounded content.
    pub content: String,
    /// Provenance.
    pub provenance: MemoryProvenance,
}

/// Recall result with truncation flag.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryRecall {
    /// Records within budget.
    pub records: Vec<MemoryRecord>,
    /// True when the budget cut the result.
    pub truncated: bool,
}

/// One fact to remember.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryFact {
    /// Bounded content.
    pub content: String,
    /// Provenance.
    pub provenance: MemoryProvenance,
}

/// Provenance-tagged write request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryWriteRequest {
    /// Employee writing.
    pub employee_id: EmployeeId,
    /// Binding to write through.
    pub binding: MemoryBinding,
    /// Namespace.
    pub scope: MemoryScope,
    /// Facts.
    pub facts: Vec<MemoryFact>,
    /// Idempotency key; a retried write must not duplicate records.
    pub idempotency_key: String,
}

/// Ceiling for facts per write.
pub const MAX_FACTS_PER_WRITE: usize = 64;
/// Ceiling for one fact or query in bytes.
pub const MAX_MEMORY_TEXT_BYTES: usize = 16 * 1024;

/// Adapter receipt for a write.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryWriteReceipt {
    /// Adapter-side receipt reference.
    pub receipt_ref: String,
    /// Records written.
    pub written: usize,
}

/// Memory adapter failures. Details are bounded and never carry secrets.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MemoryError {
    /// The deployed service lacks a required operation.
    #[error("memory adapter does not support {capability:?}")]
    Unsupported {
        /// Missing capability.
        capability: MemoryCapability,
    },
    /// The service could not be reached.
    #[error("memory service unavailable: {detail}")]
    Unavailable {
        /// Bounded detail.
        detail: Detail,
    },
    /// Adopt mode named a resource that does not exist.
    #[error("memory resource not found: {resource_ref}")]
    ResourceNotFound {
        /// Missing resource reference.
        resource_ref: String,
    },
    /// Create mode would overwrite an existing resource.
    #[error("memory resource already exists: {resource_ref}")]
    ResourceExists {
        /// Conflicting resource reference.
        resource_ref: String,
    },
    /// The request violates a local bound.
    #[error("invalid memory request: {detail}")]
    InvalidRequest {
        /// Bounded detail.
        detail: Detail,
    },
    /// The service rejected the request.
    #[error("memory service rejected the request: {detail}")]
    Rejected {
        /// Bounded detail.
        detail: Detail,
    },
}

impl MemoryError {
    /// True when a retry may succeed without operator action.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
}

impl MemoryRecallRequest {
    /// Validates bounds before any adapter call.
    pub fn validate(&self) -> Result<(), MemoryError> {
        if self.query.trim().is_empty() || self.query.len() > MAX_MEMORY_TEXT_BYTES {
            return Err(MemoryError::InvalidRequest {
                detail: Detail::new("recall query is empty or above the ceiling"),
            });
        }
        if self.budget.max_records == 0
            || self.budget.max_records > HARD_MAX_RECALL_RECORDS
            || self.budget.max_bytes == 0
            || self.budget.max_bytes > HARD_MAX_RECALL_BYTES
        {
            return Err(MemoryError::InvalidRequest {
                detail: Detail::new("recall budget is outside the hard ceilings"),
            });
        }
        Ok(())
    }
}

impl MemoryWriteRequest {
    /// Validates bounds before any adapter call.
    pub fn validate(&self) -> Result<(), MemoryError> {
        if self.facts.is_empty() || self.facts.len() > MAX_FACTS_PER_WRITE {
            return Err(MemoryError::InvalidRequest {
                detail: Detail::new("write must contain 1..=64 facts"),
            });
        }
        if self.facts.iter().any(|fact| {
            fact.content.trim().is_empty() || fact.content.len() > MAX_MEMORY_TEXT_BYTES
        }) {
            return Err(MemoryError::InvalidRequest {
                detail: Detail::new("fact content is empty or above the ceiling"),
            });
        }
        if self.idempotency_key.trim().is_empty() {
            return Err(MemoryError::InvalidRequest {
                detail: Detail::new("write idempotency key is empty"),
            });
        }
        Ok(())
    }
}

/// Memory port. Same create/adopt contract as the runtime port: adopt never
/// modifies or recreates an existing workspace or peer, and create never
/// overwrites one.
#[allow(async_fn_in_trait)]
pub trait MemoryAdapter {
    /// Stable adapter name stored in bindings.
    fn adapter_name(&self) -> &str;

    /// Probes the deployed service through the binding's endpoint reference.
    async fn probe_capabilities(
        &self,
        binding: &MemoryBinding,
    ) -> Result<MemoryCapabilities, MemoryError>;

    /// Reports workspace, user-peer, and employee-peer health.
    async fn health(&self, binding: &MemoryBinding) -> Result<MemoryHealthReport, MemoryError>;

    /// Creates or adopts the workspace and both peers.
    async fn ensure_resources(
        &self,
        request: &MemoryResourceRequest,
    ) -> Result<MemoryResourceOutcome, MemoryError>;

    /// Deletes one resource this operation created; never called for adopted
    /// resources.
    async fn delete_created_resource(
        &self,
        resource_ref: &str,
        idempotency_key: &str,
    ) -> Result<(), MemoryError>;

    /// Bounded recall with provenance.
    async fn recall(&self, request: &MemoryRecallRequest) -> Result<MemoryRecall, MemoryError>;

    /// Provenance-tagged write returning an adapter receipt.
    async fn remember(
        &self,
        request: &MemoryWriteRequest,
    ) -> Result<MemoryWriteReceipt, MemoryError>;
}
