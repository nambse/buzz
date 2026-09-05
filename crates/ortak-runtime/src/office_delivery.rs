//! One bounded Office delivery between worker cancellation passes.

use std::time::Duration;

use ortak_control::{
    outbox::OutboxKind, ports::OutboxRepository, CompanyScope, ControlError, PgControlPlane,
};
use ortak_office::{
    publisher::OfficePublisher, repository::OfficeDeliveryRepository, signer::OfficeSigner,
    OfficeDeliveryService,
};

/// Claims and attempts at most one Office output within twelve seconds.
///
/// The exact frozen draft is reauthorized before delivery. A timeout durably
/// retries that same lease/key/event; failure to record retry propagates. The
/// repository additionally enforces transaction-local lock/statement deadlines,
/// so dropping a timed-out future cannot leave unlimited server-side lock waits.
pub async fn deliver_one_office_output<S: OfficeSigner, P: OfficePublisher>(
    control: &PgControlPlane,
    scope: &CompanyScope,
    worker_id: &str,
    delivery: &OfficeDeliveryService<PgControlPlane, S, P>,
) -> Result<bool, ControlError> {
    tokio::time::timeout(Duration::from_secs(12), async {
        let mut leases = control
            .claim_due(
                scope,
                Some(OutboxKind::OfficePublish),
                worker_id,
                Duration::from_secs(60),
                1,
            )
            .await?;
        let Some(lease) = leases.pop() else {
            return Ok(false);
        };
        let attempt = tokio::time::timeout(Duration::from_secs(8), async {
            let run_id = lease.run_id.ok_or("office_delivery_run_missing")?;
            let draft = crate::office_output::office_output_draft(control, scope, run_id)
                .await
                .map_err(|_| "office_delivery_draft_read")?
                .ok_or("office_delivery_draft_missing")?;
            let authorized = control
                .enqueue_office_publish(scope, &draft)
                .await
                .map_err(|_| "office_delivery_authority_refused")?;
            delivery
                .deliver(scope, &lease, authorized.authorized())
                .await
                .map_err(|_| "office_delivery_attempt_failed")?;
            Ok::<(), &'static str>(())
        })
        .await
        .unwrap_or(Err("office_delivery_timeout"));
        if let Err(code) = attempt {
            control
                .fail(
                    scope,
                    &lease,
                    code,
                    chrono::Utc::now() + chrono::Duration::seconds(30),
                )
                .await?;
        }
        Ok(true)
    })
    .await
    .map_err(|_| ControlError::InvalidData("Office delivery pass timed out".to_owned()))?
}
