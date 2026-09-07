use crate::{
    auth::{lock_authority, Principal},
    error::{ApiError, Result},
    ApiConfig, HumanGrant, Role,
};
use ortak_control::{ports::CompanyDirectory, PgControlPlane};
use sqlx::PgConnection;

pub(super) fn enabled(grant: &HumanGrant) -> bool {
    grant.role == Role::Operator && grant.can_manage_employees && grant.can_execute_provisioning
}
pub(super) fn hash(grant: &HumanGrant) -> Result<Vec<u8>> {
    super::fingerprint(grant).map_err(|_| ApiError::unavailable())
}

/// Atomically replaces execution grants from the current API server policy.
/// Only the API owner calls this at startup; executors consume the current DB
/// policy and cannot restore an older worker configuration. Missing schema is
/// accepted only when no execution grant has been configured.
pub async fn synchronize_authorizations(
    control: &PgControlPlane,
    config: &ApiConfig,
) -> std::result::Result<(), &'static str> {
    let config = config.clone().validate()?;
    let exists: bool =
        sqlx::query_scalar("SELECT to_regclass('employee_management_policies') IS NOT NULL")
            .fetch_one(control.pool())
            .await
            .map_err(|_| "management policy schema unavailable")?;
    if !exists {
        return if config.humans.iter().any(enabled) {
            Err("management schema is required")
        } else {
            Ok(())
        };
    }
    let scope = control
        .resolve_company_for_community(config.community_id)
        .await
        .map_err(|_| "management company unavailable")?;
    let mut tx = control
        .pool()
        .begin()
        .await
        .map_err(|_| "management policy unavailable")?;
    lock_authority(&mut tx, &scope)
        .await
        .map_err(|_| "management authority unavailable")?;
    sqlx::query("UPDATE employee_management_policies SET enabled=false WHERE company_id=$1")
        .bind(scope.company_id())
        .execute(&mut *tx)
        .await
        .map_err(|_| "management policy update failed")?;
    for grant in config.humans.iter().filter(|g| enabled(g)) {
        let employees: Vec<_> = grant.employee_ids.iter().map(|id| id.as_str()).collect();
        sqlx::query("INSERT INTO employee_management_policies(company_id,public_key,fingerprint,enabled,employee_ids,channel_ids) VALUES($1,$2,$3,true,$4,$5) ON CONFLICT(company_id,public_key) DO UPDATE SET fingerprint=EXCLUDED.fingerprint,enabled=true,employee_ids=EXCLUDED.employee_ids,channel_ids=EXCLUDED.channel_ids")
            .bind(scope.company_id()).bind(&grant.public_key).bind(hash(grant).map_err(|_| "invalid management grant")?)
            .bind(employees).bind(&grant.channel_ids).execute(&mut *tx).await.map_err(|_| "management policy update failed")?;
    }
    tx.commit()
        .await
        .map_err(|_| "management policy commit failed")
}

pub(super) async fn allowed_on(
    c: &mut PgConnection,
    p: &Principal,
    employee: &str,
    channels: &[uuid::Uuid],
) -> Result<bool> {
    if !enabled(&p.grant) {
        return Ok(false);
    }
    Ok(
        sqlx::query_scalar("SELECT ortak_management_actor_allowed($1,$2,$3,$4,$5)")
            .bind(p.scope.company_id())
            .bind(p.public_key.to_hex())
            .bind(hash(&p.grant)?)
            .bind(employee)
            .bind(channels)
            .fetch_one(c)
            .await?,
    )
}

pub(super) async fn audit(
    c: &mut PgConnection,
    p: &Principal,
    employee: Option<&str>,
    command: Option<uuid::Uuid>,
    action: &str,
    outcome: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO employee_management_audit(company_id,actor,auth_event_id,employee_id,command_id,action,outcome) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(p.scope.company_id()).bind(p.public_key.to_hex()).bind(&p.auth_event_id)
        .bind(employee).bind(command).bind(action).bind(outcome).execute(c).await?;
    Ok(())
}
