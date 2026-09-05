//! Bounded reconciliation of Office revocation and durable stop requests.
//!
//! The company generation is the durable scan signal. A crash never erases it;
//! an admitted run stays due until it is revalidated or a stop request commits.

use std::time::Duration;

use ortak_control::runtime::{CancelOutcome, RuntimeAdapter, RuntimeRunRef};
use ortak_control::{CompanyScope, ControlError, PgControlPlane};
use sqlx::Row;
use uuid::Uuid;

use crate::cancellation::{
    CancellationAckOutcome, CancellationReason, RuntimeCancellationRepository,
};
use crate::{
    run_idempotency_key, Result, RunDispatchRepository, RunSupervisionError, RunSupervisor,
    SupervisorConfig,
};

/// Counts for one bounded reconciliation pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReconciliationReport {
    /// Active runs checked against current Office authority.
    pub reviewed: usize,
    /// New durable stop requests due to revoked Office authority.
    pub revocations: usize,
    /// Stop requests handed to the runtime by stable start key.
    pub stop_attempts: usize,
}

/// Reviews at most 64 active runs with changed or expired Office authority.
///
/// Runtime configuration stays pinned. An unchanged canonical input and Office
/// identity renew the admission witness; otherwise a durable stop is requested.
pub async fn reconcile_office_runs(
    control: &PgControlPlane,
    scope: &CompanyScope,
    limit: usize,
) -> Result<ReconciliationReport> {
    let due = sqlx::query(
        "SELECT r.id,
                c.status = 'active' AND cm.deletion_state = 'active'
                    AND cm.deleted_at IS NULL AS office_active
           FROM runs r
           JOIN companies c ON c.id = r.company_id
           LEFT JOIN office_company_bindings b ON b.company_id = c.id
           LEFT JOIN communities cm ON cm.id = b.community_id
           LEFT JOIN office_authority_generations g ON g.company_id = c.id
          WHERE r.company_id = $1 AND r.status IN ('queued','running','waiting')
            AND NOT EXISTS (SELECT 1 FROM runtime_cancellations x
                            WHERE x.company_id = r.company_id AND x.run_id = r.id)
            AND (r.office_admission_generation IS NULL
                 OR r.office_admission_generation <> coalesce(g.generation, 0)
                 OR r.office_admission_valid_before <= clock_timestamp()
                 OR c.status <> 'active' OR cm.id IS NULL
                 OR cm.deletion_state <> 'active' OR cm.deleted_at IS NOT NULL)
          ORDER BY r.updated_at, r.id LIMIT $2",
    )
    .bind(scope.company_id())
    .bind(limit.clamp(1, 64) as i64)
    .fetch_all(control.pool())
    .await?;
    let mut report = ReconciliationReport::default();
    let mut first_error = None;
    for row in due {
        let run_id: Uuid = row.try_get("id")?;
        let office_active: Option<bool> = row.try_get("office_active")?;
        report.reviewed += 1;
        // A removed binding or inactive community cannot acquire an admission
        // fence. Stopping needs no permission to start and must remain possible.
        let authorized = if office_active == Some(true) {
            match crate::postgres::refresh_run_office_authority(control, scope, run_id).await {
                Ok(authorized) => authorized,
                // Changed canonical facts cannot preserve an older admission.
                // Record the stop durably rather than repeatedly failing the
                // same oldest row before other stop requests can be drained.
                Err(RunSupervisionError::Control(
                    ControlError::InboxFactMismatch { .. }
                    | ControlError::UnreadableManifest { .. }
                    | ControlError::InvalidData(_)
                    | ControlError::Serde(_)
                    | ControlError::Domain(_)
                    | ControlError::UnknownCompanyBinding { .. }
                    | ControlError::CompanySuspended { .. },
                ))
                | Err(RunSupervisionError::RunPinnedDifferently { .. }) => false,
                Err(error) => {
                    first_error.get_or_insert(error);
                    continue;
                }
            }
        } else {
            false
        };
        if !authorized
            && control
                .enqueue_cancellation(scope, run_id, CancellationReason::OfficeRevoked)
                .await?
        {
            report.revocations += 1;
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(report),
    }
}

/// Performs one bounded review and stop pass; callers schedule with backoff.
/// No database transaction spans an adapter call. Errors leave a durable pending
/// request or propagate, and leased requests recover automatically after expiry.
pub async fn reconcile_runtime<A: RuntimeAdapter>(
    control: &PgControlPlane,
    adapter: &A,
    scope: &CompanyScope,
    config: &SupervisorConfig,
    limit: usize,
) -> Result<ReconciliationReport> {
    let limit = limit.clamp(1, 64);
    control
        .enqueue_human_cancellations(scope, limit as u32)
        .await?;
    // Known stop requests remain drainable even when an unrelated admission
    // read fails. The scanner's durable generation signal survives failure.
    let (mut report, review_error) = match tokio::time::timeout(
        Duration::from_secs(10),
        reconcile_office_runs(control, scope, limit),
    )
    .await
    {
        Ok(Ok(report)) => (report, None),
        Ok(Err(error)) => (ReconciliationReport::default(), Some(error)),
        Err(_) => (
            ReconciliationReport::default(),
            Some(ControlError::InvalidData("Office authority review timed out".to_owned()).into()),
        ),
    };
    let supervisor = RunSupervisor::new(control.clone(), adapter, config.clone());
    for _ in 0..limit {
        // Each lease starts immediately before its external call. A slow stop
        // cannot consume the waiting time of every other item in a batch.
        let mut leases = control
            .claim_cancellations(scope, adapter.adapter_name(), Duration::from_secs(60), 1)
            .await?;
        let Some(lease) = leases.pop() else { break };
        report.stop_attempts += 1;
        // Keep the entire stop/replay/ack attempt inside its live 60s lease.
        // A slow terminal backlog retains its already-committed cursor; retry
        // must not hold up unrelated cancellation for sixteen HTTP deadlines.
        let attempt = tokio::time::timeout(Duration::from_secs(35), async {
            let key = run_idempotency_key(scope.company_id(), lease.run_id);
            let receipt = adapter
                .cancel_start(&key, lease.reason.as_str())
                .await
                .map_err(|_| "runtime_stop_failed")?;
            if receipt.outcome == CancelOutcome::AlreadyTerminal {
                if let Some(reference) = &receipt.runtime_run_ref {
                    recover_reference(control, scope, lease.run_id, reference)
                        .await
                        .map_err(|_| "runtime_reference_conflict")?;
                    supervisor
                        .drain(scope, lease.run_id)
                        .await
                        .map_err(|_| "runtime_terminal_read_failed")?;
                }
                let state = control
                    .run_cursor_state(scope, lease.run_id)
                    .await
                    .map_err(|_| "runtime_terminal_state_retry")?
                    .ok_or("runtime_terminal_state_missing")?;
                if !state.status.is_terminal() {
                    return Err("runtime_terminal_unconfirmed");
                }
            }
            match control
                .acknowledge_cancellation(scope, &lease, receipt.runtime_run_ref.as_ref())
                .await
                .map_err(|_| "runtime_acknowledgement_retry")?
            {
                CancellationAckOutcome::RuntimeRefMismatch { .. } => {
                    Err("runtime_reference_conflict")
                }
                // A newer owner may have acknowledged concurrently. No late
                // observation may overwrite that owner's durable outcome.
                _ => Ok(()),
            }
        })
        .await
        .unwrap_or(Err("runtime_stop_timeout"));
        if let Err(code) = attempt {
            // If persistence itself fails, propagate; the original leased row
            // remains durable and reclaimable after its database-clock expiry.
            control.fail_cancellation(scope, &lease, code).await?;
        }
    }
    match review_error {
        Some(error) => Err(error),
        None => Ok(report),
    }
}

async fn recover_reference(
    control: &PgControlPlane,
    scope: &CompanyScope,
    run_id: Uuid,
    reference: &RuntimeRunRef,
) -> Result<()> {
    if reference.0.is_empty() || reference.0.len() > 1024 {
        return Err(
            ControlError::InvalidData("invalid recovered runtime reference".to_owned()).into(),
        );
    }
    let result = sqlx::query(
        "UPDATE runs SET runtime_run_ref = $3
          WHERE company_id = $1 AND id = $2
            AND (runtime_run_ref IS NULL OR runtime_run_ref = $3)",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .bind(&reference.0)
    .execute(control.pool())
    .await?;
    if result.rows_affected() != 1 {
        return Err(
            ControlError::InvalidData("runtime reference recovery conflicts".to_owned()).into(),
        );
    }
    Ok(())
}
