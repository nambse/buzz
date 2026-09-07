//! Unactivated, metadata-only encrypted-DM selection and leased crypto jobs.
//!
//! No worker/normalizer calls this repository. A job never updates office_inbox,
//! creates a routing decision/run, or stores decrypted text. A verified result
//! keeps its short lease for a future atomic confidential commit; it is not a
//! retained authority token or a recovered plaintext snapshot.

mod repository;
mod selection;
pub use repository::PgDecryptJobs;

use chrono::{DateTime, Utc};
use nostr::{EventId, PublicKey};
use ortak_control::office_identity::OfficePublicKey;
use ortak_domain::{CredentialRef, EmployeeId};
use uuid::Uuid;

use super::ExpectedEnvelope;

/// Explicit server-owned configuration, not an authorization witness. Only the
/// dm_decrypt purpose is supported here; this does not expand the key provider.
#[derive(Clone)]
pub struct ConfiguredDmPair {
    /// Immutable retained selection ID; changing the pair requires a new ID.
    pub selection_id: Uuid,
    /// Canonical private two-member channel in the separately resolved company.
    pub channel_id: Uuid,
    /// Durable employee, independent of model/runtime revision.
    pub employee_id: EmployeeId,
    /// Explicit expected human, never derived from the outer transport signer.
    pub human_public_key: OfficePublicKey,
    /// Expected employee Office key.
    pub employee_public_key: OfficePublicKey,
    /// Exact Office binding introducing that public identity.
    pub office_binding_id: Uuid,
    /// Explicit key-purpose version; not a runtime/model version.
    pub key_version: i64,
    /// Exact selected existing Office reference, never credential material.
    pub decrypt_ref: CredentialRef,
}

/// Exact stored encrypted outer tuple. Construction checks syntax, not access.
pub struct DmOuterSource {
    id: EventId,
    created_at: DateTime<Utc>,
}

impl DmOuterSource {
    /// Requires the original integral Nostr event partition, not the receipt time.
    pub fn new(id: EventId, created_at: DateTime<Utc>) -> Result<Self, DmJobError> {
        if created_at.timestamp_subsec_nanos() != 0
            || created_at.timestamp() < 0
            || created_at.timestamp() > 253_402_300_799
        {
            return Err(DmJobError::Invalid);
        }
        Ok(Self { id, created_at })
    }
}

/// Closed failures exclude database/parser strings, encrypted bytes and refs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DmJobError {
    /// Invalid explicit metadata.
    #[error("invalid encrypted DM job selection")]
    Invalid,
    /// Current canonical pair/source/cohort or exact selection does not agree.
    #[error("encrypted DM job refused")]
    Refused,
    /// Claim generation, token or deadline no longer permits the result.
    #[error("encrypted DM claim retired")]
    Stale,
    /// Database failure is propagated without exposing rejected input.
    #[error("encrypted DM job storage unavailable")]
    Unavailable,
}
impl From<sqlx::Error> for DmJobError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
impl From<ortak_control::ControlError> for DmJobError {
    fn from(_: ortak_control::ControlError) -> Self {
        Self::Unavailable
    }
}

/// Metadata-only closed outcomes for bounded failure persistence.
#[derive(Clone, Copy)]
pub enum DecryptFailure {
    /// The exact selected material is temporarily unavailable; at most 3 attempts.
    MaterialUnavailable,
    /// Crypto/signature/shape/recipient verification failed; terminal.
    CryptoInvalid,
    /// Current source/pair/key/lifecycle permission was lost; terminal.
    AuthorityChanged,
    /// The selected stored source disappeared; terminal.
    SourceUnavailable,
    /// The bounded local operation expired; terminal.
    DeadlineExceeded,
    /// Explicit cancellation, requiring no recovered content/key.
    Cancelled,
}
impl DecryptFailure {
    fn code(self) -> &'static str {
        match self {
            Self::MaterialUnavailable => "material_unavailable",
            Self::CryptoInvalid => "crypto_invalid",
            Self::AuthorityChanged => "authority_changed",
            Self::SourceUnavailable => "source_unavailable",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Immutable repository observation for future confidential identity construction.
/// Copying these public facts does not copy the private claim/token capability.
pub struct DmClaimIdentity {
    /// Company selected from the host's canonical binding.
    pub company_id: Uuid,
    /// Canonical community.
    pub community_id: Uuid,
    /// Exact retained pair channel.
    pub channel_id: Uuid,
    /// Immutable explicit selection.
    pub selection_id: Uuid,
    /// Activation generation; re-enable cannot revive an old claim.
    pub selection_generation: i64,
    /// Durable employee.
    pub employee_id: EmployeeId,
    /// Current revision frozen by the job, not part of durable employee identity.
    pub employee_revision_id: Uuid,
    /// Frozen lifecycle epoch.
    pub employee_lifecycle_epoch: i64,
    /// Office mutation witness; any change requires a new observation, not renewal.
    pub office_generation: i64,
    /// Exact Office identity binding.
    pub office_binding_id: Uuid,
    /// Explicit configured key version.
    pub key_version: i64,
    /// Exact purpose-specific opaque reference.
    pub decrypt_ref: CredentialRef,
}

/// One repository-issued claim. No Debug/Clone/Serde exposes ciphertext or tokens.
/// It permits only the selected bounded verification operation, never dispatch.
pub struct DmDecryptClaim {
    identity: DmClaimIdentity,
    expected: ExpectedEnvelope,
    outer: Vec<u8>,
    outer_hash: [u8; 32],
    generation: i64,
    token: Uuid,
    worker: Uuid,
    crypto_deadline: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}
impl DmDecryptClaim {
    /// Current-read facts to bind the subsequent confidential transaction.
    pub fn identity(&self) -> &DmClaimIdentity {
        &self.identity
    }
    /// Exact outer/pair expectations for the isolated cryptographic decoder.
    pub fn expected(&self) -> &ExpectedEnvelope {
        &self.expected
    }
    /// Bounded reconstructed signed ciphertext event; never decrypted text.
    pub fn outer_bytes(&self) -> &[u8] {
        &self.outer
    }
    /// Five-second local crypto budget, possibly shortened by authority expiry.
    pub fn crypto_deadline(&self) -> DateTime<Utc> {
        self.crypto_deadline
    }
    /// Thirty-second final-commit lease; no renewal API exists.
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
}

fn key(bytes: &[u8]) -> Result<PublicKey, DmJobError> {
    PublicKey::from_slice(bytes).map_err(|_| DmJobError::Unavailable)
}
