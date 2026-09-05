use ortak_control::{CompanyScope, PgControlPlane};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use super::{sql, OutputLease};
use crate::Result;

pub(super) async fn begin(
    control: &PgControlPlane,
) -> std::result::Result<Transaction<'_, Postgres>, sqlx::Error> {
    let mut tx = control.pool().begin().await?;
    // Local settings cannot leak into a pooled connection. Server timeouts
    // also release database work if the caller drops a timed-out Rust future.
    sqlx::query(
        "SELECT set_config('lock_timeout','500ms',true),
                set_config('statement_timeout','2s',true),
                set_config('idle_in_transaction_session_timeout','5s',true)",
    )
    .execute(&mut *tx)
    .await?;
    Ok(tx)
}

pub(super) async fn exhausted(
    control: &PgControlPlane,
    scope: &CompanyScope,
    limit: usize,
) -> Result<usize> {
    let mut tx = begin(control).await?;
    let changed = sqlx::query(sql::EXHAUSTED)
        .bind(scope.company_id())
        .bind(limit as i64)
        .execute(&mut *tx)
        .await?
        .rows_affected() as usize;
    tx.commit().await?;
    Ok(changed)
}

pub(super) async fn claim(
    control: &PgControlPlane,
    scope: &CompanyScope,
    attempted: &[Uuid],
) -> Result<Option<OutputLease>> {
    let mut tx = begin(control).await?;
    let row = sqlx::query(sql::CLAIM)
        .bind(scope.company_id())
        .bind(attempted)
        .fetch_optional(&mut *tx)
        .await?;
    let lease = row
        .map(|row| {
            Ok::<_, sqlx::Error>(OutputLease {
                run_id: row.try_get("run_id")?,
                token: row.try_get("lease_token")?,
            })
        })
        .transpose()?;
    tx.commit().await?;
    Ok(lease)
}

pub(super) async fn fail(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &OutputLease,
    code: &str,
    permanent: bool,
) -> Result<Option<String>> {
    let mut tx = begin(control).await?;
    let state = sqlx::query_scalar(
        "UPDATE runtime_office_outputs SET
            state=CASE WHEN $5 OR attempt_count=20 THEN 'failed' ELSE 'pending' END,
            next_attempt_at=clock_timestamp()+LEAST(power(2,attempt_count-1),300)*interval '1 second',
            last_error_code=$4,lease_token=NULL,lease_expires_at=NULL
         WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3
           AND lease_expires_at>clock_timestamp() RETURNING state",
    )
    .bind(scope.company_id())
    .bind(lease.run_id)
    .bind(lease.token)
    .bind(code)
    .bind(permanent)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(state)
}
