use ortak_control::outbox::OutboxKind;
use ortak_control::runtime::{RuntimeError, RuntimeRunRef};
use ortak_control::ControlError;
use thiserror::Error;
use uuid::Uuid;

use crate::state::{InvalidTransition, RunStatus};

/// Failures of the run dispatch and supervision slice.
///
/// Every variant is bounded and identifier-only; runtime details arrive
/// through [`RuntimeError`], whose contract already excludes secret values.
#[derive(Debug, Error)]
pub enum RunSupervisionError {
    /// The control plane or database failed.
    #[error(transparent)]
    Control(#[from] ControlError),

    /// The runtime adapter failed.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),

    /// A normalized event still violates the persistence contract.
    #[error(transparent)]
    RunEvent(#[from] ortak_control::run_event::RunEventError),

    /// The lease is not `run_dispatch` work.
    #[error("outbox row is {} work, not run_dispatch", .found.as_str())]
    WrongKind {
        /// Kind found on the lease.
        found: OutboxKind,
    },

    /// No outbox row with this id exists in the company scope. A lease
    /// claimed under another company lands here and touches nothing.
    #[error("outbox row {outbox_id} does not exist in this company")]
    UnknownOutboxRow {
        /// Outbox row named by the lease.
        outbox_id: Uuid,
    },

    /// The lease's decision/employee hints disagree with the durable row.
    /// Nothing is written; the row keeps its lease until expiry.
    #[error("lease for outbox row {outbox_id} disagrees with the durable row")]
    LeaseInconsistent {
        /// Outbox row.
        outbox_id: Uuid,
    },

    /// No run with this id exists in the company scope.
    #[error("run {run_id} does not exist in this company")]
    UnknownRun {
        /// Run.
        run_id: Uuid,
    },

    /// The run exists but was never correlated with a runtime run, so no
    /// adapter operation can address it.
    #[error("run {run_id} is {} and has no runtime run reference", .status.as_str())]
    NotCorrelated {
        /// Run.
        run_id: Uuid,
        /// Durable status.
        status: RunStatus,
    },

    /// The durable run pins a different runtime reference than the one the
    /// caller is operating under; the durable one wins and nothing changed.
    #[error("run {run_id} is correlated with runtime run {durable}, not {presented}")]
    RuntimeRefConflict {
        /// Run.
        run_id: Uuid,
        /// Reference on the durable row.
        durable: RuntimeRunRef,
        /// Reference presented by the caller or adapter.
        presented: RuntimeRunRef,
    },

    /// A typed event sequence is not a valid lifecycle transition.
    #[error(transparent)]
    InvalidTransition(#[from] InvalidTransition),

    /// The durable run row is pinned differently from the routing recipient
    /// it belongs to; refusing to reuse it.
    #[error("run {run_id} pins a different revision or message than its routing recipient")]
    RunPinnedDifferently {
        /// Run.
        run_id: Uuid,
    },
}

impl From<sqlx::Error> for RunSupervisionError {
    fn from(error: sqlx::Error) -> Self {
        Self::Control(ControlError::Database(error))
    }
}

/// Result alias for the run supervision slice.
pub type Result<T> = std::result::Result<T, RunSupervisionError>;
