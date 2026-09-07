use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::Row;
use uuid::Uuid;

use super::{interval_seconds, PgControlPlane};
use crate::error::{ControlError, Result};
use crate::ids::CompanyScope;
use crate::outbox::{OutboxFailOutcome, OutboxKind, OutboxLease};
use crate::ports::OutboxRepository;

async fn bounded_begin(control: &PgControlPlane) -> Result<sqlx::Transaction<'_, sqlx::Postgres>> {
    let mut tx = control.pool.begin().await?;
    sqlx::query("SELECT set_config('lock_timeout','500ms',true), set_config('statement_timeout','2s',true), set_config('idle_in_transaction_session_timeout','5s',true)")
        .execute(&mut *tx).await?;
    Ok(tx)
}

fn lease_from_row(row: &PgRow) -> Result<OutboxLease> {
    let kind: String = row.try_get("kind")?;
    Ok(OutboxLease {
        id: row.try_get("id")?,
        kind: OutboxKind::parse(&kind)
            .ok_or_else(|| ControlError::InvalidData(format!("outbox.kind holds {kind:?}")))?,
        dedup_key: row.try_get("dedup_key")?,
        routing_decision_id: row.try_get("routing_decision_id")?,
        employee_id: row.try_get("employee_id")?,
        run_id: row.try_get("run_id")?,
        payload: row.try_get("payload")?,
        attempt_count: row.try_get("attempt_count")?,
        max_attempts: row.try_get("max_attempts")?,
        lease_token: row.try_get("lease_token")?,
        lease_expires_at: row.try_get("lease_expires_at")?,
    })
}

impl OutboxRepository for PgControlPlane {
    async fn claim_due(
        &self,
        scope: &CompanyScope,
        kind: Option<OutboxKind>,
        worker_id: &str,
        lease: Duration,
        limit: i64,
    ) -> Result<Vec<OutboxLease>> {
        self.claim_due_filtered(scope, kind, worker_id, lease, limit, None)
            .await
    }

    async fn complete(&self, scope: &CompanyScope, lease: &OutboxLease) -> Result<bool> {
        let mut tx = bounded_begin(self).await?;
        let result = sqlx::query(
            "UPDATE outbox
                SET state = 'delivered', delivered_at = now(),
                    lease_owner = NULL, lease_token = NULL, lease_expires_at = NULL,
                    updated_at = now()
              WHERE company_id = $1 AND id = $2 AND lease_token = $3 AND state = 'pending'
                AND lease_expires_at > clock_timestamp()",
        )
        .bind(scope.company_id())
        .bind(lease.id)
        .bind(lease.lease_token)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result.rows_affected() == 1)
    }

    async fn fail(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        error: &str,
        retry_after: DateTime<Utc>,
    ) -> Result<OutboxFailOutcome> {
        let mut tx = bounded_begin(self).await?;
        let row = sqlx::query(
            "UPDATE outbox
                SET state = CASE WHEN attempt_count >= max_attempts THEN 'failed' ELSE 'pending' END,
                    retry_after = CASE WHEN attempt_count >= max_attempts THEN NULL ELSE $5 END,
                    last_error = $4,
                    lease_owner = NULL, lease_token = NULL, lease_expires_at = NULL,
                    updated_at = now()
              WHERE company_id = $1 AND id = $2 AND lease_token = $3 AND state = 'pending'
                AND lease_expires_at > clock_timestamp()
              RETURNING state",
        )
        .bind(scope.company_id())
        .bind(lease.id)
        .bind(lease.lease_token)
        .bind(error)
        .bind(retry_after)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        match row {
            None => Ok(OutboxFailOutcome::Stale),
            Some(row) => {
                let state: String = row.try_get("state")?;
                Ok(if state == "failed" {
                    OutboxFailOutcome::Terminal
                } else {
                    OutboxFailOutcome::Retrying
                })
            }
        }
    }

    async fn reopen(&self, scope: &CompanyScope, outbox_id: Uuid) -> Result<bool> {
        let mut tx = bounded_begin(self).await?;
        let result = sqlx::query(
            "UPDATE outbox
                SET state = 'pending', attempt_count = 0, retry_after = NULL, last_error = NULL,
                    lease_owner = NULL, lease_token = NULL, lease_expires_at = NULL,
                    updated_at = now()
              WHERE company_id = $1 AND id = $2 AND state = 'failed'",
        )
        .bind(scope.company_id())
        .bind(outbox_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result.rows_affected() == 1)
    }
}

