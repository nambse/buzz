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
        let mut tx = self.pool.begin().await?;

        // Rows whose last lease expired with every attempt used are terminal.
        sqlx::query(
            "UPDATE outbox
                SET state = 'failed',
                    lease_owner = NULL, lease_token = NULL, lease_expires_at = NULL,
                    last_error = coalesce(last_error, 'delivery attempts exhausted'),
                    updated_at = now()
              WHERE company_id = $1
                AND state = 'pending'
                AND lease_expires_at IS NOT NULL
                AND lease_expires_at <= now()
                AND attempt_count >= max_attempts",
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
                    AND ($2::text IS NULL OR kind = $2)
                    AND (retry_after IS NULL OR retry_after <= now())
                    AND (lease_expires_at IS NULL OR lease_expires_at <= now())
                    AND attempt_count < max_attempts
                  ORDER BY created_at, id
                  FOR UPDATE SKIP LOCKED
                  LIMIT $5
             )
             UPDATE outbox o
                SET lease_owner = $3,
                    lease_token = gen_random_uuid(),
                    lease_expires_at = now() + make_interval(secs => $4),
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
        .bind(limit.max(1))
        .fetch_all(&mut *tx)
        .await?;

        let leases = rows
            .iter()
            .map(lease_from_row)
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await?;
        Ok(leases)
    }

    async fn complete(&self, scope: &CompanyScope, lease: &OutboxLease) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE outbox
                SET state = 'delivered', delivered_at = now(),
                    lease_owner = NULL, lease_token = NULL, lease_expires_at = NULL,
                    updated_at = now()
              WHERE company_id = $1 AND id = $2 AND lease_token = $3 AND state = 'pending'",
        )
        .bind(scope.company_id())
        .bind(lease.id)
        .bind(lease.lease_token)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    async fn fail(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        error: &str,
        retry_after: DateTime<Utc>,
    ) -> Result<OutboxFailOutcome> {
        let row = sqlx::query(
            "UPDATE outbox
                SET state = CASE WHEN attempt_count >= max_attempts THEN 'failed' ELSE 'pending' END,
                    retry_after = CASE WHEN attempt_count >= max_attempts THEN NULL ELSE $5 END,
                    last_error = $4,
                    lease_owner = NULL, lease_token = NULL, lease_expires_at = NULL,
                    updated_at = now()
              WHERE company_id = $1 AND id = $2 AND lease_token = $3 AND state = 'pending'
              RETURNING state",
        )
        .bind(scope.company_id())
        .bind(lease.id)
        .bind(lease.lease_token)
        .bind(error)
        .bind(retry_after)
        .fetch_optional(&self.pool)
        .await?;
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
        let result = sqlx::query(
            "UPDATE outbox
                SET state = 'pending', attempt_count = 0, retry_after = NULL, last_error = NULL,
                    lease_owner = NULL, lease_token = NULL, lease_expires_at = NULL,
                    updated_at = now()
              WHERE company_id = $1 AND id = $2 AND state = 'failed'",
        )
        .bind(scope.company_id())
        .bind(outbox_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }
}
