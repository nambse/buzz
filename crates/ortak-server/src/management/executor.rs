use crate::auth::lock_authority;
use ortak_control::{
    ports::CompanyDirectory,
    provisioning::{compensate_adopted, SagaOutcome},
    CompanyScope, PgControlPlane,
};
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

/// One bounded management queue pass, containing no raw selection or error.
#[derive(Debug, serde::Serialize)]
pub enum ExecutionOutcome {
    /// No eligible command is waiting.
    Idle,
    /// The durable command was processed or deferred.
    Processed {
        /// Opaque command identifier.
        command_id: Uuid,
    },
}

/// Executes at most one command through the real prepared-resource adapters.
/// Claims expire after 180s; the whole attempt stops at 170s. Only three crash/
/// transient recovery attempts are automatic; saga failures require an audited
/// retry command and never reset the saga's own step budget.
pub async fn execute_next(
    control: &PgControlPlane,
    community: Uuid,
) -> Result<ExecutionOutcome, &'static str> {
    let scope = control
        .resolve_company_for_community(community)
        .await
        .map_err(|_| "management company unavailable")?;
    reconcile(control, &scope).await?;
    let mut tx = control
        .pool()
        .begin()
        .await
        .map_err(|_| "management claim unavailable")?;
    lock_authority(&mut tx, &scope)
        .await
        .map_err(|_| "management authority unavailable")?;
    let candidate=sqlx::query("SELECT id,actor,policy_fingerprint,employee_id,channel_ids FROM employee_management_commands WHERE company_id=$1 AND status IN ('pending','running') AND attempts<3 AND next_attempt_at<=clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp()) ORDER BY created_at,id LIMIT 1")
        .bind(scope.company_id()).fetch_optional(&mut *tx).await.map_err(|_| "management claim unavailable")?;
    let Some(candidate) = candidate else {
        return Ok(ExecutionOutcome::Idle);
    };
    let id: Uuid = candidate
        .try_get("id")
        .map_err(|_| "invalid management command")?;
    let actor: String = candidate
        .try_get("actor")
        .map_err(|_| "invalid management actor")?;
    let hash: Vec<u8> = candidate
        .try_get("policy_fingerprint")
        .map_err(|_| "invalid management policy")?;
    let employee: String = candidate
        .try_get("employee_id")
        .map_err(|_| "invalid management employee")?;
    let channels: Vec<Uuid> = candidate
        .try_get("channel_ids")
        .map_err(|_| "invalid management channels")?;
    let allowed: bool = sqlx::query_scalar("SELECT ortak_management_actor_allowed($1,$2,$3,$4,$5)")
        .bind(scope.company_id())
        .bind(actor)
        .bind(hash)
        .bind(&employee)
        .bind(channels)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| "management policy unavailable")?;
    let row=sqlx::query("SELECT action,configuration,operation_id FROM employee_management_commands WHERE company_id=$1 AND id=$2 AND status IN ('pending','running') AND attempts<3 AND next_attempt_at<=clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp()) FOR UPDATE SKIP LOCKED")
        .bind(scope.company_id()).bind(id).fetch_optional(&mut *tx).await.map_err(|_| "management claim unavailable")?;
    let Some(row) = row else {
        return Ok(ExecutionOutcome::Idle);
    };
    if !allowed {
        sqlx::query("UPDATE employee_management_commands SET status='blocked',error_code='authority_revoked',lease_token=NULL,lease_expires_at=NULL,updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
            .bind(scope.company_id()).bind(id).execute(&mut *tx).await.map_err(|_| "management refusal persistence failed")?;
        tx.commit()
            .await
            .map_err(|_| "management refusal commit failed")?;
        return Ok(ExecutionOutcome::Processed { command_id: id });
    }
    let token = Uuid::new_v4();
    sqlx::query("UPDATE employee_management_commands SET status='running',attempts=attempts+1,lease_token=$3,lease_expires_at=clock_timestamp()+interval '180 seconds',error_code=NULL,updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
        .bind(scope.company_id()).bind(id).bind(token).execute(&mut *tx).await.map_err(|_| "management claim persistence failed")?;
    let action: String = row
        .try_get("action")
        .map_err(|_| "invalid management action")?;
    let configuration: Option<serde_json::Value> = row
        .try_get("configuration")
        .map_err(|_| "invalid management selection")?;
    let operation: Option<Uuid> = row
        .try_get("operation_id")
        .map_err(|_| "invalid management operation")?;
    tx.commit()
        .await
        .map_err(|_| "management claim commit failed")?;
    let result = tokio::time::timeout(Duration::from_secs(170), async {
        let bound = control
            .for_provisioning_command(&scope, id, token)
            .await
            .map_err(|_| "authority_refused")?;
        if action == "disable" {
            bound
                .disable_employee_for_command(&scope)
                .await
                .map_err(|_| "disable_refused")?;
            return Ok(true);
        }
        if action == "compensate" {
            let operation = operation.ok_or("operation_missing")?;
            // Same employee exclusion as the CLI; no configuration or secret
            // lookup is needed for the DB-only adopted retention path.
            let mut lock = control
                .pool()
                .acquire()
                .await
                .map_err(|_| "employee_lock_failed")?;
            lock.close_on_drop();
            let acquired: bool =
                sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtextextended($1,0))")
                    .bind(format!(
                        "ortak-provision-employee:{}:{}",
                        scope.company_id(),
                        employee
                    ))
                    .fetch_one(&mut *lock)
                    .await
                    .map_err(|_| "employee_lock_failed")?;
            if !acquired {
                return Err("employee_busy");
            }
            let outcome = compensate_adopted(&bound, &scope, operation)
                .await
                .map_err(|_| "compensation_refused")?;
            return Ok(matches!(
                outcome,
                SagaOutcome::Compensated { .. } | SagaOutcome::AlreadyTerminal(_)
            ));
        }
        let json = serde_json::to_string(&configuration.ok_or("selection_missing")?)
            .map_err(|_| "selection_invalid")?;
        let result = crate::provisioning::provision_with_control(bound, &json, false)
            .await
            .map_err(|_| "provisioning_interrupted")?;
        Ok(result.status == "succeeded" || result.status == "compensated")
    })
    .await;
    match result {
        Ok(Ok(true)) => finish(control, &scope, id, token, "succeeded", None, false).await?,
        Ok(Ok(false)) => {
            finish(
                control,
                &scope,
                id,
                token,
                "failed",
                Some("provisioning_step_failed"),
                false,
            )
            .await?
        }
        Ok(Err("compensation_refused")) => {
            finish(
                control,
                &scope,
                id,
                token,
                "failed",
                Some("compensation_refused"),
                false,
            )
            .await?
        }
        Ok(Err(_)) | Err(_) => {
            finish(
                control,
                &scope,
                id,
                token,
                "pending",
                Some("attempt_interrupted"),
                true,
            )
            .await?
        }
    }
    Ok(ExecutionOutcome::Processed { command_id: id })
}

