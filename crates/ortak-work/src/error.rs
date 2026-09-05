use ortak_control::ControlError;
use ortak_domain::{DomainError, EmployeeId};
use thiserror::Error;
use uuid::Uuid;

/// Failures of the Work and Projects slice.
///
/// Every variant is bounded and identifier-only. An unknown record and a
/// record that belongs to another company produce the same not-found
/// variant, so existence in a foreign company is never observable.
#[derive(Debug, Error)]
pub enum WorkError {
    /// The authenticated human lacks the requested action on a visible project.
    #[error("Work action is not authorized")]
    AccessDenied,
    /// An operation id or immutable creation identity has a conflicting payload.
    #[error("Work operation conflicts with an existing operation")]
    OperationConflict,
    /// The bounded database operation could not finish; retry with the same operation id.
    #[error("Work operation timed out")]
    OperationTimedOut,

    /// The control plane or database failed.
    #[error(transparent)]
    Control(#[from] ControlError),

    /// A pure domain rule refused the command.
    #[error(transparent)]
    Domain(#[from] DomainError),

    /// No project with this id exists in the company scope.
    #[error("project {project_id} does not exist in this company")]
    ProjectNotFound {
        /// Requested project.
        project_id: Uuid,
    },

    /// The slug already names a project with a different display name, so
    /// the creation is not a replay of the existing one.
    #[error("project slug {slug} already exists with a different definition")]
    ProjectConflict {
        /// Conflicting slug.
        slug: String,
    },

    /// The project is archived and accepts no new work or mutation.
    #[error("project {project_id} is archived")]
    ProjectArchived {
        /// Archived project.
        project_id: Uuid,
    },

    /// No work item with this id exists in the company scope.
    #[error("work item {work_item_id} does not exist in this company")]
    WorkItemNotFound {
        /// Requested work item.
        work_item_id: Uuid,
    },

    /// The caller's expected version does not match the durable row.
    #[error("record {record_id} is at version {actual}, not {expected}")]
    VersionConflict {
        /// Work item or project.
        record_id: Uuid,
        /// Version the caller expected.
        expected: i64,
        /// Version found under the row lock.
        actual: i64,
    },

    /// The employee is unknown in the company or is not `active`.
    #[error("employee {employee_id} cannot be assigned in this company")]
    EmployeeNotAssignable {
        /// Employee.
        employee_id: EmployeeId,
    },

    /// An employee actor is unknown in the company or is not `active`.
    #[error("employee {employee_id} cannot act in this company")]
    ActorNotFound {
        /// Employee.
        employee_id: EmployeeId,
    },

    /// The source message is not a `decided` Office inbox row of the company.
    #[error("message {message_id} is not a decided Office message in this company")]
    SourceMessageNotDecided {
        /// Lowercase hex event id.
        message_id: String,
    },

    /// The source message was already promoted, and this call names a
    /// different project or a different immutable creation definition
    /// (title, description, priority, criteria, approval gates), so it is
    /// not a replay of the existing item.
    #[error("message {message_id} was already promoted as work item {work_item_id} with a different definition")]
    PromotionConflict {
        /// Lowercase hex event id.
        message_id: String,
        /// The item the message was promoted to.
        work_item_id: Uuid,
    },

    /// The dependency target belongs to a different project.
    #[error("work item {depends_on} belongs to another project")]
    CrossProjectDependency {
        /// Target item.
        depends_on: Uuid,
    },

    /// The attachment target does not exist in the company.
    #[error("{kind} attachment target does not exist in this company")]
    AttachmentTargetNotFound {
        /// Attachment kind.
        kind: &'static str,
    },

    /// The caller-supplied query or cursor is malformed.
    #[error("invalid work query: {0}")]
    InvalidQuery(&'static str),

    /// A durable row does not satisfy the closed read contract.
    #[error("work record is unreadable: {detail}")]
    InvalidRecord {
        /// Stable, identifier-only detail.
        detail: String,
    },
}

impl From<sqlx::Error> for WorkError {
    fn from(error: sqlx::Error) -> Self {
        Self::Control(ControlError::Database(error))
    }
}

/// Result alias for Work operations.
pub type Result<T> = std::result::Result<T, WorkError>;
