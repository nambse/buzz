//! Pure D4 conversation audience identities and provenance fingerprints.
//!
//! These values describe a claimed, immutable identity. They do not prove a
//! canonical Office lookup, thread ancestry, human approval, current ACL, epoch,
//! or permission to publish/recall. A future server resolver must supply those
//! proofs. No existing `MemoryScope`, project fact, or runtime gains a new path.

use chrono::{DateTime, Utc};
use ortak_domain::EmployeeId;
use uuid::Uuid;

use crate::MessageId;

mod wire;

/// Exact format hashed for the audience, independently of its source message.
pub const CONVERSATION_AUDIENCE_FORMAT_V1: &str = "ortak-reviewed-conversation-audience/1";
/// Exact format of the retained audience plus source provenance value.
pub const CONVERSATION_PROVENANCE_FORMAT_V1: &str = "ortak-reviewed-conversation-provenance/1";
/// Domain separator for new conversation facts' opaque Honcho source hash.
pub const CONVERSATION_SOURCE_FORMAT_V1: &str = "ortak-reviewed-conversation-source/1";
/// Maximum accepted canonical audience bytes, checked before JSON parsing.
pub const MAX_CONVERSATION_AUDIENCE_BYTES: usize = 2048;
/// Maximum accepted canonical provenance bytes, checked before JSON parsing.
pub const MAX_CONVERSATION_PROVENANCE_BYTES: usize = 4096;

/// Closed, input-free validation failures; no source content is retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ConversationMemoryError {
    /// A required company, community, project or channel UUID was nil.
    #[error("invalid conversation memory identity")]
    InvalidIdentity,
    /// A timestamp is outside the supported range or loses PostgreSQL precision.
    #[error("invalid conversation memory timestamp")]
    InvalidTimestamp,
    /// A hash is not exactly 32 lowercase hexadecimal bytes.
    #[error("invalid conversation memory digest")]
    InvalidDigest,
    /// The version, fields, size or exact canonical encoding is unsupported.
    #[error("invalid conversation memory wire value")]
    InvalidWire,
    /// Derived hashes or an identical root/source event's partition disagree.
    #[error("inconsistent conversation memory provenance")]
    InconsistentProvenance,
}

/// SHA-256 identity, with a strict lowercase hexadecimal wire form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversationMemoryDigest([u8; 32]);

impl ConversationMemoryDigest {
    /// Wraps an already computed digest, without claiming what its input proves.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Parses exactly 64 lowercase hexadecimal characters.
    pub fn parse_hex(value: &str) -> Result<Self, ConversationMemoryError> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ConversationMemoryError::InvalidDigest);
        }
        let mut bytes = [0; 32];
        hex::decode_to_slice(value, &mut bytes)
            .map_err(|_| ConversationMemoryError::InvalidDigest)?;
        Ok(Self(bytes))
    }

    /// Returns the immutable raw digest.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the canonical lowercase hexadecimal digest.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

/// Exact event identity including its PostgreSQL partition timestamp.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationEventIdentity {
    event_id: MessageId,
    created_at: DateTime<Utc>,
}

impl ConversationEventIdentity {
    /// Checks lossless microsecond precision and a UTC year in 1970..=9999.
    /// Existence, signature, channel, source visibility and ancestry are external.
    pub fn new(
        event_id: MessageId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, ConversationMemoryError> {
        wire::timestamp(created_at)?;
        Ok(Self {
            event_id,
            created_at,
        })
    }

    /// Signed event identifier; never the delivery-chain root by inference.
    pub fn event_id(&self) -> MessageId {
        self.event_id
    }

    /// Exact canonical event partition timestamp.
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

/// Closed audience kinds; there is deliberately no project/DM/default variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationAudienceKind {
    /// Only the explicit channel, still within the project/employee identity.
    Channel,
    /// Only the exact canonical thread root and partition in that channel.
    Thread,
}

/// Immutable conversation audience; construction does not grant memory access.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationAudienceV1 {
    company_id: Uuid,
    community_id: Uuid,
    project_id: Uuid,
    employee_id: EmployeeId,
    channel_id: Uuid,
    thread_root: Option<ConversationEventIdentity>,
}

impl ConversationAudienceV1 {
    /// Constructs an explicitly channel-wide identity. No implicit fallback is used.
    pub fn channel(
        company_id: Uuid,
        community_id: Uuid,
        project_id: Uuid,
        employee_id: EmployeeId,
        channel_id: Uuid,
    ) -> Result<Self, ConversationMemoryError> {
        if [company_id, community_id, project_id, channel_id]
            .iter()
            .any(Uuid::is_nil)
        {
            return Err(ConversationMemoryError::InvalidIdentity);
        }
        Ok(Self {
            company_id,
            community_id,
            project_id,
            employee_id,
            channel_id,
            thread_root: None,
        })
    }

