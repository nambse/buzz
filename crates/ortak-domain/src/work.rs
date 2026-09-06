//! Work and Projects aggregates (Architecture v0 §7; Implementation Plan
//! Milestone 6).
//!
//! A [`Project`] is the company's context and policy boundary for related
//! work. A [`WorkItem`] is one durable, assignable unit of company work with
//! acceptance criteria, approval gates, employee assignments, same-project
//! dependencies, and attachments to canonical Office/routing/run records.
//!
//! Everything here is pure: commands validate their inputs, check the closed
//! state machine, and return the one typed [`WorkEvent`] the control layer
//! must append to the item's history in the same transaction that persists
//! the change. Every successful command advances [`WorkItem::version`] by
//! exactly one, so the durable optimistic-concurrency version and the dense
//! history sequence stay in lockstep.
//!
//! Terminal states (`completed`, `cancelled`) freeze the item: no command
//! mutates it afterwards, and nothing here ever removes a criterion, an
//! approval, an assignment, a dependency, an attachment, or a history event.
//! Cancelling or archiving is a state change, never an erasure.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::employee::{require_bounded_text, require_stable_code};
use crate::{DomainError, EmployeeId};

mod assignment;
mod decomposition;
mod definition;
mod execution;
pub use decomposition::{NewChildWork, MAX_WORK_CHILDREN, MAX_WORK_DEPTH};
pub use definition::{CriterionEdit, EditWorkDefinition};

/// Ceiling for a project or work item title.
pub const MAX_WORK_TITLE_BYTES: usize = 200;
/// Ceiling for a project or work item description.
pub const MAX_WORK_DESCRIPTION_BYTES: usize = 8_192;
/// Ceiling for one acceptance criterion.
pub const MAX_WORK_CRITERION_BYTES: usize = 1_024;
/// Ceiling for a transition, approval, or release reason.
pub const MAX_WORK_REASON_BYTES: usize = 1_024;
/// Ceiling for an attachment label or other short reference text.
pub const MAX_WORK_REFERENCE_BYTES: usize = 256;
/// Ceiling for acceptance criteria on one item.
pub const MAX_WORK_CRITERIA: usize = 64;
/// Ceiling for approval gates on one item.
pub const MAX_WORK_APPROVALS: usize = 16;
/// Ceiling for assignments on one item.
pub const MAX_WORK_ASSIGNMENTS: usize = 16;
/// Ceiling for dependencies of one item.
pub const MAX_WORK_DEPENDENCIES: usize = 32;
/// Ceiling for attachments on one item.
pub const MAX_WORK_ATTACHMENTS: usize = 64;

// ── Projects ─────────────────────────────────────────────────────────────────

/// Stable, company-unique machine name of a project.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProjectSlug(String);

impl ProjectSlug {
    /// Parses `^[a-z0-9][a-z0-9_-]{0,63}$`, the same grammar as employee ids.
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .chars()
                .enumerate()
                .all(|(index, character)| match character {
                    'a'..='z' | '0'..='9' => true,
                    '-' | '_' => index > 0,
                    _ => false,
                });
        if valid {
            Ok(Self(value))
        } else {
            Err(DomainError::InvalidProjectSlug)
        }
    }

    /// Returns the slug text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectSlug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for ProjectSlug {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl From<ProjectSlug> for String {
    fn from(value: ProjectSlug) -> Self {
        value.0
    }
}

/// Project lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    /// Accepts new work and mutations.
    Active,
    /// Read-only; history and work stay visible.
    Archived,
}

impl ProjectStatus {
    /// Column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }

    /// Parses the column value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

/// Validated input for creating a project.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NewProject {
    /// Stable, company-unique slug; the idempotency key of creation.
    pub slug: ProjectSlug,
    /// Display name.
    pub name: String,
    /// Bounded, secret-free description.
    #[serde(default)]
    pub description: String,
}

impl NewProject {
    /// Validates bounds and rejects secret-like text.
    pub fn validate(&self) -> Result<(), DomainError> {
        require_bounded_text("project.name", &self.name, MAX_WORK_TITLE_BYTES)?;
        require_bounded_optional_text(
            "project.description",
            &self.description,
            MAX_WORK_DESCRIPTION_BYTES,
        )?;
        reject_secret_like_text("project.description", &self.description)
    }
}

/// Company work/context boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Durable identifier.
    pub id: Uuid,
    /// Stable, company-unique slug.
    pub slug: ProjectSlug,
    /// Display name.
    pub name: String,
    /// Bounded description.
    pub description: String,
    /// Lifecycle.
    pub status: ProjectStatus,
    /// Optimistic-concurrency version; starts at 1 and grows by one per event.
    pub version: i64,
}

/// Typed project history event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ProjectEvent {
    /// The project was created.
    Created {
        /// Slug at creation.
        slug: ProjectSlug,
    },
    /// The project was archived; its work and history remain.
    Archived {
        /// Optional bounded reason.
        reason: Option<String>,
    },
}

impl ProjectEvent {
    /// Stable history event type.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Created { .. } => "project.created",
            Self::Archived { .. } => "project.archived",
        }
    }
}

impl Project {
    /// Creates a project at version 1 with its creation event.
    pub fn create(id: Uuid, input: &NewProject) -> Result<(Self, ProjectEvent), DomainError> {
        input.validate()?;
        Ok((
            Self {
                id,
                slug: input.slug.clone(),
                name: input.name.clone(),
                description: input.description.clone(),
                status: ProjectStatus::Active,
                version: 1,
            },
            ProjectEvent::Created {
                slug: input.slug.clone(),
            },
        ))
    }

