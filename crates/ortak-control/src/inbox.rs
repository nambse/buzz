//! Durable Office inbox handoff types.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::ids::{ClaimGeneration, MessageId};

/// NIP-29 stream chat message kind (`buzz_core::kind::KIND_STREAM_MESSAGE`).
pub const KIND_STREAM_MESSAGE: i32 = 9;
/// Stream chat message kind v2 (`buzz_core::kind::KIND_STREAM_MESSAGE_V2`).
pub const KIND_STREAM_MESSAGE_V2: i32 = 40002;
/// NIP-17 gift wrap kind (`buzz_core::kind::KIND_GIFT_WRAP`): accepted by
/// the Office ingress into the inbox, never routed or handed to a runtime
/// until trusted DM normalization exists.
pub const KIND_GIFT_WRAP: i32 = 1059;

/// Whether `kind` is a plaintext channel text kind the control plane may
/// normalize, route, and hand to a runtime.
///
/// This is the single definition shared by the channel normalizer and the
/// run-dispatch authority guard, so the two seams cannot drift apart: an
/// inbox row whose kind fails this predicate is never a run input.
pub fn is_supported_channel_kind(kind: i32) -> bool {
    kind == KIND_STREAM_MESSAGE || kind == KIND_STREAM_MESSAGE_V2
}

/// Accepted signed Office event facts copied into the inbox row.
///
/// The relay writes this in the same transaction as the signed `events` row.
/// The company is resolved server-side from the authenticated community
/// binding; the event carries no company identifier of its own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboxEvent {
    /// Signed event id.
    pub event_id: MessageId,
    /// Partition key of the `events` row.
    pub event_created_at: DateTime<Utc>,
    /// Nostr event kind.
    pub event_kind: i32,
    /// Author public key (32 raw bytes).
    pub author_pubkey: [u8; 32],
    /// Channel the event was posted to, when channel-scoped.
    pub channel_id: Option<Uuid>,
}

/// Result of an idempotent inbox insert.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboxInsertOutcome {
    /// A new inbox row was written.
    Inserted,
    /// The row already existed; the accepted event was replayed.
    AlreadyPresent,
}

/// Lifecycle state of an inbox row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboxState {
    /// Waiting for a routing worker.
    Pending,
    /// Held by a worker under a lease.
    Claimed,
    /// A routing decision was committed.
    Decided,
    /// Finalized without a decision because the event could not be normalized.
    Dropped,
    /// Retries exhausted; visible for operator inspection.
    Failed,
}

impl InboxState {
    /// Returns the snake_case column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Decided => "decided",
            Self::Dropped => "dropped",
            Self::Failed => "failed",
        }
    }

    /// Parses the column value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "claimed" => Some(Self::Claimed),
            "decided" => Some(Self::Decided),
            "dropped" => Some(Self::Dropped),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Durable inbox row as read by workers and operators.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboxRow {
    /// Accepted event facts.
    pub event: InboxEvent,
    /// Current lifecycle state.
    pub state: InboxState,
    /// Current claim fence value.
    pub claim_generation: ClaimGeneration,
    /// Worker holding or last holding the claim.
    pub claimed_by: Option<String>,
    /// Lease expiry of the current claim.
    pub claim_expires_at: Option<DateTime<Utc>>,
    /// Number of claims taken so far.
    pub attempt_count: i32,
    /// Earliest time the row may be claimed again.
    pub retry_after: Option<DateTime<Utc>>,
    /// Last recorded failure, if any.
    pub last_error: Option<String>,
    /// When the relay accepted the event.
    pub received_at: DateTime<Utc>,
    /// When the row reached a terminal state.
    pub finalized_at: Option<DateTime<Utc>>,
}

/// A claim handed to one routing worker.
///
/// The claim is bound to the company whose inbox row it was taken from, so a
/// caller cannot pair it with another company's [`crate::CompanyScope`] even
/// when message ids and claim generations coincide across companies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboxClaim {
    /// Company that owns the claimed inbox row.
    pub company_id: Uuid,
    /// Claimed message.
    pub message_id: MessageId,
    /// Generation the worker must present to finalize.
    pub claim_generation: ClaimGeneration,
    /// Worker identity recorded on the row.
    pub claimed_by: String,
    /// Lease expiry after which another worker may reclaim the row.
    pub claim_expires_at: DateTime<Utc>,
    /// Claims taken so far, including this one.
    pub attempt_count: i32,
    /// Accepted event facts.
    pub event: InboxEvent,
}

/// Outcome of releasing a claim for a later retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InboxReleaseOutcome {
    /// The row returned to `pending` with a retry time.
    Retrying,
    /// Retries were exhausted; the row is terminal `failed`.
    Failed,
    /// The claim generation no longer matched; nothing was changed.
    Stale,
}
