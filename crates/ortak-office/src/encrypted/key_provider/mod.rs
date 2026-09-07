//! Explicit Office-key selection and NIP-44 self-wrapping, with no admission,
//! database, journal, runtime-key transfer or generic signer changes.
//!
//! Configured purposes are necessary but not current authority. A future caller
//! must obtain canonical source/revision/lifecycle/Office/key-epoch evidence and
//! recheck its claim/deadline around this operation. Parsing claims does not do
//! that. No raw Office key or arbitrary key callback is exposed by this provider.

mod operations;
mod wire;
pub use operations::{SealedDmCopy, SealedDmReply};

use std::collections::HashSet;

use nostr::Keys;
use ortak_control::confidential::ValidatedIdentity;
use ortak_domain::CredentialRef;
use serde::Deserialize;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::transport::OfficeSignerBinding;

/// Complete protected-key envelope cap, before parsing.
pub const MAX_WRAPPED_MASTER_BYTES: usize = 12 * 1024;
/// NIP-44 encoded ciphertext cap, before base64 or decryption.
pub const MAX_KEY_CIPHERTEXT_BYTES: usize = 8 * 1024;
/// Decrypted canonical key payload cap, before parsing.
pub const MAX_KEY_PLAINTEXT_BYTES: usize = 4 * 1024;

/// Closed operation purposes. None exposes a raw key or generic signing port.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
pub enum OfficeKeyPurpose {
    /// Protect an explicitly supplied per-run master.
    #[serde(rename = "confidential_wrap")]
    WrapMaster,
    /// Open an exact retained per-run master after separate current read/use checks.
    #[serde(rename = "confidential_unwrap")]
    UnwrapMaster,
    /// Verify only the repository-issued exact outer/pair decrypt claim.
    #[serde(rename = "dm_decrypt")]
    DmDecrypt,
    /// Seal one current reply or sign a bounded NIP-42 publication challenge.
    #[serde(rename = "dm_seal")]
    DmSeal,
}

/// Public allowlist entry; construction and validation perform no secret read.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DmOfficeKeyBinding {
    /// Reuses the exact existing Office signer owner/ref/public-key/env selection.
    pub signer: OfficeSignerBinding,
    /// Exact retained Office binding identity.
    pub office_binding_id: Uuid,
    /// Office-key version; independent of runtime/model revision changes.
    pub key_version: u64,
    /// Nonempty, duplicate-free, closed implemented purpose list.
    pub purposes: Vec<OfficeKeyPurpose>,
}

/// Closed errors contain no private material, refs or underlying error objects.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DmKeyError {
    /// Invalid, duplicate or oversized public configuration.
    #[error("invalid encrypted DM key configuration")]
    Configuration,
    /// Exact ownership/purpose/expected-envelope selection does not agree.
    #[error("encrypted DM key operation refused")]
    Refused,
    /// Only the selected explicit environment entry was unavailable.
    #[error("selected encrypted DM key unavailable")]
    Unavailable,
    /// Selected material is malformed or belongs to another public identity.
    #[error("selected encrypted DM key identity mismatch")]
    KeyMismatch,
    /// Protected wire is malformed, noncanonical or beyond its bound.
    #[error("invalid encrypted DM wrapped key")]
    Envelope,
    /// NIP-44 or authenticated inner identity/ref/purpose verification failed.
    #[error("encrypted DM wrapped key authentication failed")]
    Authentication,
}

/// Pure expected claims plus one exact opaque Office reference. This is not an
/// authorized/current constructor; its caller must supply independent authority.
pub struct DmKeySelection {
    identity: ValidatedIdentity,
    signer_ref: CredentialRef,
}

impl DmKeySelection {
    /// Retains all claims, including revision/lifecycle/source/run/key identity.
    pub fn from_expected_claims(identity: &ValidatedIdentity, signer_ref: CredentialRef) -> Self {
        Self {
            identity: identity.clone(),
            signer_ref,
        }
    }
}

/// Canonical immutable encrypted envelope, with no Debug or implicit Serde.
/// Its parse method proves only shape, not a valid ciphertext or current right.
pub struct WrappedMasterKey {
    identity: ValidatedIdentity,
    signer_ref: CredentialRef,
    ciphertext: String,
    bytes: Vec<u8>,
}

impl WrappedMasterKey {
    /// Parses bounded canonical public/ciphertext metadata without resolving keys.
    pub fn parse(bytes: &[u8]) -> Result<Self, DmKeyError> {
        wire::parse_outer(bytes)
    }

    /// Exact bytes that a future atomic persistence/retry lane must retain.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn require_expected(&self, selection: &DmKeySelection) -> Result<(), DmKeyError> {
        if self.identity != selection.identity || self.signer_ref != selection.signer_ref {
            return Err(DmKeyError::Refused);
        }
        Ok(())
    }
}

/// Authenticated, zeroizing master material. No Office key is included; no
/// Debug/Clone/Serde permits accidental logging or persistence.
pub struct UnwrappedMasterKey(Zeroizing<[u8; 32]>);

impl UnwrappedMasterKey {
    /// Transfers only the per-run data key to the separately authorized central
    /// confidential codec. This never transfers an Office key to a runtime.
    pub fn into_owned(self) -> Zeroizing<[u8; 32]> {
        self.0
    }
}

