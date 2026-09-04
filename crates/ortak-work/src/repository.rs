//! Repository port and its commands.
//!
//! Every command names the actor and, for item mutations, the version the
//! caller last observed. The adapter compares that version under the row
//! lock and refuses a stale write with [`WorkError::VersionConflict`].
//!
//! [`WorkError::VersionConflict`]: crate::WorkError::VersionConflict

use ortak_control::CompanyScope;
use ortak_domain::{
    ApprovalDecision, AssignmentRole, AttachmentRef, EmployeeId, NewProject, NewWorkItem,
    WorkActor, WorkState,
};
use uuid::Uuid;

use crate::error::Result;
use crate::model::{ProjectRecord, WorkItemAggregate, WorkListPage, WorkListQuery};

/// Create a project; idempotent by slug.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateProject {
    /// Validated input.
    pub input: NewProject,
    /// Who creates it.
    pub actor: WorkActor,
}

/// Outcome of project creation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectCreation {
    /// The project (new or pre-existing).
    pub project: ProjectRecord,
    /// True when this call created it.
    pub created: bool,
}

/// Archive a project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveProject {
    /// Project.
    pub project_id: Uuid,
    /// Version the caller last observed.
    pub expected_version: i64,
    /// Bounded reason.
    pub reason: Option<String>,
    /// Who archives it.
    pub actor: WorkActor,
}

/// Create a work item, or promote one from a decided Office message when
/// `input.source_message_id` is set (idempotent by message).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateWorkItem {
    /// Validated input.
    pub input: NewWorkItem,
    /// Who creates it.
    pub actor: WorkActor,
}

/// Outcome of work item creation or promotion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkItemCreation {
    /// The item (new or pre-existing for a replayed promotion).
    pub item: WorkItemAggregate,
    /// True when this call created it.
    pub created: bool,
}

/// Assign an active employee.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignEmployee {
    /// Work item.
    pub work_item_id: Uuid,
    /// Version the caller last observed.
    pub expected_version: i64,
    /// Employee; must be `active` in the company.
    pub employee_id: EmployeeId,
    /// Role.
    pub role: AssignmentRole,
    /// Who assigns.
    pub actor: WorkActor,
}

/// Add a same-project `blocked_by` dependency.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDependency {
    /// Work item that will wait.
    pub work_item_id: Uuid,
    /// Version the caller last observed.
    pub expected_version: i64,
    /// Item it waits for; same company and project.
    pub depends_on: Uuid,
    /// Who adds it.
    pub actor: WorkActor,
}

/// Move a work item through the state machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransitionWorkItem {
    /// Work item.
    pub work_item_id: Uuid,
    /// Version the caller last observed.
    pub expected_version: i64,
    /// Target state.
    pub target: WorkState,
    /// Bounded reason.
    pub reason: Option<String>,
    /// Who transitions.
    pub actor: WorkActor,
}

/// Satisfy one acceptance criterion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SatisfyCriterion {
    /// Work item.
    pub work_item_id: Uuid,
    /// Version the caller last observed.
    pub expected_version: i64,
    /// Criterion.
    pub criterion_id: Uuid,
    /// Who satisfies it.
    pub actor: WorkActor,
}

/// Resolve one approval gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolveApproval {
    /// Work item.
    pub work_item_id: Uuid,
    /// Version the caller last observed.
    pub expected_version: i64,
    /// Gate.
    pub approval_id: Uuid,
    /// Decision.
    pub decision: ApprovalDecision,
    /// Bounded reason.
    pub reason: Option<String>,
    /// Who resolves it.
    pub actor: WorkActor,
}

/// Attach a canonical company record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachRecord {
    /// Work item.
    pub work_item_id: Uuid,
    /// Version the caller last observed.
    pub expected_version: i64,
    /// Referenced record; must exist in the company.
    pub reference: AttachmentRef,
    /// Optional bounded label.
    pub label: Option<String>,
    /// Who attaches it.
    pub actor: WorkActor,
}

/// Company-scoped Work and Projects persistence.
#[allow(async_fn_in_trait)]
pub trait WorkRepository {
    /// Creates the project, or returns the existing project for the slug
    /// when its display name matches; a different name is a conflict.
    async fn create_project(
        &self,
        scope: &CompanyScope,
        command: &CreateProject,
    ) -> Result<ProjectCreation>;

    /// Archives an active project under a version compare-and-set. Its work
    /// and history remain readable.
    async fn archive_project(
        &self,
        scope: &CompanyScope,
        command: &ArchiveProject,
    ) -> Result<ProjectRecord>;

    /// Reads one project; unknown and cross-company ids fail closed.
    async fn project(&self, scope: &CompanyScope, project_id: Uuid) -> Result<ProjectRecord>;

    /// Creates a work item in `proposed`. With a source message the call is
    /// idempotent: the message must be a `decided` inbox row of the
    /// company, and a replay with the same project and creation definition
    /// returns the existing item unchanged, even after that project was
    /// archived. A replay naming a different project or definition is a
    /// [`WorkError::PromotionConflict`]. The source message is attached at
    /// creation, and so is its routing decision when that decision woke at
    /// least one employee.
    ///
    /// [`WorkError::PromotionConflict`]: crate::WorkError::PromotionConflict
    async fn create_work_item(
        &self,
        scope: &CompanyScope,
        command: &CreateWorkItem,
    ) -> Result<WorkItemCreation>;

    /// Assigns an `active` employee of the company.
    async fn assign_employee(
        &self,
        scope: &CompanyScope,
        command: &AssignEmployee,
    ) -> Result<WorkItemAggregate>;

    /// Adds a dependency after a cycle check over the project graph taken
    /// under the project row lock (`FOR UPDATE`, taken before the item row
    /// like every other item mutation). Cross-company targets are not
    /// found; cross-project targets are refused.
    async fn add_dependency(
        &self,
        scope: &CompanyScope,
        command: &AddDependency,
    ) -> Result<WorkItemAggregate>;

    /// Applies one state transition with its entry gates.
    async fn transition_work_item(
        &self,
        scope: &CompanyScope,
        command: &TransitionWorkItem,
    ) -> Result<WorkItemAggregate>;

    /// Satisfies one pending criterion.
    async fn satisfy_criterion(
        &self,
        scope: &CompanyScope,
        command: &SatisfyCriterion,
    ) -> Result<WorkItemAggregate>;

    /// Resolves one pending approval gate.
    async fn resolve_approval(
        &self,
        scope: &CompanyScope,
        command: &ResolveApproval,
    ) -> Result<WorkItemAggregate>;

    /// Attaches a record that exists in the company.
    async fn attach_record(
        &self,
        scope: &CompanyScope,
        command: &AttachRecord,
    ) -> Result<WorkItemAggregate>;

    /// Reads one aggregate with its history; unknown and cross-company ids
    /// fail closed.
    async fn work_item(
        &self,
        scope: &CompanyScope,
        work_item_id: Uuid,
    ) -> Result<WorkItemAggregate>;

    /// Lists a project's work newest first with deterministic keyset paging.
    async fn list_project_work(
        &self,
        scope: &CompanyScope,
        project_id: Uuid,
        query: &WorkListQuery,
    ) -> Result<WorkListPage>;
}
