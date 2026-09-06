//! New protected execution repository. It is unmounted until the source gate.
use super::{
    ConfidentialAdmissionError as Error, CurrentConfidentialPayload, PgConfidentialRuns, Result,
};
use chrono::{DateTime, Utc};
use ortak_control::{postgres::lock_office_authority_on, CompanyScope};
use ortak_domain::RuntimeBinding;
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

/// Dedicated protected execution pool; does not start a worker or select keys.
pub struct PgConfidentialExecution {
    pub(super) pool: PgPool,
}
/// Repository-issued, unforgeable lease. No Clone, Debug, Serialize or raw token.
pub(crate) struct ConfidentialLease {
    pub(super) company: Uuid,
    pub(super) community: Uuid,
    pub(crate) run: Uuid,
    pub(super) token: Uuid,
    pub(super) generation: i64,
    pub(super) lane: Lane,
    pub(crate) expires: DateTime<Utc>,
    pub(super) copy: i32,
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Lane {
    Dispatch,
    Observe,
    Seal,
    Cancel,
    Publish,
}
pub(crate) struct ExecutionCurrent {
    pub(crate) payload: CurrentConfidentialPayload,
    pub(crate) binding: RuntimeBinding,
    pub(crate) reply_to: Option<String>,
}
impl PgConfidentialExecution {
    /// Constructs inactive explicit storage. The integrating migration must add
    /// the execution fragment before any operation is invoked.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
    pub(crate) async fn begin(&self) -> Result<Transaction<'_, Postgres>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout='2s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL statement_timeout='10s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL idle_in_transaction_session_timeout='15s'")
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
    pub(crate) async fn current_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        scope: &CompanyScope,
        lease: &ConfidentialLease,
    ) -> Result<ExecutionCurrent> {
        if lease.company != scope.company_id()
            || Some(lease.community) != scope.community_id()
            || lease.expires <= Utc::now()
        {
            return Err(Error::Refused);
        }
        // Current authority is established before selecting a wrapped key. The
        // lease check follows Office/selection/source and precedes content use.
        let payload = PgConfidentialRuns::load_current_on(tx, scope, lease.run)
            .await?
            .ok_or(Error::Refused)?;
        self.lease_on(tx, lease).await?;
        let row=sqlx::query("SELECT ortak_confidential_runtime_binding(c.company_id,r.employee_revision_id) AS binding,encode(j.reply_to,'hex') AS reply_to FROM confidential_runs c JOIN runs r ON r.company_id=c.company_id AND r.id=c.run_id JOIN encrypted_dm_decrypt_jobs j ON j.company_id=c.company_id AND j.source_id=c.source_id WHERE c.company_id=$1 AND c.run_id=$2")
            .bind(lease.company).bind(lease.run).fetch_one(&mut **tx).await?;
        let binding: serde_json::Value = row.try_get("binding")?;
        let binding = serde_json::from_value(binding).map_err(|_| Error::Payload)?;
        Ok(ExecutionCurrent {
            payload,
            binding,
            reply_to: row.try_get("reply_to")?,
        })
    }
    pub(super) async fn lease_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        lease: &ConfidentialLease,
    ) -> Result<()> {
        let table = match lease.lane {
            Lane::Dispatch => "dispatch",
            Lane::Observe | Lane::Seal | Lane::Cancel => "execution",
            Lane::Publish => "reply",
        };
        let current:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM confidential_run_dispatches WHERE $6='dispatch' AND company_id=$1 AND run_id=$2 AND state='pending' AND generation=$3 AND lease_token=$4 AND lease_expires_at>$5 UNION ALL SELECT 1 FROM confidential_execution_leases WHERE $6='execution' AND company_id=$1 AND run_id=$2 AND state=$7 AND generation=$3 AND lease_token=$4 AND lease_expires_at>$5 UNION ALL SELECT 1 FROM confidential_reply_outbox WHERE $6='reply' AND company_id=$1 AND run_id=$2 AND copy=$8 AND state='pending' AND generation=$3 AND lease_token=$4 AND lease_expires_at>$5)")
            .bind(lease.company).bind(lease.run).bind(lease.generation).bind(lease.token).bind(Utc::now()).bind(table)
            .bind(match lease.lane{Lane::Cancel=>"cancelling",Lane::Seal=>"sealing",_=>"observing"}).bind(lease.copy).fetch_one(&mut **tx).await?;
        if !current {
            return Err(Error::Refused);
        }
        Ok(())
    }
    pub(super) async fn fence_metadata(
        tx: &mut Transaction<'_, Postgres>,
        scope: &CompanyScope,
        run: Uuid,
    ) -> Result<()> {
        lock_office_authority_on(tx, scope).await?;
        let present:Option<Uuid>=sqlx::query_scalar("SELECT r.id FROM runs r JOIN confidential_runs c ON c.company_id=r.company_id AND c.run_id=r.id WHERE c.company_id=$1 AND c.community_id=$2 AND c.run_id=$3 FOR UPDATE OF r")
            .bind(scope.company_id()).bind(scope.community_id()).bind(run).fetch_optional(&mut **tx).await?;
        if present.is_none() {
            return Err(Error::Refused);
        }
        Ok(())
    }
    pub(super) async fn stop_on(
        tx: &mut Transaction<'_, Postgres>,
        scope: &CompanyScope,
        run: Uuid,
        code: &str,
    ) -> Result<()> {
        if !matches!(
            code,
            "unavailable" | "authority_changed" | "protocol" | "deadline_exceeded" | "cancelled"
        ) {
            return Err(Error::Payload);
        }
        // Stop intent, local terminal metadata and protected settlement are one
        // atomic persist. No external receipt or stopped child is fabricated.
        sqlx::query("INSERT INTO runtime_cancellations(company_id,run_id,reason) VALUES($1,$2,'office_revoked') ON CONFLICT(company_id,run_id) DO NOTHING")
            .bind(scope.company_id()).bind(run).execute(&mut **tx).await?;
        sqlx::query("UPDATE runs SET status='cancelled',cancel_reason='office_revoked',error_code='confidential_cancelled',finished_at=clock_timestamp(),updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND status IN('queued','running','waiting')")
            .bind(scope.company_id()).bind(run).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO confidential_execution_leases(company_id,community_id,run_id,state,error_code) SELECT company_id,community_id,run_id,'cancelling',$3 FROM confidential_runs WHERE company_id=$1 AND run_id=$2 ON CONFLICT(company_id,run_id) DO UPDATE SET state='cancelling',lease_token=NULL,lease_expires_at=NULL,next_attempt_at=clock_timestamp(),error_code=$3 WHERE confidential_execution_leases.state IN('observing','sealing')")
            .bind(scope.company_id()).bind(run).bind(code).execute(&mut **tx).await?;
        Ok(())
    }
}
