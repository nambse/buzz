//! Bounded terminal output reconciliation; one artifact/review transaction per run.
use super::*;
use ortak_control::run_event::{assemble_final_text, FinalTextRefusal};
use sha2::{Digest, Sha256};

/// Counts from one bounded, durable Work output pass.
#[derive(Clone, Debug, Default)]
pub struct WorkOutputReport {
    /// Terminal jobs claimed, including rejected outputs.
    pub attempted: usize,
    /// Complete artifacts offered for human review.
    pub materialized: usize,
}

struct Lease {
    run_id: Uuid,
    token: Uuid,
}
enum Disposition {
    Materialized,
    Rejected(&'static str),
}

/// Reconcile at most eight terminal runs. Provider calls never occur here.
/// Retry failures propagate if their durable retry record cannot be saved.
pub async fn schedule_work_outputs(
    control: &PgControlPlane,
    scope: &CompanyScope,
    limit: usize,
) -> Result<WorkOutputReport> {
    tokio::time::timeout(
        Duration::from_secs(30),
        schedule_inner(control, scope, limit),
    )
    .await
    .map_err(|_| WorkError::OperationTimedOut)?
}

async fn schedule_inner(
    control: &PgControlPlane,
    scope: &CompanyScope,
    limit: usize,
) -> Result<WorkOutputReport> {
    // A crash on the twentieth lease still reaches a retained terminal result.
    sqlx::query("WITH due AS(SELECT company_id,run_id FROM runtime_work_outputs WHERE company_id=$1
        AND state='pending' AND attempt_count>=20 AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())
        ORDER BY next_attempt_at,run_id FOR UPDATE SKIP LOCKED LIMIT 64), closed AS(
        UPDATE runtime_work_outputs j SET state='failed',completed_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL,
        last_error_code='work_output_attempts_exhausted' FROM due WHERE j.company_id=due.company_id AND j.run_id=due.run_id
        RETURNING j.company_id,j.run_id)
        UPDATE work_executions x SET reconciled_at=clock_timestamp(),result_code='work_output_attempts_exhausted'
        FROM closed WHERE x.company_id=closed.company_id AND x.run_id=closed.run_id AND x.reconciled_at IS NULL")
        .bind(scope.company_id()).execute(control.pool()).await?;
    let mut report = WorkOutputReport::default();
    for _ in 0..limit.clamp(1, 8) {
        let row = sqlx::query("WITH due AS(SELECT company_id,run_id FROM runtime_work_outputs
            WHERE company_id=$1 AND state='pending' AND next_attempt_at<=clock_timestamp()
            AND attempt_count<20 AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())
            ORDER BY next_attempt_at,created_at,run_id FOR UPDATE SKIP LOCKED LIMIT 1)
            UPDATE runtime_work_outputs j SET lease_token=gen_random_uuid(),lease_expires_at=clock_timestamp()+interval '15 seconds',
            attempt_count=least(j.attempt_count+1,20) FROM due WHERE j.company_id=due.company_id AND j.run_id=due.run_id
            RETURNING j.run_id,j.lease_token")
            .bind(scope.company_id()).fetch_optional(control.pool()).await?;
        let Some(row) = row else { break };
        let lease = Lease {
            run_id: row.try_get("run_id")?,
            token: row.try_get("lease_token")?,
        };
        report.attempted += 1;
        let result = bounded(materialize(control, scope, &lease)).await;
        match result {
            Ok(Disposition::Materialized) => report.materialized += 1,
            Ok(Disposition::Rejected(code)) => {
                record_failure(control, scope, &lease, code, true).await?
            }
            Err(
                WorkError::AccessDenied
                | WorkError::WorkItemNotFound { .. }
                | WorkError::ProjectNotFound { .. }
                | WorkError::ProjectArchived { .. }
                | WorkError::SourceMessageNotDecided { .. }
                | WorkError::EmployeeNotAssignable { .. },
            ) => {
                record_failure(
                    control,
                    scope,
                    &lease,
                    "work_output_authority_changed",
                    true,
                )
                .await?;
            }
            Err(error) => {
                record_failure(control, scope, &lease, "work_output_retry", false).await?;
                return Err(error);
            }
        }
    }
    Ok(report)
}

