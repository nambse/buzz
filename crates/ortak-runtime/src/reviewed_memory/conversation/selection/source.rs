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

pub(super) async fn requires_origin(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    run: Uuid,
    project: Uuid,
) -> RuntimeResult<bool> {
    if authority.work_origin().is_some() {
        let source: Option<Vec<u8>> = sqlx::query_scalar("SELECT w.source_message_id FROM work_executions x
            JOIN work_items w ON w.company_id=x.company_id AND w.id=x.work_item_id AND w.project_id=x.project_id
            WHERE x.company_id=$1 AND x.run_id=$2 AND x.project_id=$3")
            .bind(scope.company_id()).bind(run).bind(project).fetch_optional(connection).await?.ok_or_else(invalid)?;
        Ok(source.is_some())
    } else {
        let origin: String = sqlx::query_scalar(
            "SELECT d.origin_type FROM runs r
            JOIN routing_decisions d ON d.company_id=r.company_id AND d.id=r.routing_decision_id
            WHERE r.company_id=$1 AND r.id=$2 AND d.id=$3 AND r.employee_id=$4
                AND r.employee_revision_id=$5 AND r.message_id=$6 AND r.root_message_id=$7",
        )
        .bind(scope.company_id())
        .bind(run)
        .bind(authority.routing_decision_id())
        .bind(authority.employee_id().as_str())
        .bind(authority.employee_revision_id())
        .bind(authority.message_id().map(|id| id.as_bytes().to_vec()))
        .bind(authority.root_message_id().map(|id| id.as_bytes().to_vec()))
        .fetch_optional(connection)
        .await?
        .ok_or_else(invalid)?;
        match origin.as_str() {
            "human" => Ok(true),
            "employee" | "integration" | "system" => Ok(false),
            _ => Err(invalid()),
        }
    }
}
