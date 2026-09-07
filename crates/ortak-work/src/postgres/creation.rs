//! Shared transaction implementation; never exported outside this crate.

use super::*;

pub(super) async fn create_project_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    command: &CreateProject,
) -> Result<ProjectCreation> {
    command.input.validate()?;
    verify_actor(&mut *connection, scope, &command.actor).await?;
    let (project, event) = Project::create(Uuid::new_v4(), &command.input)?;
    let inserted = sqlx::query(
        "INSERT INTO projects
                 (company_id, id, slug, name, description, status, version,
                  created_by_type, created_by_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (company_id, slug) DO NOTHING",
    )
    .bind(scope.company_id())
    .bind(project.id)
    .bind(project.slug.as_str())
    .bind(&project.name)
    .bind(&project.description)
    .bind(project.status.as_str())
    .bind(project.version)
    .bind(command.actor.type_str())
    .bind(command.actor.id_str())
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if inserted == 0 {
        let row = sqlx::query(PROJECT_BY_SLUG_SQL)
            .bind(scope.company_id())
            .bind(project.slug.as_str())
            .fetch_optional(&mut *connection)
            .await?
            .ok_or_else(|| invalid("project vanished after a slug conflict"))?;
        let existing = project_record(&row)?;
        if existing.project.name != command.input.name {
            return Err(WorkError::ProjectConflict {
                slug: project.slug.to_string(),
            });
        }
        return Ok(ProjectCreation {
            project: existing,
            created: false,
        });
    }
    insert_project_history(&mut *connection, scope, &project, &command.actor, &event).await?;
    let row = sqlx::query(PROJECT_SQL)
        .bind(scope.company_id())
        .bind(project.id)
        .fetch_one(&mut *connection)
        .await?;
    let record = project_record(&row)?;
    Ok(ProjectCreation {
        project: record,
        created: true,
    })
}

pub(super) async fn create_work_item_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    command: &CreateWorkItem,
) -> Result<WorkItemCreation> {
    create_with_id_on(connection, scope, command, None).await
}

/// Called only after an immutable fresh-child reservation in the same transaction.
pub(super) async fn create_manual_child_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    command: &CreateWorkItem,
    child: Uuid,
) -> Result<WorkItemCreation> {
    if child.is_nil() || command.input.source_message_id.is_some() {
        return Err(invalid("decomposition requires a new manual child"));
    }
    create_with_id_on(connection, scope, command, Some(child)).await
}

async fn create_with_id_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    command: &CreateWorkItem,
    child: Option<Uuid>,
) -> Result<WorkItemCreation> {
    let input = &command.input;
    input.validate()?;
    verify_actor(&mut *connection, scope, &command.actor).await?;

    // Promotion authority: the message must be a decided inbox row of
    // this company; its dispatching decision is derived, never supplied.
    // A replay is resolved before the caller-named project is examined,
    // so an item whose project was archived after promotion is still
    // returned, and a replay that names another project or definition
    // is refused rather than silently answered with the original.
    let mut source = None;
    if let Some(hex) = &input.source_message_id {
        let message_id = MessageId::parse_hex(hex)?;
        if !source_message_is_decided(&mut *connection, scope, message_id).await? {
            return Err(WorkError::SourceMessageNotDecided {
                message_id: hex.clone(),
            });
        }
        if let Some(existing) =
            existing_item_for_source(&mut *connection, scope, message_id).await?
        {
            return replayed_promotion(&mut *connection, scope, existing, input, message_id).await;
        }
        let decision_id = waking_decision_for_message(&mut *connection, scope, message_id).await?;
        source = Some((message_id, decision_id));
    }

    // The project must exist in the company and be active; FOR SHARE
    // blocks a concurrent archive until this creation commits. Only the
    // project row is locked here (module docs: lock order).
    let project = sqlx::query(PROJECT_FOR_SHARE_SQL)
        .bind(scope.company_id())
        .bind(input.project_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(WorkError::ProjectNotFound {
            project_id: input.project_id,
        })?;
    if project_record(&project)?.project.status == ProjectStatus::Archived {
        return Err(WorkError::ProjectArchived {
            project_id: input.project_id,
        });
    }

    let ids = NewWorkItemIds {
        id: child.unwrap_or_else(Uuid::new_v4),
        criterion_ids: input.criteria.iter().map(|_| Uuid::new_v4()).collect(),
        approval_ids: input.approvals.iter().map(|_| Uuid::new_v4()).collect(),
    };
    let (mut item, event) = WorkItem::create(ids, input)?;
    if let Some((message_id, decision_id)) = &source {
        item.attach_at_creation(
            Uuid::new_v4(),
            AttachmentRef::OfficeMessage {
                message_id: message_id.to_hex(),
            },
        )?;
        if let Some(decision_id) = decision_id {
            item.attach_at_creation(
                Uuid::new_v4(),
                AttachmentRef::RoutingDecision {
                    decision_id: *decision_id,
                },
            )?;
        }
    }

    let inserted = sqlx::query(
            "INSERT INTO work_items
                 (company_id, id, project_id, title, description, priority, state, version,
                  source_message_id, source_routing_decision_id, created_by_type, created_by_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (company_id, source_message_id) WHERE source_message_id IS NOT NULL DO NOTHING",
        )
        .bind(scope.company_id())
        .bind(item.id)
        .bind(item.project_id)
        .bind(&item.title)
        .bind(&item.description)
        .bind(item.priority.as_str())
        .bind(item.state.as_str())
        .bind(item.version)
        .bind(
            source
                .as_ref()
                .map(|(message_id, _)| message_id.as_bytes().to_vec()),
        )
        .bind(source.as_ref().and_then(|(_, decision_id)| *decision_id))
        .bind(command.actor.type_str())
        .bind(command.actor.id_str())
        .execute(&mut *connection)
        .await;
    match inserted {
        Ok(ref result) if result.rows_affected() == 1 => {}
        Ok(_) if source.is_some() => {
            // A concurrent promotion of the same message won; resolve
            // this call as a replay of it under the same rules.
            let (message_id, _) = source.ok_or_else(|| invalid("source vanished"))?;
            let existing = existing_item_for_source(&mut *connection, scope, message_id)
                .await?
                .ok_or_else(|| invalid("promoted item vanished after a source conflict"))?;
            return replayed_promotion(&mut *connection, scope, existing, input, message_id).await;
        }
        Ok(_) => return Err(invalid("work insert affected no row")),
        Err(error) => return Err(error.into()),
    }

    for criterion in &item.criteria {
        sqlx::query(
            "INSERT INTO work_acceptance_criteria
                     (company_id, work_item_id, id, position, text)
                 VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(scope.company_id())
        .bind(item.id)
        .bind(criterion.id)
        .bind(i16::try_from(criterion.position).map_err(|_| invalid("too many criteria"))?)
        .bind(&criterion.text)
        .execute(&mut *connection)
        .await?;
    }
    for approval in &item.approvals {
        sqlx::query(
            "INSERT INTO work_approvals (company_id, work_item_id, id, gate, required)
                 VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(scope.company_id())
        .bind(item.id)
        .bind(approval.id)
        .bind(&approval.gate)
        .bind(approval.required)
        .execute(&mut *connection)
        .await?;
    }
    for attachment in &item.attachments {
        insert_attachment(
            &mut *connection,
            scope,
            item.id,
            attachment,
            &WorkActor::System,
        )
        .await?;
    }
    insert_history(&mut *connection, scope, &item, &command.actor, &event).await?;
    let aggregate = require_aggregate(&mut *connection, scope, item.id).await?;
    Ok(WorkItemCreation {
        item: aggregate,
        created: true,
    })
}
