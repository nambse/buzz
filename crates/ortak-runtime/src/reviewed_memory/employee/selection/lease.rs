use super::*;
use ortak_control::outbox::{OutboxKind, OutboxLease};
use sqlx::PgConnection;

pub(super) async fn lease(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
) -> RuntimeResult<OutboxLease> {
    let row = sqlx::query("SELECT kind,dedup_key,routing_decision_id,employee_id,run_id,attempt_count,max_attempts,lease_expires_at
        FROM outbox WHERE company_id=$1 AND id=$2 AND state='pending' AND lease_token=$3
        AND lease_expires_at>clock_timestamp()")
        .bind(scope.company_id()).bind(authority.outbox_id()).bind(authority.lease_token())
        .fetch_optional(connection).await?.ok_or_else(invalid)?;
    let kind = OutboxKind::parse(row.try_get::<String, _>("kind")?.as_str()).ok_or_else(invalid)?;
    if !matches!(kind, OutboxKind::RunDispatch | OutboxKind::WorkRunDispatch) {
        return Err(invalid());
    }
    Ok(OutboxLease {
        id: authority.outbox_id(),
        kind,
        dedup_key: row.try_get("dedup_key")?,
        routing_decision_id: row.try_get("routing_decision_id")?,
        employee_id: row.try_get("employee_id")?,
        run_id: row.try_get("run_id")?,
        payload: serde_json::Value::Null,
        attempt_count: row.try_get("attempt_count")?,
        max_attempts: row.try_get("max_attempts")?,
        lease_token: authority.lease_token(),
        lease_expires_at: row.try_get("lease_expires_at")?,
    })
}
