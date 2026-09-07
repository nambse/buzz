//! Opt-in NIP-59 verification for one human/employee pair. The decoder is I/O-free;
//! the separate key provider resolves only an explicitly selected Office entry.
//!
//! Cryptographic validity proves neither current membership nor permission to
//! resolve a key, persist plaintext or dispatch a run. The caller must supply an
//! explicitly purpose-authorized key and canonical source expectations, then
//! recheck current authority after decoding. No generic normalizer calls this.

use chrono::{DateTime, Utc};
use nostr::{EventId, Keys, PublicKey};
use zeroize::Zeroizing;

mod codec;
pub mod jobs;
pub mod key_provider;
pub mod publish;
mod wire;

/// Verified metadata format; not a retained authorization or snapshot format.
pub const VERIFIED_DM_FORMAT: &str = "ortak-verified-encrypted-dm/1";
/// Complete signed outer JSON ceiling, before parsing.
pub const MAX_OUTER_BYTES: usize = 64 * 1024;
/// Decrypted signed seal JSON ceiling, before parsing.
pub const MAX_SEAL_BYTES: usize = 32 * 1024;
/// Decrypted unsigned rumor JSON ceiling, before parsing.
pub const MAX_RUMOR_BYTES: usize = 16 * 1024;
/// UTF-8 message ceiling; no truncation is performed.
pub const MAX_TEXT_BYTES: usize = 8 * 1024;
/// Defensive tag allocation/validation ceiling; only p and optional e are valid.
pub const MAX_TAGS: usize = 16;
/// Aggregate UTF-8 bytes in tags.
pub const MAX_TAG_BYTES: usize = 2 * 1024;
/// UTF-8 bytes in a single tag value.
pub const MAX_TAG_VALUE_BYTES: usize = 256;

/// Closed failures without rejected content, identifiers or crypto error text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DecodeError {
    /// A serialized or encrypted input exceeds its stage's bound.
    #[error("encrypted DM input exceeds its bound")]
    Bounds,
    /// Invalid JSON, duplicate/unknown fields or invalid canonical identifiers.
    #[error("invalid encrypted DM encoding")]
    Encoding,
    /// Invalid or nonintegral PostgreSQL event partition timestamp.
    #[error("invalid encrypted DM timestamp")]
    Timestamp,
    /// Stored outer facts do not agree with the verified event.
    #[error("encrypted DM outer source changed")]
    SourceMismatch,
    /// The key does not match the explicitly selected recipient.
    #[error("encrypted DM key does not match its recipient")]
    KeyMismatch,
    /// A signed layer has the wrong kind.
    #[error("invalid encrypted DM event kind")]
    Kind,
    /// A signed layer's ID or signature is invalid.
    #[error("invalid encrypted DM signature or ID")]
    Signature,
    /// NIP-44 v2 decoding/authentication failed.
    #[error("encrypted DM decryption failed")]
    Decryption,
    /// Recipient is not the single exact selected employee.
    #[error("encrypted DM recipient mismatch")]
    Recipient,
    /// Seal and rumor sender do not equal the selected human.
    #[error("encrypted DM sender mismatch")]
    Sender,
    /// Tags are ambiguous, oversized or outside the narrow p/e grammar.
    #[error("invalid encrypted DM tags")]
    Tags,
    /// A present inner ID does not hash the actual rumor fields.
    #[error("invalid encrypted DM rumor ID")]
    RumorId,
    /// Empty, oversized or unsupported-control text.
    #[error("invalid encrypted DM text")]
    Text,
}

/// Borrowed key selected by an already-authorized caller for DM decryption.
///
/// Construction checks identity only. It does not establish purpose permission,
/// load credentials or discover keys. No Debug or serialization is implemented.
pub struct DmDecryptKey<'a> {
    keys: &'a Keys,
    recipient: PublicKey,
}

impl<'a> DmDecryptKey<'a> {
    /// Binds one caller-owned key to its exact expected recipient identity.
    pub fn for_recipient(keys: &'a Keys, recipient: PublicKey) -> Result<Self, DecodeError> {
        if keys.public_key() != recipient {
            return Err(DecodeError::KeyMismatch);
        }
        Ok(Self { keys, recipient })
    }
}

/// Canonical outer facts and selected pair, supplied by the future authority seam.
/// These values are expectations, not proof of a DB lookup or current ACL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpectedEnvelope {
    outer_id: EventId,
    outer_author: PublicKey,
    partition_at: DateTime<Utc>,
    human: PublicKey,
    recipient: PublicKey,
}