async fn materialize(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &Lease,
) -> Result<Disposition> {
    let row=sqlx::query("SELECT x.requested_by,x.employee_id,x.work_item_id,a.community_id,a.channel_id,o.auth_event_id
        FROM work_executions x JOIN project_api_bindings a ON a.company_id=x.company_id AND a.project_id=x.project_id
        JOIN work_api_operations o ON o.company_id=x.company_id AND o.actor_pubkey=x.requested_by AND o.operation_id=x.operation_id
        WHERE x.company_id=$1 AND x.run_id=$2")
        .bind(scope.company_id()).bind(lease.run_id).fetch_optional(control.pool()).await?
        .ok_or_else(||invalid("Work output request provenance missing"))?;
    let employee = EmployeeId::parse(row.try_get::<String, _>("employee_id")?)?;
    let item_id: Uuid = row.try_get("work_item_id")?;
    let event: Vec<u8> = row.try_get("auth_event_id")?;
    // This is a durable delegated operation, with its original human and exact
    // project/employee audience. Current membership and grants are still rechecked.
    let principal = ApiWorkPrincipal::new(
        row.try_get("community_id")?,
        row.try_get("requested_by")?,
        event
            .as_slice()
            .try_into()
            .map_err(|_| invalid("invalid Work auth event identity"))?,
        true,
        false,
        [row.try_get("channel_id")?].into_iter().collect(),
        [employee.clone()].into_iter().collect(),
    )?;
    let service = AuthorizedWork::new(control.clone(), scope.clone(), principal);
    let (mut tx, deadline) = service.begin().await?;
    let (project, aggregate) = service.item_on(&mut tx, item_id, true).await?;
    service.contribute(project.role)?;
    service
        .execution_employee_on(&mut tx, project.channel_id, &employee)
        .await?;
    let reviewed_current: bool = sqlx::query_scalar("SELECT ortak_lock_run_reviewed_memory($1,$2)")
        .bind(scope.company_id())
        .bind(lease.run_id)
        .fetch_one(&mut *tx)
        .await?;
    if !reviewed_current {
        return Ok(Disposition::Rejected("work_output_authority_changed"));
    }
    let row=sqlx::query("SELECT x.execution_version,x.employee_revision_id,x.reconciled_at,
        r.status,r.delivery_intent,r.employee_id,r.employee_revision_id AS run_revision,r.work_item_id,
        r.routing_decision_id,r.message_id,r.root_message_id,
        j.terminal_sequence,j.state,j.lease_token,j.lease_expires_at>clock_timestamp() AS live,
        ev.event_type,pinned.manifest->'office'=active.manifest->'office' AS same_office,
        r.employee_lifecycle_epoch=emp.lifecycle_epoch AS current_lifecycle,
        EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=r.company_id AND c.run_id=r.id)
         OR EXISTS(SELECT 1 FROM run_cancel_requests c WHERE c.company_id=r.company_id AND c.run_id=r.id) AS cancelled
        FROM work_executions x JOIN runs r ON r.company_id=x.company_id AND r.id=x.run_id
        JOIN runtime_work_outputs j ON j.company_id=r.company_id AND j.run_id=r.id
        JOIN run_events ev ON ev.company_id=j.company_id AND ev.run_id=j.run_id AND ev.sequence=j.terminal_sequence
        JOIN employees emp ON emp.company_id=x.company_id AND emp.id=x.employee_id
        JOIN employee_revisions pinned ON pinned.company_id=x.company_id AND pinned.employee_id=x.employee_id AND pinned.id=x.employee_revision_id
        JOIN employee_revisions active ON active.company_id=emp.company_id AND active.employee_id=emp.id AND active.id=emp.active_revision_id
        WHERE x.company_id=$1 AND x.run_id=$2 FOR UPDATE OF r,j")
        .bind(scope.company_id()).bind(lease.run_id).fetch_one(&mut *tx).await?;
    if row.try_get::<String, _>("state")? != "pending"
        || row.try_get::<Option<Uuid>, _>("lease_token")? != Some(lease.token)
        || !row.try_get::<bool, _>("live")?
    {
        return Ok(Disposition::Rejected("work_output_stale_lease"));
    }
    if row.try_get::<String, _>("status")? != "completed"
        || row.try_get::<String, _>("event_type")? != "run.completed"
        || row
            .try_get::<Option<String>, _>("delivery_intent")?
            .as_deref()
            != Some("silent")
    {
        return Ok(Disposition::Rejected("work_output_not_completed"));
    }
    let mut item = aggregate.item;
    if project.record.project.status != ProjectStatus::Active
        || item.state != WorkState::InProgress
        || item.version != row.try_get::<i64, _>("execution_version")?
        || !item.definition_editable()
        || !item.blocking_dependencies().is_empty()
        || !row.try_get::<bool, _>("same_office")?
        || !row.try_get::<bool, _>("current_lifecycle")?
        || row.try_get::<bool, _>("cancelled")?
        || row.try_get::<Option<Uuid>, _>("work_item_id")? != Some(item_id)
        || row.try_get::<String, _>("employee_id")? != employee.as_str()
        || row.try_get::<Uuid, _>("employee_revision_id")?
            != row.try_get::<Uuid, _>("run_revision")?
        || row
            .try_get::<Option<Uuid>, _>("routing_decision_id")?
            .is_some()
        || row.try_get::<Option<Vec<u8>>, _>("message_id")?.is_some()
        || row
            .try_get::<Option<Vec<u8>>, _>("root_message_id")?
            .is_some()
        || !item.assignments.iter().any(|a| {
            a.employee_id == employee
                && a.status == AssignmentStatus::Active
                && matches!(a.role, AssignmentRole::Owner | AssignmentRole::Contributor)
        })
    {
        return Ok(Disposition::Rejected("work_output_authority_changed"));
    }
    let terminal: i64 = row.try_get("terminal_sequence")?;
    let text = match final_text(&mut tx, scope, lease.run_id, terminal).await? {
        Ok(text) => text,
        Err(code) => return Ok(Disposition::Rejected(code)),
    };
    let artifact_id = Uuid::new_v4();
    let attachment_id = Uuid::new_v4();
    let version = item.version;
    let event = item.execution_result_ready(lease.run_id, artifact_id, attachment_id)?;
    sqlx::query("INSERT INTO artifacts(company_id,id,project_id,work_item_id,run_id,terminal_sequence,employee_id,employee_revision_id,content_bytes,content_hash,size_bytes)
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(scope.company_id()).bind(artifact_id).bind(item.project_id).bind(item.id).bind(lease.run_id).bind(terminal)
        .bind(employee.as_str()).bind(row.try_get::<Uuid,_>("employee_revision_id")?)
        .bind(text.as_bytes()).bind(Sha256::digest(text.as_bytes()).as_slice()).bind(text.len() as i32).execute(&mut *tx).await?;
    let attachment = item
        .attachments
        .iter()
        .find(|a| a.id == attachment_id)
        .ok_or_else(|| invalid("result attachment missing"))?;
    insert_attachment(&mut tx, scope, item.id, attachment, &WorkActor::System).await?;
    persist_event(&mut tx, scope, &item, version, &WorkActor::System, &event).await?;
    let changed=sqlx::query("UPDATE runtime_work_outputs SET state='materialized',artifact_id=$4,completed_at=clock_timestamp(),
        lease_token=NULL,lease_expires_at=NULL,last_error_code=NULL WHERE company_id=$1 AND run_id=$2 AND state='pending'
        AND lease_token=$3 AND lease_expires_at>clock_timestamp()")
        .bind(scope.company_id()).bind(lease.run_id).bind(lease.token).bind(artifact_id).execute(&mut *tx).await?.rows_affected();
    if changed != 1 {
        return Err(WorkError::OperationTimedOut);
    }
    sqlx::query("UPDATE work_executions SET reconciled_at=clock_timestamp(),result_code='result_ready' WHERE company_id=$1 AND run_id=$2")
        .bind(scope.company_id()).bind(lease.run_id).execute(&mut *tx).await?;
    service.finish(tx, deadline).await?;
    Ok(Disposition::Materialized)
}

async fn final_text(
    c: &mut PgConnection,
    scope: &CompanyScope,
    run: Uuid,
    terminal: i64,
) -> Result<std::result::Result<String, &'static str>> {
    let turn: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload->'turn' FROM run_events WHERE company_id=$1 AND run_id=$2
        AND sequence<$3 AND event_type='assistant.delta' ORDER BY sequence DESC LIMIT 1",
    )
    .bind(scope.company_id())
    .bind(run)
    .bind(terminal)
    .fetch_optional(&mut *c)
    .await?;
    let Some(turn) = turn else {
        return Ok(Err("work_output_empty"));
    };
    let stats=sqlx::query("SELECT count(*) AS fragments,coalesce(sum(octet_length(payload::text)),0)::bigint AS bytes
        FROM run_events WHERE company_id=$1 AND run_id=$2 AND sequence<$3 AND event_type='assistant.delta' AND payload->'turn'=$4")
        .bind(scope.company_id()).bind(run).bind(terminal).bind(&turn).fetch_one(&mut *c).await?;
    if stats.try_get::<i64, _>("fragments")? > 4096
        || stats.try_get::<i64, _>("bytes")? > 1024 * 1024
    {
        return Ok(Err("work_output_fragment_limit"));
    }
    let payloads = sqlx::query_scalar(
        "SELECT payload FROM run_events WHERE company_id=$1 AND run_id=$2 AND sequence<$3
        AND event_type='assistant.delta' AND payload->'turn'=$4 ORDER BY sequence LIMIT 4097",
    )
    .bind(scope.company_id())
    .bind(run)
    .bind(terminal)
    .bind(turn)
    .fetch_all(c)
    .await?;
    Ok(
        assemble_final_text(payloads).map_err(|reason| match reason {
            FinalTextRefusal::FragmentLimit => "work_output_fragment_limit",
            FinalTextRefusal::InvalidDelta => "work_output_invalid_delta",
            FinalTextRefusal::Truncated => "work_output_truncated",
            FinalTextRefusal::Oversized => "work_output_oversized",
            FinalTextRefusal::Empty => "work_output_empty",
        }),
    )
}

async fn record_failure(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &Lease,
    code: &str,
    permanent: bool,
) -> Result<()> {
    let mut tx = control.pool().begin().await?;
    let state:Option<String>=sqlx::query_scalar("UPDATE runtime_work_outputs SET state=CASE WHEN $5 OR attempt_count>=20 THEN 'failed' ELSE 'pending' END,
        completed_at=CASE WHEN $5 OR attempt_count>=20 THEN clock_timestamp() ELSE NULL END,
        lease_token=NULL,lease_expires_at=NULL,last_error_code=$4,next_attempt_at=clock_timestamp()+interval '30 seconds'
        WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3 AND lease_expires_at>clock_timestamp() RETURNING state")
        .bind(scope.company_id()).bind(lease.run_id).bind(lease.token).bind(code).bind(permanent).fetch_optional(&mut *tx).await?;
    if state.as_deref() == Some("failed") {
        sqlx::query("UPDATE work_executions SET reconciled_at=clock_timestamp(),result_code=$3 WHERE company_id=$1 AND run_id=$2 AND reconciled_at IS NULL")
            .bind(scope.company_id()).bind(lease.run_id).bind(code).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
