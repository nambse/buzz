//! `OfficeSigner` port (Architecture v0 §4.7).
//!
//! A signer sees an opaque credential reference and a validated unsigned
//! event. It returns a [`FrozenSignedEvent`], which can only be built through
//! [`FrozenSignedEvent::seal`], so every implementation's output is verified
//! against the expected public key and the exact unsigned fields before the
//! control plane can use it. Private key material never crosses this port.

use ortak_control::adapter::Detail;
use ortak_control::office_identity::OfficePublicKey;
use ortak_domain::{CredentialRef, EmployeeId};
use uuid::Uuid;

use crate::event::{FrozenSignedEvent, OfficeEventError, UnsignedOfficeEvent};

/// One signing request: the validated unsigned event plus the opaque signer
/// reference that must produce the expected author key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigningRequest {
    unsigned: UnsignedOfficeEvent,
    signer_ref: CredentialRef,
}

impl SigningRequest {
    /// Pairs a validated unsigned event with the signer that must sign it.
    /// Crate-private: requests are built only from an
    /// [`AuthorizedOfficePublish`](crate::repository::AuthorizedOfficePublish),
    /// so the signer reference is always the one the control plane derived.
    pub(crate) fn new(unsigned: UnsignedOfficeEvent, signer_ref: CredentialRef) -> Self {
        Self {
            unsigned,
            signer_ref,
        }
    }

    /// Company boundary.
    pub fn company_id(&self) -> Uuid {
        self.unsigned.intent().company_id
    }

    /// Run the event belongs to.
    pub fn run_id(&self) -> Uuid {
        self.unsigned.intent().run_id
    }

    /// Authoring employee.
    pub fn employee_id(&self) -> &EmployeeId {
        &self.unsigned.intent().employee_id
    }

    /// Pinned employee revision.
    pub fn employee_revision_id(&self) -> Uuid {
        self.unsigned.intent().employee_revision_id
    }

    /// Validated unsigned event.
    pub fn unsigned(&self) -> &UnsignedOfficeEvent {
        &self.unsigned
    }

    /// Opaque credential-manager / KMS / remote-signer reference.
    pub fn signer_ref(&self) -> &CredentialRef {
        &self.signer_ref
    }

    /// Public key the signature must verify under.
    pub fn expected_public_key(&self) -> &OfficePublicKey {
        self.unsigned.public_key()
    }
}

/// Signer failures. Messages carry references and bounded detail, never keys.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OfficeSignerError {
    /// The signer reference cannot be resolved.
    #[error("signer reference cannot be resolved: {signer_ref}")]
    SignerUnresolvable {
        /// Opaque reference.
        signer_ref: String,
    },
    /// The signer could not be reached; a retry may succeed.
    #[error("signer unavailable: {detail}")]
    Unavailable {
        /// Bounded detail.
        detail: Detail,
    },
    /// The signer refused to sign.
    #[error("signer rejected the request: {detail}")]
    Rejected {
        /// Bounded detail.
        detail: Detail,
    },
    /// The signer's output failed verification; nothing was accepted.
    #[error("signer output failed verification: {0}")]
    Verification(#[from] OfficeEventError),
}

impl OfficeSignerError {
    /// Builds an unresolvable-reference error without retaining any value.
    pub fn unresolvable(signer_ref: &CredentialRef) -> Self {
        Self::SignerUnresolvable {
            signer_ref: signer_ref.as_str().to_owned(),
        }
    }

    /// True when a retry may succeed without operator action.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
}

/// Delivery-time signing port.
#[allow(async_fn_in_trait)]
pub trait OfficeSigner {
    /// Signs the request through the referenced signer and returns the
    /// verified, frozen result. Implementations must build the result with
    /// [`FrozenSignedEvent::seal`]; there is no other constructor.
    async fn sign(&self, request: &SigningRequest) -> Result<FrozenSignedEvent, OfficeSignerError>;
}

impl<T: OfficeSigner + ?Sized> OfficeSigner for &T {
    async fn sign(&self, request: &SigningRequest) -> Result<FrozenSignedEvent, OfficeSignerError> {
        (**self).sign(request).await
    }
}
