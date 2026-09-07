//! Bounded, durable post-delivery memory writes with canonical Office admission.

use std::time::Duration;

use ortak_control::memory::{MemoryAdapter, MemoryError, MemoryWriteReceipt};
use ortak_control::memory_jobs::{MemoryWriteJobLease, MemoryWriteJobRepository};
use ortak_control::postgres::{lock_office_authority_on, prepare_memory_write_on};
use ortak_control::{CompanyScope, ControlError, PgControlPlane};

const PASS_TIMEOUT: Duration = Duration::from_secs(12);
const JOB_TIMEOUT: Duration = Duration::from_secs(8);
const WRITE_TIMEOUT: Duration = Duration::from_secs(3);

/// Outcome of one memory scheduling pass; a pass claims at most one row.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MemoryOutputReport {
    /// One when a durable job was claimed, otherwise zero.
    pub attempted: usize,
    /// One only when the validated adapter receipt was acknowledged durably.
    pub acknowledged: usize,
    /// One when failure recording was attempted; its durable row holds the state.
    pub failed_attempts: usize,
}

enum Failure {
    Control(ControlError),
    Memory(MemoryError),
    Stale,
    Timeout,
}

impl From<ControlError> for Failure {
    fn from(error: ControlError) -> Self {
        Self::Control(error)
    }
}
impl From<sqlx::Error> for Failure {
    fn from(error: sqlx::Error) -> Self {
        Self::Control(error.into())
    }
}

impl Failure {
    fn record(&self) -> (&'static str, bool) {
        match self {
            Self::Control(ControlError::Database(_)) => ("memory_output_database_retry", false),
            Self::Control(_) => ("memory_output_authority_refused", true),
            Self::Memory(error) if error.is_retryable() => ("memory_output_service_retry", false),
            Self::Memory(MemoryError::Unsupported { .. }) => {
                ("memory_output_validation_required", false)
            }
            Self::Memory(_) => ("memory_output_service_refused", true),
            Self::Stale => ("memory_output_stale", false),
            Self::Timeout => ("memory_output_timeout", false),
        }
    }
}

/// Writes at most one acknowledged Office output into its pinned run scratch.
///
/// Canonical source, destination, identity and memory authority are checked in
/// one bounded transaction before I/O. The adapter receives the frozen request
/// and stable key, so a lost receipt or process restart retries the same write.
/// No transaction stays open across a network call. A timeout leaves a durable
/// lease or bounded retry record; failure to persist that record propagates.
/// The whole pass is bounded to twelve seconds to allow cancellation to resume.
pub async fn schedule_memory_output<A: MemoryAdapter>(
    control: &PgControlPlane,
    adapter: &A,
    scope: &CompanyScope,
) -> Result<MemoryOutputReport, ControlError> {
    tokio::time::timeout(PASS_TIMEOUT, schedule_one(control, adapter, scope))
        .await
        .map_err(|_| ControlError::InvalidData("memory output pass timed out".to_owned()))?
}

async fn schedule_one<A: MemoryAdapter>(
    control: &PgControlPlane,
    adapter: &A,
    scope: &CompanyScope,
) -> Result<MemoryOutputReport, ControlError> {
    let Some(lease) = control
        .claim_memory_write(scope, adapter.adapter_name(), Duration::from_secs(60))
        .await?
    else {
        return Ok(MemoryOutputReport::default());
    };
    let mut report = MemoryOutputReport {
        attempted: 1,
        ..MemoryOutputReport::default()
    };
    let attempt = tokio::time::timeout(JOB_TIMEOUT, write_one(control, adapter, scope, &lease))
        .await
        .unwrap_or(Err(Failure::Timeout));
    match attempt {
        Ok(receipt) => {
            report.acknowledged = usize::from(
                control
                    .acknowledge_memory_write(scope, &lease, &receipt)
                    .await?,
            );
        }
        Err(Failure::Stale) => {}
        Err(failure) => {
            let (code, permanent) = failure.record();
            control
                .fail_memory_write(scope, &lease, code, permanent)
                .await?;
            report.failed_attempts = 1;
        }
    }
    Ok(report)
}

async fn write_one<A: MemoryAdapter>(
    control: &PgControlPlane,
    adapter: &A,
    scope: &CompanyScope,
    lease: &MemoryWriteJobLease,
) -> Result<MemoryWriteReceipt, Failure> {
    let mut tx = control.pool().begin().await?;
    sqlx::query(
        "SELECT set_config('lock_timeout','500ms',true),
                set_config('statement_timeout','2s',true),
                set_config('idle_in_transaction_session_timeout','5s',true)",
    )
    .execute(&mut *tx)
    .await?;
    let witness = lock_office_authority_on(&mut tx, scope).await?;
    crate::office_output::revalidate_delivered_output_on(&mut tx, scope, lease.run_id).await?;
    let request = prepare_memory_write_on(&mut tx, scope, lease, &witness)
        .await?
        .ok_or(Failure::Stale)?;
    tx.commit().await?;
    tokio::time::timeout(WRITE_TIMEOUT, adapter.remember(&request))
        .await
        .map_err(|_| Failure::Timeout)?
        .map_err(Failure::Memory)
}
