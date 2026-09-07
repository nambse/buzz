use super::*;
use chrono::{DateTime, Utc};
use ortak_control::memory::employee::EmployeeMemoryProvenanceV1;
use serde::{Deserialize, Serialize};

/// Distinct reviewed employee protocol; never the legacy generic memory scope.
pub const REVIEWED_EMPLOYEE_PROTOCOL: &str = "reviewed-employee/1";

/// Exact owned namespace inspection. Construction is private to the adapter.
/// An inspection is ownership evidence, not an I/O or current Office grant.
#[derive(Clone)]
pub struct ReviewedEmployeeNamespace {
    pub(super) original: HonchoCreatedResourcesReceipt,
    pub(super) namespace: String,
    pub(super) namespace_hash: String,
    pub(super) binding_hash: String,
}
impl ReviewedEmployeeNamespace {
    /// The original immutable resource creation receipt.
    pub fn original(&self) -> &HonchoCreatedResourcesReceipt {
        &self.original
    }
    /// Canonical employee namespace bytes, with no fabricated project.
    pub fn canonical_namespace(&self) -> &str {
        &self.namespace
    }
    /// Digest of the canonical namespace.
    pub fn namespace_hash(&self) -> &str {
        &self.namespace_hash
    }
    /// New-family binding digest; distinct from the reviewed-project digest.
    pub fn binding_hash(&self) -> &str {
        &self.binding_hash
    }
}

/// Explicit finite diagnostic intent; persist it before invoking the adapter.
/// Retry or cleanup uses this same operation. No source/user text is accepted.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmployeeNamespaceDiagnostic {
    /// Caller-journaled synthetic diagnostic identity.
    pub operation_id: Uuid,
    /// Current revision whose evidence is being validated, not namespace identity.
    pub employee_revision_id: Uuid,
    /// Current lifecycle generation, not a claim of runtime admission.
    pub employee_lifecycle_epoch: i64,
    /// Exact 64 lowercase hex synthetic bytes; never a user-selected fact.
    pub challenge: String,
}

/// A completed finite diagnostic, retaining no synthetic challenge bytes.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmployeeNamespaceDiagnosticReceipt {
    /// Exact durable operation identity.
    pub operation_id: Uuid,
    /// Revision observed for this operation.
    pub employee_revision_id: Uuid,
    /// Lifecycle observed for this operation.
    pub employee_lifecycle_epoch: i64,
    /// Digest of the synthetic UTF-8 challenge.
    pub challenge_hash: String,
    /// Canonical write commitment; absent if cleanup preceded any write.
    pub write_request_hash: Option<String>,
    /// Canonical cleanup commitment, required for completed recovery.
    pub withdraw_request_hash: String,
    /// Confirmed absence of this operation's synthetic content only.
    pub erased: bool,
    /// Irreversible remote cleanup timestamp.
    pub tombstone_at: DateTime<Utc>,
}

/// Sealed current in-process evidence, only after exact readback and cleanup ACK.
/// Serialization/deserialization cannot mint this value. Current SQL authority
/// and deployment destination selection remain independent requirements.
pub struct EmployeeNamespaceWitness {
    pub(super) namespace: ReviewedEmployeeNamespace,
    pub(super) receipt: EmployeeNamespaceDiagnosticReceipt,
    pub(super) adapter_instance: Uuid,
    pub(super) expires: Instant,
    pub(super) validated_at: DateTime<Utc>,
}
impl EmployeeNamespaceWitness {
    /// Exact inspected namespace for target registration.
    pub fn namespace(&self) -> &ReviewedEmployeeNamespace {
        &self.namespace
    }
    /// Completed immutable diagnostic metadata.
    pub fn diagnostic(&self) -> &EmployeeNamespaceDiagnosticReceipt {
        &self.receipt
    }
    /// Bounded remaining monotonic witness duration; zero means unusable.
    pub fn remaining(&self) -> Duration {
        self.expires.saturating_duration_since(Instant::now())
    }
    /// Approximate wall-clock observation time; callers use the monotonic bound.
    pub fn validated_at(&self) -> DateTime<Utc> {
        self.validated_at
    }
}

/// Immutable central commitment used for both publish and withdrawal.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedEmployeeCommitment {
    /// Original target row, never replaced for cleanup.
    pub target_id: Uuid,
    /// Original reviewed fact identity.
    pub fact_id: Uuid,
    /// One explicitly approved destination channel.
    pub destination_channel_id: Uuid,
    /// Immutable approved edited-content digest.
    pub content_hash: String,
    /// Immutable canonical source digest.
    pub source_hash: String,
    /// Immutable full sharing provenance digest.
    pub sharing_hash: String,
}

/// Explicitly approved edited bytes; Debug never contains the text.
#[derive(Clone)]
pub struct ReviewedEmployeePublication {
    /// Exact original export commitment.
    pub commitment: ReviewedEmployeeCommitment,
    /// Human edited UTF-8 text, at most 4 KiB.
    pub content: String,
    /// Canonical reviewed v1 provenance; structural claims alone authorize nothing.
    pub provenance: EmployeeMemoryProvenanceV1,
}

/// Strict remote record projection, distinct from old project records.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedEmployeeRecord {
    /// Exactly reviewed-employee/1.
    pub protocol: String,
    /// Fixed selected company.
    pub company_id: Uuid,
    /// Durable employee namespace owner.
    pub employee_id: EmployeeId,
    /// Fixed selected deployment.
    pub deployment_id: Uuid,
    /// Original owned workspace name.
    pub workspace_id: String,
    /// Central fact identity.
    pub record_id: Uuid,
    /// Original central target identity.
    pub target_id: Uuid,
    /// Explicit destination audience.
    pub destination_channel_id: Uuid,
    /// Canonical namespace digest.
    pub namespace_hash: String,
    /// New-family binding digest.
    pub binding_hash: String,
    /// Current remote text state; never a runtime authority claim.
    pub status: crate::ReviewedProjectStatus,
    /// Present only in exact selected active recall, never in a mutation ACK.
    pub content: Option<String>,
    /// Immutable content commitment, including a pre-publication withdrawal.
    pub content_hash: String,
    /// Immutable source commitment.
    pub source_hash: String,
    /// Immutable sharing commitment.
    pub sharing_hash: String,
    /// Exact canonical string; absent before any publication.
    pub provenance: Option<String>,
    /// Immutable expiry; absent before any publication.
    pub expires_at: Option<DateTime<Utc>>,
    /// Text absence proven only in this extension store.
    pub erased_from_reviewed_store: bool,
    /// Retained tombstone; no source or backup-erasure claim.
    pub tombstone_at: Option<DateTime<Utc>>,
}

/// Validated remote ACK, retaining exact commitment and remote metadata.
pub struct ReviewedEmployeeAcknowledgement {
    /// Text-free remote projection.
    pub record: ReviewedEmployeeRecord,
    /// Canonical typed remote request commitment.
    pub request_hash: String,
    /// True only for an observed 201 publication response.
    pub created: bool,
}

/// At most eight explicitly requested remote records and 8 KiB edited text.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedEmployeeRecall {
    /// Exact remote records in caller-selected order.
    pub records: Vec<ReviewedEmployeeRecord>,
    /// The finite text budget omitted additional selected records.
    pub truncated: bool,
}
