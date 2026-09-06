//! Pure bounded confidential payload claims, without any effect authority.
//!
//! Parsing verifies only shape and canonical bytes. It does not establish a
//! canonical source, membership, key purpose, current epoch or permission.
//! There is deliberately no `Authorized` constructor or ordinary RunSpec wire.

mod wire;

use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

/// Maximum identity or authenticated header size.
pub const MAX_HEADER_BYTES: usize = 2 * 1024;
/// Maximum complete serialized envelope, checked before parsing.
pub const MAX_ENVELOPE_BYTES: usize = 96 * 1024;

/// A bounded error containing no input, plaintext, key or nested parser error.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ConfidentialWireError {
    /// A length or ordinal exceeds the versioned budget.
    #[error("confidential payload bound exceeded")]
    Bound,
    /// The input is not a strict canonical version-1 object.
    #[error("invalid confidential payload encoding")]
    Encoding,
    /// A claim is structurally invalid; this does not inspect current authority.
    #[error("invalid confidential identity claim")]
    Identity,
    /// The envelope differs from the caller's independently expected claims.
    #[error("confidential payload expectation mismatch")]
    Expectation,
}

/// Closed domain separation for keys and payload budgets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayloadPurpose {
    /// One frozen input, ordinal zero.
    Snapshot,
    /// An ordered runtime event, ordinals one through 512.
    RuntimeEvent,
    /// One assembled reply draft, ordinal zero.
    ReplyDraft,
}

impl PayloadPurpose {
    /// Exact ASCII HKDF/wire purpose string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::RuntimeEvent => "runtime_event",
            Self::ReplyDraft => "reply_draft",
        }
    }

    /// Maximum serialized plaintext size, independent of future inner schemas.
    pub fn max_plaintext_bytes(self) -> usize {
        match self {
            Self::Snapshot => 48 * 1024,
            Self::RuntimeEvent => 32 * 1024,
            Self::ReplyDraft => 16 * 1024,
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, ConfidentialWireError> {
        match value {
            "snapshot" => Ok(Self::Snapshot),
            "runtime_event" => Ok(Self::RuntimeEvent),
            "reply_draft" => Ok(Self::ReplyDraft),
            _ => Err(ConfidentialWireError::Encoding),
        }
    }

    fn validate(self, ordinal: u32, bytes: usize) -> Result<(), ConfidentialWireError> {
        let valid = match self {
            Self::Snapshot | Self::ReplyDraft => ordinal == 0,
            Self::RuntimeEvent => (1..=512).contains(&ordinal),
        };
        if !valid || bytes > self.max_plaintext_bytes() {
            return Err(ConfidentialWireError::Bound);
        }
        Ok(())
    }
}

/// Canonically encoded identity **claims**, never current source/ACL evidence.
/// No Debug or implicit serialization is provided for private metadata.
#[derive(Clone, Eq, PartialEq)]
pub struct ValidatedIdentity {
    wire: wire::IdentityWire,
    bytes: Vec<u8>,
}

/// Borrowed identity claims needed for exact Office-key selection. These do not
/// establish a current revision, lifecycle, binding or key-purpose permission.
pub struct ConfidentialKeyClaims<'a> {
    /// Canonical company UUID claim.
    pub company_id: &'a str,
    /// Durable employee claim, independent of model/session.
    pub employee_id: &'a str,
    /// Pinned run revision; it does not select a different Office key.
    pub employee_revision_id: &'a str,
    /// Pinned lifecycle epoch, authenticated as part of the full identity.
    pub employee_lifecycle_epoch: &'a str,
    /// Exact retained Office binding UUID claim.
    pub office_binding_id: &'a str,
    /// Exact expected employee Office public key, lowercase hex.
    pub employee_public_key: &'a str,
    /// The per-run master-key UUID, not an Office credential reference.
    pub key_id: &'a str,
    /// Canonical Office-key version claim.
    pub key_version: &'a str,
}

impl ValidatedIdentity {
    /// Borrows validated claims for an explicitly configured key provider.
    /// The full canonical identity must still be authenticated on wrap/open.
    pub fn key_claims(&self) -> ConfidentialKeyClaims<'_> {
        self.wire.key_claims()
    }

    /// Parses strict canonical claim bytes without granting any authority.
    pub fn parse(bytes: &[u8]) -> Result<Self, ConfidentialWireError> {
        let value = Self::from_wire(wire::parse(bytes, MAX_HEADER_BYTES)?)?;
        if value.bytes != bytes {
            return Err(ConfidentialWireError::Encoding);
        }
        Ok(value)
    }

    fn from_wire(wire: wire::IdentityWire) -> Result<Self, ConfidentialWireError> {
        wire.validate()?;
        let bytes = wire::encode(&wire, MAX_HEADER_BYTES)?;
        Ok(Self { wire, bytes })
    }

    /// Exact canonical identity bytes, for an explicitly gated caller only.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// SHA256 used as HKDF salt; hashing does not make claims authoritative.
    pub fn sha256(&self) -> [u8; 32] {
        Sha256::digest(&self.bytes).into()
    }
}

/// Immutable versioned AAD and its structurally validated claims.
#[derive(Clone)]
pub struct PayloadHeader {
    identity: ValidatedIdentity,
    purpose: PayloadPurpose,
    ordinal: u32,
    plaintext_bytes: usize,
    bytes: Vec<u8>,
}

