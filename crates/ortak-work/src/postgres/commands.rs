//! Shared transaction implementation; never exported outside this crate.

use super::*;

pub(super) async fn archive_project_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    command: &ArchiveProject,
) -> Result<ProjectRecord> {
    verify_actor(&mut *connection, scope, &command.actor).await?;
    let row = sqlx::query(PROJECT_FOR_UPDATE_SQL)
        .bind(scope.company_id())
        .bind(command.project_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(WorkError::ProjectNotFound {
            project_id: command.project_id,
        })?;
    let mut record = project_record(&row)?;
    if record.project.version != command.expected_version {
        return Err(WorkError::VersionConflict {
            record_id: command.project_id,
            expected: command.expected_version,
            actual: record.project.version,
        });
    }
    let event = record.project.archive(command.reason.clone())?;
    let updated = sqlx::query(
        "UPDATE projects
                SET status = 'archived', version = $3, updated_at = now(), archived_at = now()
              WHERE company_id = $1 AND id = $2 AND version = $4",
    )
    .bind(scope.company_id())
    .bind(command.project_id)
    .bind(record.project.version)
    .bind(command.expected_version)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if updated != 1 {
        return Err(WorkError::VersionConflict {
            record_id: command.project_id,
            expected: command.expected_version,
            actual: record.project.version,
        });
    }
    insert_project_history(
        &mut *connection,
        scope,
        &record.project,
        &command.actor,
        &event,
    )
    .await?;
    let row = sqlx::query(PROJECT_SQL)
        .bind(scope.company_id())
        .bind(command.project_id)
        .fetch_one(&mut *connection)
        .await?;
    let record = project_record(&row)?;
    Ok(record)
}

pub(super) async fn assign_employee_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    command: &AssignEmployee,
) -> Result<WorkItemAggregate> {
    verify_actor(&mut *connection, scope, &command.actor).await?;
    let mut item = lock_item(
        &mut *connection,
        scope,
        command.work_item_id,
        command.expected_version,
        ProjectLock::Share,
    )
    .await?;
    if !employee_is_active(&mut *connection, scope, &command.employee_id).await? {
        return Err(WorkError::EmployeeNotAssignable {
            employee_id: command.employee_id.clone(),
        });
    }
    let event = item.assign(command.employee_id.clone(), command.role)?;
    sqlx::query(
        "INSERT INTO work_assignments
                 (company_id, work_item_id, employee_id, role, status,
                  assigned_by_type, assigned_by_id)
             VALUES ($1, $2, $3, $4, 'active', $5, $6)
             ON CONFLICT (company_id, work_item_id, employee_id) DO UPDATE
                SET role = EXCLUDED.role,
                    status = 'active',
                    released_at = NULL,
                    assigned_by_type = EXCLUDED.assigned_by_type,
                    assigned_by_id = EXCLUDED.assigned_by_id,
                    assigned_at = now(),
                    updated_at = now()",
    )
    .bind(scope.company_id())
    .bind(item.id)
    .bind(command.employee_id.as_str())
    .bind(command.role.as_str())
    .bind(command.actor.type_str())
    .bind(command.actor.id_str())
    .execute(&mut *connection)
    .await?;
    persist_event(
        &mut *connection,
        scope,
        &item,
        command.expected_version,
        &command.actor,
        &event,
    )
    .await?;
    let aggregate = require_aggregate(&mut *connection, scope, item.id).await?;
    Ok(aggregate)
}

pub(super) async fn add_dependency_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    command: &AddDependency,
) -> Result<WorkItemAggregate> {
    verify_actor(&mut *connection, scope, &command.actor).await?;

    // Project row FOR UPDATE, then the item row: every graph mutation
    // of the project serializes on the project row, in the same
    // project → item order as every other item mutation.
    let mut item = lock_item(
        &mut *connection,
        scope,
        command.work_item_id,
        command.expected_version,
        ProjectLock::Exclusive,
    )
    .await?;
    let project_id = item.project_id;

    let target =
        sqlx::query("SELECT project_id, state FROM work_items WHERE company_id = $1 AND id = $2")
            .bind(scope.company_id())
            .bind(command.depends_on)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(WorkError::WorkItemNotFound {
                work_item_id: command.depends_on,
            })?;
    let target_project: Uuid = target.try_get("project_id")?;
    if target_project != project_id {
        return Err(WorkError::CrossProjectDependency {
            depends_on: command.depends_on,
        });
    }
    let target_state: String = target.try_get("state")?;
    let target_state = parse_state(&target_state)?;

    let mut edges: BTreeMap<Uuid, BTreeSet<Uuid>> = BTreeMap::new();
    for row in sqlx::query(
        "SELECT work_item_id, depends_on_work_item_id FROM work_dependencies
              WHERE company_id = $1 AND project_id = $2 AND released_at IS NULL
              ORDER BY work_item_id, depends_on_work_item_id LIMIT 4097",
    )
    .bind(scope.company_id())
    .bind(project_id)
    .fetch_all(&mut *connection)
    .await?
    {
        let from: Uuid = row.try_get("work_item_id")?;
        let to: Uuid = row.try_get("depends_on_work_item_id")?;
        edges.entry(from).or_default().insert(to);
    }
    if edges.values().map(BTreeSet::len).sum::<usize>() >= 4096 {
        return Err(WorkError::InvalidQuery(
            "dependency graph exceeds 4096 active edges",
        ));
    }
    if creates_dependency_cycle(&edges, command.work_item_id, command.depends_on) {
        return Err(ortak_domain::DomainError::DependencyCycle.into());
    }

    let event = item.add_dependency(command.depends_on, target_state)?;
    sqlx::query(
        "INSERT INTO work_dependencies
                 (company_id, project_id, work_item_id, depends_on_work_item_id,
                  created_by_type, created_by_id)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT(company_id,work_item_id,depends_on_work_item_id)
             DO UPDATE SET released_at=NULL",
    )
    .bind(scope.company_id())
    .bind(project_id)
    .bind(item.id)
    .bind(command.depends_on)
    .bind(command.actor.type_str())
    .bind(command.actor.id_str())
    .execute(&mut *connection)
    .await?;
    persist_event(
        &mut *connection,
        scope,
        &item,
        command.expected_version,
        &command.actor,
        &event,
    )
    .await?;
    let aggregate = require_aggregate(&mut *connection, scope, item.id).await?;
    Ok(aggregate)
}

