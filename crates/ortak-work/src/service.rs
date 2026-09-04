//! Thin application service in front of [`WorkRepository`].
//!
//! The service validates every command with the pure domain rules before a
//! transaction is opened, so malformed input never reaches the database,
//! and offers the one convenience the product flow needs: promoting a
//! decided Office message into a project's work.

use ortak_control::{CompanyScope, MessageId};
use ortak_domain::{
    ApprovalGateSpec, NewProject, NewWorkItem, WorkActor, WorkPriority, MAX_WORK_REASON_BYTES,
};
use uuid::Uuid;

use crate::error::{Result, WorkError};
use crate::model::{ProjectRecord, WorkItemAggregate, WorkListPage, WorkListQuery};
use crate::repository::{
    AddDependency, ArchiveProject, AssignEmployee, AttachRecord, CreateProject, CreateWorkItem,
    ProjectCreation, ResolveApproval, SatisfyCriterion, TransitionWorkItem, WorkItemCreation,
    WorkRepository,
};

/// Company-scoped Work and Projects application service.
#[derive(Clone, Debug)]
pub struct WorkService<R> {
    repository: R,
}

fn require_version(expected_version: i64) -> Result<()> {
    if expected_version < 1 {
        return Err(WorkError::InvalidQuery(
            "expected_version must be at least 1",
        ));
    }
    Ok(())
}

fn require_reason(reason: Option<&String>) -> Result<()> {
    if reason.is_some_and(|reason| reason.len() > MAX_WORK_REASON_BYTES) {
        return Err(WorkError::InvalidQuery("reason exceeds the bounded size"));
    }
    Ok(())
}

impl<R: WorkRepository> WorkService<R> {
    /// Builds a service over a repository.
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    /// Returns the underlying repository.
    pub fn repository(&self) -> &R {
        &self.repository
    }

    /// Creates a project idempotently by slug.
    pub async fn create_project(
        &self,
        scope: &CompanyScope,
        input: NewProject,
        actor: WorkActor,
    ) -> Result<ProjectCreation> {
        input.validate()?;
        actor.validate()?;
        self.repository
            .create_project(scope, &CreateProject { input, actor })
            .await
    }

    /// Archives a project.
    pub async fn archive_project(
        &self,
        scope: &CompanyScope,
        command: ArchiveProject,
    ) -> Result<ProjectRecord> {
        require_version(command.expected_version)?;
        require_reason(command.reason.as_ref())?;
        command.actor.validate()?;
        self.repository.archive_project(scope, &command).await
    }

    /// Reads one project.
    pub async fn project(&self, scope: &CompanyScope, project_id: Uuid) -> Result<ProjectRecord> {
        self.repository.project(scope, project_id).await
    }

    /// Creates a work item (or promotes one when the input names a source
    /// message).
    pub async fn create_work_item(
        &self,
        scope: &CompanyScope,
        input: NewWorkItem,
        actor: WorkActor,
    ) -> Result<WorkItemCreation> {
        input.validate()?;
        actor.validate()?;
        self.repository
            .create_work_item(scope, &CreateWorkItem { input, actor })
            .await
    }

    /// Promotes a decided Office message into a project's work. Idempotent
    /// by `(company, message)`: a replay with the same project and
    /// definition returns the existing item; a different project or
    /// definition is a [`WorkError::PromotionConflict`].
    #[allow(clippy::too_many_arguments)]
    pub async fn promote_message(
        &self,
        scope: &CompanyScope,
        project_id: Uuid,
        message_id: MessageId,
        title: String,
        description: String,
        priority: WorkPriority,
        criteria: Vec<String>,
        approvals: Vec<ApprovalGateSpec>,
        actor: WorkActor,
    ) -> Result<WorkItemCreation> {
        self.create_work_item(
            scope,
            NewWorkItem {
                project_id,
                title,
                description,
                priority,
                criteria,
                approvals,
                source_message_id: Some(message_id.to_hex()),
            },
            actor,
        )
        .await
    }

    /// Assigns an active employee.
    pub async fn assign_employee(
        &self,
        scope: &CompanyScope,
        command: AssignEmployee,
    ) -> Result<WorkItemAggregate> {
        require_version(command.expected_version)?;
        command.actor.validate()?;
        self.repository.assign_employee(scope, &command).await
    }

    /// Adds a same-project dependency with a cycle check.
    pub async fn add_dependency(
        &self,
        scope: &CompanyScope,
        command: AddDependency,
    ) -> Result<WorkItemAggregate> {
        require_version(command.expected_version)?;
        command.actor.validate()?;
        if command.depends_on == command.work_item_id {
            return Err(ortak_domain::DomainError::SelfDependency.into());
        }
        self.repository.add_dependency(scope, &command).await
    }

    /// Applies one state transition.
    pub async fn transition_work_item(
        &self,
        scope: &CompanyScope,
        command: TransitionWorkItem,
    ) -> Result<WorkItemAggregate> {
        require_version(command.expected_version)?;
        require_reason(command.reason.as_ref())?;
        command.actor.validate()?;
        self.repository.transition_work_item(scope, &command).await
    }

    /// Satisfies one criterion.
    pub async fn satisfy_criterion(
        &self,
        scope: &CompanyScope,
        command: SatisfyCriterion,
    ) -> Result<WorkItemAggregate> {
        require_version(command.expected_version)?;
        command.actor.validate()?;
        self.repository.satisfy_criterion(scope, &command).await
    }

    /// Resolves one approval gate.
    pub async fn resolve_approval(
        &self,
        scope: &CompanyScope,
        command: ResolveApproval,
    ) -> Result<WorkItemAggregate> {
        require_version(command.expected_version)?;
        require_reason(command.reason.as_ref())?;
        command.actor.validate()?;
        self.repository.resolve_approval(scope, &command).await
    }

    /// Attaches a canonical record.
    pub async fn attach_record(
        &self,
        scope: &CompanyScope,
        command: AttachRecord,
    ) -> Result<WorkItemAggregate> {
        require_version(command.expected_version)?;
        command.reference.validate()?;
        command.actor.validate()?;
        self.repository.attach_record(scope, &command).await
    }

    /// Reads one aggregate with history.
    pub async fn work_item(
        &self,
        scope: &CompanyScope,
        work_item_id: Uuid,
    ) -> Result<WorkItemAggregate> {
        self.repository.work_item(scope, work_item_id).await
    }

    /// Lists a project's work.
    pub async fn list_project_work(
        &self,
        scope: &CompanyScope,
        project_id: Uuid,
        query: &WorkListQuery,
    ) -> Result<WorkListPage> {
        self.repository
            .list_project_work(scope, project_id, query)
            .await
    }
}
