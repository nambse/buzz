use super::*;
use sha2::{Digest, Sha256};

pub(crate) async fn next_run(
    control: &PgControlPlane,
    scope: &CompanyScope,
    after: Option<Uuid>,
) -> Result<Option<SelectedRun>> {
    let row=sqlx::query("SELECT u.run_id,b.grant_bytes,a.call_id,a.file_id,a.arguments_hash,a.ordinal
        FROM run_workspace_uses u JOIN workspace_bindings b ON b.company_id=u.company_id AND b.id=u.workspace_id
        JOIN runs r ON r.company_id=u.company_id AND r.id=u.run_id
        LEFT JOIN LATERAL(SELECT call_id,file_id,arguments_hash,ordinal FROM workspace_tool_actions
            WHERE company_id=u.company_id AND run_id=u.run_id AND state IN('pending','result_ready')
            AND next_attempt_at<=clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())
            ORDER BY ordinal LIMIT 1) a ON true
        WHERE u.company_id=$1 AND r.runtime_run_ref IS NOT NULL AND r.status IN('running','waiting')
        ORDER BY (u.run_id>coalesce($2,'00000000-0000-0000-0000-000000000000'::uuid)) DESC,u.run_id LIMIT 1")
        .bind(scope.company_id()).bind(after).fetch_optional(control.pool()).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let request = if let Some(call_id) = row.try_get::<Option<String>, _>("call_id")? {
        Some(WorkspaceToolRequest {
            call_id,
            file_id: row.try_get("file_id")?,
            arguments_hash: hex::encode(row.try_get::<Vec<u8>, _>("arguments_hash")?),
            ordinal: row.try_get::<i32, _>("ordinal")? as u8,
        })
    } else {
        None
    };
    Ok(Some(SelectedRun {
        run_id: row.try_get("run_id")?,
        grant: decode_grant(row.try_get("grant_bytes")?)?,
        request,
    }))
}

pub(crate) async fn interrupt_run(
    control: &PgControlPlane,
    scope: &CompanyScope,
    run: Uuid,
) -> Result<()> {
    // The durable cancellation owns provider containment, including when this
    // worker no longer has the old workspace adapter configured.
    use crate::cancellation::RuntimeCancellationRepository;
    control
        .enqueue_cancellation(
            scope,
            run,
            crate::cancellation::CancellationReason::WorkRevoked,
        )
        .await?;
    Ok(())
}

