//! Office identity port used by provisioning (Architecture v0 §4.7 and §6).
//!
//! Provisioning must prove that the opaque signer reference produces the
//! configured public key and that the key is an Office member before an
//! employee revision activates. This port covers only that: signer proof,
//! membership create-or-adopt, membership health, and profile publication.
//! Delivery-time signing lives behind the `OfficeSigner` port.

use ortak_domain::{CredentialRef, EmployeeId, OfficeBinding, ProvisioningMode};
use serde::{Deserialize, Serialize};

use crate::adapter::{Detail, HealthReport, ResourceOutcome};

/// 32-byte Office public key.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct OfficePublicKey([u8; 32]);

impl OfficePublicKey {
    /// Parses a 64-character hex key.
    pub fn parse_hex(value: &str) -> Result<Self, OfficeIdentityError> {
        let bytes = hex::decode(value).map_err(|_| OfficeIdentityError::InvalidPublicKey)?;
        <[u8; 32]>::try_from(bytes.as_slice())
            .map(Self)
            .map_err(|_| OfficeIdentityError::InvalidPublicKey)
    }

    /// Raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl TryFrom<String> for OfficePublicKey {
    type Error = OfficeIdentityError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse_hex(&value)
    }
}

impl From<OfficePublicKey> for String {
    fn from(value: OfficePublicKey) -> Self {
        value.to_hex()
    }
}

/// Result of asking the signer to prove its key.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SignerVerification {
    /// Public key the signer actually produced.
    pub produced_public_key: OfficePublicKey,
    /// True when it equals the configured key.
    pub matches_expected: bool,
}

/// Create-or-adopt request for Office membership of an employee key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfficeMembershipRequest {
    /// Employee.
    pub employee_id: EmployeeId,
    /// Create membership or adopt an existing member.
    pub mode: ProvisioningMode,
    /// Secret-free binding.
    pub binding: OfficeBinding,
    /// Step idempotency key.
    pub idempotency_key: String,
}

/// Secret-free employee profile publication receipt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfilePublication {
    /// Published event id (hex) or adapter receipt.
    pub receipt_ref: String,
}

/// Office identity failures.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OfficeIdentityError {
    /// A public key is not 64 hex characters.
    #[error("office public key must be 64 hexadecimal characters")]
    InvalidPublicKey,
    /// The signer reference cannot be resolved. The reference is reported,
    /// never key material.
    #[error("signer reference cannot be resolved: {signer_ref}")]
    SignerUnresolvable {
        /// Opaque reference.
        signer_ref: String,
    },
    /// The Office could not be reached.
    #[error("office unavailable: {detail}")]
    Unavailable {
        /// Bounded detail.
        detail: Detail,
    },
    /// Adopt mode named a key that is not a member.
    #[error("office member not found for key {public_key}")]
    MemberNotFound {
        /// Hex public key.
        public_key: String,
    },
    /// Create mode found an existing member; Ortak never replaces it.
    #[error("office member already exists for key {public_key}")]
    MemberExists {
        /// Hex public key.
        public_key: String,
    },
    /// The Office rejected the request.
    #[error("office rejected the request: {detail}")]
    Rejected {
        /// Bounded detail.
        detail: Detail,
    },
}

impl OfficeIdentityError {
    /// Builds a signer error from an opaque reference.
    pub fn signer(signer_ref: &CredentialRef) -> Self {
        Self::SignerUnresolvable {
            signer_ref: signer_ref.as_str().to_owned(),
        }
    }

    /// True when a retry may succeed without operator action.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
}

/// Office identity port for provisioning.
#[allow(async_fn_in_trait)]
pub trait OfficeIdentityAdapter {
    /// Asks the signer behind `signer_ref` to prove which public key it
    /// produces, without exporting key material.
    async fn verify_signer(
        &self,
        signer_ref: &CredentialRef,
        expected: &OfficePublicKey,
    ) -> Result<SignerVerification, OfficeIdentityError>;

    /// Creates or adopts Office membership for the key (create/adopt contract
    /// as for the runtime port).
    async fn ensure_membership(
        &self,
        request: &OfficeMembershipRequest,
    ) -> Result<ResourceOutcome, OfficeIdentityError>;

    /// Removes a membership this operation created; never called for
    /// adopted members.
    async fn remove_created_membership(
        &self,
        resource_ref: &str,
        idempotency_key: &str,
    ) -> Result<(), OfficeIdentityError>;

    /// Reports whether the key is currently an Office member.
    async fn membership_health(
        &self,
        public_key: &OfficePublicKey,
    ) -> Result<HealthReport, OfficeIdentityError>;

    /// Publishes (or re-publishes) the secret-free employee profile.
    async fn publish_profile(
        &self,
        employee_id: &EmployeeId,
        binding: &OfficeBinding,
        display_name: &str,
        idempotency_key: &str,
    ) -> Result<ProfilePublication, OfficeIdentityError>;
}