impl PayloadHeader {
    /// Builds bounded AAD from claims; the caller still owns authorization.
    pub fn new(
        identity: &ValidatedIdentity,
        purpose: PayloadPurpose,
        ordinal: u32,
        plaintext_bytes: usize,
    ) -> Result<Self, ConfidentialWireError> {
        purpose.validate(ordinal, plaintext_bytes)?;
        let wire = wire::HeaderWire {
            algorithm: "A256GCM".into(),
            format: "ortak-confidential-payload/1".into(),
            identity: identity.wire.clone(),
            ordinal,
            plaintext_bytes,
            purpose: purpose.as_str().into(),
        };
        let bytes = wire::encode(&wire, MAX_HEADER_BYTES)?;
        Ok(Self {
            identity: identity.clone(),
            purpose,
            ordinal,
            plaintext_bytes,
            bytes,
        })
    }

    fn from_wire(wire: wire::HeaderWire) -> Result<Self, ConfidentialWireError> {
        if wire.algorithm != "A256GCM" || wire.format != "ortak-confidential-payload/1" {
            return Err(ConfidentialWireError::Encoding);
        }
        Self::new(
            &ValidatedIdentity::from_wire(wire.identity)?,
            PayloadPurpose::parse(&wire.purpose)?,
            wire.ordinal,
            wire.plaintext_bytes,
        )
    }

    /// Exact bytes authenticated by AES-GCM.
    pub fn aad(&self) -> &[u8] {
        &self.bytes
    }
    /// Validated identity claims, not a current authority witness.
    pub fn identity(&self) -> &ValidatedIdentity {
        &self.identity
    }
    /// Closed purpose included in both AAD and key derivation.
    pub fn purpose(&self) -> PayloadPurpose {
        self.purpose
    }
    /// Immutable purpose-relative record ordinal.
    pub fn ordinal(&self) -> u32 {
        self.ordinal
    }
    /// Exact authenticated plaintext length.
    pub fn plaintext_bytes(&self) -> usize {
        self.plaintext_bytes
    }

    /// Refuses substitutions before a caller attempts authenticated opening.
    pub fn require_expected(
        &self,
        identity: &ValidatedIdentity,
        purpose: PayloadPurpose,
        ordinal: u32,
    ) -> Result<(), ConfidentialWireError> {
        purpose.validate(ordinal, self.plaintext_bytes)?;
        if &self.identity != identity || self.purpose != purpose || self.ordinal != ordinal {
            return Err(ConfidentialWireError::Expectation);
        }
        Ok(())
    }
}

/// Canonical ciphertext container. Construction/parsing does not verify its tag.
pub struct ConfidentialEnvelope {
    header: PayloadHeader,
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
    bytes: Vec<u8>,
}

impl ConfidentialEnvelope {
    /// Validates lengths and serializes an already produced ciphertext/tag.
    pub fn from_parts(
        header: PayloadHeader,
        nonce: [u8; 12],
        ciphertext: Vec<u8>,
    ) -> Result<Self, ConfidentialWireError> {
        if ciphertext.len() != header.plaintext_bytes + 16 {
            return Err(ConfidentialWireError::Bound);
        }
        let wire = wire::EnvelopeWire {
            ciphertext: STANDARD.encode(&ciphertext),
            header: wire::parse(header.aad(), MAX_HEADER_BYTES)?,
            nonce: STANDARD.encode(nonce),
        };
        let bytes = wire::encode(&wire, MAX_ENVELOPE_BYTES)?;
        Ok(Self {
            header,
            nonce,
            ciphertext,
            bytes,
        })
    }

    /// Checks total size before JSON parsing and encoded lengths before base64.
    pub fn parse(bytes: &[u8]) -> Result<Self, ConfidentialWireError> {
        let wire: wire::EnvelopeWire = wire::parse(bytes, MAX_ENVELOPE_BYTES)?;
        let header = PayloadHeader::from_wire(wire.header)?;
        let expected = header.plaintext_bytes + 16;
        if wire.ciphertext.len() != expected.div_ceil(3) * 4 || wire.nonce.len() != 16 {
            return Err(ConfidentialWireError::Bound);
        }
        let ciphertext = STANDARD
            .decode(&wire.ciphertext)
            .map_err(|_| ConfidentialWireError::Encoding)?;
        let nonce: [u8; 12] = STANDARD
            .decode(&wire.nonce)
            .map_err(|_| ConfidentialWireError::Encoding)?
            .try_into()
            .map_err(|_| ConfidentialWireError::Encoding)?;
        let value = Self::from_parts(header, nonce, ciphertext)?;
        if value.bytes != bytes {
            return Err(ConfidentialWireError::Encoding);
        }
        Ok(value)
    }

    /// Exact serialized bytes for eventual atomic persistence/replay.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Structurally validated authenticated header.
    pub fn header(&self) -> &PayloadHeader {
        &self.header
    }
    /// Public random 96-bit nonce.
    pub fn nonce(&self) -> &[u8; 12] {
        &self.nonce
    }
    /// Ciphertext followed by the full 16-byte authentication tag.
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }
}

#[cfg(test)]
mod tests;