    /// Constructs a thread identity from an explicit, complete root locator.
    /// A future canonical resolver must prove its channel and bounded ancestry.
    pub fn thread(
        company_id: Uuid,
        community_id: Uuid,
        project_id: Uuid,
        employee_id: EmployeeId,
        channel_id: Uuid,
        thread_root: ConversationEventIdentity,
    ) -> Result<Self, ConversationMemoryError> {
        let mut value = Self::channel(
            company_id,
            community_id,
            project_id,
            employee_id,
            channel_id,
        )?;
        value.thread_root = Some(thread_root);
        Ok(value)
    }

    /// Owning company, not a resolved `CompanyScope` authorization value.
    pub fn company_id(&self) -> Uuid {
        self.company_id
    }

    /// Office community identity.
    pub fn community_id(&self) -> Uuid {
        self.community_id
    }

    /// Existing project storage boundary.
    pub fn project_id(&self) -> Uuid {
        self.project_id
    }

    /// Sole employee audience; model and runtime revision are intentionally absent.
    pub fn employee_id(&self) -> &EmployeeId {
        &self.employee_id
    }

    /// Explicit canonical channel identity.
    pub fn channel_id(&self) -> Uuid {
        self.channel_id
    }

    /// Closed kind derived from a complete root locator.
    pub fn kind(&self) -> ConversationAudienceKind {
        if self.thread_root.is_some() {
            ConversationAudienceKind::Thread
        } else {
            ConversationAudienceKind::Channel
        }
    }

    /// Exact thread root, absent only for explicit channel audiences.
    pub fn thread_root(&self) -> Option<&ConversationEventIdentity> {
        self.thread_root.as_ref()
    }

    /// Fixed sorted-key UTF-8 JSON, including explicit null channel root fields.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ConversationMemoryError> {
        wire::audience_bytes(self)
    }

    /// SHA-256 of the audience only; source messages do not change this identity.
    pub fn audience_hash(&self) -> Result<ConversationMemoryDigest, ConversationMemoryError> {
        Ok(wire::digest(&self.canonical_bytes()?))
    }

    /// Accepts only the bounded exact v1 encoding, never normalizing ambiguous input.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, ConversationMemoryError> {
        wire::parse_audience(bytes)
    }
}

/// Source evidence retained separately from the reusable conversation audience.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationProvenanceV1 {
    audience: ConversationAudienceV1,
    source: ConversationEventIdentity,
    source_evidence_hash: ConversationMemoryDigest,
}

impl ConversationProvenanceV1 {
    /// Binds a source locator and its precomputed evidence hash to one audience.
    /// The future resolver must prove that the evidence actually covers this
    /// source and its canonical fields. Neither a hash nor this constructor is
    /// approval, visibility, thread-membership or current-use authority.
    pub fn new(
        audience: ConversationAudienceV1,
        source: ConversationEventIdentity,
        source_evidence_hash: ConversationMemoryDigest,
    ) -> Result<Self, ConversationMemoryError> {
        if audience.thread_root().is_some_and(|root| {
            root.event_id == source.event_id && root.created_at != source.created_at
        }) {
            return Err(ConversationMemoryError::InconsistentProvenance);
        }
        Ok(Self {
            audience,
            source,
            source_evidence_hash,
        })
    }

    /// Complete reusable audience identity.
    pub fn audience(&self) -> &ConversationAudienceV1 {
        &self.audience
    }

    /// Exact source locator, independent of a routing/delivery-chain root.
    pub fn source(&self) -> &ConversationEventIdentity {
        &self.source
    }

    /// Original evidence digest, preserved without reinterpretation.
    pub fn source_evidence_hash(&self) -> ConversationMemoryDigest {
        self.source_evidence_hash
    }

    /// New conversation-only, domain-separated audience/evidence binding digest.
    /// This must never replace an existing project fact's source hash.
    pub fn source_hash(&self) -> Result<ConversationMemoryDigest, ConversationMemoryError> {
        wire::source_hash(self)
    }

    /// Canonical retained provenance including both recomputable hash witnesses.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ConversationMemoryError> {
        wire::provenance_bytes(self)
    }

    /// Refuses unsupported shapes, encodings, versions and forged hash witnesses.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, ConversationMemoryError> {
        wire::parse_provenance(bytes)
    }
}

#[cfg(test)]
mod tests;
