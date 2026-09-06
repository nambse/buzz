//! Run-owned reader lifecycle, independent of the later immutable workspace use.
use super::*;
use ortak_control::adapter::Detail;
use ortak_control::runtime::RuntimeError;
use ortak_control::workspace::{WorkspaceExecutionObserver, WorkspaceReaderIdentity};

fn failure() -> RuntimeError {
    RuntimeError::Unavailable {
        detail: Detail::new("workspace reader ownership or containment unavailable"),
    }
}

/// Opaque observer for an already committed run-owned reader execution.
#[derive(Clone, Debug)]
pub struct ReaderObserver {
    control: PgControlPlane,
    scope: CompanyScope,
    token: Uuid,
}
impl WorkspaceExecutionObserver for ReaderObserver {
    fn execution_token(&self) -> Uuid {
        self.token
    }
    async fn started(&self, pid: Option<u32>) -> std::result::Result<(), RuntimeError> {
        let changed = sqlx::query(
            "UPDATE workspace_reader_executions SET state='running',pid=$3
            WHERE company_id=$1 AND id=$2 AND state='planned' AND owner_deadline>clock_timestamp()",
        )
        .bind(self.scope.company_id())
        .bind(self.token)
        .bind(pid.map(i64::from))
        .execute(self.control.pool())
        .await
        .map_err(|_| failure())?
        .rows_affected();
        if changed != 1 {
            return Err(failure());
        }
        Ok(())
    }
    async fn stopped(&self) -> std::result::Result<(), RuntimeError> {
        let changed = sqlx::query(
            "UPDATE workspace_reader_executions SET state='stopped',stopped_at=clock_timestamp(),
            stop_proof=CASE WHEN executable IS NULL THEN 'in_process_returned' ELSE 'reaped' END
            WHERE company_id=$1 AND id=$2 AND state IN('planned','running')",
        )
        .bind(self.scope.company_id())
        .bind(self.token)
        .execute(self.control.pool())
        .await
        .map_err(|_| failure())?
        .rows_affected();
        if changed != 1 {
            return Err(failure());
        }
        Ok(())
    }
}

pub(crate) async fn plan_reader(
    control: &PgControlPlane,
    scope: &CompanyScope,
    run: Uuid,
    grant: &WorkspaceGrant,
    request_key: &str,
    owner_lease: Uuid,
    identity: Option<WorkspaceReaderIdentity>,
) -> Result<ReaderObserver> {
    let mut tx = control.pool().begin().await?;
    // The same run row serializes reader planning with durable cancellation ACK.
    sqlx::query("SELECT id FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company_id())
        .bind(run)
        .fetch_one(&mut *tx)
        .await?;
    let cancelled: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2)
        OR EXISTS(SELECT 1 FROM run_cancel_requests WHERE company_id=$1 AND run_id=$2)",
    )
    .bind(scope.company_id())
    .bind(run)
    .fetch_one(&mut *tx)
    .await?;
    if cancelled {
        return Err(invalid(
            "workspace reader cannot start after cancellation".into(),
        ));
    }
    let community: Uuid = sqlx::query_scalar(
        "SELECT community_id FROM workspace_bindings WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company_id())
    .bind(grant.revision)
    .fetch_one(&mut *tx)
    .await?;
    let deadline:Option<chrono::DateTime<chrono::Utc>>=sqlx::query_scalar("SELECT lease_expires_at FROM outbox WHERE company_id=$1 AND run_id=$2
        AND $4='prepare' AND kind='work_run_dispatch' AND state='pending' AND lease_token=$3
        UNION ALL SELECT lease_expires_at FROM workspace_tool_actions WHERE company_id=$1 AND run_id=$2
        AND $4='read:'||call_id AND state='pending' AND lease_token=$3")
        .bind(scope.company_id()).bind(run).bind(owner_lease).bind(request_key).fetch_optional(&mut *tx).await?.flatten();
    let deadline = deadline.ok_or_else(|| invalid("reader owning lease unavailable".into()))?;
    let token = Uuid::new_v4();
    let executable = identity.as_ref().map(|i| i.executable.as_str());
    let hash = identity
        .as_ref()
        .map(|i| hex::decode(&i.sha256).map_err(|_| invalid("reader artifact hash invalid".into())))
        .transpose()?;
    sqlx::query("INSERT INTO workspace_reader_executions(company_id,community_id,run_id,id,workspace_id,request_key,
        owner_lease,owner_deadline,executable,executable_hash,operating_uid) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(scope.company_id()).bind(community).bind(run).bind(token).bind(grant.revision).bind(request_key).bind(owner_lease)
        .bind(deadline).bind(executable).bind(hash).bind(identity.as_ref().map(|i|i64::from(i.uid))).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(ReaderObserver {
        control: control.clone(),
        scope: scope.clone(),
        token,
    })
}

/// One expired, unresolved process. Expired ownership alone is never stop proof.
#[derive(Clone, Debug)]
pub struct UnresolvedReader {
    /// Owning run.
    pub run_id: Uuid,
    /// Exact immutable execution marker.
    pub execution_token: Uuid,
    /// Exact selected executable/hash/UID; absent in-process work requires
    /// separate owner-process containment and cannot be guessed stopped.
    pub identity: Option<WorkspaceReaderIdentity>,
}
/// Reads at most one expired unresolved reader for bounded process verification.
pub async fn unresolved_reader(
    control: &PgControlPlane,
    scope: &CompanyScope,
) -> Result<Option<UnresolvedReader>> {
    let row=sqlx::query("SELECT run_id,id,executable,executable_hash,operating_uid FROM workspace_reader_executions
        WHERE company_id=$1 AND state<>'stopped' AND owner_deadline<=clock_timestamp() ORDER BY owner_deadline,id LIMIT 1")
        .bind(scope.company_id()).fetch_optional(control.pool()).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let identity = if let Some(executable) = row.try_get::<Option<String>, _>("executable")? {
        Some(WorkspaceReaderIdentity {
            executable,
            sha256: hex::encode(row.try_get::<Vec<u8>, _>("executable_hash")?),
            uid: row.try_get::<i64, _>("operating_uid")? as u32,
        })
    } else {
        None
    };
    Ok(Some(UnresolvedReader {
        run_id: row.try_get("run_id")?,
        execution_token: row.try_get("id")?,
        identity,
    }))
}
/// Records only a production process-inspection proof of exact owned execution
/// absence. This is recovery accounting, never permission for a new input read.
pub async fn confirm_reader_absence(
    control: &PgControlPlane,
    scope: &CompanyScope,
    reader: &UnresolvedReader,
) -> Result<bool> {
    if reader.identity.is_none() {
        return Err(invalid(
            "in-process reader ownership cannot be inferred absent".into(),
        ));
    }
    let changed=sqlx::query("UPDATE workspace_reader_executions SET state='stopped',stop_proof='confirmed_absence',stopped_at=clock_timestamp()
        WHERE company_id=$1 AND id=$2 AND run_id=$3 AND state<>'stopped' AND owner_deadline<=clock_timestamp()")
        .bind(scope.company_id()).bind(reader.execution_token).bind(reader.run_id).execute(control.pool()).await?.rows_affected()==1;
    Ok(changed)
}

pub(crate) async fn reader_stopped(
    control: &PgControlPlane,
    scope: &CompanyScope,
    token: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_reader_executions WHERE company_id=$1 AND id=$2 AND state='stopped')")
        .bind(scope.company_id()).bind(token).fetch_one(control.pool()).await?)
}