pub(super) async fn transition_work_item_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    command: &TransitionWorkItem,
) -> Result<WorkItemAggregate> {
    verify_actor(&mut *connection, scope, &command.actor).await?;
    let mut item = lock_item(
        &mut *connection,
        scope,
        command.work_item_id,
        command.expected_version,
        ProjectLock::Share,
    )
    .await?;
    let event = item.transition(command.target, command.reason.clone())?;
    persist_event(
        &mut *connection,
        scope,
        &item,
        command.expected_version,
        &command.actor,
        &event,
    )
    .await?;
    let aggregate = require_aggregate(&mut *connection, scope, item.id).await?;
    Ok(aggregate)
}

pub(super) async fn satisfy_criterion_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    command: &SatisfyCriterion,
) -> Result<WorkItemAggregate> {
    verify_actor(&mut *connection, scope, &command.actor).await?;
    let mut item = lock_item(
        &mut *connection,
        scope,
        command.work_item_id,
        command.expected_version,
        ProjectLock::Share,
    )
    .await?;
    let event = item.satisfy_criterion(command.criterion_id, command.actor.clone())?;
    let updated = sqlx::query(
        "UPDATE work_acceptance_criteria
                SET status = 'satisfied', satisfied_by_type = $4, satisfied_by_id = $5,
                    satisfied_at = now()
              WHERE company_id = $1 AND work_item_id = $2 AND id = $3 AND status = 'pending'",
    )
    .bind(scope.company_id())
    .bind(item.id)
    .bind(command.criterion_id)
    .bind(command.actor.type_str())
    .bind(command.actor.id_str())
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if updated != 1 {
        return Err(invalid("criterion row disagrees with the aggregate"));
    }
    persist_event(
        &mut *connection,
        scope,
        &item,
        command.expected_version,
        &command.actor,
        &event,
    )
    .await?;
    let aggregate = require_aggregate(&mut *connection, scope, item.id).await?;
    Ok(aggregate)
}

pub(super) async fn resolve_approval_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    command: &ResolveApproval,
) -> Result<WorkItemAggregate> {
    verify_actor(&mut *connection, scope, &command.actor).await?;
    let mut item = lock_item(
        &mut *connection,
        scope,
        command.work_item_id,
        command.expected_version,
        ProjectLock::Share,
    )
    .await?;
    let event = item.resolve_approval(
        command.approval_id,
        command.decision,
        command.reason.clone(),
        command.actor.clone(),
    )?;
    let updated = sqlx::query(
        "UPDATE work_approvals
                SET status = $4, resolved_by_type = $5, resolved_by_id = $6, reason = $7,
                    resolved_at = now()
              WHERE company_id = $1 AND work_item_id = $2 AND id = $3 AND status = 'pending'",
    )
    .bind(scope.company_id())
    .bind(item.id)
    .bind(command.approval_id)
    .bind(command.decision.status().as_str())
    .bind(command.actor.type_str())
    .bind(command.actor.id_str())
    .bind(command.reason.as_deref())
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if updated != 1 {
        return Err(invalid("approval row disagrees with the aggregate"));
    }
    persist_event(
        &mut *connection,
        scope,
        &item,
        command.expected_version,
        &command.actor,
        &event,
    )
    .await?;
    let aggregate = require_aggregate(&mut *connection, scope, item.id).await?;
    Ok(aggregate)
}

pub(super) async fn attach_record_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    command: &AttachRecord,
) -> Result<WorkItemAggregate> {
    command.reference.validate()?;
    verify_actor(&mut *connection, scope, &command.actor).await?;
    let mut item = lock_item(
        &mut *connection,
        scope,
        command.work_item_id,
        command.expected_version,
        ProjectLock::Share,
    )
    .await?;
    if !attachment_target_exists(&mut *connection, scope, &command.reference).await? {
        return Err(WorkError::AttachmentTargetNotFound {
            kind: command.reference.kind_str(),
        });
    }
    let attachment_id = Uuid::new_v4();
    let event = item.attach(
        attachment_id,
        command.reference.clone(),
        command.label.clone(),
    )?;
    let attachment = item
        .attachments
        .iter()
        .find(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| invalid("attachment missing after domain command"))?;
    insert_attachment(&mut *connection, scope, item.id, attachment, &command.actor).await?;
    persist_event(
        &mut *connection,
        scope,
        &item,
        command.expected_version,
        &command.actor,
        &event,
    )
    .await?;
    let aggregate = require_aggregate(&mut *connection, scope, item.id).await?;
    Ok(aggregate)
}
