use super::*;
use ortak_runtime::postgres::workspace_tools::revoke;
use ortak_runtime::reconciliation::reconcile_runtime;

async fn lost_start_cleanup(already_terminal: bool) {
    let x = selected_with_lost_start(true).await;
    let before: Option<String> =
        sqlx::query_scalar("SELECT runtime_run_ref FROM runs WHERE company_id=$1 AND id=$2")
            .bind(x.f.company)
            .bind(x.run)
            .fetch_one(&x.f.pool)
            .await
            .unwrap();
    assert!(before.is_none());
    assert_eq!(x.runtime.inner.start_specs().len(), 1);
    if already_terminal {
        x.runtime.inner.push_event(
            &x.reference,
            RunEventPayload::RunCompleted {
                delivery_intent: ortak_control::run_event::DeliveryIntentKind::Silent,
            },
        );
    }
    assert!(revoke(&x.f.control, &x.scope, x.grant.revision)
        .await
        .unwrap());
    // Reference recovery without a current stop lease remains refused after
    // revocation. It cannot silently become a new execution admission.
    assert!(
        sqlx::query("UPDATE runs SET runtime_run_ref=$3 WHERE company_id=$1 AND id=$2")
            .bind(x.f.company)
            .bind(x.run)
            .bind(&x.reference.0)
            .execute(&x.f.pool)
            .await
            .is_err()
    );
    let report = reconcile_runtime(
        &x.f.control,
        &x.runtime,
        &x.scope,
        &SupervisorConfig::default(),
        4,
    )
    .await
    .unwrap();
    assert_eq!(report.stop_attempts, 1);
    let row:(String,String,String)=sqlx::query_as("SELECT r.status,r.runtime_run_ref,c.state FROM runs r
        JOIN runtime_cancellations c ON c.company_id=r.company_id AND c.run_id=r.id WHERE r.company_id=$1 AND r.id=$2")
        .bind(x.f.company).bind(x.run).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(
        row,
        (
            if already_terminal {
                "completed"
            } else {
                "cancelled"
            }
            .into(),
            x.reference.0.clone(),
            "acknowledged".into()
        )
    );
    assert!(
        sqlx::query("UPDATE runs SET work_admission_token=$3 WHERE company_id=$1 AND id=$2")
            .bind(x.f.company)
            .bind(x.run)
            .bind(Uuid::new_v4())
            .execute(&x.f.pool)
            .await
            .is_err()
    );
    let output = ortak_work::schedule_work_outputs(&x.f.control, &x.scope, 8)
        .await
        .unwrap();
    assert_eq!(output.materialized, 0);
    assert_eq!(x.inputs.reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL55432 with proposal74"]
async fn workspace_lost_start_ack_then_withdrawal_can_persist_confirmed_cancel_reference() {
    lost_start_cleanup(false).await;
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL55432 with proposal74"]
async fn workspace_lost_start_ack_then_withdrawal_can_drain_already_terminal_reference_only() {
    lost_start_cleanup(true).await;
}
