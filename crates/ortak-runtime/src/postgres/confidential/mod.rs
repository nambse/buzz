//! Protected encrypted-DM admission, deliberately not wired to a worker.
//!
//! Preparation observes one repository-issued verified claim. Crypto happens
//! outside its transaction; commit repeats the current checks and consumes the
//! claim atomically with protected bytes, routing and dispatch metadata. Neither
//! parsed claims nor an old receipt authorize opening content. The caller must
//! retain the transaction used by `load_current_on` through its local use check,
//! and obtain fresh authority again before any subsequent external effect.

mod authority;
mod prepare;
mod repository;
mod wire;
pub use authority::EncryptedDmAuthority;
mod dispatch;
mod events;
mod execution;
mod reply;
pub(crate) use execution::ConfidentialLease;
pub use execution::PgConfidentialExecution;

use chrono::{DateTime, Utc};
use ortak_control::confidential::{ConfidentialEnvelope, ValidatedIdentity};
use ortak_domain::CredentialRef;
use ortak_office::encrypted::key_provider::WrappedMasterKey;
use sqlx::PgPool;
use uuid::Uuid;
use zeroize::Zeroizing;

/// Closed failures never retain database, crypto or JSON input/error strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ConfidentialAdmissionError {
    /// Current source, selection, policy, claim or deadline no longer agrees.
    #[error("confidential admission refused")]
    Refused,
    /// Caller-provided protected bytes do not match the prepared observation.
    #[error("confidential payload mismatch")]
    Payload,
    /// Durable storage failed; no receipt is implied.
    #[error("confidential storage unavailable")]
    Unavailable,
}
impl From<sqlx::Error> for ConfidentialAdmissionError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
impl From<ortak_control::ControlError> for ConfidentialAdmissionError {
    fn from(_: ortak_control::ControlError) -> Self {
        Self::Unavailable
    }
}
impl From<ortak_office::encrypted::jobs::DmJobError> for ConfidentialAdmissionError {
    fn from(value: ortak_office::encrypted::jobs::DmJobError) -> Self {
        match value {
            ortak_office::encrypted::jobs::DmJobError::Unavailable => Self::Unavailable,
            _ => Self::Refused,
        }
    }
}
type Result<T> = std::result::Result<T, ConfidentialAdmissionError>;

/// Private prepared plaintext. No Debug/Clone/Serde or ordinary RunSpec export.
/// A prepared observation is short lived, not authority retained across awaits.
pub struct PreparedConfidentialRun {
    identity: ValidatedIdentity,
    plaintext: Zeroizing<Vec<u8>>,
    source_id: Vec<u8>,
    run_id: Uuid,
    key_id: Uuid,
    signer_ref: CredentialRef,
    deadline: DateTime<Utc>,
}
impl PreparedConfidentialRun {
    /// Server-derived expected identity for explicit wrap-purpose selection.
    pub fn identity(&self) -> &ValidatedIdentity {
        &self.identity
    }
    /// Exact selected Office reference; it is not a runtime credential.
    pub fn signer_ref(&self) -> &CredentialRef {
        &self.signer_ref
    }
    /// Bound local protection deadline, never extended by a retry.
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.deadline
    }
}

/// Only protected bytes survive preparation. No public field allows substitution.
pub struct ProtectedConfidentialRun {
    prepared: PreparedConfidentialRun,
    snapshot: ConfidentialEnvelope,
    wrapped: WrappedMasterKey,
}

/// Metadata-only result, safe to retain after authority or key availability loss.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfidentialAdmissionReceipt {
    /// Original run, including when a different wrapper repeats the same rumor.
    pub run_id: Uuid,
    /// This outer source reused a previously committed rumor; no new dispatch.
    pub duplicate_rumor: bool,
}

/// Current-read ciphertext and expected identity. No key is loaded by storage.
/// Dropping the caller transaction retires this observation's authority.
pub struct CurrentConfidentialPayload {
    identity: ValidatedIdentity,
    snapshot: ConfidentialEnvelope,
    wrapped: WrappedMasterKey,
    signer_ref: CredentialRef,
    valid_before: DateTime<Utc>,
}
impl CurrentConfidentialPayload {
    /// Independently derived expected claims, not just the envelope's header.
    pub fn identity(&self) -> &ValidatedIdentity {
        &self.identity
    }
    /// Exact ciphertext selected under the current authority fence.
    pub fn snapshot(&self) -> &ConfidentialEnvelope {
        &self.snapshot
    }
    /// Exact self-wrapped per-run master for the explicit unwrap provider.
    pub fn wrapped_master(&self) -> &WrappedMasterKey {
        &self.wrapped
    }
    /// Exact owned Office key reference; never an ambient lookup selector.
    pub fn signer_ref(&self) -> &CredentialRef {
        &self.signer_ref
    }
    /// Current-read deadline, including execution and current binding/TTL bounds.
    pub fn valid_before(&self) -> DateTime<Utc> {
        self.valid_before
    }
}

/// Explicit application pool, no worker, subscription or environment resolution.
pub struct PgConfidentialRuns {
    pool: PgPool,
}
impl PgConfidentialRuns {
    /// Constructs an inactive repository. The unnumbered SQL must be installed
    /// by the integrating release before calling it.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[cfg(test)]
mod tests;
