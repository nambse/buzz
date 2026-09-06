//! Explicit, inactive confidential-DM execution. No subscription or worker is
//! installed by constructing these ports. Every content/effect operation keeps
//! the current Office/source fence and its deadline; recovery uses no keys.

mod dispatch;
mod inner;
mod reply;
mod supervision;

use crate::{hermes::HermesAdapter, postgres::confidential::PgConfidentialExecution};
use chrono::{DateTime, Utc};
use ortak_control::CompanyScope;
use ortak_office::encrypted::key_provider::EnvDmKeyProvider;

/// Closed execution failures never retain payloads, keys or backend errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum EncryptedExecutionError {
    /// Current authority, exact lease or finite deadline was lost.
    #[error("confidential execution refused")]
    Refused,
    /// The authenticated inner protocol or state sequence was invalid.
    #[error("confidential execution protocol mismatch")]
    Protocol,
    /// An external or durable operation failed; no success is implied.
    #[error("confidential execution unavailable")]
    Unavailable,
}
type Result<T> = std::result::Result<T, EncryptedExecutionError>;
impl From<crate::postgres::confidential::ConfidentialAdmissionError> for EncryptedExecutionError {
    fn from(error: crate::postgres::confidential::ConfidentialAdmissionError) -> Self {
        use crate::postgres::confidential::ConfidentialAdmissionError as Admission;
        match error {
            Admission::Refused => Self::Refused,
            Admission::Payload => Self::Protocol,
            Admission::Unavailable => Self::Unavailable,
        }
    }
}
impl From<sqlx::Error> for EncryptedExecutionError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}

/// One bounded operation, not a background retry loop or permission grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionProgress {
    /// No due work.
    Idle,
    /// A durable state/receipt advanced.
    Recorded,
    /// A bounded retry or containment obligation was retained.
    Deferred,
}

/// Selected company, repository, protected adapter and explicit Office provider.
/// Integration must advertise the adapter's installed ConfidentialDmV1 capability
/// before constructing this service; no ordinary adapter fallback exists.
pub struct EncryptedExecution<'a> {
    scope: &'a CompanyScope,
    repository: &'a PgConfidentialExecution,
    adapter: &'a HermesAdapter,
    keys: &'a EnvDmKeyProvider,
}
impl<'a> EncryptedExecution<'a> {
    /// Builds an inactive executor from explicitly owned ports.
    pub fn new(
        scope: &'a CompanyScope,
        repository: &'a PgConfidentialExecution,
        adapter: &'a HermesAdapter,
        keys: &'a EnvDmKeyProvider,
    ) -> Self {
        Self {
            scope,
            repository,
            adapter,
            keys,
        }
    }
}

fn remaining(deadline: DateTime<Utc>) -> Result<std::time::Duration> {
    let value = (deadline - Utc::now())
        .to_std()
        .map_err(|_| EncryptedExecutionError::Refused)?;
    if value.is_zero() {
        return Err(EncryptedExecutionError::Refused);
    }
    Ok(value.min(std::time::Duration::from_secs(5)))
}

#[cfg(test)]
mod tests;