/// Immutable explicit allowlist. Keys are resolved lazily for exactly one
/// permitted operation; unsupported purpose/metadata is refused before any read.
pub struct EnvDmKeyProvider {
    bindings: Vec<DmOfficeKeyBinding>,
}

impl EnvDmKeyProvider {
    /// Explicit no-key provider for retained keyless cancellation/recovery.
    /// Every content/key operation refuses before consulting any environment.
    /// This does not make an empty configured allowlist valid for new work.
    pub fn denied() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// Validates the complete 1..64-entry allowlist before any secret I/O.
    /// Duplicate owner/version, public key or env aliases are refused.
    pub fn new(bindings: Vec<DmOfficeKeyBinding>) -> Result<Self, DmKeyError> {
        if bindings.is_empty() || bindings.len() > 64 {
            return Err(DmKeyError::Configuration);
        }
        let mut owners = HashSet::new();
        let mut public_keys = HashSet::new();
        let mut environments = HashSet::new();
        for binding in &bindings {
            binding
                .signer
                .validate()
                .map_err(|_| DmKeyError::Configuration)?;
            if binding.office_binding_id.is_nil()
                || binding.key_version > i64::MAX as u64
                || binding.purposes.is_empty()
                || binding.purposes.len() > 4
                || binding.purposes.iter().collect::<HashSet<_>>().len() != binding.purposes.len()
                || !owners.insert((
                    binding.signer.company_id,
                    binding.signer.employee_id.clone(),
                    binding.office_binding_id,
                    binding.key_version,
                ))
                || !public_keys.insert(binding.signer.public_key.to_hex())
                || !environments.insert(binding.signer.secret_env.clone())
            {
                return Err(DmKeyError::Configuration);
            }
        }
        Ok(Self { bindings })
    }

    fn selected(
        &self,
        request: &DmKeySelection,
        purpose: OfficeKeyPurpose,
    ) -> Result<&DmOfficeKeyBinding, DmKeyError> {
        let claims = request.identity.key_claims();
        self.bindings
            .iter()
            .find(|binding| {
                binding.purposes.contains(&purpose)
                    && binding.signer.company_id.to_string() == claims.company_id
                    && binding.signer.employee_id.as_str() == claims.employee_id
                    && binding.office_binding_id.to_string() == claims.office_binding_id
                    && binding.key_version.to_string() == claims.key_version
                    && binding.signer.public_key.to_hex() == claims.employee_public_key
                    && binding.signer.signer_ref == request.signer_ref
            })
            .ok_or(DmKeyError::Refused)
    }

    /// Self-wraps one caller-owned master after exact configured purpose selection.
    /// Generates fresh NIP-44 ciphertext; retries must reuse the retained result.
    pub fn wrap_master(
        &self,
        selection: &DmKeySelection,
        master: &Zeroizing<[u8; 32]>,
    ) -> Result<WrappedMasterKey, DmKeyError> {
        self.wrap_with_reader(selection, master, read_exact_env)
    }

    fn wrap_with_reader(
        &self,
        selection: &DmKeySelection,
        master: &Zeroizing<[u8; 32]>,
        mut read: impl FnMut(&str) -> Result<String, DmKeyError>,
    ) -> Result<WrappedMasterKey, DmKeyError> {
        let binding = self.selected(selection, OfficeKeyPurpose::WrapMaster)?;
        let keys = resolve_exact(binding, &mut read)?;
        wire::wrap(&keys, selection, master)
    }

    /// Opens only an exact expected envelope and configured unwrap purpose. All
    /// current source/member/revision/epoch rights remain the caller's obligation.
    pub fn unwrap_master(
        &self,
        selection: &DmKeySelection,
        wrapped: &WrappedMasterKey,
    ) -> Result<UnwrappedMasterKey, DmKeyError> {
        self.unwrap_with_reader(selection, wrapped, read_exact_env)
    }

    fn unwrap_with_reader(
        &self,
        selection: &DmKeySelection,
        wrapped: &WrappedMasterKey,
        mut read: impl FnMut(&str) -> Result<String, DmKeyError>,
    ) -> Result<UnwrappedMasterKey, DmKeyError> {
        wrapped.require_expected(selection)?;
        let binding = self.selected(selection, OfficeKeyPurpose::UnwrapMaster)?;
        let keys = resolve_exact(binding, &mut read)?;
        wire::unwrap(&keys, selection, wrapped)
    }
}

fn read_exact_env(name: &str) -> Result<String, DmKeyError> {
    std::env::var(name).map_err(|_| DmKeyError::Unavailable)
}

fn resolve_exact(
    binding: &DmOfficeKeyBinding,
    read: &mut impl FnMut(&str) -> Result<String, DmKeyError>,
) -> Result<Keys, DmKeyError> {
    // Even an injected resolver error is closed; do not retain parser sources.
    let secret =
        Zeroizing::new(read(&binding.signer.secret_env).map_err(|_| DmKeyError::Unavailable)?);
    if secret.len() != 64 {
        return Err(DmKeyError::KeyMismatch);
    }
    let secret =
        nostr::SecretKey::from_hex(secret.as_str()).map_err(|_| DmKeyError::KeyMismatch)?;
    let keys = Keys::new(secret);
    if keys.public_key().to_bytes() != *binding.signer.public_key.as_bytes() {
        return Err(DmKeyError::KeyMismatch);
    }
    Ok(keys)
}

#[cfg(test)]
mod tests;
