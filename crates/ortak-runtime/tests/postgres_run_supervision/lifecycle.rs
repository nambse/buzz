//! Old Office work cannot revive across a real sealed disable/re-enable cycle.
use super::*;
use ortak_control::memory::{MemoryAdapter, MemoryBudget, MemoryRecallRequest, MemoryScope};
use ortak_domain::CredentialRef;
use ortak_office::{
    fakes::{FakeOfficePublisher, FakeOfficeSigner},
    DeliveryConfig, OfficeDeliveryService,
};
use ortak_runtime::{
    memory_output::schedule_memory_output, office_delivery::deliver_one_office_output,
    office_output::schedule_office_outputs, reconciliation::reconcile_runtime,
};
#[path = "../../../ortak-control/tests/lifecycle_support.rs"]
mod lifecycle_support;

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL with lifecycle schema"]
async fn lifecycle_old_office_decision_and_held_authority_refuse_but_fresh_epoch_dispatches() {
    let mut f = Fixture::new().await;
    f.route("Cem, old epoch").await;
    let lease = f.lease(Duration::from_secs(60)).await;
    let held = authorized(
        f.control
            .authorize_dispatch(&f.scope, &lease)
            .await
            .unwrap(),
    );
    f.revision_id = lifecycle_support::cycle(&f.pool, &f.control, &f.scope, &f.employee).await;
    assert!(matches!(
        f.control
            .authorize_dispatch(&f.scope, &lease)
            .await
            .unwrap(),
        DispatchAuthorization::Refused(DispatchRefusal::EmployeeLifecycleChanged)
    ));
    assert!(matches!(
        f.control.prepare_run(&f.scope, &held).await.unwrap(),
        PrepareOutcome::Refused(DispatchRefusal::OfficeAuthorityChanged)
    ));
    assert_eq!(f.run_rows().await, 0);
    f.route("Cem, fresh epoch").await;
    let fresh = f.lease(Duration::from_secs(60)).await;
    let outcome = f
        .supervisor(f.config())
        .dispatch(&f.scope, &fresh)
        .await
        .unwrap();
    let DispatchOutcome::Started { run_id, .. } = outcome else {
        panic!("fresh epoch must start")
    };
    let pin: i64 = sqlx::query_scalar(
        "SELECT employee_lifecycle_epoch FROM runs WHERE company_id=$1 AND id=$2",
    )
    .bind(f.scope.company_id())
    .bind(run_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(pin, 1);
    assert_eq!(f.adapter.start_specs().len(), 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL with lifecycle schema"]
async fn lifecycle_worker_stops_old_active_run_even_when_disabled_status_was_missed() {
    let f = Fixture::new().await;
    let (run, reference, _) = f.started().await;
    lifecycle_support::cycle(&f.pool, &f.control, &f.scope, &f.employee).await;
    // No reconciliation ran while Disabled. Equivalent fresh identity is Active
    // now, so status-only admission would incorrectly preserve the old process.
    let report = reconcile_runtime(&f.control, &f.adapter, &f.scope, &f.config(), 64)
        .await
        .unwrap();
    assert_eq!(
        (report.reviewed, report.revocations, report.stop_attempts),
        (1, 1, 1)
    );
    assert_eq!(f.run(run).await.status, "cancelled");
    assert!(
        f.adapter
            .next_events(&reference, None, 64)
            .await
            .unwrap()
            .terminal
    );
    let state: String = sqlx::query_scalar(
        "SELECT state FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company_id())
    .bind(run)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(state, "acknowledged");
    assert_eq!(
        reconcile_runtime(&f.control, &f.adapter, &f.scope, &f.config(), 64)
            .await
            .unwrap()
            .stop_attempts,
        0
    );
    assert_eq!(f.adapter.start_specs().len(), 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL with lifecycle schema"]
async fn lifecycle_terminal_office_job_cannot_freeze_an_old_epoch_answer() {
    let f = Fixture::new().await;
    let (run, reference, _) = f.started().await;
    office_output::complete(
        &f,
        run,
        &reference,
        DeliveryIntentKind::Reply,
        vec![BoundedText::raw("old answer")],
    )
    .await;
    lifecycle_support::cycle(&f.pool, &f.control, &f.scope, &f.employee).await;
    let report = schedule_office_outputs(&f.control, &f.scope, 64)
        .await
        .unwrap();
    assert_eq!(
        (report.attempted, report.enqueued, report.failed),
        (1, 0, 1)
    );
    assert_eq!(office_output::output_count(&f, run).await, 0);
    assert_eq!(
        f.run(run).await.status,
        "completed",
        "retained terminal history is unchanged"
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL with lifecycle schema"]
async fn lifecycle_frozen_office_delivery_and_post_delivery_memory_cannot_revive() {
    for delivered_before_cycle in [false, true] {
        let signer_ref = "credential://fixture/lifecycle-signer";
        let signer = FakeOfficeSigner::new().with_generated_signer(signer_ref);
        let mut employee = fixture_employee();
        employee.office.public_key = signer.public_key(signer_ref).unwrap().to_hex();
        employee.office.signer_ref = CredentialRef::parse(signer_ref).unwrap();
        let f = Fixture::new_for_employee(employee).await;
        let (run, reference, _) = f.started().await;
        office_output::complete(
            &f,
            run,
            &reference,
            DeliveryIntentKind::Reply,
            vec![BoundedText::raw("old answer")],
        )
        .await;
        assert_eq!(
            schedule_office_outputs(&f.control, &f.scope, 64)
                .await
                .unwrap()
                .enqueued,
            1
        );
        let publisher = FakeOfficePublisher::new();
        let delivery = OfficeDeliveryService::new(
            f.control.clone(),
            &signer,
            &publisher,
            DeliveryConfig::default(),
        );
        if delivered_before_cycle {
            assert!(
                deliver_one_office_output(&f.control, &f.scope, "epoch-before", &delivery)
                    .await
                    .unwrap()
            );
            assert_eq!(publisher.publish_calls(), 1);
        }
        lifecycle_support::cycle(&f.pool, &f.control, &f.scope, &f.employee).await;
        if delivered_before_cycle {
            let report = schedule_memory_output(&f.control, &f.memory, &f.scope)
                .await
                .unwrap();
            assert_eq!(
                (
                    report.attempted,
                    report.acknowledged,
                    report.failed_attempts
                ),
                (1, 0, 1)
            );
            let state:(String,String)=sqlx::query_as("SELECT state,last_error_code FROM runtime_memory_writes WHERE company_id=$1 AND run_id=$2").bind(f.scope.company_id()).bind(run).fetch_one(&f.pool).await.unwrap();
            assert_eq!(
                state,
                ("failed".into(), "memory_output_authority_refused".into())
            );
            let recall = f
                .memory
                .recall(&MemoryRecallRequest {
                    employee_id: f.employee.id.clone(),
                    binding: f.employee.memory.clone().unwrap(),
                    scope: MemoryScope::RunScratch { run_id: run },
                    query: "old answer".into(),
                    budget: MemoryBudget::default(),
                })
                .await
                .unwrap();
            assert!(recall.records.is_empty());
            assert_eq!(publisher.publish_calls(), 1);
        } else {
            assert!(
                deliver_one_office_output(&f.control, &f.scope, "epoch-after", &delivery)
                    .await
                    .unwrap()
            );
            assert_eq!(signer.sign_calls(), 0);
            assert_eq!(publisher.publish_calls(), 0);
        }
    }
}
