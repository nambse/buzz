//! Cohort selection is server-owned and shares the existing Office mutation fence.

use std::collections::BTreeSet;

use ortak_domain::EmployeeId;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use super::{lock_office_authority_on, PgControlPlane};
use crate::cohort::{RoutingCohort, MAX_ROUTING_COHORT_SIZE};
use crate::{CompanyScope, ControlError, Result};

pub(super) fn refused() -> ControlError {
    ControlError::InvalidProposal("central routing capture is absent, stale or outside selection")
}

pub(super) async fn lock(connection: &mut PgConnection, scope: &CompanyScope) -> Result<()> {
    sqlx::query("SELECT set_config('lock_timeout','500ms',true), set_config('statement_timeout','2s',true), set_config('idle_in_transaction_session_timeout','5s',true)")
        .execute(&mut *connection).await?;
    lock_office_authority_on(connection, scope).await?;
    Ok(())
}

/// Reads the exact current dispatch selection. Call under the Office fence
/// when the answer will authorize a write; absent configuration is false.
pub async fn routing_channel_enabled_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    channel_id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM office_routing_cohorts c
             JOIN office_company_bindings b ON b.company_id=c.company_id AND b.community_id=c.community_id
             JOIN office_routing_channels s ON s.company_id=c.company_id AND s.community_id=c.community_id
             WHERE c.company_id=$1 AND c.state='enabled' AND s.channel_id=$2
               AND ($3::uuid IS NULL OR c.community_id=$3))",
    ).bind(scope.company_id()).bind(channel_id).bind(scope.community_id())
        .fetch_one(connection).await?)
}

impl PgControlPlane {
    /// Replaces one bounded selection atomically and starts a fresh capture.
    ///
    /// Install the selected relay's atomic capture hook before calling. This
    /// explicit operation pauses dispatch until every selected channel's pinned
    /// reconciliation finishes. Resume an interrupted scan using `routing_cohort`
    /// rather than calling this again and resetting its capture generation.
    pub async fn begin_routing_capture(
        &self,
        scope: &CompanyScope,
        channel_ids: &[Uuid],
        employee_ids: &[EmployeeId],
    ) -> Result<RoutingCohort> {
        if channel_ids.is_empty()
            || employee_ids.is_empty()
            || channel_ids.len() > MAX_ROUTING_COHORT_SIZE
            || employee_ids.len() > MAX_ROUTING_COHORT_SIZE
            || channel_ids.iter().any(Uuid::is_nil)
            || channel_ids.iter().collect::<BTreeSet<_>>().len() != channel_ids.len()
            || employee_ids.iter().collect::<BTreeSet<_>>().len() != employee_ids.len()
        {
            return Err(refused());
        }
        let mut tx = self.pool.begin().await?;
        lock(&mut tx, scope).await?;
        let community_id: Uuid = sqlx::query_scalar(
            "SELECT b.community_id FROM office_company_bindings b
             JOIN companies c ON c.id=b.company_id AND c.status='active'
             JOIN communities cm ON cm.id=b.community_id AND cm.deletion_state='active' AND cm.deleted_at IS NULL
             WHERE b.company_id=$1 AND ($2::uuid IS NULL OR b.community_id=$2)",
        ).bind(scope.company_id()).bind(scope.community_id()).fetch_optional(&mut *tx).await?
            .ok_or_else(refused)?;
        let channels = sqlx::query(
            "SELECT id,channel_type::text AS kind FROM channels WHERE community_id=$1 AND id=ANY($2)
             AND channel_type IN ('stream','dm') AND archived_at IS NULL AND deleted_at IS NULL",
        )
        .bind(community_id)
        .bind(channel_ids)
        .fetch_all(&mut *tx)
        .await?;
        for channel in &channels {
            if channel.try_get::<String, _>("kind")? == "dm" {
                let direct = super::direct_channel_on(
                    &mut tx,
                    scope.company_id(),
                    scope.community_id(),
                    channel.try_get("id")?,
                )
                .await?
                .ok_or_else(refused)?;
                if !direct.permits_execution() || !employee_ids.contains(&direct.employee_id) {
                    return Err(refused());
                }
            }
        }
        let ids: Vec<_> = employee_ids.iter().map(EmployeeId::as_str).collect();
        let valid_employees: i64 =
            sqlx::query_scalar("SELECT count(*) FROM employees WHERE company_id=$1 AND id=ANY($2)")
                .bind(scope.company_id())
                .bind(ids)
                .fetch_one(&mut *tx)
                .await?;
        if channels.len() != channel_ids.len() || valid_employees != employee_ids.len() as i64 {
            return Err(refused());
        }
        sqlx::query(
            "INSERT INTO office_routing_cohorts(company_id,community_id) VALUES ($1,$2)
                     ON CONFLICT(company_id) DO UPDATE SET state='off'",
        )
        .bind(scope.company_id())
        .bind(community_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM office_routing_channels WHERE company_id=$1")
            .bind(scope.company_id())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM office_routing_employees WHERE company_id=$1")
            .bind(scope.company_id())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO office_routing_channels(company_id,community_id,channel_id)
                     SELECT $1,$2,unnest($3::uuid[])",
        )
        .bind(scope.company_id())
        .bind(community_id)
        .bind(channel_ids)
        .execute(&mut *tx)
        .await?;
        let ids: Vec<_> = employee_ids.iter().map(EmployeeId::as_str).collect();
        sqlx::query("INSERT INTO office_routing_employees(company_id,employee_id) SELECT $1,unnest($2::text[])")
            .bind(scope.company_id()).bind(ids).execute(&mut *tx).await?;
        let capture_id: Uuid = sqlx::query_scalar(
            "UPDATE office_routing_cohorts SET state='capture',capture_id=gen_random_uuid()
             WHERE company_id=$1 RETURNING capture_id",
        )
        .bind(scope.company_id())
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        let mut channels = channel_ids.to_vec();
        channels.sort();
        let mut employees = employee_ids.to_vec();
        employees.sort();
        Ok(RoutingCohort {
            company_id: scope.company_id(),
            community_id,
            capture_id,
            state: "capture".into(),
            channel_ids: channels,
            employee_ids: employees,
        })
    }