    /// Archives an active project. History and work are untouched.
    pub fn archive(&mut self, reason: Option<String>) -> Result<ProjectEvent, DomainError> {
        if self.status == ProjectStatus::Archived {
            return Err(DomainError::ProjectArchived);
        }
        let reason = validate_reason(reason)?;
        self.status = ProjectStatus::Archived;
        self.version += 1;
        Ok(ProjectEvent::Archived { reason })
    }
}

// ── Actors ───────────────────────────────────────────────────────────────────

/// Who performed a work mutation. Recorded on every history event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum WorkActor {
    /// A human principal (server-resolved identity such as a pubkey hex).
    Human(String),
    /// An employee; the control layer verifies it exists in the company.
    Employee(EmployeeId),
    /// The server itself (promotion, automation).
    System,
}

impl WorkActor {
    /// Validates the bounded human identity.
    pub fn validate(&self) -> Result<(), DomainError> {
        match self {
            Self::Human(id) => require_bounded_text("actor.id", id, MAX_WORK_REFERENCE_BYTES),
            Self::Employee(_) | Self::System => Ok(()),
        }
    }

    /// Column value for the actor type.
    pub fn type_str(&self) -> &'static str {
        match self {
            Self::Human(_) => "human",
            Self::Employee(_) => "employee",
            Self::System => "system",
        }
    }

    /// Column value for the actor id, when any.
    pub fn id_str(&self) -> Option<&str> {
        match self {
            Self::Human(id) => Some(id),
            Self::Employee(id) => Some(id.as_str()),
            Self::System => None,
        }
    }

    /// Rebuilds an actor from its two columns.
    pub fn from_columns(actor_type: &str, actor_id: Option<&str>) -> Result<Self, DomainError> {
        match (actor_type, actor_id) {
            ("human", Some(id)) => Ok(Self::Human(id.to_owned())),
            ("employee", Some(id)) => Ok(Self::Employee(EmployeeId::parse(id)?)),
            ("system", None) => Ok(Self::System),
            _ => Err(DomainError::InvalidField {
                field: "actor.type",
            }),
        }
    }
}

// ── Closed vocabularies ──────────────────────────────────────────────────────

/// Work item lifecycle. `completed` and `cancelled` are terminal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    /// Captured but not yet accepted as work.
    Proposed,
    /// Accepted and startable.
    Ready,
    /// Being executed.
    InProgress,
    /// Cannot proceed until something outside the item changes.
    Blocked,
    /// Awaiting acceptance and approvals.
    Review,
    /// All criteria satisfied and required approvals approved. Terminal.
    Completed,
    /// Abandoned. Terminal; history is preserved.
    Cancelled,
}

impl WorkState {
    /// Column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Review => "review",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses the column value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "proposed" => Some(Self::Proposed),
            "ready" => Some(Self::Ready),
            "in_progress" => Some(Self::InProgress),
            "blocked" => Some(Self::Blocked),
            "review" => Some(Self::Review),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    /// Whether no further mutation is allowed.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }

    /// The closed transition table. `completed` is reachable only from
    /// `review`; any non-terminal state can be cancelled; terminal states
    /// have no successors.
    pub fn can_transition_to(self, target: Self) -> bool {
        use WorkState::*;
        matches!(
            (self, target),
            (Proposed, Ready)
                | (Proposed, Cancelled)
                | (Ready, InProgress)
                | (Ready, Blocked)
                | (Ready, Proposed)
                | (Ready, Cancelled)
                | (InProgress, Review)
                | (InProgress, Blocked)
                | (InProgress, Ready)
                | (InProgress, Cancelled)
                | (Blocked, Ready)
                | (Blocked, InProgress)
                | (Blocked, Cancelled)
                | (Review, Completed)
                | (Review, InProgress)
                | (Review, Cancelled)
        )
    }
}

impl fmt::Display for WorkState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Work priority.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkPriority {
    /// Can wait.
    Low,
    /// Default.
    #[default]
    Normal,
    /// Ahead of normal work.
    High,
    /// Interrupts other work.
    Urgent,
}

impl WorkPriority {
    /// Column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Urgent => "urgent",
        }
    }

    /// Parses the column value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "low" => Some(Self::Low),
            "normal" => Some(Self::Normal),
            "high" => Some(Self::High),
            "urgent" => Some(Self::Urgent),
            _ => None,
        }
    }
}

/// Role of an assigned employee.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentRole {
    /// Accountable for delivery.
    Owner,
    /// Works on the item.
    Contributor,
    /// Reviews the item.
    Reviewer,
}

impl AssignmentRole {
    /// Column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Contributor => "contributor",
            Self::Reviewer => "reviewer",
        }
    }

    /// Parses the column value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "contributor" => Some(Self::Contributor),
            "reviewer" => Some(Self::Reviewer),
            _ => None,
        }
    }
}

/// Whether an assignment is in effect.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentStatus {
    /// In effect.
    Active,
    /// Released; kept for history.
    Released,
}

impl AssignmentStatus {
    /// Column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Released => "released",
        }
    }

    /// Parses the column value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "released" => Some(Self::Released),
            _ => None,
        }
    }
}

/// Acceptance criterion state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionStatus {
    /// Not yet satisfied.
    Pending,
    /// Satisfied; final.
    Satisfied,
}

impl CriterionStatus {
    /// Column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Satisfied => "satisfied",
        }
    }

    /// Parses the column value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "satisfied" => Some(Self::Satisfied),
            _ => None,
        }
    }
}

/// Approval gate state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    /// Awaiting a decision.
    Pending,
    /// Approved; final.
    Approved,
    /// Rejected; final. A required rejected gate blocks completion.
    Rejected,
}

impl ApprovalStatus {
    /// Column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }

    /// Parses the column value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "approved" => Some(Self::Approved),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }
}

/// Decision applied to a pending approval gate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    /// Approve the gate.
    Approve,
    /// Reject the gate.
    Reject,
}

