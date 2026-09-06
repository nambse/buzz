//! Isolated confidential payload cryptography, with no resolver or I/O except
//! OS nonce generation. The caller owns the key and all current authorization.
//!
//! No worker/adapter/storage route calls this module. Opening verifies an AEAD
//! and exact expected claims, not current source, membership or effect rights.
//! Persist the returned envelope atomically before exposure and replay those
//! same bytes; never seal again as an idempotency/retry implementation.

use aes_gcm::{
    Aes256Gcm, Nonce, Tag,
    aead::{AeadInPlace, KeyInit},
};
use hkdf::Hkdf;
use ortak_control::confidential::{
    ConfidentialEnvelope, ConfidentialWireError, PayloadHeader, PayloadPurpose, ValidatedIdentity,
};
use sha2::Sha256;
use zeroize::Zeroizing;

mod transport;
pub use transport::{ConfidentialStartBody, prepare_start_body};

/// Closed failures deliberately discard library/parser error details.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ConfidentialCryptoError {
    /// The immutable wire or caller's expectation is invalid.
    #[error("invalid confidential payload claims")]
    Wire(#[from] ConfidentialWireError),
    /// The OS could not supply a fresh nonce; no fallback exists.
    #[error("confidential nonce unavailable")]
    Entropy,
    /// A library operation failed; contains no key, payload or backend exception.
    #[error("confidential cryptography unavailable")]
    Crypto,
    /// The exact authenticated ciphertext could not be opened.
    #[error("confidential authentication failed")]
    Authentication,
}

/// Explicit caller-owned per-run master material; no Debug, Clone or Serde.
/// The caller must erase any original copy and establish key-purpose authority.
pub struct ConfidentialMasterKey(Zeroizing<[u8; 32]>);

impl ConfidentialMasterKey {
    /// Takes ownership of exactly 32 bytes already selected by the caller.
    /// This does not resolve, generate, persist or authorize a key.
    pub fn from_owned(bytes: Zeroizing<[u8; 32]>) -> Self {
        Self(bytes)
    }
}

/// Volatile authenticated bytes, deliberately without Debug, Clone or Serde.
/// The future caller must validate its inner schema/identity before use.
pub struct OpenedConfidentialPayload(Zeroizing<Vec<u8>>);

impl OpenedConfidentialPayload {
    /// Borrow the plaintext only within a separately authorized operation.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

fn hkdf_into(
    ikm: &[u8],
    salt: &[u8],
    info: &[u8],
    output: &mut [u8],
) -> Result<(), ConfidentialCryptoError> {
    Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(info, output)
        .map_err(|_| ConfidentialCryptoError::Crypto)
}

fn derive(
    master: &ConfidentialMasterKey,
    identity: &ValidatedIdentity,
    purpose: PayloadPurpose,
) -> Result<Zeroizing<[u8; 32]>, ConfidentialCryptoError> {
    let mut output = Zeroizing::new([0u8; 32]);
    let mut info = b"ortak-confidential-dm-aead/1\0".to_vec();
    info.extend_from_slice(purpose.as_str().as_bytes());
    hkdf_into(
        master.0.as_ref(),
        &identity.sha256(),
        &info,
        output.as_mut(),
    )?;
    Ok(output)
}

fn encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, ConfidentialCryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| ConfidentialCryptoError::Crypto)?;
    // An unsuccessful operation cannot drop a normal Vec containing plaintext.
    let mut buffer = Zeroizing::new(plaintext.to_vec());
    let tag = cipher
        .encrypt_in_place_detached(Nonce::from_slice(nonce), aad, buffer.as_mut_slice())
        .map_err(|_| ConfidentialCryptoError::Crypto)?;
    let mut ciphertext = buffer.to_vec();
    ciphertext.extend_from_slice(&tag);
    Ok(ciphertext)
}

// Private deterministic seam for the shared vector. Production callers cannot
// inject/replay a nonce; the sole public seal function always draws fresh entropy.
fn seal_inner(
    master: &ConfidentialMasterKey,
    header: PayloadHeader,
    plaintext: &[u8],
    nonce: [u8; 12],
) -> Result<ConfidentialEnvelope, ConfidentialCryptoError> {
    if plaintext.len() != header.plaintext_bytes() {
        return Err(ConfidentialWireError::Bound.into());
    }
    let key = derive(master, header.identity(), header.purpose())?;
    let ciphertext = encrypt(&key, &nonce, header.aad(), plaintext)?;
    Ok(ConfidentialEnvelope::from_parts(header, nonce, ciphertext)?)
}

/// Protects one new bounded payload with an OS-generated nonce.
/// It does not authorize or persist a record, or guarantee durable nonce/ordinal
/// uniqueness: the later storage transaction must enforce both before exposure.
pub fn seal(
    master: &ConfidentialMasterKey,
    identity: &ValidatedIdentity,
    purpose: PayloadPurpose,
    ordinal: u32,
    plaintext: &[u8],
) -> Result<ConfidentialEnvelope, ConfidentialCryptoError> {
    let header = PayloadHeader::new(identity, purpose, ordinal, plaintext.len())?;
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|_| ConfidentialCryptoError::Entropy)?;
    seal_inner(master, header, plaintext, nonce)
}

/// Opens an envelope only for the caller's independently expected full identity,
/// purpose and ordinal. Authentication failure returns no partial plaintext.
pub fn open(
    master: &ConfidentialMasterKey,
    identity: &ValidatedIdentity,
    purpose: PayloadPurpose,
    ordinal: u32,
    envelope: &ConfidentialEnvelope,
) -> Result<OpenedConfidentialPayload, ConfidentialCryptoError> {
    envelope
        .header()
        .require_expected(identity, purpose, ordinal)?;
    let key = derive(master, identity, purpose)?;
    let cipher =
        Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| ConfidentialCryptoError::Crypto)?;
    let body_len = envelope.header().plaintext_bytes();
    let (ciphertext, tag) = envelope.ciphertext().split_at(body_len);
    let mut buffer = Zeroizing::new(ciphertext.to_vec());
    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(envelope.nonce()),
            envelope.header().aad(),
            buffer.as_mut_slice(),
            Tag::from_slice(tag),
        )
        .map_err(|_| ConfidentialCryptoError::Authentication)?;
    Ok(OpenedConfidentialPayload(buffer))
}

#[cfg(test)]
mod tests;
