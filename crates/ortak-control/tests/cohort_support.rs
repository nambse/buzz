//! Explicit cohort setup for disposable production-seam fixtures.

use ortak_control::{CompanyScope, PgControlPlane};
use ortak_domain::EmployeeId;
use uuid::Uuid;

pub async fn select_and_reconcile(
    control: &PgControlPlane,
    scope: &CompanyScope,
    channels: &[Uuid],
    employees: &[EmployeeId],
) {
    let capture = control
        .begin_routing_capture(scope, channels, employees)
        .await
        .expect("explicit fixture capture");
    for channel in channels {
        let mut progress = control
            .start_inbox_reconciliation(scope, capture.capture_id, *channel)
            .await
            .expect("pin fixture window");
        for _ in 0..64 {
            if progress.completed {
                break;
            }
            progress = control
                .reconcile_inbox_batch(scope, capture.capture_id, *channel, 256)
                .await
                .expect("bounded fixture reconciliation");
        }
        assert!(
            progress.completed,
            "fixture reconciliation exceeded its bound"
        );
    }
    control
        .enable_routing_cohort(scope, capture.capture_id)
        .await
        .expect("enable completed fixture capture");
}
