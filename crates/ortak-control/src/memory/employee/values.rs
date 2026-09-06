use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{wire, EmployeeMemoryError};
use crate::{office_identity::OfficePublicKey, MessageId};

/// Opaque SHA-256 value with strict lowercase hexadecimal serialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmployeeMemoryDigest([u8; 32]);

impl EmployeeMemoryDigest {
    /// Wraps bytes without claiming what was hashed or who authorized it.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Parses exactly 64 lowercase ASCII hexadecimal characters.
    pub fn parse_hex(value: &str) -> Result<Self, EmployeeMemoryError> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(EmployeeMemoryError::InvalidDigest);
        }
        let mut bytes = [0; 32];
        hex::decode_to_slice(value, &mut bytes).map_err(|_| EmployeeMemoryError::InvalidDigest)?;
        Ok(Self(bytes))
    }

    /// Raw digest bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Canonical lowercase hexadecimal bytes.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

/// Exact Office source claim. No source text is serialized or retained here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployeeMemorySourceV1 {
    community_id: Uuid,
    channel_id: Uuid,
    event_id: MessageId,
    event_created_at: DateTime<Utc>,
    author_public_key: OfficePublicKey,
    evidence_hash: EmployeeMemoryDigest,
}

impl EmployeeMemorySourceV1 {
    /// Checks locator shape only. Canonical events/inbox agreement, plaintext
    /// eligibility, signature, visibility and evidence hashing are server work.
    pub fn new(
        community_id: Uuid,
        channel_id: Uuid,
        event_id: MessageId,
        event_created_at: DateTime<Utc>,
        author_public_key: OfficePublicKey,
        evidence_hash: EmployeeMemoryDigest,
    ) -> Result<Self, EmployeeMemoryError> {
        if community_id.is_nil() || channel_id.is_nil() {
            return Err(EmployeeMemoryError::InvalidIdentity);
        }
        wire::timestamp(event_created_at)?;
        Ok(Self {
            community_id,
            channel_id,
            event_id,
            event_created_at,
            author_public_key,
            evidence_hash,
        })
    }

    /// Original Office community.
    pub fn community_id(&self) -> Uuid {
        self.community_id
    }

    /// Original source channel, which may differ from the approved destination.
    pub fn channel_id(&self) -> Uuid {
        self.channel_id
    }

    /// Exact signed event ID, never a routing root or caller-supplied text label.
    pub fn event_id(&self) -> MessageId {
        self.event_id
    }

    /// Exact PostgreSQL event partition timestamp.
    pub fn event_created_at(&self) -> DateTime<Utc> {
        self.event_created_at
    }

    /// Claimed original author; equality with canonical storage is external.
    pub fn author_public_key(&self) -> OfficePublicKey {
        self.author_public_key
    }

    /// Digest of server-resolved source evidence, not of edited fact text.
    pub fn evidence_hash(&self) -> EmployeeMemoryDigest {
        self.evidence_hash
    }
}

/// Explicit sharing approval claim, never an employee-private retention grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmployeeSharingApprovalV1 {
    approval_id: Uuid,
    approved_by: OfficePublicKey,
    content_hash: EmployeeMemoryDigest,
    expires_at: DateTime<Utc>,
}

impl EmployeeSharingApprovalV1 {
    /// Checks durable identity and timestamp shape only. The source-review and
    /// destination-sharing grants, exact edited content, expiry window and
    /// committed review receipt must be verified by the authenticated facade.
    pub fn new(
        approval_id: Uuid,
        approved_by: OfficePublicKey,
        content_hash: EmployeeMemoryDigest,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, EmployeeMemoryError> {
        if approval_id.is_nil() {
            return Err(EmployeeMemoryError::InvalidIdentity);
        }
        wire::timestamp(expires_at)?;
        Ok(Self {
            approval_id,
            approved_by,
            content_hash,
            expires_at,
        })
    }

    /// Original explicit sharing-review operation identity.
    pub fn approval_id(&self) -> Uuid {
        self.approval_id
    }

    /// Approving human claim; a facade must bind it to the signed actor.
    pub fn approved_by(&self) -> OfficePublicKey {
        self.approved_by
    }

    /// SHA-256 of the exact human-edited UTF-8 fact bytes.
    pub fn content_hash(&self) -> EmployeeMemoryDigest {
        self.content_hash
    }

    /// Immutable declared expiry; parsing does not compare against wall time.
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
}