async fn finish(
    control: &PgControlPlane,
    scope: &CompanyScope,
    id: Uuid,
    token: Uuid,
    status: &str,
    code: Option<&str>,
    retry: bool,
) -> Result<(), &'static str> {
    // A late future cannot acknowledge a different lease. A revoked actor's
    // already committed result may still be recorded; this is accounting only.
    sqlx::query("UPDATE employee_management_commands SET status=CASE WHEN $6 AND attempts>=3 THEN 'failed' ELSE $4 END,error_code=CASE WHEN $6 AND attempts>=3 THEN 'command_attempts_exhausted' ELSE $5 END,lease_token=NULL,lease_expires_at=NULL,next_attempt_at=clock_timestamp()+make_interval(secs=>LEAST(30,5*attempts)),updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND lease_token=$3 AND status='running'")
        .bind(scope.company_id()).bind(id).bind(token).bind(status).bind(code).bind(retry).execute(control.pool()).await.map_err(|_| "management outcome persistence failed")?;
    Ok(())
}
async fn reconcile(control: &PgControlPlane, scope: &CompanyScope) -> Result<(), &'static str> {
    sqlx::query("WITH due AS(SELECT c.id FROM employee_management_commands c JOIN provisioning_operations o ON o.company_id=c.company_id AND o.id=c.operation_id WHERE c.company_id=$1 AND c.status IN ('pending','running') AND o.status IN ('succeeded','compensated') ORDER BY c.created_at,c.id LIMIT 64 FOR UPDATE OF c SKIP LOCKED) UPDATE employee_management_commands c SET status='succeeded',error_code=NULL,lease_token=NULL,lease_expires_at=NULL,updated_at=clock_timestamp() FROM due WHERE c.company_id=$1 AND c.id=due.id")
        .bind(scope.company_id()).execute(control.pool()).await.map_err(|_| "management completion reconciliation failed")?;
    sqlx::query("WITH due AS(SELECT id FROM employee_management_commands WHERE company_id=$1 AND status IN ('pending','running') AND attempts=3 AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp()) ORDER BY created_at,id LIMIT 64 FOR UPDATE SKIP LOCKED) UPDATE employee_management_commands c SET status='failed',error_code='command_attempts_exhausted',lease_token=NULL,lease_expires_at=NULL,updated_at=clock_timestamp() FROM due WHERE c.company_id=$1 AND c.id=due.id")
        .bind(scope.company_id()).execute(control.pool()).await.map_err(|_| "management exhaustion persistence failed")?;
    Ok(())
}
