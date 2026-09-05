//! Durable stop requests, retried by adapter and acknowledged independently of
//! local run termination. No Office admission is needed to stop revoked work.

use std::time::Duration;

use chrono::{DateTime, Utc};
use ortak_control::run_event::{BoundedText, RedactionPolicy, RunEvent, RunEventPayload};
use ortak_control::runtime::RuntimeRunRef;
use ortak_control::{CompanyScope, ControlError, PgControlPlane};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::{Result, RunStatus, RunSupervisionError};

mod sql;

/// Durable reason to stop the adapter's run, including a lost start receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationReason {
    /// Office facts no longer authorize this execution.
    OfficeRevoked,
    /// An authorized human requested cancellation.
    HumanRequested,
}

impl CancellationReason {
    /// Stable storage and adapter reason code; contains no user text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OfficeRevoked => "office_revoked",
            Self::HumanRequested => "human_requested",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "office_revoked" => Ok(Self::OfficeRevoked),
            "human_requested" => Ok(Self::HumanRequested),
            _ => Err(invalid("unknown cancellation reason")),
        }
    }
}

/// A bounded stop attempt. Token and expiry are rechecked against durable state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CancellationLease {
    /// Company owning the run.
    pub company_id: Uuid,
    /// Stable local run id used to reconcile lost runtime start receipts.
    pub run_id: Uuid,
    /// Adapter pinned on the durable run.
    pub runtime_adapter: String,
    /// Unique fencing token for this attempt.
    pub lease_token: Uuid,
    /// Durable first reason; subsequent requests do not rewrite attribution.
    pub reason: CancellationReason,
    /// Number of attempts including this claim.
    pub attempt_count: u32,
    /// Maximum attempts, at most twenty.
    pub max_attempts: u32,
    /// Database-clock deadline of this attempt.
    pub lease_expires_at: DateTime<Utc>,
}

/// Persistence result after a verified adapter terminal acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CancellationAckOutcome {
    /// The stop and human request acknowledgement committed atomically.
    Acknowledged {
        /// Final run status, preserving an existing terminal outcome.
        status: RunStatus,
    },
    /// The lease is expired, replaced, terminal, or belongs to another company.
    StaleLease,
    /// The adapter receipt disagrees with the durable correlation.
    RuntimeRefMismatch {
        /// The durable reference that was preserved.
        durable: Option<RuntimeRunRef>,
    },
}

/// Durable result of a failed stop attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationFailOutcome {
    /// The queue will retry after bounded exponential backoff.
    Retrying,
    /// The attempt budget is exhausted; the failure remains visible.
    Failed,
    /// No write was made under an obsolete or expired lease.
    StaleLease,
}

/// Persistence seam for stop workers; no method performs an adapter call.
#[allow(async_fn_in_trait)]
pub trait RuntimeCancellationRepository {
    /// Adds one durable stop per company/run, even when locally terminal.
    /// An existing acknowledged or failed stop is never reset by replay.
    async fn enqueue_cancellation(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        reason: CancellationReason,
    ) -> Result<bool>;

    /// Imports at most 64 pending human requests and mirrors up to 64 late
    /// requests whose existing adapter stop already reached a terminal result.
    /// Returns the number of newly queued stops; never resets a terminal stop.
    async fn enqueue_human_cancellations(&self, scope: &CompanyScope, limit: u32) -> Result<u64>;

    /// Claims up to 64 due stops for the durable adapter; expired final
    /// attempts become durable failures. Lease duration must be 1ms..=300s.
    async fn claim_cancellations(
        &self,
        scope: &CompanyScope,
        adapter_name: &str,
        lease_duration: Duration,
        limit: u32,
    ) -> Result<Vec<CancellationLease>>;

    /// Call only after true terminal acknowledgement from the adapter. Locks
    /// run→queue→human request and appends normalized cancellation if needed.
    /// `None` is accepted only while the durable reference is also absent.
    async fn acknowledge_cancellation(
        &self,
        scope: &CompanyScope,
        lease: &CancellationLease,
        expected_runtime_ref: Option<&RuntimeRunRef>,
    ) -> Result<CancellationAckOutcome>;

    /// Records a bounded identifier error code and backoff, or final failure.
    async fn fail_cancellation(
        &self,
        scope: &CompanyScope,
        lease: &CancellationLease,
        error_code: &str,
    ) -> Result<CancellationFailOutcome>;
}

