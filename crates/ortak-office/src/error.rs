use ortak_control::outbox::OutboxKind;
use ortak_control::{ControlError, MessageId};
use thiserror::Error;
use uuid::Uuid;

use crate::event::OfficeEventError;
use crate::publisher::OfficePublishError;
use crate::signer::OfficeSignerError;

/// Failures of the Office delivery slice.
#[derive(Debug, Error)]
pub enum OfficeDeliveryError {
    /// An event violates bounds or verification.
    #[error(transparent)]
    Event(#[from] OfficeEventError),

    /// The signer failed or its output failed verification.
    #[error(transparent)]
    Signer(#[from] OfficeSignerError),

    /// The Office publish failed.
    #[error(transparent)]
    Publish(#[from] OfficePublishError),

    /// The control plane or database failed.
    #[error(transparent)]
    Control(#[from] ControlError),

    /// The request or event names a company other than the resolved scope.
    #[error("company {found} does not match the resolved scope {expected}")]
    CompanyMismatch {
        /// Scope company.
        expected: Uuid,
        /// Company on the request or event.
        found: Uuid,
    },

    /// The lease or row is not an `office_publish` row.
    #[error("outbox row is {} work, not office_publish", .found.as_str())]
    WrongKind {
        /// Kind found.
        found: OutboxKind,
    },

    /// The lease or row belongs to a different run than the request or event.
    #[error("outbox row run {found:?} does not match run {expected}")]
    WrongRun {
        /// Run on the request or event.
        expected: Uuid,
        /// Run on the lease or row.
        found: Option<Uuid>,
    },

    /// The authorized publish was issued for a different outbox row.
    #[error("authorized publish is bound to outbox row {expected}, not {found}")]
    WrongRow {
        /// Row the authorized publish was issued for.
        expected: Uuid,
        /// Row on the lease.
        found: Uuid,
    },

    /// The row pinned a different intent, employee, revision, or key.
    #[error("outbox row {outbox_id} pins a different intent than the one presented")]
    IntentMismatch {
        /// Outbox row.
        outbox_id: Uuid,
    },

    /// No run with this id exists in the company scope.
    #[error("run {run_id} does not exist in this company")]
    UnknownRun {
        /// Run.
        run_id: Uuid,
    },

    /// The run is not in a state that may publish: it must be `completed`
    /// with a `reply` or `channel` delivery intent.
    #[error(
        "run {run_id} is not publishable: status {status:?}, delivery intent {delivery_intent:?}"
    )]
    RunNotPublishable {
        /// Run.
        run_id: Uuid,
        /// `runs.status`.
        status: String,
        /// `runs.delivery_intent`.
        delivery_intent: Option<String>,
    },

    /// The revision the run pins has no usable Office binding.
    #[error(
        "employee {employee_id} revision {employee_revision_id} has no usable office binding: {reason}"
    )]
    BindingUnauthorized {
        /// Employee from the run row.
        employee_id: String,
        /// Revision from the run row.
        employee_revision_id: Uuid,
        /// Why the binding was refused.
        reason: BindingRejection,
    },

    /// The row already holds different frozen bytes; they were left untouched.
    #[error("outbox row {outbox_id} already holds a different frozen signed event")]
    FrozenPayloadConflict {
        /// Outbox row.
        outbox_id: Uuid,
    },

    /// Another row of the company already froze this event id.
    #[error("signed event {event_id} is already frozen on another outbox row")]
    DuplicateEventId {
        /// Signed event id.
        event_id: MessageId,
    },

    /// No row with this id exists in the company scope.
    #[error("outbox row {outbox_id} does not exist in this company")]
    NotFound {
        /// Outbox row.
        outbox_id: Uuid,
    },

    /// The stored row is malformed or its frozen bytes fail verification.
    #[error("outbox row {outbox_id} is invalid: {detail}")]
    InvalidRow {
        /// Outbox row.
        outbox_id: Uuid,
        /// Stable detail.
        detail: String,
    },
}

impl From<sqlx::Error> for OfficeDeliveryError {
    fn from(error: sqlx::Error) -> Self {
        Self::Control(ControlError::Database(error))
    }
}

/// Why the Office binding of a pinned revision cannot sign.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingRejection {
    /// The revision manifest names no valid Office public key.
    RevisionWithoutKey,
    /// No `employee_office_bindings` row holds the revision's key.
    Missing,
    /// The row holding the key belongs to another employee.
    WrongEmployee,
    /// The row's signer reference differs from the revision manifest.
    SignerMismatch,
    /// The signer never proved it produces the key (`verified_at` is null).
    Unverified,
    /// `valid_from` is in the future.
    NotYetValid,
    /// `valid_until` has passed.
    Retired,
}

impl std::fmt::Display for BindingRejection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::RevisionWithoutKey => "revision manifest names no valid office public key",
            Self::Missing => "no office binding holds the revision's key",
            Self::WrongEmployee => "the revision's key is bound to another employee",
            Self::SignerMismatch => "binding signer reference differs from the revision manifest",
            Self::Unverified => "binding is not verified",
            Self::NotYetValid => "binding validity window has not started",
            Self::Retired => "binding is retired",
        })
    }
}

/// Result alias for the Office delivery slice.
pub type Result<T> = std::result::Result<T, OfficeDeliveryError>;