impl ApprovalDecision {
    /// Resulting gate status.
    pub fn status(self) -> ApprovalStatus {
        match self {
            Self::Approve => ApprovalStatus::Approved,
            Self::Reject => ApprovalStatus::Rejected,
        }
    }
}

// ── Children ─────────────────────────────────────────────────────────────────

/// One acceptance criterion.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriterion {
    /// Durable identifier.
    pub id: Uuid,
    /// Stable order on the item.
    pub position: u16,
    /// Bounded criterion text; amendable only before review evidence exists.
    pub text: String,
    /// Current state.
    pub status: CriterionStatus,
    /// Who satisfied it.
    pub satisfied_by: Option<WorkActor>,
}

/// One approval gate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalGate {
    /// Durable identifier.
    pub id: Uuid,
    /// Stable gate code (for example `human_review`), unique on the item.
    pub gate: String,
    /// Whether completion requires this gate to be approved.
    pub required: bool,
    /// Current state.
    pub status: ApprovalStatus,
    /// Who resolved it.
    pub resolved_by: Option<WorkActor>,
    /// Bounded resolution reason.
    pub reason: Option<String>,
}

/// Input for one approval gate at creation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalGateSpec {
    /// Stable gate code.
    pub gate: String,
    /// Whether completion requires approval.
    pub required: bool,
}

/// One employee assignment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    /// Assigned employee.
    pub employee_id: EmployeeId,
    /// Role.
    pub role: AssignmentRole,
    /// Whether in effect.
    pub status: AssignmentStatus,
}

/// One `blocked_by` dependency with the current state of its target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkDependency {
    /// Work item this item waits for; same company and project.
    pub depends_on: Uuid,
    /// State of the target as loaded with the aggregate.
    pub depends_on_state: WorkState,
}

impl WorkDependency {
    /// A dependency stops blocking once its target is completed or
    /// cancelled (a cancelled target is void, not unfinished).
    pub fn is_blocking(&self) -> bool {
        !self.depends_on_state.is_terminal()
    }
}

/// Canonical record an item is attached to. Only references to rows that
/// exist in the control-plane schema; never payloads.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttachmentRef {
    /// A decided Office message (lowercase hex event id).
    OfficeMessage {
        /// 64-character lowercase hex event id.
        message_id: String,
    },
    /// A dispatching routing decision.
    RoutingDecision {
        /// Decision id.
        decision_id: Uuid,
    },
    /// An employee run.
    Run {
        /// Run id.
        run_id: Uuid,
    },
    /// Immutable text output of an authorized Work execution.
    Artifact {
        /// Same-company, same-item artifact identifier.
        artifact_id: Uuid,
    },
}

impl AttachmentRef {
    /// Column value for the kind.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::OfficeMessage { .. } => "office_message",
            Self::RoutingDecision { .. } => "routing_decision",
            Self::Run { .. } => "run",
            Self::Artifact { .. } => "artifact",
        }
    }

    /// Validates the reference shape.
    pub fn validate(&self) -> Result<(), DomainError> {
        match self {
            Self::OfficeMessage { message_id } => {
                if message_id.len() != 64
                    || !message_id
                        .chars()
                        .all(|character| character.is_ascii_hexdigit() && !character.is_uppercase())
                {
                    return Err(DomainError::InvalidField {
                        field: "attachment.message_id",
                    });
                }
                Ok(())
            }
            Self::Artifact { artifact_id } if artifact_id.is_nil() => {
                Err(DomainError::InvalidField {
                    field: "attachment.artifact_id",
                })
            }
            Self::RoutingDecision { .. } | Self::Run { .. } | Self::Artifact { .. } => Ok(()),
        }
    }
}

/// One attachment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkAttachment {
    /// Durable identifier.
    pub id: Uuid,
    /// Referenced record.
    pub reference: AttachmentRef,
    /// Optional bounded label.
    pub label: Option<String>,
}

// ── Events ───────────────────────────────────────────────────────────────────

/// Typed, bounded history event appended with every successful command.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkEvent {
    /// One human request atomically queued a run and attached its provenance.
    ExecutionRequested {
        /// Durable run.
        run_id: Uuid,
        /// Assigned executor.
        employee_id: EmployeeId,
        /// State before the request.
        from: WorkState,
    },
    /// A complete run output was saved and offered for human review.
    ExecutionResultReady {
        /// Executed run.
        run_id: Uuid,
        /// Immutable, bounded text artifact.
        artifact_id: Uuid,
    },
    /// One atomic definition edit; bounded metadata rather than full text snapshots.
    DefinitionEdited {
        /// Canonical prior creation-definition hash, preserving original promotion retries.
        previous_definition_hash: String,
        /// Whether the title changed.
        title_changed: bool,
        /// Whether the description changed.
        description_changed: bool,
        /// Existing criteria whose text changed, in retained order.
        edited_criterion_ids: Vec<Uuid>,
        /// Newly appended criteria, in position order.
        added_criterion_ids: Vec<Uuid>,
    },
    /// The item was created (possibly promoted from a message).
    Created {
        /// Title at creation.
        title: String,
        /// Source Office message when promoted.
        source_message_id: Option<String>,
    },
    /// The state changed.
    StateChanged {
        /// Previous state.
        from: WorkState,
        /// New state.
        to: WorkState,
        /// Bounded reason.
        reason: Option<String>,
    },
    /// An employee was assigned or re-activated.
    Assigned {
        /// Employee.
        employee_id: EmployeeId,
        /// Role.
        role: AssignmentRole,
    },
    /// An active assignment was released without removing its provenance.
    AssignmentReleased {
        /// Previously assigned employee.
        employee_id: EmployeeId,
        /// Human supplied bounded reason.
        reason: String,
    },
    /// One atomic reassignment or role change, retaining the previous assignment.
    AssignmentReassigned {
        /// Previously assigned employee.
        employee_id: EmployeeId,
        /// Currently eligible replacement (may be the same employee for a role change).
        replacement_employee_id: EmployeeId,
        /// New assignment role, independent of human approval permission.
        role: AssignmentRole,
        /// Human supplied bounded reason.
        reason: String,
    },
    /// A same-project dependency was added.
    DependencyAdded {
        /// Target item.
        depends_on: Uuid,
    },
    /// An active dependency was released; its storage identity and history remain.
    DependencyRemoved {
        /// Previously blocking item.
        depends_on: Uuid,
        /// Bounded human explanation.
        reason: String,
    },
    /// One independently defined child was created atomically with this version.
    ChildCreated {
        /// New child; visibility must be checked separately before projection.
        child_id: Uuid,
    },
    /// A criterion was satisfied.
    CriterionSatisfied {
        /// Criterion.
        criterion_id: Uuid,
    },
    /// An approval gate was resolved.
    ApprovalResolved {
        /// Gate.
        approval_id: Uuid,
        /// Resulting status.
        status: ApprovalStatus,
        /// Bounded reason.
        reason: Option<String>,
    },
    /// A canonical record was attached.
    Attached {
        /// Attachment.
        attachment_id: Uuid,
        /// Reference.
        reference: AttachmentRef,
    },
}