impl RuntimeCancellationRepository for PgControlPlane {
    async fn enqueue_cancellation(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        reason: CancellationReason,
    ) -> Result<bool> {
        let inserted = sqlx::query(
            "INSERT INTO runtime_cancellations (company_id,run_id,reason)
             SELECT company_id,id,$3 FROM runs WHERE company_id=$1 AND id=$2
             ON CONFLICT (company_id,run_id) DO NOTHING",
        )
        .bind(scope.company_id())
        .bind(run_id)
        .bind(reason.as_str())
        .execute(self.pool())
        .await?
        .rows_affected();
        Ok(inserted == 1)
    }

    async fn enqueue_human_cancellations(&self, scope: &CompanyScope, limit: u32) -> Result<u64> {
        let mut tx = self.pool().begin().await?;
        let imported = sqlx::query(sql::IMPORT_HUMAN)
            .bind(scope.company_id())
            .bind(i64::from(limit.min(64)))
            .execute(&mut *tx)
            .await?
            .rows_affected();
        sqlx::query(sql::MIRROR_LATE_HUMAN)
            .bind(scope.company_id())
            .bind(i64::from(limit.min(64)))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(imported)
    }

    async fn claim_cancellations(
        &self,
        scope: &CompanyScope,
        adapter_name: &str,
        lease_duration: Duration,
        limit: u32,
    ) -> Result<Vec<CancellationLease>> {
        if lease_duration < Duration::from_millis(1) || lease_duration > Duration::from_secs(300) {
            return Err(invalid("cancellation lease duration must be 1ms..=300s"));
        }
        let mut tx = self.pool().begin().await?;
        let cap = i64::from(limit.min(64));
        let mut changed: Vec<Uuid> = sqlx::query_scalar(sql::EXHAUSTED)
            .bind(scope.company_id())
            .bind(adapter_name)
            .bind(cap)
            .fetch_all(&mut *tx)
            .await?;
        let rows = sqlx::query(sql::CLAIM)
            .bind(scope.company_id())
            .bind(adapter_name)
            .bind(cap)
            .bind(lease_duration.as_millis() as i64)
            .fetch_all(&mut *tx)
            .await?;
        let mut leases = Vec::with_capacity(rows.len());
        for row in rows {
            let run_id = row.try_get("run_id")?;
            changed.push(run_id);
            leases.push(CancellationLease {
                company_id: scope.company_id(),
                run_id,
                runtime_adapter: row.try_get("runtime_adapter")?,
                lease_token: row.try_get("lease_token")?,
                reason: CancellationReason::parse(row.try_get("reason")?)?,
                attempt_count: row.try_get::<i32, _>("attempt_count")? as u32,
                max_attempts: row.try_get::<i32, _>("max_attempts")? as u32,
                lease_expires_at: row.try_get("lease_expires_at")?,
            });
        }
        mirror_human(&mut tx, scope, &changed).await?;
        tx.commit().await?;
        Ok(leases)
    }

