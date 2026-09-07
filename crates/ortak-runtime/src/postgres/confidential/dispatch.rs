use super::execution::{ConfidentialLease, Lane, PgConfidentialExecution};
use super::{ConfidentialAdmissionError as Error, Result};
use crate::hermes::ConfidentialRunReceipt;
use chrono::Utc;
use ortak_control::{postgres::lock_office_authority_on, CompanyScope};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

impl PgConfidentialExecution {
    pub(crate) async fn claim_dispatch(
        &self,
        scope: &CompanyScope,
    ) -> Result<Option<ConfidentialLease>> {
        let mut tx = self.begin().await?;
        lock_office_authority_on(&mut tx, scope).await?;
        let run:Option<Uuid>=sqlx::query_scalar("SELECT d.run_id FROM confidential_run_dispatches d WHERE d.company_id=$1 AND d.community_id=$2 AND d.state='pending' AND d.next_attempt_at<=clock_timestamp() AND (d.lease_expires_at IS NULL OR d.lease_expires_at+interval '5 seconds'<=clock_timestamp()) ORDER BY d.next_attempt_at,d.run_id LIMIT 1")
            .bind(scope.company_id()).bind(scope.community_id()).fetch_optional(&mut *tx).await?;
        let Some(run) = run else { return Ok(None) };
        let current: bool = sqlx::query_scalar("SELECT ortak_lock_confidential_dm($1,$2)")
            .bind(scope.company_id())
            .bind(run)
            .fetch_one(&mut *tx)
            .await?;
        Self::fence_metadata(&mut tx, scope, run).await?;
        let row=sqlx::query("SELECT attempts FROM confidential_run_dispatches WHERE company_id=$1 AND run_id=$2 AND state='pending' AND (lease_expires_at IS NULL OR lease_expires_at+interval '5 seconds'<=clock_timestamp()) FOR UPDATE SKIP LOCKED")
            .bind(scope.company_id()).bind(run).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        if !current || row.try_get::<i32, _>("attempts")? >= 3 {
            Self::stop_on(
                &mut tx,
                scope,
                run,
                if current {
                    "unavailable"
                } else {
                    "authority_changed"
                },
            )
            .await?;
            sqlx::query("UPDATE confidential_run_dispatches SET state='failed',error_code='authority_changed',finished_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL WHERE company_id=$1 AND run_id=$2")
                .bind(scope.company_id()).bind(run).execute(&mut *tx).await?;
            tx.commit().await?;
            return Ok(None);
        }
        let token = Uuid::new_v4();
        let row=sqlx::query("UPDATE confidential_run_dispatches d SET attempts=attempts+1,generation=generation+1,lease_token=$3,lease_expires_at=least(c.execution_deadline,clock_timestamp()+interval '30 seconds'),error_code=NULL FROM confidential_runs c WHERE d.company_id=$1 AND d.run_id=$2 AND c.company_id=d.company_id AND c.run_id=d.run_id RETURNING d.community_id,d.generation,d.lease_expires_at")
            .bind(scope.company_id()).bind(run).bind(token).fetch_one(&mut *tx).await?;
        let lease = ConfidentialLease {
            company: scope.company_id(),
            community: row.try_get("community_id")?,
            run,
            token,
            generation: row.try_get("generation")?,
            expires: row.try_get("lease_expires_at")?,
            lane: Lane::Dispatch,
            copy: 0,
        };
        tx.commit().await?;
        Ok(Some(lease))
    }
    pub(crate) async fn record_start_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        scope: &CompanyScope,
        lease: &ConfidentialLease,
        receipt: &ConfidentialRunReceipt,
    ) -> Result<()> {
        if receipt.runtime_run_ref.0 != format!("ortak:{}:{}", scope.company_id(), lease.run) {
            return Err(Error::Payload);
        }
        Self::fence_metadata(tx, scope, lease.run).await?;
        self.lease_on(tx, lease).await?;
        let changed=sqlx::query("UPDATE runs SET runtime_run_ref=$3,started_at=coalesce(started_at,$4),updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND (runtime_run_ref IS NULL OR runtime_run_ref=$3)")
            .bind(scope.company_id()).bind(lease.run).bind(&receipt.runtime_run_ref.0).bind(receipt.started_at).execute(&mut **tx).await?.rows_affected();
        if changed != 1 {
            return Err(Error::Refused);
        }
        let current: bool = sqlx::query_scalar("SELECT ortak_confidential_dm_current($1,$2)")
            .bind(scope.company_id())
            .bind(lease.run)
            .fetch_one(&mut **tx)
            .await?;
        if current {
            sqlx::query("UPDATE runs SET status='running',updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND status='queued'").bind(scope.company_id()).bind(lease.run).execute(&mut **tx).await?;
            sqlx::query("INSERT INTO confidential_execution_leases(company_id,community_id,run_id) VALUES($1,$2,$3) ON CONFLICT(company_id,run_id) DO NOTHING")
                .bind(scope.company_id()).bind(scope.community_id()).bind(lease.run).execute(&mut **tx).await?;
        } else {
            Self::stop_on(tx, scope, lease.run, "authority_changed").await?;
        }
        sqlx::query("UPDATE confidential_run_dispatches SET state='delivered',finished_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL,error_code=NULL WHERE company_id=$1 AND run_id=$2 AND generation=$3 AND lease_token=$4")
            .bind(scope.company_id()).bind(lease.run).bind(lease.generation).bind(lease.token).execute(&mut **tx).await?;
        Ok(())
    }
    pub(crate) async fn defer_dispatch(
        &self,
        scope: &CompanyScope,
        lease: &ConfidentialLease,
        stop: bool,
    ) -> Result<()> {
        let mut tx = self.begin().await?;
        Self::fence_metadata(&mut tx, scope, lease.run).await?;
        self.lease_on(&mut tx, lease).await?;
        if stop || lease.generation >= 3 || lease.expires <= Utc::now() {
            Self::stop_on(
                &mut tx,
                scope,
                lease.run,
                if stop {
                    "authority_changed"
                } else {
                    "unavailable"
                },
            )
            .await?;
            sqlx::query("UPDATE confidential_run_dispatches SET state='failed',error_code='unavailable',finished_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL WHERE company_id=$1 AND run_id=$2")
                .bind(scope.company_id()).bind(lease.run).execute(&mut *tx).await?;
        } else {
            sqlx::query("UPDATE confidential_run_dispatches SET next_attempt_at=clock_timestamp()+(CASE WHEN attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END),error_code='unavailable',lease_token=NULL,lease_expires_at=NULL WHERE company_id=$1 AND run_id=$2")
                .bind(scope.company_id()).bind(lease.run).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