impl PgControlPlane {
    /// Leases dispatches only for their immutable pinned runtime adapter, so
    /// workers for another adapter cannot consume their retry budgets.
    pub async fn claim_runtime_dispatches(
        &self,
        scope: &CompanyScope,
        runtime_adapter: &str,
        worker_id: &str,
        lease: Duration,
        limit: i64,
    ) -> Result<Vec<OutboxLease>> {
        if runtime_adapter.is_empty()
            || runtime_adapter.len() > 64
            || worker_id.is_empty()
            || worker_id.len() > 128
            || lease < Duration::from_millis(1)
            || lease > Duration::from_secs(300)
        {
            return Err(ControlError::InvalidData(
                "invalid runtime dispatch lease configuration".to_owned(),
            ));
        }
        self.claim_due_filtered(
            scope,
            Some(OutboxKind::RunDispatch),
            worker_id,
            lease,
            limit,
            Some(runtime_adapter),
        )
        .await
    }

    async fn claim_due_filtered(
        &self,
        scope: &CompanyScope,
        kind: Option<OutboxKind>,
        worker_id: &str,
        lease: Duration,
        limit: i64,
        runtime_adapter: Option<&str>,
    ) -> Result<Vec<OutboxLease>> {
        let mut tx = bounded_begin(self).await?;

        // Rows whose last lease expired with every attempt used are terminal.
        sqlx::query(
            "WITH exhausted AS (
                 SELECT company_id, id FROM outbox
                  WHERE company_id = $1 AND state = 'pending'
                    AND lease_expires_at <= clock_timestamp()
                    AND attempt_count >= max_attempts
                  ORDER BY lease_expires_at, id
                  FOR UPDATE SKIP LOCKED LIMIT 64
             ) UPDATE outbox o
                SET state = 'failed',
                    lease_owner = NULL, lease_token = NULL, lease_expires_at = NULL,
                    last_error = coalesce(last_error, 'delivery attempts exhausted'),
                    updated_at = now()
               FROM exhausted e
              WHERE o.company_id = e.company_id AND o.id = e.id",
        )
        .bind(scope.company_id())
        .execute(&mut *tx)
        .await?;

        let rows = sqlx::query(
            "WITH due AS (
                 SELECT company_id, id
                   FROM outbox
                  WHERE company_id = $1
                    AND state = 'pending'
                    AND ($2::text IS NULL OR kind = $2
                        OR ($6::text IS NOT NULL AND $2='run_dispatch' AND kind='work_run_dispatch'))
                    AND ($6::text IS NULL OR EXISTS (
                        SELECT 1 FROM routing_recipients rr
                        JOIN employee_runtime_bindings rb
                          ON rb.company_id = rr.company_id AND rb.employee_id = rr.employee_id
                         AND rb.revision_id = rr.employee_revision_id
                        WHERE rr.company_id = outbox.company_id
                          AND rr.routing_decision_id = outbox.routing_decision_id
                          AND rr.employee_id = outbox.employee_id AND rb.adapter = $6)
                        OR (outbox.kind='work_run_dispatch' AND EXISTS (
                            SELECT 1 FROM work_executions x JOIN runs r ON r.company_id=x.company_id AND r.id=x.run_id
                            JOIN employee_runtime_bindings rb ON rb.company_id=x.company_id AND rb.employee_id=x.employee_id
                                AND rb.revision_id=x.employee_revision_id
                            WHERE x.company_id=outbox.company_id AND x.run_id=outbox.run_id
                                AND x.employee_id=outbox.employee_id AND r.runtime_adapter=$6 AND rb.adapter=$6)))
                    AND (retry_after IS NULL OR retry_after <= clock_timestamp())
                    AND (lease_expires_at IS NULL OR lease_expires_at <= clock_timestamp())
                    AND attempt_count < max_attempts
                  ORDER BY created_at, id
                  FOR UPDATE SKIP LOCKED
                  LIMIT $5
             )
             UPDATE outbox o
                SET lease_owner = $3,
                    lease_token = gen_random_uuid(),
                    lease_expires_at = clock_timestamp() + make_interval(secs => $4),
                    attempt_count = o.attempt_count + 1,
                    updated_at = now()
               FROM due
              WHERE o.company_id = due.company_id AND o.id = due.id
              RETURNING o.id, o.kind, o.dedup_key, o.routing_decision_id, o.employee_id,
                        o.run_id, o.payload, o.attempt_count, o.max_attempts,
                        o.lease_token, o.lease_expires_at",
        )
        .bind(scope.company_id())
        .bind(kind.map(OutboxKind::as_str))
        .bind(worker_id)
        .bind(interval_seconds(lease))
        .bind(limit.clamp(1, 64))
        .bind(runtime_adapter)
        .fetch_all(&mut *tx)
        .await?;

        let leases = rows
            .iter()
            .map(lease_from_row)
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await?;
        Ok(leases)
    }
}
