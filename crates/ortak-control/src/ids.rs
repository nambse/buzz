use std::fmt;

use uuid::Uuid;

use crate::error::{ControlError, Result};

/// Server-resolved company boundary.
///
/// A scope can only be constructed by this crate after a lookup through the
/// authenticated community binding or the company registry; a client-supplied
/// company identifier can never become one. Every repository method takes a
/// scope, so no query is observable across companies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompanyScope {
    company_id: Uuid,
    community_id: Option<Uuid>,
}

impl CompanyScope {
    pub(crate) fn new(company_id: Uuid, community_id: Option<Uuid>) -> Self {
        Self {
            company_id,
            community_id,
        }
    }

    /// Returns the resolved company identifier.
    pub fn company_id(&self) -> Uuid {
        self.company_id
    }

    /// Returns the authenticated community this scope was resolved from, if any.
    pub fn community_id(&self) -> Option<Uuid> {
        self.community_id
    }
}

/// Raw 32-byte signed Office event identifier, matching `events.id`.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MessageId([u8; 32]);

impl MessageId {
    /// Wraps raw event id bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Parses a 64-character lowercase or uppercase hex event id.
    pub fn parse_hex(value: &str) -> Result<Self> {
        let decoded = hex::decode(value)
            .map_err(|_| ControlError::InvalidData(format!("message id is not hex: {value}")))?;
        Self::try_from_slice(&decoded)
    }

    /// Converts a database `BYTEA` value into a message id.
    pub fn try_from_slice(bytes: &[u8]) -> Result<Self> {
        <[u8; 32]>::try_from(bytes)
            .map(Self)
            .map_err(|_| ControlError::InvalidData("message id must be 32 bytes".to_owned()))
    }

    /// Returns the raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the lowercase hex form used by domain envelopes and audit rows.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for MessageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "MessageId({})", self.to_hex())
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

/// Monotonic Office inbox claim fence.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClaimGeneration(pub i64);

impl fmt::Display for ClaimGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}