impl WorkEvent {
    /// Stable history event type.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Created { .. } => "work.created",
            Self::ExecutionRequested { .. } => "work.execution_requested",
            Self::ExecutionResultReady { .. } => "work.execution_result_ready",
            Self::DefinitionEdited { .. } => "work.definition_edited",
            Self::StateChanged { .. } => "work.state_changed",
            Self::Assigned { .. } => "work.assigned",
            Self::AssignmentReleased { .. } => "work.assignment_released",
            Self::AssignmentReassigned { .. } => "work.assignment_reassigned",
            Self::DependencyAdded { .. } => "work.dependency_added",
            Self::DependencyRemoved { .. } => "work.dependency_removed",
            Self::ChildCreated { .. } => "work.child_created",
            Self::CriterionSatisfied { .. } => "work.criterion_satisfied",
            Self::ApprovalResolved { .. } => "work.approval_resolved",
            Self::Attached { .. } => "work.attached",
        }
    }

    /// State transition carried by the event, if any.
    pub fn state_change(&self) -> Option<(WorkState, WorkState)> {
        match self {
            Self::StateChanged { from, to, .. } => Some((*from, *to)),
            Self::ExecutionRequested { from, .. } if *from != WorkState::InProgress => {
                Some((*from, WorkState::InProgress))
            }
            Self::ExecutionResultReady { .. } => Some((WorkState::InProgress, WorkState::Review)),
            _ => None,
        }
    }
}

/// Why an item cannot complete.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "blocker", rename_all = "snake_case")]
pub enum CompletionBlocker {
    /// A criterion is still pending.
    UnsatisfiedCriterion {
        /// Criterion.
        criterion_id: Uuid,
    },
    /// A required gate is still pending.
    PendingApproval {
        /// Gate.
        approval_id: Uuid,
    },
    /// A required gate was rejected.
    RejectedApproval {
        /// Gate.
        approval_id: Uuid,
    },
}

// ── Work item aggregate ──────────────────────────────────────────────────────

/// Validated input for creating a work item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NewWorkItem {
    /// Owning project.
    pub project_id: Uuid,
    /// Bounded title.
    pub title: String,
    /// Bounded, secret-free description.
    #[serde(default)]
    pub description: String,
    /// Priority.
    #[serde(default)]
    pub priority: WorkPriority,
    /// Acceptance criteria in order.
    #[serde(default)]
    pub criteria: Vec<String>,
    /// Approval gates.
    #[serde(default)]
    pub approvals: Vec<ApprovalGateSpec>,
    /// Decided Office message this item is promoted from; the idempotency
    /// key of promotion. Lowercase hex event id.
    pub source_message_id: Option<String>,
}

impl NewWorkItem {
    /// Validates bounds, closed codes, uniqueness, and secret-free text.
    pub fn validate(&self) -> Result<(), DomainError> {
        require_bounded_text("work.title", &self.title, MAX_WORK_TITLE_BYTES)?;
        reject_secret_like_text("work.title", &self.title)?;
        require_bounded_optional_text(
            "work.description",
            &self.description,
            MAX_WORK_DESCRIPTION_BYTES,
        )?;
        reject_secret_like_text("work.description", &self.description)?;
        if self.criteria.len() > MAX_WORK_CRITERIA {
            return Err(DomainError::InvalidField {
                field: "work.criteria",
            });
        }
        for criterion in &self.criteria {
            require_bounded_text("work.criteria", criterion, MAX_WORK_CRITERION_BYTES)?;
            reject_secret_like_text("work.criteria", criterion)?;
        }
        if self.approvals.len() > MAX_WORK_APPROVALS {
            return Err(DomainError::InvalidField {
                field: "work.approvals",
            });
        }
        let mut gates = BTreeSet::new();
        for approval in &self.approvals {
            require_stable_code("work.approvals.gate", &approval.gate, 64)?;
            if !gates.insert(approval.gate.as_str()) {
                return Err(DomainError::InvalidField {
                    field: "work.approvals.gate",
                });
            }
        }
        if let Some(source) = &self.source_message_id {
            AttachmentRef::OfficeMessage {
                message_id: source.clone(),
            }
            .validate()
            .map_err(|_| DomainError::InvalidField {
                field: "work.source_message_id",
            })?;
        }
        Ok(())
    }
}

