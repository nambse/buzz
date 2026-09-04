//! Transactional dispatch/delivery outbox types.

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Outbox work kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxKind {
    /// Start a run for one decision recipient.
    RunDispatch,
    /// Publish a frozen signed Office event for a run.
    OfficePublish,
}

impl OutboxKind {
    /// Returns the column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunDispatch => "run_dispatch",
            Self::OfficePublish => "office_publish",
        }
    }

    /// Parses the column value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "run_dispatch" => Some(Self::RunDispatch),
            "office_publish" => Some(Self::OfficePublish),
            _ => None,
        }
    }
}

/// Outbox row lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxState {
    /// Waiting for delivery or retry.
    Pending,
    /// Delivered exactly once.
    Delivered,
    /// Retries exhausted; an operator may reopen it.
    Failed,
}

impl OutboxState {
    /// Parses the column value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "delivered" => Some(Self::Delivered),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// A leased outbox row. Completion and failure must present the lease token.
#[derive(Clone, Debug, PartialEq)]
pub struct OutboxLease {
    /// Outbox row id.
    pub id: Uuid,
    /// Work kind.
    pub kind: OutboxKind,
    /// Company-unique idempotency key.
    pub dedup_key: String,
    /// Decision that produced a run dispatch.
    pub routing_decision_id: Option<Uuid>,
    /// Recipient employee for a run dispatch.
    pub employee_id: Option<String>,
    /// Run for an Office publish.
    pub run_id: Option<Uuid>,
    /// Kind-specific payload.
    pub payload: serde_json::Value,
    /// Attempts including this lease.
    pub attempt_count: i32,
    /// Maximum attempts before terminal failure.
    pub max_attempts: i16,
    /// Per-claim fence token.
    pub lease_token: Uuid,
    /// Lease expiry.
    pub lease_expires_at: DateTime<Utc>,
}

/// Outcome of recording a delivery failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxFailOutcome {
    /// The row is pending again with a retry time.
    Retrying,
    /// Attempts were exhausted; the row is terminal `failed`.
    Terminal,
    /// The lease token no longer matched; nothing was changed.
    Stale,
}

/// Summary of one run-dispatch outbox row written by a routing commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchTicket {
    /// Outbox row id.
    pub outbox_id: Uuid,
    /// Recipient employee.
    pub employee_id: String,
    /// Company-unique idempotency key.
    pub dedup_key: String,
}
