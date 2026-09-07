use chrono::{DateTime, Utc};
use ortak_domain::{EmployeeId, MemoryBinding};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Server-authorized project audience; configured binding/project allowlists are rechecked.
#[derive(Clone, Debug)]
pub struct ReviewedProjectScope {
    /// Stable employee identity.
    pub employee_id: EmployeeId,
    /// Full selected memory binding.
    pub binding: MemoryBinding,
    /// Exact project approved by the caller's current-authority service.
    pub project_id: Uuid,
}

/// One explicitly reviewed publication, with a caller-persisted operation key.
#[derive(Clone)]
pub struct ReviewedProjectPublication {
    /// Stable caller-owned record identity, unchanged across retries or withdrawal.
    pub record_id: Uuid,
    /// Durable operation key; never generate another key to retry uncertain I/O.
    pub idempotency_key: String,
    /// Human-edited text, at most 4 KiB. Not included in Debug output.
    pub content: String,
    /// Hash of the canonical reviewed evidence.
    pub source_hash: String,
    /// Durable Ortak approval identity.
    pub approval_id: Uuid,
    /// Approving human's lowercase hex public key.
    pub approved_by: String,
    /// Immutable permitted-use expiry, at most 90 days on first publication.
    pub expires_at: DateTime<Utc>,
}

/// Explicit text-removal operation; this never deletes native resources or legacy memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewedProjectRemoval {
    /// Human withdrawal may precede a delayed publication.
    Withdraw,
    /// Remove expired text only after the selected database clock reaches expiry.
    Expire,
}

/// Current reviewed record eligibility.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewedProjectStatus {
    /// Current unexpired text is available to the authorized caller.
    Active,
    /// Use expired; physical text removal is separately identified.
    Expired,
    /// Irreversible human withdrawal.
    Withdrawn,
}

/// Retained approval attribution contains no source text.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedProjectProvenance {
    /// Durable Ortak approval identity.
    pub approval_id: Uuid,
    /// Approving human public key.
    pub approved_by: String,
    /// Hash of canonical source evidence.
    pub source_hash: String,
    /// Selected store's immutable publication time.
    pub created_at: DateTime<Utc>,
}

/// Exact `reviewed-project/1` record, distinct from native Honcho messages.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedProjectRecord {
    /// Selected extension wire protocol.
    pub protocol: String,
    /// Separate record family, never inferred from a native session.
    pub record_family: String,
    /// Frozen native workspace public name.
    pub workspace_id: String,
    /// Exact project boundary.
    pub project_id: Uuid,
    /// Stable reviewed record identity.
    pub record_id: Uuid,
    /// Server-derived company.
    pub company_id: Uuid,
    /// Stable employee identity.
    pub employee_id: EmployeeId,
    /// Original resource receipt/native identity fingerprint.
    pub binding_hash: String,
    /// Current eligibility state.
    pub status: ReviewedProjectStatus,
    /// Present only on an authorized read of active text; omitted from acknowledgements.
    pub content: Option<String>,
    /// Immutable reviewed content hash; absent for withdrawal before publication.
    pub content_hash: Option<String>,
    /// Immutable expiry, absent before any publication.
    pub expires_at: Option<DateTime<Utc>>,
    /// Approval provenance, absent before any publication.
    pub provenance: Option<ReviewedProjectProvenance>,
    /// Proves absence only from the referenced extension's current text store.
    pub erased_from_reviewed_store: bool,
    /// Retained tombstone time. Does not claim backup or source-evidence erasure.
    pub tombstone_at: Option<DateTime<Utc>>,
}

/// Durable operation acknowledgement; scope/hash/identity are validated before return.
pub struct ReviewedProjectReceipt {
    /// Current record projection contains no text.
    pub record: ReviewedProjectRecord,
    /// Canonical hash of the exact operation request.
    pub request_hash: String,
    /// Whether the publication first committed on this request.
    pub created: bool,
}

/// Finite retained-record inspection page.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedProjectPage {
    /// At most 25 records in stable UUID order.
    pub records: Vec<ReviewedProjectRecord>,
    /// Last returned UUID when more records remain.
    pub next_after: Option<Uuid>,
}

/// Bounded active context, separate from automatic runtime admission.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedProjectRecall {
    /// At most eight active records and 8 KiB of text.
    pub records: Vec<ReviewedProjectRecord>,
    /// Additional matches exceeded the finite result budget.
    pub truncated: bool,
}