/// Identifiers the control layer allocates for the children of a new item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewWorkItemIds {
    /// Item id.
    pub id: Uuid,
    /// One id per criterion, in order.
    pub criterion_ids: Vec<Uuid>,
    /// One id per approval gate, in order.
    pub approval_ids: Vec<Uuid>,
}

/// One durable unit of company work.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItem {
    /// Durable identifier.
    pub id: Uuid,
    /// Owning project.
    pub project_id: Uuid,
    /// Bounded title.
    pub title: String,
    /// Bounded description.
    pub description: String,
    /// Priority.
    pub priority: WorkPriority,
    /// Lifecycle state.
    pub state: WorkState,
    /// Optimistic-concurrency version; starts at 1 and grows by one per event.
    pub version: i64,
    /// Source Office message when promoted (lowercase hex).
    pub source_message_id: Option<String>,
    /// Acceptance criteria in position order.
    pub criteria: Vec<AcceptanceCriterion>,
    /// Approval gates.
    pub approvals: Vec<ApprovalGate>,
    /// Employee assignments.
    pub assignments: Vec<Assignment>,
    /// `blocked_by` dependencies.
    pub dependencies: Vec<WorkDependency>,
    /// Attachments.
    pub attachments: Vec<WorkAttachment>,
}

impl WorkItem {
    /// Creates an item in `proposed` at version 1 with its creation event.
    pub fn create(
        ids: NewWorkItemIds,
        input: &NewWorkItem,
    ) -> Result<(Self, WorkEvent), DomainError> {
        input.validate()?;
        if ids.criterion_ids.len() != input.criteria.len()
            || ids.approval_ids.len() != input.approvals.len()
        {
            return Err(DomainError::InvalidField { field: "work.ids" });
        }
        let criteria = input
            .criteria
            .iter()
            .zip(&ids.criterion_ids)
            .enumerate()
            .map(|(position, (text, id))| AcceptanceCriterion {
                id: *id,
                position: position as u16,
                text: text.clone(),
                status: CriterionStatus::Pending,
                satisfied_by: None,
            })
            .collect();
        let approvals = input
            .approvals
            .iter()
            .zip(&ids.approval_ids)
            .map(|(spec, id)| ApprovalGate {
                id: *id,
                gate: spec.gate.clone(),
                required: spec.required,
                status: ApprovalStatus::Pending,
                resolved_by: None,
                reason: None,
            })
            .collect();
        let item = Self {
            id: ids.id,
            project_id: input.project_id,
            title: input.title.clone(),
            description: input.description.clone(),
            priority: input.priority,
            state: WorkState::Proposed,
            version: 1,
            source_message_id: input.source_message_id.clone(),
            criteria,
            approvals,
            assignments: Vec::new(),
            dependencies: Vec::new(),
            attachments: Vec::new(),
        };
        let event = WorkEvent::Created {
            title: item.title.clone(),
            source_message_id: item.source_message_id.clone(),
        };
        Ok((item, event))
    }

    /// Records a server-derived attachment as part of creation, before any
    /// history beyond the creation event exists. Unlike [`Self::attach`] it
    /// does not advance the version: the creation event already covers it.
    /// Refused once the item has moved past version 1.
    pub fn attach_at_creation(
        &mut self,
        attachment_id: Uuid,
        reference: AttachmentRef,
    ) -> Result<(), DomainError> {
        if self.version != 1 {
            return Err(DomainError::InvalidField {
                field: "work.attachments",
            });
        }
        reference.validate()?;
        if self
            .attachments
            .iter()
            .any(|attachment| attachment.reference == reference)
        {
            return Err(DomainError::DuplicateAttachment);
        }
        if self.attachments.len() >= MAX_WORK_ATTACHMENTS {
            return Err(DomainError::InvalidField {
                field: "work.attachments",
            });
        }
        self.attachments.push(WorkAttachment {
            id: attachment_id,
            reference,
            label: None,
        });
        Ok(())
    }

    fn ensure_mutable(&self) -> Result<(), DomainError> {
        if self.state.is_terminal() {
            Err(DomainError::WorkItemTerminal { state: self.state })
        } else {
            Ok(())
        }
    }

    /// Dependencies whose target is still unfinished.
    pub fn blocking_dependencies(&self) -> Vec<Uuid> {
        self.dependencies
            .iter()
            .filter(|dependency| dependency.is_blocking())
            .map(|dependency| dependency.depends_on)
            .collect()
    }

    /// Everything that currently prevents completion.
    pub fn completion_blockers(&self) -> Vec<CompletionBlocker> {
        let mut blockers = Vec::new();
        for criterion in &self.criteria {
            if criterion.status != CriterionStatus::Satisfied {
                blockers.push(CompletionBlocker::UnsatisfiedCriterion {
                    criterion_id: criterion.id,
                });
            }
        }
        for approval in self.approvals.iter().filter(|gate| gate.required) {
            match approval.status {
                ApprovalStatus::Approved => {}
                ApprovalStatus::Pending => blockers.push(CompletionBlocker::PendingApproval {
                    approval_id: approval.id,
                }),
                ApprovalStatus::Rejected => blockers.push(CompletionBlocker::RejectedApproval {
                    approval_id: approval.id,
                }),
            }
        }
        blockers
    }

