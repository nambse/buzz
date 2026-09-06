//! Authenticated manual Work facade. No raw transaction helper is exported.
use super::*;
use ortak_domain::NewProject;
use std::future::Future;
use std::time::Duration;

mod authority;
mod dependencies;
pub use dependencies::*;
mod decomposition;
pub use decomposition::*;
mod execution;
pub use execution::WorkExecutionReceipt;
mod execution_reads;
pub use execution_reads::{WorkExecutionView, WorkTextArtifact};
mod facts;
pub use facts::*;
mod output;
mod reviewed_exports;
pub use output::{schedule_work_outputs, WorkOutputReport};
mod queries;
mod queue;
pub use queue::{EmployeeWorkQueueEntry, EmployeeWorkQueuePage};
mod receipt;
mod types;
use receipt::fingerprint;
pub use types::*;

/// Project-authorized Work operations over a server-authenticated human.
/// Every call rechecks live Office and durable project authority in the database.
/// This facade never starts a runtime or grants visibility to run/artifact contents.
#[derive(Clone)]
pub struct AuthorizedWork {
    control: PgControlPlane,
    scope: CompanyScope,
    principal: ApiWorkPrincipal,
}
async fn bounded<T>(future: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .map_err(|_| WorkError::OperationTimedOut)?
}
impl AuthorizedWork {
    /// Bind trusted server configuration to an already resolved company.
    pub fn new(control: PgControlPlane, scope: CompanyScope, principal: ApiWorkPrincipal) -> Self {
        Self {
            control,
            scope,
            principal,
        }
    }
    fn actor(&self) -> WorkActor {
        WorkActor::Human(self.principal.public_key.clone())
    }
    /// Atomically create a one-channel project and its authenticated owner grant.
    pub async fn create_project(
        &self,
        operation_id: Uuid,
        channel_id: Uuid,
        input: NewProject,
    ) -> Result<ApiProjectCreation> {
        bounded(self.create_project_inner(operation_id, channel_id, input)).await
    }
    async fn create_project_inner(
        &self,
        op: Uuid,
        channel: Uuid,
        input: NewProject,
    ) -> Result<ApiProjectCreation> {
        input.validate()?;
        if !self.principal.operator || !self.principal.can_create_projects {
            return Err(WorkError::AccessDenied);
        }
        let hash = fingerprint((channel, &input))?;
        let (mut tx, deadline) = self.begin().await?;
        let receipt = self
            .operation_on(&mut tx, op, "create_project", &hash)
            .await?;
        if !self.channel_on(&mut tx, channel).await? {
            return Err(WorkError::AccessDenied);
        }
        if let Some(receipt) = receipt {
            let project = self.project_on(&mut tx, receipt.project_id).await?;
            if project.role != ProjectRole::Owner || project.channel_id != channel {
                return Err(WorkError::AccessDenied);
            }
            self.finish(tx, deadline).await?;
            return Ok(ApiProjectCreation {
                project,
                created: false,
            });
        }
        let creation = creation::create_project_on(
            &mut tx,
            &self.scope,
            &CreateProject {
                input: input.clone(),
                actor: self.actor(),
            },
        )
        .await?;
        let id = creation.project.project.id;
        if creation.created {
            sqlx::query("INSERT INTO project_api_bindings(company_id,project_id,community_id,channel_id,created_by)
 VALUES($1,$2,$3,$4,$5)").bind(self.scope.company_id()).bind(id).bind(self.principal.community_id)
                .bind(channel).bind(&self.principal.public_key).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO project_access_grants(company_id,project_id,actor_pubkey,role,granted_by)
 VALUES($1,$2,$3,'owner',$3)").bind(self.scope.company_id()).bind(id).bind(&self.principal.public_key).execute(&mut *tx).await?;
        }
        let project = self.project_on(&mut tx, id).await.map_err(|e| match e {
            WorkError::ProjectNotFound { .. } => WorkError::OperationConflict,
            other => other,
        })?;
        if project.role != ProjectRole::Owner
            || project.channel_id != channel
            || project.record.project.name != input.name
            || project.record.project.description != input.description
        {
            return Err(WorkError::OperationConflict);
        }
        self.record_on(
            &mut tx,
            op,
            "create_project",
            &hash,
            id,
            None,
            project.record.project.version,
            deadline,
        )
        .await?;
        self.finish(tx, deadline).await?;
        Ok(ApiProjectCreation {
            project,
            created: creation.created,
        })
    }
    /// Create manual work or promote a canonical message from the project's channel.
    /// The same operation id and payload are safe after an uncertain response.
    pub async fn create_work_item(
        &self,
        operation_id: Uuid,
        input: NewWorkItem,
    ) -> Result<WorkItemCreation> {
        bounded(self.create_work_item_inner(operation_id, input)).await
    }
    async fn create_work_item_inner(
        &self,
        op: Uuid,
        input: NewWorkItem,
    ) -> Result<WorkItemCreation> {
        input.validate()?;
        let hash = fingerprint(&input)?;
        let (mut tx, deadline) = self.begin().await?;
        let receipt = self
            .operation_on(&mut tx, op, "create_work_item", &hash)
            .await?;
        let project = self.project_on(&mut tx, input.project_id).await?;
        self.contribute(project.role)?;
        if let Some(source) = &input.source_message_id {
            self.source_on(&mut tx, project.channel_id, source).await?;
        }
        if let Some(receipt) = receipt {
            if receipt.project_id != input.project_id {
                return Err(WorkError::OperationConflict);
            }
            let id = receipt
                .work_item_id
                .ok_or_else(|| invalid("missing work receipt target"))?;
            let (_, item) = self.item_on(&mut tx, id, false).await?;
            self.finish(tx, deadline).await?;
            return Ok(WorkItemCreation {
                item,
                created: false,
            });
        }
        let created = creation::create_work_item_on(
            &mut tx,
            &self.scope,
            &CreateWorkItem {
                input,
                actor: self.actor(),
            },
        )
        .await
        .map_err(|e| match e {
            WorkError::PromotionConflict { .. } => WorkError::OperationConflict,
            other => other,
        })?;
        // This also locks and rereads a promotion replay coherently before returning it.
        let (_, item) = self.item_on(&mut tx, created.item.item.id, false).await?;
        self.record_on(
            &mut tx,
            op,
            "create_work_item",
            &hash,
            item.item.project_id,
            Some(item.item.id),
            item.item.version,
            deadline,
        )
        .await?;
        self.finish(tx, deadline).await?;
        Ok(WorkItemCreation {
            item,
            created: created.created,
        })
    }
    /// Apply one actor-free manual command under current project and source authority.
    /// Replays never apply the domain mutation a second time.
    pub async fn mutate(
        &self,
        operation_id: Uuid,
        work_item_id: Uuid,
        expected_version: i64,
        action: WorkMutation,
    ) -> Result<WorkItemAggregate> {
        bounded(self.mutate_inner(operation_id, work_item_id, expected_version, action)).await
    }
    async fn mutate_inner(
        &self,
        op: Uuid,
        id: Uuid,
        version: i64,
        action: WorkMutation,
    ) -> Result<WorkItemAggregate> {
        if version < 1 {
            return Err(WorkError::InvalidQuery(
                "expected_version must be at least 1",
            ));
        }
        if let WorkMutation::EditDefinition { definition } = &action {
            definition.validate()?;
            if definition.criteria.len() + definition.additional_criteria.len() > 16 {
                return Err(WorkError::InvalidQuery(
                    "manual definition exceeds 16 criteria",
                ));
            }
        }
        if matches!(&action,
            WorkMutation::Transition { reason: Some(reason), .. }
            | WorkMutation::ResolveApproval { reason: Some(reason), .. }
            if reason.len() > ortak_domain::MAX_WORK_REASON_BYTES)
        {
            return Err(WorkError::InvalidQuery("reason exceeds the bounded size"));
        }
        if let WorkMutation::ReleaseAssignment { reason, .. }
        | WorkMutation::Reassign { reason, .. } = &action
        {
            if reason.trim().is_empty()
                || reason.len() > ortak_domain::MAX_WORK_REASON_BYTES
                || reason.chars().any(char::is_control)
                || ortak_control::run_event::RedactionPolicy::new().redact(reason) != *reason
            {
                return Err(WorkError::InvalidQuery("invalid assignment reason"));
            }
        }
        let hash = fingerprint((id, version, &action))?;
        let (mut tx, deadline) = self.begin().await?;
        let receipt = self
            .operation_on(&mut tx, op, "mutate_work_item", &hash)
            .await?;
        let (project, item) = self.item_on(&mut tx, id, true).await?;
        if let Some(receipt) = &receipt {
            if receipt.work_item_id != Some(id) || receipt.project_id != item.item.project_id {
                return Err(WorkError::OperationConflict);
            }
        }
        // Replays authorize the operation that committed, not whichever state
        // the item reached later. Its original from-state is immutable history.
        let transition_from = match (&receipt, &action) {
            (Some(receipt), WorkMutation::Transition { target, reason }) => {
                self.replay_transition_from(&mut tx, receipt, id, version, *target, reason)
                    .await?
            }
            _ => item.item.state,
        };
        match &action {
            WorkMutation::EditDefinition { .. } => self.contribute(project.role)?,
            WorkMutation::Assign { employee_id, .. } => {
                self.contribute(project.role)?;
                self.employee_on(&mut tx, project.channel_id, employee_id)
                    .await?;
            }
            WorkMutation::ReleaseAssignment { employee_id, .. }
            | WorkMutation::Reassign { employee_id, .. } => {
                self.contribute(project.role)?;
                // Releasing an inactive or removed member is a recovery operation.
                // Only the replacement needs current employee/member eligibility.
                if !self.principal.employee_ids.contains(employee_id) {
                    return Err(WorkError::AccessDenied);
                }
                if let WorkMutation::Reassign {
                    replacement_employee_id,
                    ..
                } = &action
                {
                    self.employee_on(&mut tx, project.channel_id, replacement_employee_id)
                        .await?;
                }
            }
            WorkMutation::Transition { target, .. } => {
                if *target == WorkState::Completed
                    || transition_from == WorkState::Review && *target == WorkState::InProgress
                {
                    self.review(project.role)?;
                } else {
                    self.contribute(project.role)?;
                }
            }
            WorkMutation::SatisfyCriterion { .. } | WorkMutation::ResolveApproval { .. } => {
                self.review(project.role)?
            }
        }
        if let Some(receipt) = receipt {
            if receipt.work_item_id != Some(id) || receipt.project_id != item.item.project_id {
                return Err(WorkError::OperationConflict);
            }
            self.finish(tx, deadline).await?;
            return Ok(item);
        }
        let actor = self.actor();
        let item = match action {
            action @ (WorkMutation::ReleaseAssignment { .. } | WorkMutation::Reassign { .. }) => {
                assignment::change_on(&mut tx, &self.scope, id, version, &action, &actor).await?
            }
            WorkMutation::EditDefinition { definition } => {
                definition::edit_on(&mut tx, &self.scope, id, version, &definition, &actor).await?
            }
            WorkMutation::Assign { employee_id, role } => {
                commands::assign_employee_on(
                    &mut tx,
                    &self.scope,
                    &AssignEmployee {
                        work_item_id: id,
                        expected_version: version,
                        employee_id,
                        role,
                        actor,
                    },
                )
                .await?
            }
            WorkMutation::Transition { target, reason } => {
                commands::transition_work_item_on(
                    &mut tx,
                    &self.scope,
                    &TransitionWorkItem {
                        work_item_id: id,
                        expected_version: version,
                        target,
                        reason,
                        actor,
                    },
                )
                .await?
            }
            WorkMutation::SatisfyCriterion { criterion_id } => {
                commands::satisfy_criterion_on(
                    &mut tx,
                    &self.scope,
                    &SatisfyCriterion {
                        work_item_id: id,
                        expected_version: version,
                        criterion_id,
                        actor,
                    },
                )
                .await?
            }
            WorkMutation::ResolveApproval {
                approval_id,
                decision,
                reason,
            } => {
                commands::resolve_approval_on(
                    &mut tx,
                    &self.scope,
                    &ResolveApproval {
                        work_item_id: id,
                        expected_version: version,
                        approval_id,
                        decision,
                        reason,
                        actor,
                    },
                )
                .await?
            }
        };
        self.record_on(
            &mut tx,
            op,
            "mutate_work_item",
            &hash,
            item.item.project_id,
            Some(id),
            item.item.version,
            deadline,
        )
        .await?;
        self.finish(tx, deadline).await?;
        Ok(item)
    }
}