    async fn acknowledge_cancellation(
        &self,
        scope: &CompanyScope,
        lease: &CancellationLease,
        expected_runtime_ref: Option<&RuntimeRunRef>,
    ) -> Result<CancellationAckOutcome> {
        if lease.company_id != scope.company_id() {
            return Ok(CancellationAckOutcome::StaleLease);
        }
        if expected_runtime_ref.is_some_and(|reference| {
            reference.0.is_empty() || reference.0.len() > 1024 || reference.0.contains('\0')
        }) {
            return Err(invalid("invalid cancellation runtime reference"));
        }
        let mut tx = self.pool().begin().await?;
        let row = sqlx::query(
            "SELECT status,runtime_run_ref FROM runs
            WHERE company_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(scope.company_id())
        .bind(lease.run_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(RunSupervisionError::UnknownRun {
            run_id: lease.run_id,
        })?;
        let status = RunStatus::parse(row.try_get("status")?)
            .ok_or_else(|| invalid("unknown run status"))?;
        let durable = row
            .try_get::<Option<String>, _>("runtime_run_ref")?
            .map(RuntimeRunRef);
        let reason: Option<String> = sqlx::query_scalar(
            "SELECT reason FROM runtime_cancellations
            WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3
              AND lease_expires_at>clock_timestamp() FOR UPDATE",
        )
        .bind(scope.company_id())
        .bind(lease.run_id)
        .bind(lease.lease_token)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(reason) = reason else {
            return Ok(CancellationAckOutcome::StaleLease);
        };
        if durable
            .as_ref()
            .is_some_and(|value| Some(value) != expected_runtime_ref)
            || (expected_runtime_ref.is_none() && durable.is_some())
        {
            return Ok(CancellationAckOutcome::RuntimeRefMismatch { durable });
        }
        let final_status = if status.is_terminal() {
            status
        } else {
            append_cancelled(&mut tx, scope, lease.run_id, &reason).await?;
            RunStatus::Cancelled
        };
        if let Some(reference) = expected_runtime_ref.filter(|_| durable.is_none()) {
            sqlx::query(
                "UPDATE runs SET runtime_run_ref=$3,updated_at=clock_timestamp()
                WHERE company_id=$1 AND id=$2",
            )
            .bind(scope.company_id())
            .bind(lease.run_id)
            .bind(&reference.0)
            .execute(&mut *tx)
            .await?;
        }
        let changed = sqlx::query("UPDATE runtime_cancellations SET state='acknowledged',
            acknowledged_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL,last_error_code=NULL
            WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3
              AND lease_expires_at>clock_timestamp()")
            .bind(scope.company_id()).bind(lease.run_id).bind(lease.lease_token)
            .execute(&mut *tx).await?.rows_affected();
        if changed == 0 {
            return Ok(CancellationAckOutcome::StaleLease);
        }
        mirror_human(&mut tx, scope, &[lease.run_id]).await?;
        // A confirmed stop settles the dispatch, including a leased lost-start
        // retry. Office publication remains a separate delivery decision.
        sqlx::query(
            "UPDATE outbox o SET state='delivered',delivered_at=clock_timestamp(),
            lease_owner=NULL,lease_token=NULL,lease_expires_at=NULL,updated_at=clock_timestamp()
            FROM runs r WHERE r.company_id=$1 AND r.id=$2
              AND o.company_id=r.company_id AND o.run_id=r.id AND o.kind='run_dispatch'
              AND o.routing_decision_id=r.routing_decision_id AND o.employee_id=r.employee_id
              AND o.state='pending'",
        )
        .bind(scope.company_id())
        .bind(lease.run_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(CancellationAckOutcome::Acknowledged {
            status: final_status,
        })
    }

    async fn fail_cancellation(
        &self,
        scope: &CompanyScope,
        lease: &CancellationLease,
        error_code: &str,
    ) -> Result<CancellationFailOutcome> {
        if lease.company_id != scope.company_id() {
            return Ok(CancellationFailOutcome::StaleLease);
        }
        if !valid_error_code(error_code) {
            return Err(invalid("invalid cancellation error code"));
        }
        let mut tx = self.pool().begin().await?;
        let state: Option<String> = sqlx::query_scalar("UPDATE runtime_cancellations SET
            state=CASE WHEN attempt_count>=max_attempts THEN 'failed' ELSE 'pending' END,
            next_attempt_at=clock_timestamp()+LEAST(power(2,attempt_count-1),300)*interval '1 second',
            lease_token=NULL,lease_expires_at=NULL,last_error_code=$4
            WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3
              AND lease_expires_at>clock_timestamp() RETURNING state")
            .bind(scope.company_id()).bind(lease.run_id).bind(lease.lease_token).bind(error_code)
            .fetch_optional(&mut *tx).await?;
        let Some(state) = state else {
            return Ok(CancellationFailOutcome::StaleLease);
        };
        mirror_human(&mut tx, scope, &[lease.run_id]).await?;
        tx.commit().await?;
        Ok(if state == "failed" {
            CancellationFailOutcome::Failed
        } else {
            CancellationFailOutcome::Retrying
        })
    }
}

async fn mirror_human(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    ids: &[Uuid],
) -> Result<()> {
    sqlx::query(sql::MIRROR_HUMAN)
        .bind(scope.company_id())
        .bind(ids)
        .execute(connection)
        .await?;
    Ok(())
}

async fn append_cancelled(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    run_id: Uuid,
    reason: &str,
) -> Result<()> {
    let event = RunEvent::normalize(
        run_id,
        Utc::now(),
        None,
        &RunEventPayload::RunCancelled {
            reason: BoundedText::raw(reason),
        },
        &RedactionPolicy::new(),
    )?;
    sqlx::query("INSERT INTO run_events (company_id,run_id,sequence,event_type,occurred_at,payload)
        SELECT $1,$2,COALESCE(MAX(sequence)+1,0),$3,$4,$5 FROM run_events WHERE company_id=$1 AND run_id=$2")
        .bind(scope.company_id()).bind(run_id).bind(event.event_type().as_str())
        .bind(event.occurred_at).bind(event.payload_json()?).execute(&mut *connection).await?;
    sqlx::query(
        "UPDATE runs SET status='cancelled',cancel_reason=$3,
        finished_at=clock_timestamp(),updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .bind(reason)
    .execute(connection)
    .await?;
    Ok(())
}

fn invalid(message: &str) -> RunSupervisionError {
    ControlError::InvalidData(message.to_owned()).into()
}

fn valid_error_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code.as_bytes()[0].is_ascii_lowercase()
        && code.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.')
        })
}