    /// Moves the item to `target` if the transition table allows it and the
    /// entry gates hold: `in_progress` requires no blocking dependency,
    /// `completed` requires every criterion satisfied and every required
    /// approval approved.
    pub fn transition(
        &mut self,
        target: WorkState,
        reason: Option<String>,
    ) -> Result<WorkEvent, DomainError> {
        self.ensure_mutable()?;
        let reason = validate_reason(reason)?;
        if !self.state.can_transition_to(target) {
            return Err(DomainError::InvalidWorkTransition {
                from: self.state,
                to: target,
            });
        }
        if target == WorkState::InProgress {
            let blocking = self.blocking_dependencies();
            if !blocking.is_empty() {
                return Err(DomainError::DependenciesUnresolved {
                    count: blocking.len(),
                });
            }
        }
        if target == WorkState::Completed {
            let blockers = self.completion_blockers();
            if !blockers.is_empty() {
                return Err(DomainError::CompletionBlocked { blockers });
            }
        }
        let from = self.state;
        self.state = target;
        self.version += 1;
        Ok(WorkEvent::StateChanged {
            from,
            to: target,
            reason,
        })
    }

    /// Assigns an employee, or re-activates a released assignment. The
    /// control layer must have verified the employee is active in the
    /// company before calling this.
    pub fn assign(
        &mut self,
        employee_id: EmployeeId,
        role: AssignmentRole,
    ) -> Result<WorkEvent, DomainError> {
        self.ensure_mutable()?;
        if let Some(existing) = self
            .assignments
            .iter_mut()
            .find(|assignment| assignment.employee_id == employee_id)
        {
            if existing.status == AssignmentStatus::Active {
                return Err(DomainError::DuplicateAssignment);
            }
            existing.status = AssignmentStatus::Active;
            existing.role = role;
        } else {
            if self.assignments.len() >= MAX_WORK_ASSIGNMENTS {
                return Err(DomainError::InvalidField {
                    field: "work.assignments",
                });
            }
            self.assignments.push(Assignment {
                employee_id: employee_id.clone(),
                role,
                status: AssignmentStatus::Active,
            });
        }
        self.version += 1;
        Ok(WorkEvent::Assigned { employee_id, role })
    }

    /// Records that this item is blocked by `depends_on`. Self-dependency is
    /// refused here; cycle detection over the project graph is the control
    /// layer's job (see [`creates_dependency_cycle`]).
    pub fn add_dependency(
        &mut self,
        depends_on: Uuid,
        depends_on_state: WorkState,
    ) -> Result<WorkEvent, DomainError> {
        self.ensure_mutable()?;
        if depends_on == self.id {
            return Err(DomainError::SelfDependency);
        }
        if self
            .dependencies
            .iter()
            .any(|dependency| dependency.depends_on == depends_on)
        {
            return Err(DomainError::DuplicateWorkDependency);
        }
        if self.dependencies.len() >= MAX_WORK_DEPENDENCIES {
            return Err(DomainError::InvalidField {
                field: "work.dependencies",
            });
        }
        self.dependencies.push(WorkDependency {
            depends_on,
            depends_on_state,
        });
        self.version += 1;
        Ok(WorkEvent::DependencyAdded { depends_on })
    }

    /// Remove one active blocker without changing work status or human acceptance.
    pub fn remove_dependency(
        &mut self,
        depends_on: Uuid,
        reason: String,
    ) -> Result<WorkEvent, DomainError> {
        self.ensure_mutable()?;
        let reason = validate_reason(Some(reason))?.ok_or(DomainError::InvalidField {
            field: "work.dependency.reason",
        })?;
        if !self.dependencies.iter().any(|d| d.depends_on == depends_on) {
            return Err(DomainError::UnknownWorkDependency);
        }
        let version = self
            .version
            .checked_add(1)
            .ok_or(DomainError::InvalidField {
                field: "work.version",
            })?;
        self.dependencies.retain(|d| d.depends_on != depends_on);
        self.version = version;
        Ok(WorkEvent::DependencyRemoved { depends_on, reason })
    }

    /// Marks a pending criterion satisfied.
    pub fn satisfy_criterion(
        &mut self,
        criterion_id: Uuid,
        actor: WorkActor,
    ) -> Result<WorkEvent, DomainError> {
        self.ensure_mutable()?;
        actor.validate()?;
        let criterion = self
            .criteria
            .iter_mut()
            .find(|criterion| criterion.id == criterion_id)
            .ok_or(DomainError::UnknownCriterion)?;
        if criterion.status == CriterionStatus::Satisfied {
            return Err(DomainError::CriterionAlreadySatisfied);
        }
        criterion.status = CriterionStatus::Satisfied;
        criterion.satisfied_by = Some(actor);
        self.version += 1;
        Ok(WorkEvent::CriterionSatisfied { criterion_id })
    }

    /// Resolves a pending approval gate.
    pub fn resolve_approval(
        &mut self,
        approval_id: Uuid,
        decision: ApprovalDecision,
        reason: Option<String>,
        actor: WorkActor,
    ) -> Result<WorkEvent, DomainError> {
        self.ensure_mutable()?;
        actor.validate()?;
        let reason = validate_reason(reason)?;
        let approval = self
            .approvals
            .iter_mut()
            .find(|approval| approval.id == approval_id)
            .ok_or(DomainError::UnknownApproval)?;
        if approval.status != ApprovalStatus::Pending {
            return Err(DomainError::ApprovalAlreadyResolved);
        }
        approval.status = decision.status();
        approval.resolved_by = Some(actor);
        approval.reason = reason.clone();
        self.version += 1;
        Ok(WorkEvent::ApprovalResolved {
            approval_id,
            status: decision.status(),
            reason,
        })
    }

    /// Attaches a canonical record. The control layer must have verified the
    /// referenced row exists in the company before calling this.
    pub fn attach(
        &mut self,
        attachment_id: Uuid,
        reference: AttachmentRef,
        label: Option<String>,
    ) -> Result<WorkEvent, DomainError> {
        self.ensure_mutable()?;
        reference.validate()?;
        let label = match label {
            Some(label) => {
                require_bounded_text("attachment.label", &label, MAX_WORK_REFERENCE_BYTES)?;
                Some(label)
            }
            None => None,
        };
        if self
            .attachments
            .iter()
            .any(|attachment| attachment.reference == reference)
        {
            return Err(DomainError::DuplicateAttachment);
        }
        if self.attachments.len() >= MAX_WORK_ATTACHMENTS {
            return Err(DomainError::InvalidField {
                field: "work.attachments",
            });
        }
        self.attachments.push(WorkAttachment {
            id: attachment_id,
            reference: reference.clone(),
            label,
        });
        self.version += 1;
        Ok(WorkEvent::Attached {
            attachment_id,
            reference,
        })
    }
}

