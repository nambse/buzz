use super::*;
use ortak_domain::{EmployeeId, MemoryBinding};
use serde::{Deserialize, Serialize};

/// The two immutable remote operations; withdrawal also handles expiry safely.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewedExportAction {
    /// Publish exactly the approved record.
    Publish,
    /// Tombstone and remove only the exported record's text.
    Withdraw,
}
impl ReviewedExportAction {
    /// Stable database/wire action.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Withdraw => "withdraw",
        }
    }
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "publish" => Ok(Self::Publish),
            "withdraw" => Ok(Self::Withdraw),
            _ => Err(invalid()),
        }
    }
}
/// A bounded advertisement produced only after the worker's explicit actual I/O validation.
/// Possessing this public metadata alone does not authorize a network operation.
#[derive(Clone)]
pub struct ReviewedMemoryTarget {
    /// Exact explicitly configured project.
    pub project_id: Uuid,
    /// Stable employee identity.
    pub employee_id: EmployeeId,
    /// Original service deployment.
    pub deployment_id: Uuid,
    /// Immutable adapter binding.
    pub binding: MemoryBinding,
    /// Original full, non-secret creation receipt; parsed/verified by the owning adapter.
    pub creation_receipt: Value,
    /// Remaining actual validation lifetime, capped at sixty seconds by persistence.
    pub valid_for: Duration,
    /// Separate operator opt-in; publication alone never enables runtime use.
    pub runtime_consumption_enabled: bool,
}
/// Explicit conversation runtime opt-in accompanying an owned project target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewedConversationTarget {
    /// Existing project advertised in the same complete worker recipe.
    pub project_id: Uuid,
    /// Stable employee whose owned target receives the opt-in.
    pub employee_id: EmployeeId,
    /// Exact currently bound stream channel; never inferred from a model.
    pub channel_id: Uuid,
}
/// Current export state without native identifiers, credentials or unapproved text.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReviewedExportView {
    /// Exact fact; one immutable export exists at most once.
    pub fact_id: Uuid,
    /// Publication is pending, acknowledged or failed.
    pub publication: ReviewedExportJobView,
    /// Scheduled or requested cleanup uses the same job and key.
    pub cleanup: ReviewedExportJobView,
    /// Only an actual validated withdrawal acknowledgement establishes this.
    pub erased_from_reviewed_store: bool,
    /// Current explicit runtime opt-in and approval/source/binding eligibility.
    pub runtime_consumption_enabled: bool,
}
/// One bounded job, including an explicit audited retry affordance.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReviewedExportJobView {
    /// pending, acknowledged or failed; pending cleanup may be scheduled for expiry.
    pub state: String,
    /// Stable retry generation; clients send this to reopen exactly one failure.
    pub retry_version: i32,
    /// Attempts in the current bounded retry cycle.
    pub attempt_count: i32,
    /// Next scheduled attempt, including the immutable expiry cleanup deadline.
    pub next_attempt_at: DateTime<Utc>,
    /// Closed error classification, never provider output.
    pub error_code: Option<String>,
}
/// One database-issued exclusive lease. Fields do not authorize a remote request.
#[derive(Clone, Debug)]
pub struct ReviewedExportLease {
    /// Stable fact/export identity.
    pub fact_id: Uuid,
    /// Exact action.
    pub action: ReviewedExportAction,
    /// Current exclusive database token.
    pub token: Uuid,
    /// Monotonic lifetime attempt identity.
    pub total_attempts: i32,
}
/// Canonically prepared exact request. Text is deliberately absent from Debug.
pub struct PreparedReviewedExport {
    /// Company fixed by the server scope.
    pub company_id: Uuid,
    /// Exact project.
    pub project_id: Uuid,
    /// Original employee.
    pub employee_id: EmployeeId,
    /// Original deployment.
    pub deployment_id: Uuid,
    /// Complete original binding.
    pub binding: MemoryBinding,
    /// Original creation evidence, never a new adoption authority.
    pub creation_receipt: Value,
    /// Database-issued stable job.
    pub lease: ReviewedExportLease,
    /// Exact immutable remote operation key.
    pub idempotency_key: String,
    /// Exact expected canonical request digest.
    pub request_hash: Vec<u8>,
    /// Approved text only for publication, absent for cleanup.
    pub content: Option<String>,
    /// Immutable source attribution hash.
    pub source_hash: String,
    /// Original approving human.
    pub approved_by: String,
    /// Original approval operation.
    pub approval_id: Uuid,
    /// Immutable expiry.
    pub expires_at: DateTime<Utc>,
}
/// Closed hash-only result already validated by the owning adapter.
pub struct ReviewedExportAcknowledgement {
    /// Exact request digest.
    pub request_hash: Vec<u8>,
    /// Exact original receipt/native identity digest.
    pub binding_hash: Vec<u8>,
    /// Absent only for removal before any publication.
    pub content_hash: Option<Vec<u8>>,
    /// Closed remote active/expired/withdrawn state.
    pub remote_status: String,
    /// Narrow proof for the exact reviewed-store text row only.
    pub erased_from_reviewed_store: bool,
    /// Retained native tombstone timestamp.
    pub tombstone_at: Option<DateTime<Utc>>,
}
