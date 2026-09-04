use ortak_control::ControlError;
use thiserror::Error;
use uuid::Uuid;

/// Failures of the Activity read slice.
///
/// Every variant is bounded and identifier-only. An unknown run and a run
/// that belongs to another company are the same [`ActivityError::RunNotFound`]
/// so that existence in a foreign company is never observable.
#[derive(Debug, Error)]
pub enum ActivityError {
    /// No run with this id exists in the company scope.
    #[error("run {run_id} does not exist in this company")]
    RunNotFound {
        /// Requested run.
        run_id: Uuid,
    },

    /// The caller-supplied filters or cursor are malformed.
    #[error("invalid activity query: {0}")]
    InvalidQuery(&'static str),

    /// A durable row does not satisfy the closed read contract (unknown
    /// vocabulary value, payload/type mismatch, or unreadable payload). The
    /// read fails closed instead of rendering a partial record.
    #[error("activity record is unreadable: {detail}")]
    InvalidRecord {
        /// Stable, identifier-only detail.
        detail: String,
    },

    /// The control plane or database failed.
    #[error(transparent)]
    Control(#[from] ControlError),
}

impl From<sqlx::Error> for ActivityError {
    fn from(error: sqlx::Error) -> Self {
        Self::Control(ControlError::Database(error))
    }
}

/// Result alias for Activity reads.
pub type Result<T> = std::result::Result<T, ActivityError>;