/// Whether adding the edge `item → depends_on` to `edges` (item → its
/// `blocked_by` targets) closes a cycle: true when `depends_on` already
/// reaches `item`, or when the edge is a self-loop.
pub fn creates_dependency_cycle(
    edges: &BTreeMap<Uuid, BTreeSet<Uuid>>,
    item: Uuid,
    depends_on: Uuid,
) -> bool {
    if item == depends_on {
        return true;
    }
    let mut stack = vec![depends_on];
    let mut seen = BTreeSet::new();
    while let Some(current) = stack.pop() {
        if current == item {
            return true;
        }
        if !seen.insert(current) {
            continue;
        }
        if let Some(next) = edges.get(&current) {
            stack.extend(next.iter().copied());
        }
    }
    false
}

// ── Shared validation ────────────────────────────────────────────────────────

fn require_bounded_optional_text(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), DomainError> {
    if value.is_empty() {
        return Ok(());
    }
    require_bounded_text(field, value, max_bytes)
}

fn validate_reason(reason: Option<String>) -> Result<Option<String>, DomainError> {
    match reason {
        Some(reason) => {
            require_bounded_text("work.reason", &reason, MAX_WORK_REASON_BYTES)?;
            reject_secret_like_text("work.reason", &reason)?;
            Ok(Some(reason))
        }
        None => Ok(None),
    }
}

