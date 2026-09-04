//! Credential-reference resolution port.
//!
//! Provisioning only needs to know that an opaque [`CredentialRef`] exists in
//! the credential manager. The port therefore never returns a value; it
//! answers existence and lets the adapter that owns the reference resolve it
//! later under its own authorization.

use ortak_domain::CredentialRef;

use crate::adapter::Detail;

/// Existence status of one reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialReferenceStatus {
    /// The manager knows the reference and the caller may resolve it.
    Resolvable,
    /// The manager has no such reference.
    Missing,
}

/// Credential manager failures; never carry values.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CredentialError {
    /// The manager could not be reached.
    #[error("credential manager unavailable: {detail}")]
    Unavailable {
        /// Bounded detail.
        detail: Detail,
    },
    /// The caller is not allowed to see this reference.
    #[error("credential reference is not authorized for this caller: {credential_ref}")]
    Unauthorized {
        /// Opaque reference.
        credential_ref: String,
    },
}

/// Credential reference existence port.
#[allow(async_fn_in_trait)]
pub trait CredentialResolver {
    /// Checks whether the reference exists without returning any value.
    async fn verify_reference(
        &self,
        credential_ref: &CredentialRef,
    ) -> Result<CredentialReferenceStatus, CredentialError>;
}