    /// Reads the current durable selection so capture/scan retries keep their generation.
    pub async fn routing_cohort(&self, scope: &CompanyScope) -> Result<Option<RoutingCohort>> {
        let mut tx = self.pool.begin().await?;
        lock(&mut tx, scope).await?;
        let row = sqlx::query(
            "SELECT community_id,capture_id,state FROM office_routing_cohorts
                              WHERE company_id=$1 AND ($2::uuid IS NULL OR community_id=$2)",
        )
        .bind(scope.company_id())
        .bind(scope.community_id())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let channel_ids: Vec<Uuid> = sqlx::query_scalar("SELECT channel_id FROM office_routing_channels WHERE company_id=$1 ORDER BY channel_id LIMIT 65")
            .bind(scope.company_id()).fetch_all(&mut *tx).await?;
        let employees: Vec<String> = sqlx::query_scalar("SELECT employee_id FROM office_routing_employees WHERE company_id=$1 ORDER BY employee_id LIMIT 65")
            .bind(scope.company_id()).fetch_all(&mut *tx).await?;
        if channel_ids.len() > MAX_ROUTING_COHORT_SIZE || employees.len() > MAX_ROUTING_COHORT_SIZE
        {
            return Err(refused());
        }
        let employee_ids = employees
            .into_iter()
            .map(EmployeeId::parse)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        tx.commit().await?;
        Ok(Some(RoutingCohort {
            company_id: scope.company_id(),
            community_id: row.try_get("community_id")?,
            capture_id: row.try_get("capture_id")?,
            state: row.try_get("state")?,
            channel_ids,
            employee_ids,
        }))
    }

    /// Enables only the exact capture whose current selected channels all have
    /// completed durable scan receipts. The database enforces that final gate.
    pub async fn enable_routing_cohort(
        &self,
        scope: &CompanyScope,
        capture_id: Uuid,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        lock(&mut tx, scope).await?;
        let changed = sqlx::query("UPDATE office_routing_cohorts SET state='enabled'
                                  WHERE company_id=$1 AND capture_id=$2 AND state IN ('capture','enabled')")
            .bind(scope.company_id()).bind(capture_id).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(refused());
        }
        tx.commit().await?;
        Ok(())
    }

    /// Stops capture and dispatch and advances the authority fence for existing runs.
    pub async fn disable_routing_cohort(&self, scope: &CompanyScope) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        lock(&mut tx, scope).await?;
        sqlx::query("UPDATE office_routing_cohorts SET state='off' WHERE company_id=$1")
            .bind(scope.company_id())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
