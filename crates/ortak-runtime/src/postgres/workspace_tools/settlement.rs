//! Recovery needs retained results and confirmed stop evidence, not new read access.
use super::*;
use crate::cancellation::{CancellationReason, RuntimeCancellationRepository};
use ortak_control::workspace::WorkspaceToolPort;

/// Schedules/settles at most one old selected run independently of the current
/// input adapter configuration. Before replaying a retained result, the owning
/// runtime's stable-key cancellation must have an actual durable ACK. The bridge
/// may then acknowledge only an identical existing result, never deliver a new
/// late result to a model.
pub async fn settle_workspace_receipts<P: WorkspaceToolPort>(
    control: &PgControlPlane,
    scope: &CompanyScope,
    port: &P,
    selected_revisions: &[Uuid],
) -> Result<bool> {
    if selected_revisions.len() > 16 {
        return Err(invalid("workspace recovery selection exceeds bound".into()));
    }
    let abandoned:Option<Uuid>=sqlx::query_scalar("SELECT r.id FROM runs r JOIN run_workspace_uses u ON u.company_id=r.company_id AND u.run_id=r.id
        WHERE r.company_id=$1 AND r.status IN('queued','running','waiting') AND NOT (u.workspace_id=ANY($2))
        AND NOT EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=r.company_id AND c.run_id=r.id)
        ORDER BY r.id LIMIT 1")
        .bind(scope.company_id()).bind(selected_revisions).fetch_optional(control.pool()).await?;
    if let Some(run) = abandoned {
        control
            .enqueue_cancellation(scope, run, CancellationReason::WorkRevoked)
            .await?;
        return Ok(true);
    }
    let row=sqlx::query("SELECT a.run_id,a.call_id,c.state AS stop_state FROM workspace_tool_actions a
        JOIN runs r ON r.company_id=a.company_id AND r.id=a.run_id
        LEFT JOIN runtime_cancellations c ON c.company_id=a.company_id AND c.run_id=a.run_id
        WHERE a.company_id=$1 AND a.state IN('pending','result_ready')
          AND a.next_attempt_at<=clock_timestamp() AND (a.lease_expires_at IS NULL OR a.lease_expires_at<=clock_timestamp())
          AND (r.status IN('completed','failed','cancelled') OR c.state='acknowledged')
          AND (c.run_id IS NULL OR c.state='acknowledged')
        ORDER BY a.next_attempt_at,a.run_id,a.ordinal LIMIT 1")
        .bind(scope.company_id()).fetch_optional(control.pool()).await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let run: Uuid = row.try_get("run_id")?;
    let call: String = row.try_get("call_id")?;
    if row.try_get::<Option<String>, _>("stop_state")?.as_deref() != Some("acknowledged") {
        control
            .enqueue_cancellation(scope, run, CancellationReason::WorkRevoked)
            .await?;
        return Ok(true);
    }
    let Some((lease, grant, result)) = claim_after_stop(control, scope, run, &call).await? else {
        return Ok(true);
    };
    let key = crate::run_idempotency_key(scope.company_id(), run);
    match tokio::time::timeout(
        std::time::Duration::from_secs(2),
        port.resolve_workspace_tool(&key, &grant, &lease.request, &result),
    )
    .await
    {
        Ok(Ok(_)) => acknowledge(control, scope, &lease).await?,
        Ok(Err(_)) | Err(_) => retry(control, scope, &lease).await?,
    }
    Ok(true)
}

async fn claim_after_stop(
    control: &PgControlPlane,
    scope: &CompanyScope,
    run: Uuid,
    call: &str,
) -> Result<Option<(ActionLease, WorkspaceGrant, WorkspaceResult)>> {
    let mut tx = control.pool().begin().await?;
    sqlx::query("SELECT id FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company_id())
        .bind(run)
        .fetch_one(&mut *tx)
        .await?;
    let stopped:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2 AND state='acknowledged')
        AND NOT EXISTS(SELECT 1 FROM workspace_reader_executions WHERE company_id=$1 AND run_id=$2 AND state<>'stopped')")
        .bind(scope.company_id()).bind(run).fetch_one(&mut *tx).await?;
    if !stopped {
        return Ok(None);
    }
    let row=sqlx::query("SELECT a.state,a.file_id,a.ordinal,a.arguments_hash,a.attempt_count,r.result_bytes
        FROM workspace_tool_actions a LEFT JOIN workspace_tool_receipts r USING(company_id,run_id,call_id)
        WHERE a.company_id=$1 AND a.run_id=$2 AND a.call_id=$3 AND a.state IN('pending','result_ready')
          AND a.next_attempt_at<=clock_timestamp() AND (a.lease_expires_at IS NULL OR a.lease_expires_at<=clock_timestamp()) FOR UPDATE OF a")
        .bind(scope.company_id()).bind(run).bind(call).fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let bytes: Option<Vec<u8>> = row.try_get("result_bytes")?;
    if bytes.is_none() || row.try_get::<i32, _>("attempt_count")? >= 3 {
        sqlx::query("UPDATE workspace_tool_actions SET state='interrupted',lease_token=NULL,lease_expires_at=NULL,updated_at=clock_timestamp()
            WHERE company_id=$1 AND run_id=$2 AND call_id=$3")
            .bind(scope.company_id()).bind(run).bind(call).execute(&mut *tx).await?;
        tx.commit().await?;
        return Ok(None);
    }
    let request = WorkspaceToolRequest {
        call_id: call.into(),
        file_id: row.try_get("file_id")?,
        ordinal: row.try_get::<i32, _>("ordinal")? as u8,
        arguments_hash: hex::encode(row.try_get::<Vec<u8>, _>("arguments_hash")?),
    };
    let (grant, _) = selected_on(&mut tx, scope, run).await?;
    let result: WorkspaceResult = serde_json::from_slice(
        bytes
            .as_deref()
            .ok_or_else(|| invalid("retained workspace result missing".into()))?,
    )
    .map_err(|_| invalid("retained workspace result invalid".into()))?;
    result.validate(&grant, &request)?;
    let token = Uuid::new_v4();
    let attempt:i32=sqlx::query_scalar("UPDATE workspace_tool_actions SET lease_token=$4,lease_expires_at=clock_timestamp()+INTERVAL '20 seconds',
        attempt_count=attempt_count+1,updated_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND call_id=$3 RETURNING attempt_count")
        .bind(scope.company_id()).bind(run).bind(call).bind(token).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Some((
        ActionLease {
            run_id: run,
            request,
            token,
            attempt,
        },
        grant,
        result,
    )))
}