impl ExpectedEnvelope {
    /// Uses the actual stored event partition time, never the current clock.
    /// The first mode requires distinct human and employee public identities.
    pub fn new(
        outer_id: EventId,
        outer_author: PublicKey,
        partition_at: DateTime<Utc>,
        human: PublicKey,
        recipient: PublicKey,
    ) -> Result<Self, DecodeError> {
        wire::partition_seconds(partition_at)?;
        if human == recipient {
            return Err(DecodeError::Sender);
        }
        Ok(Self {
            outer_id,
            outer_author,
            partition_at,
            human,
            recipient,
        })
    }
}

/// Immutable cryptographic result. It grants no read, persistence or execution.
///
/// Original decrypted rumor bytes and text are held in zeroizing owned buffers.
/// No Clone, Debug or serialization implementation exposes them accidentally.
/// Library-internal transient crypto/JSON allocations are outside this wrapper;
/// this type makes no claim about process dumps, swap or durable confidentiality.
pub struct VerifiedDmRumor {
    source: ExpectedEnvelope,
    outer_hash: [u8; 32],
    seal_id: EventId,
    seal_created_at: DateTime<Utc>,
    rumor_id: EventId,
    rumor_created_at: DateTime<Utc>,
    rumor_hash: [u8; 32],
    reply_to: Option<EventId>,
    text: Zeroizing<String>,
    rumor_bytes: Zeroizing<Vec<u8>>,
}

impl VerifiedDmRumor {
    /// Verified outer ID, signer, partition and selected pair expectations.
    pub fn source(&self) -> &ExpectedEnvelope {
        &self.source
    }
    /// SHA-256 of the exact supplied signed outer JSON bytes.
    pub fn outer_hash(&self) -> &[u8; 32] {
        &self.outer_hash
    }
    /// Verified signed seal ID; the signer equals the selected human.
    pub fn seal_id(&self) -> EventId {
        self.seal_id
    }
    /// Original seal time, which may intentionally differ from the rumor time.
    pub fn seal_created_at(&self) -> DateTime<Utc> {
        self.seal_created_at
    }
    /// Verified canonical unsigned event ID for later deduplication.
    pub fn rumor_id(&self) -> EventId {
        self.rumor_id
    }
    /// Original rumor timestamp; no wall-clock freshness is implied.
    pub fn rumor_created_at(&self) -> DateTime<Utc> {
        self.rumor_created_at
    }
    /// SHA-256 of the exact decrypted rumor JSON bytes, not edited message text.
    pub fn rumor_hash(&self) -> &[u8; 32] {
        &self.rumor_hash
    }
    /// Syntactically valid reply claim; same-pair history must be checked later.
    pub fn reply_to(&self) -> Option<EventId> {
        self.reply_to
    }
    /// Decrypted text. Callers must not log or persist it through generic paths.
    pub fn text(&self) -> &str {
        self.text.as_str()
    }
    /// Original JSON for a future confidential freeze; never a plaintext store.
    pub fn rumor_bytes(&self) -> &[u8] {
        self.rumor_bytes.as_slice()
    }
}

impl ExpectedEnvelope {
    /// Exact stored outer event ID.
    pub fn outer_id(&self) -> EventId {
        self.outer_id
    }
    /// Verified transport signer, not the human author.
    pub fn outer_author(&self) -> PublicKey {
        self.outer_author
    }
    /// Exact PostgreSQL source partition timestamp.
    pub fn partition_at(&self) -> DateTime<Utc> {
        self.partition_at
    }
    /// Explicit selected human; membership is outside this codec.
    pub fn human(&self) -> PublicKey {
        self.human
    }
    /// Explicit selected employee key, independent of model/runtime identity.
    pub fn recipient(&self) -> PublicKey {
        self.recipient
    }
}

/// Verifies two NIP-44 v2 layers and their exact signed/unsigned provenance.
///
/// Pure synchronous work over bounded bytes; no database, network, clock or
/// credential lookup. The caller owns scheduling/deadline and must recheck
/// current source, participants, lifecycle, key purpose/version and claim before
/// any effect. A returned reply reference proves no ancestry or membership.
pub fn decode(
    key: &DmDecryptKey<'_>,
    expected: &ExpectedEnvelope,
    outer_bytes: &[u8],
) -> Result<VerifiedDmRumor, DecodeError> {
    codec::decode(key, expected, outer_bytes)
}

#[cfg(test)]
mod tests;
