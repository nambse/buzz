use ortak_domain::DomainError;
use thiserror::Error;
use uuid::Uuid;

/// Failures produced by the Ortak control plane and its adapters.
#[derive(Debug, Error)]
pub enum ControlError {
    /// A SQLx driver-level error.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// JSON serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// A durable value violates the Ortak domain contract.
    #[error(transparent)]
    Domain(#[from] DomainError),

    /// The company policy could not construct a router.
    #[error(transparent)]
    Router(#[from] ortak_router::RouterError),

    /// A runtime adapter failed.
    #[error(transparent)]
    Runtime(#[from] crate::runtime::RuntimeError),

    /// A memory adapter failed.
    #[error(transparent)]
    Memory(#[from] crate::memory::MemoryError),

    /// The Office identity adapter failed.
    #[error(transparent)]
    OfficeIdentity(#[from] crate::office_identity::OfficeIdentityError),

    /// The credential manager failed.
    #[error(transparent)]
    Credential(#[from] crate::credentials::CredentialError),

    /// A run event violates the persistence contract.
    #[error(transparent)]
    RunEvent(#[from] crate::run_event::RunEventError),

    /// A provisioning operation cannot proceed.
    #[error(transparent)]
    Provisioning(crate::provisioning::ProvisioningError),

    /// A run-event append targeted an unknown or finished run, or replayed a cursor.
    #[error("run {run_id} cannot accept events: {detail}")]
    RunEventRejected {
        /// Run.
        run_id: Uuid,
        /// Stable detail.
        detail: &'static str,
    },

    /// A durable row is malformed or inconsistent with the schema contract.
    #[error("invalid data: {0}")]
    InvalidData(String),

    /// The authenticated community has no server-owned company binding.
    #[error("no company binding for community {community_id}")]
    UnknownCompanyBinding {
        /// Authenticated community that failed to resolve.
        community_id: Uuid,
    },

    /// The company slug is not registered.
    #[error("unknown company slug {slug}")]
    UnknownCompany {
        /// Operator-supplied slug.
        slug: String,
    },

    /// The company cannot accept routing work in its current lifecycle state.
    #[error("company {company_id} is suspended")]
    CompanySuspended {
        /// Suspended company.
        company_id: Uuid,
    },

    /// The proposal does not describe the message it claims to decide.
    #[error("routing proposal is inconsistent: {0}")]
    InvalidProposal(&'static str),

    /// A durable employee manifest could not be read as an employee definition.
    #[error("employee {employee_id} has an unreadable active revision manifest")]
    UnreadableManifest {
        /// Employee whose active revision is unreadable.
        employee_id: String,
    },

    /// A unique visit reservation collided while the chain row lock was held.
    #[error("employee {employee_id} already holds a visit reservation in this chain")]
    VisitConflict {
        /// Employee whose reservation collided.
        employee_id: String,
    },

    /// Bounded refresh/re-score attempts were exhausted without a commit.
    #[error("routing revalidation exhausted after {attempts} attempts")]
    RevalidationExhausted {
        /// Number of attempts made.
        attempts: u32,
    },
}

/// Result alias for control-plane operations.
pub type Result<T> = std::result::Result<T, ControlError>;
