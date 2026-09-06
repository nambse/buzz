use super::*;
use ortak_control::memory::employee::EmployeeMemoryProvenanceV1;
use ortak_domain::EmployeeId;

/// Two irreversible stable remote operations; expiry and Stop share withdrawal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmployeeExportAction {
    /// Publish explicitly approved immutable edited text once.
    Publish,
    /// Irreversibly remove that original record's remote text.
    Withdraw,
}
impl EmployeeExportAction {
    /// Closed persisted spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Withdraw => "withdraw",
        }
    }
    pub(super) fn parse(value: &str) -> Result<Self> {
        match value {
            "publish" => Ok(Self::Publish),
            "withdraw" => Ok(Self::Withdraw),
            _ => Err(invalid()),
        }
    }
}
/// Exact durable claimant; expiry never establishes remote cleanup success.
#[derive(Clone)]
pub struct EmployeeExportLease {
    pub(super) fact_id: Uuid,
    pub(super) action: EmployeeExportAction,
    pub(super) token: Uuid,
    pub(super) total_attempts: i32,
}
impl EmployeeExportLease {
    /// Original fact.
    pub fn fact_id(&self) -> Uuid {
        self.fact_id
    }
    /// Original remote action.
    pub fn action(&self) -> EmployeeExportAction {
        self.action
    }
}
/// Prepared immutable original target. No current binding is substituted during
/// cleanup. Text and canonical provenance are present only for publication.
pub struct PreparedEmployeeExport {
    /// Fixed company.
    pub company_id: Uuid,
    /// Durable employee namespace owner.
    pub employee_id: EmployeeId,
    /// Exact owned original creation, without registration witness metadata.
    pub original: HonchoCreatedResourcesReceipt,
    /// Original action and ownership.
    pub lease: EmployeeExportLease,
    /// Typed central hash commitment.
    pub commitment: ReviewedEmployeeCommitment,
    /// Original new-family namespace digest.
    pub namespace_hash: String,
    /// Original new-family binding digest.
    pub binding_hash: String,
    /// Exact typed request digest.
    pub request_hash: String,
    /// Explicit edited text, publication only.
    pub content: Option<String>,
    /// Canonical reviewed sharing, publication only.
    pub provenance: Option<EmployeeMemoryProvenanceV1>,
}