pub(crate) async fn claim(
    control: &PgControlPlane,
    scope: &CompanyScope,
    run: Uuid,
    request: &WorkspaceToolRequest,
) -> Result<Option<ClaimedAction>> {
    let mut tx = control.pool().begin().await?;
    if !current_on(&mut tx, scope, run).await? {
        tx.rollback().await?;
        interrupt_run(control, scope, run).await?;
        return Ok(None);
    }
    let (grant, prepared) = selected_on(&mut tx, scope, run).await?;
    request.validate(&grant)?;
    let community: Uuid = sqlx::query_scalar(
        "SELECT community_id FROM run_workspace_uses WHERE company_id=$1 AND run_id=$2",
    )
    .bind(scope.company_id())
    .bind(run)
    .fetch_one(&mut *tx)
    .await?;
    let hash = hex::decode(&request.arguments_hash)
        .map_err(|_| invalid("workspace argument hash invalid".into()))?;
    sqlx::query("INSERT INTO workspace_tool_actions(company_id,community_id,run_id,call_id,file_id,arguments_hash,ordinal)
        VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(company_id,run_id,call_id) DO NOTHING")
        .bind(scope.company_id()).bind(community).bind(run).bind(&request.call_id).bind(request.file_id).bind(&hash)
        .bind(request.ordinal as i32).execute(&mut *tx).await?;
    let row = sqlx::query(
        "SELECT file_id,arguments_hash,ordinal,state,attempt_count FROM workspace_tool_actions
        WHERE company_id=$1 AND run_id=$2 AND call_id=$3 FOR UPDATE",
    )
    .bind(scope.company_id())
    .bind(run)
    .bind(&request.call_id)
    .fetch_one(&mut *tx)
    .await?;
    if row.try_get::<Uuid, _>("file_id")? != request.file_id
        || row.try_get::<Vec<u8>, _>("arguments_hash")? != hash
        || row.try_get::<i32, _>("ordinal")? != i32::from(request.ordinal)
    {
        return Err(invalid("workspace call identity replay differs".into()));
    }
    if matches!(
        row.try_get::<String, _>("state")?.as_str(),
        "delivered" | "interrupted"
    ) {
        tx.commit().await?;
        return Ok(None);
    }
    if row.try_get::<i32, _>("attempt_count")? >= 3 {
        let changed = sqlx::query("UPDATE workspace_tool_actions SET state='interrupted',lease_token=NULL,lease_expires_at=NULL,updated_at=clock_timestamp()
            WHERE company_id=$1 AND run_id=$2 AND call_id=$3 AND next_attempt_at<=clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())")
            .bind(scope.company_id()).bind(run).bind(&request.call_id).execute(&mut *tx).await?.rows_affected();
        if changed == 1 {
            // The exhausted action and its required stop are one persist.
            // A crash cannot retire the action before queuing containment.
            sqlx::query(
                "INSERT INTO runtime_cancellations(company_id,run_id,reason)
                VALUES($1,$2,'work_revoked') ON CONFLICT(company_id,run_id) DO NOTHING",
            )
            .bind(scope.company_id())
            .bind(run)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        return Ok(None);
    }
    let token = Uuid::new_v4();
    let attempt:Option<i32>=sqlx::query_scalar("UPDATE workspace_tool_actions SET lease_token=$4,
        lease_expires_at=clock_timestamp()+INTERVAL '20 seconds',attempt_count=attempt_count+1,updated_at=clock_timestamp()
        WHERE company_id=$1 AND run_id=$2 AND call_id=$3 AND next_attempt_at<=clock_timestamp()
          AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp()) RETURNING attempt_count")
        .bind(scope.company_id()).bind(run).bind(&request.call_id).bind(token).fetch_optional(&mut *tx).await?;
    let Some(attempt) = attempt else {
        tx.commit().await?;
        return Ok(None);
    };
    let bytes:Option<Vec<u8>>=sqlx::query_scalar("SELECT result_bytes FROM workspace_tool_receipts WHERE company_id=$1 AND run_id=$2 AND call_id=$3")
        .bind(scope.company_id()).bind(run).bind(&request.call_id).fetch_optional(&mut *tx).await?;
    let result = bytes
        .map(|bytes| {
            serde_json::from_slice::<WorkspaceResult>(&bytes)
                .map_err(|_| invalid("workspace receipt cannot be decoded".into()))
        })
        .transpose()?;
    if let Some(result) = &result {
        result.validate(&grant, request)?;
    }
    tx.commit().await?;
    Ok(Some(ClaimedAction {
        lease: ActionLease {
            run_id: run,
            request: request.clone(),
            token,
            attempt,
        },
        prepared,
        result,
    }))
}

pub(crate) async fn record(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &ActionLease,
    result: &WorkspaceResult,
) -> Result<bool> {
    let mut tx = control.pool().begin().await?;
    if !current_on(&mut tx, scope, lease.run_id).await? {
        tx.rollback().await?;
        interrupt_run(control, scope, lease.run_id).await?;
        return Ok(false);
    }
    let (grant, _) = selected_on(&mut tx, scope, lease.run_id).await?;
    result.validate(&grant, &lease.request)?;
    let row=sqlx::query("SELECT community_id FROM workspace_tool_actions WHERE company_id=$1 AND run_id=$2 AND call_id=$3
        AND state='pending' AND lease_token=$4 AND attempt_count=$5 AND lease_expires_at>clock_timestamp() FOR UPDATE")
        .bind(scope.company_id()).bind(lease.run_id).bind(&lease.request.call_id).bind(lease.token).bind(lease.attempt).fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        tx.rollback().await?;
        return Ok(false);
    };
    let bytes = result.canonical_bytes()?;
    let hash = Sha256::digest(&bytes).to_vec();
    sqlx::query("INSERT INTO workspace_tool_receipts(company_id,community_id,run_id,call_id,arguments_hash,lease_token,attempt_count,result_bytes,result_hash)
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(scope.company_id()).bind(row.try_get::<Uuid,_>("community_id")?).bind(lease.run_id).bind(&lease.request.call_id)
        .bind(hex::decode(&lease.request.arguments_hash).map_err(|_|invalid("workspace request hash invalid".into()))?)
        .bind(lease.token).bind(lease.attempt).bind(bytes).bind(hash).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE workspace_tool_actions SET state='result_ready',updated_at=clock_timestamp()
        WHERE company_id=$1 AND run_id=$2 AND call_id=$3",
    )
    .bind(scope.company_id())
    .bind(lease.run_id)
    .bind(&lease.request.call_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

pub(crate) async fn delivery_current(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &ActionLease,
) -> Result<bool> {
    let mut tx = control.pool().begin().await?;
    if !current_on(&mut tx, scope, lease.run_id).await? {
        tx.rollback().await?;
        interrupt_run(control, scope, lease.run_id).await?;
        return Ok(false);
    }
    let live:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_tool_actions WHERE company_id=$1 AND run_id=$2
        AND call_id=$3 AND state='result_ready' AND lease_token=$4 AND attempt_count=$5 AND lease_expires_at>clock_timestamp())")
        .bind(scope.company_id()).bind(lease.run_id).bind(&lease.request.call_id).bind(lease.token).bind(lease.attempt).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(live)
}

pub(crate) async fn acknowledge(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &ActionLease,
) -> Result<()> {
    let changed=sqlx::query("UPDATE workspace_tool_actions SET state='delivered',lease_token=NULL,lease_expires_at=NULL,updated_at=clock_timestamp()
        WHERE company_id=$1 AND run_id=$2 AND call_id=$3 AND state='result_ready' AND lease_token=$4
          AND attempt_count=$5 AND lease_expires_at>clock_timestamp()")
        .bind(scope.company_id()).bind(lease.run_id).bind(&lease.request.call_id).bind(lease.token).bind(lease.attempt).execute(control.pool()).await?.rows_affected();
    if changed != 1 {
        return Err(invalid("workspace acknowledgement lease changed".into()));
    }
    Ok(())
}

pub(crate) async fn retry(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &ActionLease,
) -> Result<()> {
    let changed = sqlx::query(
        "UPDATE workspace_tool_actions SET lease_token=NULL,lease_expires_at=NULL,
        next_attempt_at=clock_timestamp()+INTERVAL '1 second',updated_at=clock_timestamp()
        WHERE company_id=$1 AND run_id=$2 AND call_id=$3 AND state='result_ready' AND lease_token=$4
          AND attempt_count=$5 AND lease_expires_at>clock_timestamp()",
    )
    .bind(scope.company_id())
    .bind(lease.run_id)
    .bind(&lease.request.call_id)
    .bind(lease.token)
    .bind(lease.attempt)
    .execute(control.pool())
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(invalid("workspace retry lease changed".into()));
    }
    Ok(())
}