/// Defense in depth for free text that ends up in durable, widely readable
/// rows: refuses obvious key material. Credential *references*
/// (`credential://…`) are allowed; values are not.
fn reject_secret_like_text(field: &'static str, value: &str) -> Result<(), DomainError> {
    let lower = value.to_ascii_lowercase();
    let has_private_key_block = lower.contains("-----begin") && lower.contains("private key");
    let has_nsec = lower
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| token.starts_with("nsec1") && token.len() >= 30);
    if has_private_key_block || has_nsec {
        Err(DomainError::UnsafeAdapterOption { field })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use uuid::Uuid;

    use super::{
        creates_dependency_cycle, ApprovalDecision, ApprovalGateSpec, AssignmentRole,
        CompletionBlocker, NewWorkItem, NewWorkItemIds, ProjectSlug, WorkActor, WorkEvent,
        WorkItem, WorkPriority, WorkState,
    };
    use crate::{DomainError, EmployeeId};

    fn new_item(criteria: &[&str], approvals: &[(&str, bool)]) -> WorkItem {
        let input = NewWorkItem {
            project_id: Uuid::new_v4(),
            title: "Ship the fitness app onboarding".to_owned(),
            description: String::new(),
            priority: WorkPriority::Normal,
            criteria: criteria.iter().map(|text| (*text).to_owned()).collect(),
            approvals: approvals
                .iter()
                .map(|(gate, required)| ApprovalGateSpec {
                    gate: (*gate).to_owned(),
                    required: *required,
                })
                .collect(),
            source_message_id: None,
        };
        let ids = NewWorkItemIds {
            id: Uuid::new_v4(),
            criterion_ids: criteria.iter().map(|_| Uuid::new_v4()).collect(),
            approval_ids: approvals.iter().map(|_| Uuid::new_v4()).collect(),
        };
        let (item, event) = WorkItem::create(ids, &input).expect("valid item");
        assert!(matches!(event, WorkEvent::Created { .. }));
        assert_eq!(item.version, 1);
        item
    }

    fn drive_to_review(item: &mut WorkItem) {
        for target in [WorkState::Ready, WorkState::InProgress, WorkState::Review] {
            item.transition(target, None).expect("legal transition");
        }
    }

    #[test]
    fn transition_table_is_closed_and_terminal_states_freeze_the_item() {
        use WorkState::*;
        let states = [
            Proposed, Ready, InProgress, Blocked, Review, Completed, Cancelled,
        ];
        for from in states {
            for to in states {
                let allowed = from.can_transition_to(to);
                assert_eq!(
                    allowed,
                    !from.is_terminal()
                        && from != to
                        && (to != Completed || from == Review)
                        && !(from == Proposed && matches!(to, InProgress | Blocked | Review))
                        && !(from == Ready && to == Review)
                        && !(from == Blocked && matches!(to, Proposed | Review))
                        && !(from == InProgress && to == Proposed)
                        && !(from == Review && matches!(to, Proposed | Ready | Blocked)),
                    "{from} -> {to}"
                );
            }
        }

        let mut item = new_item(&[], &[]);
        item.transition(WorkState::Cancelled, Some("duplicate".to_owned()))
            .expect("cancel");
        assert_eq!(item.version, 2);
        assert_eq!(
            item.transition(WorkState::Ready, None),
            Err(DomainError::WorkItemTerminal {
                state: WorkState::Cancelled
            })
        );
        assert_eq!(
            item.assign(EmployeeId::parse("cem").expect("id"), AssignmentRole::Owner),
            Err(DomainError::WorkItemTerminal {
                state: WorkState::Cancelled
            })
        );
        assert_eq!(
            item.version, 2,
            "a refused command never advances the version"
        );
    }

    #[test]
    fn completion_requires_every_criterion_and_every_required_approval() {
        let mut item = new_item(
            &["Tests pass", "Docs updated"],
            &[("human_review", true), ("security", false)],
        );
        drive_to_review(&mut item);
        let version_before = item.version;

        let blocked = item.transition(WorkState::Completed, None);
        let Err(DomainError::CompletionBlocked { blockers }) = blocked else {
            panic!("expected completion to be blocked, got {blocked:?}");
        };
        assert_eq!(blockers.len(), 3);
        assert_eq!(item.state, WorkState::Review);
        assert_eq!(item.version, version_before);

        let actor = WorkActor::Human("sefa".to_owned());
        for criterion_id in item
            .criteria
            .iter()
            .map(|criterion| criterion.id)
            .collect::<Vec<_>>()
        {
            item.satisfy_criterion(criterion_id, actor.clone())
                .expect("satisfy");
        }
        let unknown = item.satisfy_criterion(Uuid::new_v4(), actor.clone());
        assert_eq!(unknown, Err(DomainError::UnknownCriterion));
        let repeated = item.satisfy_criterion(item.criteria[0].id, actor.clone());
        assert_eq!(repeated, Err(DomainError::CriterionAlreadySatisfied));

        let required = item.approvals[0].id;
        item.resolve_approval(required, ApprovalDecision::Reject, None, actor.clone())
            .expect("reject");
        let rejected = item.transition(WorkState::Completed, None);
        assert_eq!(
            rejected,
            Err(DomainError::CompletionBlocked {
                blockers: vec![CompletionBlocker::RejectedApproval {
                    approval_id: required
                }]
            })
        );
        assert_eq!(
            item.resolve_approval(required, ApprovalDecision::Approve, None, actor.clone()),
            Err(DomainError::ApprovalAlreadyResolved)
        );

        // A rejected required gate is final for this item; a fresh item with
        // the gate approved completes, and the optional gate never blocks.
        let mut item = new_item(
            &["Tests pass"],
            &[("human_review", true), ("security", false)],
        );
        drive_to_review(&mut item);
        item.satisfy_criterion(item.criteria[0].id, actor.clone())
            .expect("satisfy");
        item.resolve_approval(
            item.approvals[0].id,
            ApprovalDecision::Approve,
            Some("lgtm".to_owned()),
            actor,
        )
        .expect("approve");
        let event = item
            .transition(WorkState::Completed, None)
            .expect("complete");
        assert_eq!(
            event.state_change(),
            Some((WorkState::Review, WorkState::Completed))
        );
        assert_eq!(item.version, 7);
    }

    #[test]
    fn dependencies_gate_start_and_reject_self_and_cycles() {
        let mut item = new_item(&[], &[]);
        let other = Uuid::new_v4();
        assert_eq!(
            item.add_dependency(item.id, WorkState::Ready),
            Err(DomainError::SelfDependency)
        );
        item.add_dependency(other, WorkState::InProgress)
            .expect("dependency");
        assert_eq!(
            item.add_dependency(other, WorkState::InProgress),
            Err(DomainError::DuplicateWorkDependency)
        );
        item.transition(WorkState::Ready, None).expect("ready");
        assert_eq!(
            item.transition(WorkState::InProgress, None),
            Err(DomainError::DependenciesUnresolved { count: 1 })
        );
        item.dependencies[0].depends_on_state = WorkState::Cancelled;
        item.transition(WorkState::InProgress, None)
            .expect("void dependency no longer blocks");

        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let mut edges: BTreeMap<Uuid, BTreeSet<Uuid>> = BTreeMap::new();
        edges.entry(a).or_default().insert(b);
        edges.entry(b).or_default().insert(c);
        assert!(!creates_dependency_cycle(&edges, c, Uuid::new_v4()));
        assert!(
            creates_dependency_cycle(&edges, c, a),
            "c -> a closes a -> b -> c -> a"
        );
        assert!(creates_dependency_cycle(&edges, a, a));
        assert!(
            !creates_dependency_cycle(&edges, a, c),
            "a -> c is a shortcut, not a cycle"
        );
    }

    #[test]
    fn inputs_are_bounded_closed_and_secret_free() {
        assert!(ProjectSlug::parse("fitness-app").is_ok());
        assert!(ProjectSlug::parse("Fitness App").is_err());
        assert!(ProjectSlug::parse("-lead").is_err());

        let base = NewWorkItem {
            project_id: Uuid::new_v4(),
            title: "ok".to_owned(),
            description: String::new(),
            priority: WorkPriority::High,
            criteria: Vec::new(),
            approvals: Vec::new(),
            source_message_id: None,
        };
        assert!(base.validate().is_ok());
        assert!(NewWorkItem {
            title: "x".repeat(201),
            ..base.clone()
        }
        .validate()
        .is_err());
        assert!(NewWorkItem {
            description: format!("key: nsec1{}", "q".repeat(58)),
            ..base.clone()
        }
        .validate()
        .is_err());
        assert!(NewWorkItem {
            description: "see credential://vault/team/key".to_owned(),
            ..base.clone()
        }
        .validate()
        .is_ok());
        assert!(NewWorkItem {
            approvals: vec![
                ApprovalGateSpec {
                    gate: "review".to_owned(),
                    required: true
                },
                ApprovalGateSpec {
                    gate: "review".to_owned(),
                    required: false
                },
            ],
            ..base.clone()
        }
        .validate()
        .is_err());
        assert!(NewWorkItem {
            approvals: vec![ApprovalGateSpec {
                gate: "Human Review".to_owned(),
                required: true
            }],
            ..base.clone()
        }
        .validate()
        .is_err());
        assert!(NewWorkItem {
            source_message_id: Some("AB".repeat(32)),
            ..base.clone()
        }
        .validate()
        .is_err());
        assert!(NewWorkItem {
            source_message_id: Some("ab".repeat(32)),
            ..base
        }
        .validate()
        .is_ok());
    }
}
