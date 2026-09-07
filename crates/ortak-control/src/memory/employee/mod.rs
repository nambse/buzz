//! Pure employee-owned reviewed-memory identities with one explicit channel.
//!
//! These values describe claims, not authority. They prove no Office lookup,
//! human approval, current source/destination access, binding ownership or
//! permission to retain, publish, recall or disclose. Legacy memory scopes and
//! project/conversation wire formats are deliberately not widened.

use crate::office_identity::OfficePublicKey;
use ortak_domain::EmployeeId;
use uuid::Uuid;

mod values;
mod wire;

pub use values::{EmployeeMemoryDigest, EmployeeMemorySourceV1, EmployeeSharingApprovalV1};

/// Exact audience format, independent of the chosen source and review.
pub const EMPLOYEE_MEMORY_AUDIENCE_FORMAT_V1: &str = "ortak-reviewed-employee-audience/1";
/// Exact provenance format, including a distinct sharing approval claim.
pub const EMPLOYEE_MEMORY_PROVENANCE_FORMAT_V1: &str = "ortak-reviewed-employee-provenance/1";
/// Domain separator binding source evidence to the explicit audience.
pub const EMPLOYEE_MEMORY_SOURCE_FORMAT_V1: &str = "ortak-reviewed-employee-source/1";
/// Format distinguishing a sharing approval from private storage permission.
pub const EMPLOYEE_MEMORY_SHARING_FORMAT_V1: &str = "ortak-reviewed-employee-sharing/1";
/// Maximum audience bytes, checked before parsing.
pub const MAX_EMPLOYEE_MEMORY_AUDIENCE_BYTES: usize = 2048;
/// Maximum provenance bytes, checked before parsing.
pub const MAX_EMPLOYEE_MEMORY_PROVENANCE_BYTES: usize = 4096;

/// Closed validation failures; rejected values are never copied into errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum EmployeeMemoryError {
    /// A required durable UUID was nil.
    #[error("invalid employee memory identity")]
    InvalidIdentity,
    /// Timestamp precision or supported range was violated.
    #[error("invalid employee memory timestamp")]
    InvalidTimestamp,
    /// A digest or public identity was not canonical lowercase 32-byte hex.
    #[error("invalid employee memory digest")]
    InvalidDigest,
    /// Unsupported fields, version, size or noncanonical serialization.
    #[error("invalid employee memory wire value")]
    InvalidWire,
    /// Immutable audience/source/approval claims or derived hashes disagree.
    #[error("inconsistent employee memory provenance")]
    InconsistentProvenance,
}

/// Closed employee-owned scope kinds. Neither implies employee-global use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmployeeMemoryKind {
    /// Experience shared only into the explicit destination channel.
    Experience,
    /// Experience about the explicit human, shared only into that channel.
    Relationship,
}

/// Durable employee identity and exactly one explicitly selected destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployeeMemoryAudienceV1 {
    company_id: Uuid,
    employee_id: EmployeeId,
    destination_community_id: Uuid,
    destination_channel_id: Uuid,
    human_public_key: Option<OfficePublicKey>,
}

impl EmployeeMemoryAudienceV1 {
    /// Constructs a channel-specific experience claim, with no project fallback.
    pub fn experience(
        company_id: Uuid,
        employee_id: EmployeeId,
        destination_community_id: Uuid,
        destination_channel_id: Uuid,
    ) -> Result<Self, EmployeeMemoryError> {
        if [company_id, destination_community_id, destination_channel_id]
            .iter()
            .any(Uuid::is_nil)
        {
            return Err(EmployeeMemoryError::InvalidIdentity);
        }
        Ok(Self {
            company_id,
            employee_id,
            destination_community_id,
            destination_channel_id,
            human_public_key: None,
        })
    }

    /// Constructs a relationship claim naming its human explicitly.
    pub fn relationship(
        company_id: Uuid,
        employee_id: EmployeeId,
        destination_community_id: Uuid,
        destination_channel_id: Uuid,
        human_public_key: OfficePublicKey,
    ) -> Result<Self, EmployeeMemoryError> {
        let mut value = Self::experience(
            company_id,
            employee_id,
            destination_community_id,
            destination_channel_id,
        )?;
        value.human_public_key = Some(human_public_key);
        Ok(value)
    }

