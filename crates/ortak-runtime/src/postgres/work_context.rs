//! First-snapshot Work sources under the existing Office/project/item fence.

use ortak_control::conversation_context::{ContextMessage, ContextSelection};
use ortak_control::work_context::{
    ContextArtifact, WorkContext, MAX_WORK_CONTEXT_BYTES, MAX_WORK_HISTORY_BYTES,
    MAX_WORK_MESSAGES, MAX_WORK_MESSAGE_BYTES, MAX_WORK_TEAMMATES,
};
use ortak_control::CompanyScope;
use ortak_domain::{Employee, EmployeeId};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use super::conversation_context::{bounded, employee};
use super::invalid;
use crate::authority::DispatchAuthority;
use crate::Result;

pub(super) async fn select(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    run: Uuid,
) -> Result<Option<WorkContext>> {
    let Some(work) = authority.work_origin() else {
        return Ok(None);
    };
    let row = sqlx::query("SELECT x.context_version,x.requested_version,x.execution_version,x.requested_at,
        x.reference_artifact_id,a.community_id,a.channel_id,encode(w.source_message_id,'hex') AS source_id,rev.manifest
        FROM work_executions x JOIN work_items w ON w.company_id=x.company_id AND w.id=x.work_item_id
        JOIN project_api_bindings a ON a.company_id=x.company_id AND a.project_id=x.project_id
        JOIN employee_revisions rev ON rev.company_id=x.company_id AND rev.employee_id=x.employee_id AND rev.id=x.employee_revision_id
        WHERE x.company_id=$1 AND x.run_id=$2 AND x.work_item_id=$3 AND x.project_id=$4
          AND x.employee_id=$5 AND x.employee_revision_id=$6")
        .bind(scope.company_id()).bind(run).bind(work.work_item_id).bind(work.project_id)
        .bind(authority.employee_id().as_str()).bind(authority.employee_revision_id())
        .fetch_one(&mut *connection).await?;
    match row.try_get::<i16, _>("context_version")? {
        0 => return Ok(None),
        1 => {}
        _ => return Err(invalid("unsupported Work context selection".into())),
    }
    let community: Uuid = row.try_get("community_id")?;
    let channel: Uuid = row.try_get("channel_id")?;
    let eligible = ortak_office::normalizer::channel_eligible_employees(
        &mut *connection,
        scope.company_id(),
        community,
        channel,
    )
    .await?;
    if !eligible.contains(authority.employee_id()) {
        return Err(invalid("Work context employee no longer eligible".into()));
    }
    let ids: Vec<&str> = eligible.iter().map(EmployeeId::as_str).collect();
    let roster = sqlx::query("SELECT e.active_revision_id,r.manifest FROM employees e
        JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
        WHERE e.company_id=$1 AND e.status='active' AND e.id=ANY($2) AND e.id<>$3 ORDER BY e.id LIMIT $4")
        .bind(scope.company_id()).bind(ids).bind(authority.employee_id().as_str())
        .bind(MAX_WORK_TEAMMATES as i64).fetch_all(&mut *connection).await?;
    let mut teammates = Vec::new();
    for row in roster {
        let manifest: Employee = serde_json::from_value(row.try_get("manifest")?)
            .map_err(|_| invalid("Work team manifest invalid".into()))?;
        teammates.push(employee(manifest, row.try_get("active_revision_id")?));
    }
    let manifest: Employee = serde_json::from_value(row.try_get("manifest")?)
        .map_err(|_| invalid("Work employee manifest invalid".into()))?;
    let mut context = WorkContext {
        version: 1,
        snapshot_id: run,
        work_item_id: work.work_item_id,
        project_id: work.project_id,
        requested_version: row.try_get("requested_version")?,
        execution_version: row.try_get("execution_version")?,
        channel_id: channel,
        source_message_id: row.try_get("source_id")?,
        thread_root_message_id: None,
        cutoff_received_at: row.try_get("requested_at")?,
        employee: employee(manifest, authority.employee_revision_id()),
        teammates,
        messages: Vec::new(),
        omitted_history: false,
        prior_artifact: None,
    };
    if let Some(id) = row.try_get::<Option<Uuid>, _>("reference_artifact_id")? {
        let artifact = sqlx::query("SELECT a.run_id,a.employee_id,a.content_bytes,encode(a.content_hash,'hex') AS hash,x.execution_version
            FROM artifacts a JOIN work_executions x ON x.company_id=a.company_id AND x.run_id=a.run_id
            WHERE a.company_id=$1 AND a.id=$2 AND a.work_item_id=$3 AND a.project_id=$4")
            .bind(scope.company_id()).bind(id).bind(work.work_item_id).bind(work.project_id)
            .fetch_one(&mut *connection).await?;
        context.prior_artifact = Some(ContextArtifact {
            artifact_id: id,
            run_id: artifact.try_get("run_id")?,
            employee_id: EmployeeId::parse(artifact.try_get::<String, _>("employee_id")?)
                .map_err(|_| invalid("Work artifact author invalid".into()))?,
            execution_version: artifact.try_get("execution_version")?,
            content: String::from_utf8(artifact.try_get("content_bytes")?)
                .map_err(|_| invalid("Work artifact text invalid".into()))?,
            sha256: artifact.try_get("hash")?,
        });
    }
    if let Some(source) = context.source_message_id.clone() {
        let origin = sqlx::query("SELECT encode(coalesce(t.root_event_id,ev.id),'hex') AS root_id,encode(t.parent_event_id,'hex') AS parent_id
            FROM office_inbox i JOIN events ev ON ev.community_id=$2 AND ev.id=i.event_id AND ev.created_at=i.event_created_at
            LEFT JOIN thread_metadata t ON t.community_id=ev.community_id AND t.event_id=ev.id AND t.event_created_at=ev.created_at
            WHERE i.company_id=$1 AND i.event_id=decode($4,'hex') AND i.channel_id=$3 AND i.state='decided'
              AND ev.channel_id=$3 AND ev.deleted_at IS NULL AND ev.kind IN(9,40002) AND ev.received_at<=$5")
            .bind(scope.company_id()).bind(community).bind(channel).bind(&source)
            .bind(context.cutoff_received_at).fetch_one(&mut *connection).await?;
        let root: String = origin.try_get("root_id")?;
        let parent: Option<String> = origin.try_get("parent_id")?;
        context.thread_root_message_id = Some(root.clone());
        let messages = sqlx::query(include_str!("work_context/messages.sql"))
            .bind(community)
            .bind(channel)
            .bind(context.cutoff_received_at)
            .bind(&source)
            .bind(&root)
            .bind(scope.company_id())
            .bind((MAX_WORK_MESSAGES + 1) as i64)
            .bind(&parent)
            .fetch_all(&mut *connection)
            .await?;
        context.omitted_history = messages.len() > MAX_WORK_MESSAGES;
        let mut bytes = 0;
        for row in messages.into_iter().take(MAX_WORK_MESSAGES) {
            let id: String = row.try_get("message_id")?;
            let original: String = row.try_get("content")?;
            let content = bounded(
                &original,
                MAX_WORK_MESSAGE_BYTES.min(MAX_WORK_HISTORY_BYTES - bytes),
            );
            if content.trim().is_empty() {
                if id == source || id == root || parent.as_ref() == Some(&id) {
                    return Err(invalid("Work source has no usable text".into()));
                }
                context.omitted_history = true;
                continue;
            }
            bytes += content.len();
            context.messages.push(ContextMessage {
                selection: if id == root {
                    ContextSelection::ThreadRoot
                } else {
                    ContextSelection::ThreadRecent
                },
                message_id: id,
                created_at: row.try_get("created_at")?,
                author_public_key: row.try_get("author_public_key")?,
                author_employee_id: row
                    .try_get::<Option<String>, _>("author_employee_id")?
                    .map(EmployeeId::parse)
                    .transpose()
                    .map_err(|_| invalid("Work source author invalid".into()))?,
                author_name: bounded(&row.try_get::<String, _>("author_name")?, 200),
                parent_message_id: row.try_get("parent_id")?,
                thread_root_message_id: row.try_get("thread_id")?,
                truncated: content != original,
                content,
                source_content_hash: row.try_get("content_hash")?,
            });
        }
    }
    context
        .messages
        .sort_by(|a, b| (a.created_at, &a.message_id).cmp(&(b.created_at, &b.message_id)));
    while serde_json::to_vec(&context)
        .map_err(|_| invalid("Work context encoding failed".into()))?
        .len()
        > MAX_WORK_CONTEXT_BYTES
    {
        if context.teammates.pop().is_none() {
            return Err(invalid("Work reference envelope exceeds budget".into()));
        }
    }
    if !context.valid_for(
        run,
        authority.employee_id(),
        authority.employee_revision_id(),
        work.work_item_id,
    ) {
        return Err(invalid("selected Work context invalid".into()));
    }
    Ok(Some(context))
}
