//! Server-created identities and the bounded manual Work command surface.
use crate::{ProjectRecord, Result, WorkError};
use ortak_domain::{ApprovalDecision, AssignmentRole, EmployeeId, WorkState};
use serde::Serialize;
use std::collections::BTreeSet;
use uuid::Uuid;

/// Trusted server configuration plus the authenticated NIP-98 public identity.
/// Never deserialize this type from an HTTP request. Membership is rechecked in PostgreSQL.
#[derive(Clone)]
pub struct ApiWorkPrincipal {
    pub(super) community_id: Uuid,
    pub(super) public_key: String,
    pub(super) key_bytes: Vec<u8>,
    pub(super) auth_event_id: [u8; 32],
    pub(super) operator: bool,
    pub(super) can_create_projects: bool,
    pub(super) channel_ids: BTreeSet<Uuid>,
    pub(super) employee_ids: BTreeSet<EmployeeId>,
}
impl ApiWorkPrincipal {
    /// Construct only from a verified principal and server-owned audience configuration.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        community_id: Uuid,
        public_key: String,
        auth_event_id: [u8; 32],
        operator: bool,
        can_create_projects: bool,
        channel_ids: BTreeSet<Uuid>,
        employee_ids: BTreeSet<EmployeeId>,
    ) -> Result<Self> {
        let key_bytes = hex::decode(&public_key)
            .map_err(|_| WorkError::InvalidQuery("invalid public identity"))?;
        if community_id.is_nil()
            || key_bytes.len() != 32
            || hex::encode(&key_bytes) != public_key
            || channel_ids.is_empty()
            || channel_ids.len() > 64
            || channel_ids.iter().any(Uuid::is_nil)
            || employee_ids.len() > 64
        {
            return Err(WorkError::InvalidQuery("invalid Work audience"));
        }
        Ok(Self {
            community_id,
            public_key,
            key_bytes,
            auth_event_id,
            operator,
            can_create_projects,
            channel_ids,
            employee_ids,
        })
    }
}

/// Durable project-specific human permission; global operator never bypasses this grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRole {
    /// Read only.
    Viewer,
    /// Create, assign, and report ordinary status.
    Contributor,
    /// Resolve criteria, approvals, and review outcomes.
    Reviewer,
    /// All currently exposed manual project operations.
    Owner,
}
impl ProjectRole {
    pub(super) fn parse(value: &str) -> Result<Self> {
        match value {
            "viewer" => Ok(Self::Viewer),
            "contributor" => Ok(Self::Contributor),
            "reviewer" => Ok(Self::Reviewer),
            "owner" => Ok(Self::Owner),
            _ => Err(WorkError::InvalidRecord {
                detail: "invalid project role".into(),
            }),
        }
    }
    pub(super) fn contributes(self) -> bool {
        matches!(self, Self::Contributor | Self::Owner)
    }
    pub(super) fn reviews(self) -> bool {
        matches!(self, Self::Reviewer | Self::Owner)
    }
}
/// Authorized project record. This authority does not grant runtime or artifact visibility.
#[derive(Clone, Debug, Serialize)]
pub struct ApiProject {
    /// Current project state.
    pub record: ProjectRecord,
    /// Immutable source Office channel.
    pub channel_id: Uuid,
    /// Current durable project grant.
    pub role: ProjectRole,
}
/// Atomic project/owner creation outcome.
#[derive(Clone, Debug, Serialize)]
pub struct ApiProjectCreation {
    /// Authorized project.
    pub project: ApiProject,
    /// False for an authorized idempotent replay.
    pub created: bool,
}
/// Bounded project list with a stable continuation.
#[derive(Clone, Debug, Serialize)]
pub struct ApiProjectPage {
    /// At most 25 projects.
    pub items: Vec<ApiProject>,
    /// Continue with the same principal and scope; grants are rechecked.
    pub next_cursor: Option<String>,
}
/// An actor-free manual action. The facade inserts the authenticated human actor.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum WorkMutation {
    /// Edit the manual definition before any review evidence exists.
    EditDefinition {
        /// Bounded replacements and retained criterion identities.
        definition: ortak_domain::EditWorkDefinition,
    },
    /// Assign a currently active, channel-authorized employee.
    Assign {
        /// Employee in the server configured cohort.
        employee_id: EmployeeId,
        /// Assignment role; does not grant human review permission.
        role: AssignmentRole,
    },
    /// Release an existing assignment, including an inactive employee's assignment.
    ReleaseAssignment {
        /// Employee in the configured audience.
        employee_id: EmployeeId,
        /// Bounded nonempty human explanation.
        reason: String,
    },
    /// Atomically replace an assignment, or change the same employee's role.
    Reassign {
        /// Currently assigned employee in the configured audience.
        employee_id: EmployeeId,
        /// Currently active and channel-authorized replacement.
        replacement_employee_id: EmployeeId,
        /// New role; never grants human approval authority.
        role: AssignmentRole,
        /// Bounded nonempty human explanation.
        reason: String,
    },
    /// Change manual status. Completion and review rejection require human review authority.
    Transition {
        /// Target state.
        target: WorkState,
        /// Bounded optional reason.
        reason: Option<String>,
    },
    /// Human reviewer accepts one criterion.
    SatisfyCriterion {
        /// Criterion from this work item.
        criterion_id: Uuid,
    },
    /// Human reviewer resolves one approval gate.
    ResolveApproval {
        /// Approval from this work item.
        approval_id: Uuid,
        /// Decision.
        decision: ApprovalDecision,
        /// Bounded optional reason.
        reason: Option<String>,
    },
}