    /// Owning company claim, not an authorized CompanyScope.
    pub fn company_id(&self) -> Uuid {
        self.company_id
    }

    /// Durable owner, independent of model, runtime and memory deployment.
    pub fn employee_id(&self) -> &EmployeeId {
        &self.employee_id
    }

    /// Community containing the selected destination and source.
    pub fn destination_community_id(&self) -> Uuid {
        self.destination_community_id
    }

    /// Only the explicit channel is named; future channels are not included.
    pub fn destination_channel_id(&self) -> Uuid {
        self.destination_channel_id
    }

    /// Explicit relationship human, absent only for experience.
    pub fn human_public_key(&self) -> Option<OfficePublicKey> {
        self.human_public_key
    }

    /// Closed kind derived from the complete identity.
    pub fn kind(&self) -> EmployeeMemoryKind {
        if self.human_public_key.is_some() {
            EmployeeMemoryKind::Relationship
        } else {
            EmployeeMemoryKind::Experience
        }
    }

    /// Lexicographic compact UTF-8 JSON, with an explicit null experience human.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, EmployeeMemoryError> {
        wire::audience_bytes(self)
    }

    /// SHA-256 of only this audience; reviews and source messages are excluded.
    pub fn audience_hash(&self) -> Result<EmployeeMemoryDigest, EmployeeMemoryError> {
        Ok(wire::digest(&self.canonical_bytes()?))
    }

    /// Accepts only bounded exact canonical bytes, without repairing input.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, EmployeeMemoryError> {
        wire::parse_audience(bytes)
    }
}

/// Immutable source and sharing-approval claims, separate from reusable audience.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployeeMemoryProvenanceV1 {
    audience: EmployeeMemoryAudienceV1,
    source: EmployeeMemorySourceV1,
    approval: EmployeeSharingApprovalV1,
}

impl EmployeeMemoryProvenanceV1 {
    /// Checks internal consistency only. A future facade must resolve the source,
    /// authenticate the approver and prove source-review/destination-share grants.
    pub fn new(
        audience: EmployeeMemoryAudienceV1,
        source: EmployeeMemorySourceV1,
        approval: EmployeeSharingApprovalV1,
    ) -> Result<Self, EmployeeMemoryError> {
        if source.community_id() != audience.destination_community_id
            || audience
                .human_public_key
                .is_some_and(|human| human != approval.approved_by())
        {
            return Err(EmployeeMemoryError::InconsistentProvenance);
        }
        Ok(Self {
            audience,
            source,
            approval,
        })
    }

    /// Complete immutable audience claim.
    pub fn audience(&self) -> &EmployeeMemoryAudienceV1 {
        &self.audience
    }

    /// Exact original Office evidence locator and digest.
    pub fn source(&self) -> &EmployeeMemorySourceV1 {
        &self.source
    }

    /// Sharing approval claim, not proof of a committed or current approval.
    pub fn approval(&self) -> &EmployeeSharingApprovalV1 {
        &self.approval
    }

    /// Domain-separated audience/source binding; never a legacy source hash.
    pub fn source_hash(&self) -> Result<EmployeeMemoryDigest, EmployeeMemoryError> {
        wire::source_hash(self)
    }

    /// Exact provenance bytes including source, audience and sharing approval.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, EmployeeMemoryError> {
        wire::provenance_bytes(self)
    }

    /// Hashes all retained claims, including edited-content digest and expiry.
    pub fn sharing_hash(&self) -> Result<EmployeeMemoryDigest, EmployeeMemoryError> {
        Ok(wire::digest(&self.canonical_bytes()?))
    }

    /// Refuses unsupported shapes, recomputed-hash disagreement and noncanonical bytes.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, EmployeeMemoryError> {
        wire::parse_provenance(bytes)
    }
}

#[cfg(test)]
mod tests;
