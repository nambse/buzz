use super::*;
use ortak_runtime::reconciliation::{reconcile_office_runs, reconcile_runtime};

async fn revoke(fixture: &Fixture) {
    sqlx::query(
        "UPDATE channel_members SET removed_at=clock_timestamp()
        WHERE community_id=$1 AND pubkey=$2",
    )
    .bind(fixture.community_id)
    .bind(hex::decode(fixture_employee().office.public_key).expect("employee key"))
    .execute(&fixture.pool)
    .await
    .expect("revoke membership");
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn post_admission_revocation_is_stopped_durably_by_the_production_worker() {
    let fixture = Fixture::new().await;
    let (run_id, runtime_ref, _) = fixture.started().await;
    revoke(&fixture).await;
    let report = reconcile_runtime(
        &fixture.control,
        &fixture.adapter,
        &fixture.scope,
        &fixture.config(),
        64,
    )
    .await
    .expect("reconcile actual stop");
    assert_eq!(
        (report.reviewed, report.revocations, report.stop_attempts),
        (1, 1, 1)
    );
    assert_eq!(fixture.run(run_id).await.status, "cancelled");
    assert_eq!(
        fixture.run(run_id).await.cancel_reason.as_deref(),
        Some("office_revoked")
    );
    let queue: String = sqlx::query_scalar(
        "SELECT state FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2",
    )
    .bind(fixture.scope.company_id())
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("durable queue");
    assert_eq!(queue, "acknowledged");
    assert!(
        fixture
            .adapter
            .next_events(&runtime_ref, None, 64)
            .await
            .expect("runtime stopped")
            .terminal
    );
    assert_eq!(
        fixture
            .events(run_id)
            .await
            .iter()
            .filter(|event| event.1 == "run.cancelled")
            .count(),
        1
    );
    let replay = reconcile_runtime(
        &fixture.control,
        &fixture.adapter,
        &fixture.scope,
        &fixture.config(),
        64,
    )
    .await
    .expect("replay reconciliation");
    assert_eq!(replay.stop_attempts, 0);
    assert_eq!(fixture.adapter.start_specs().len(), 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn permissions_only_revision_renews_authority_without_stopping_pinned_execution() {
    let fixture = Fixture::new().await;
    let (run_id, _, _) = fixture.started().await;
    let old_token: Uuid =
        sqlx::query_scalar("SELECT office_admission_token FROM runs WHERE company_id=$1 AND id=$2")
            .bind(fixture.scope.company_id())
            .bind(run_id)
            .fetch_one(&fixture.pool)
            .await
            .expect("old admission");
    let mut newer = fixture_employee();
    newer.permissions = PermissionPolicy::default();
    let revision = activate_employee(&fixture.pool, fixture.scope.company_id(), &newer, true).await;
    assert_ne!(revision, fixture.revision_id);
    let report = reconcile_office_runs(&fixture.control, &fixture.scope, 64)
        .await
        .expect("refresh canonical authority");
    assert_eq!((report.reviewed, report.revocations), (1, 0));
    let renewed=sqlx::query("SELECT r.office_admission_token,r.office_admission_generation,g.generation
        FROM runs r JOIN office_authority_generations g USING(company_id) WHERE r.company_id=$1 AND r.id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).fetch_one(&fixture.pool).await.expect("renewed admission");
    assert_ne!(renewed.get::<Uuid, _>("office_admission_token"), old_token);
    assert_eq!(
        renewed.get::<i64, _>("office_admission_generation"),
        renewed.get::<i64, _>("generation")
    );
    assert_eq!(
        fixture.run(run_id).await.employee_revision_id,
        fixture.revision_id
    );
    assert_eq!(fixture.run(run_id).await.status, "running");
    assert_eq!(
        fixture.adapter.start_specs()[0].permissions,
        fixture_employee().permissions
    );
    assert!(
        reconcile_office_runs(&fixture.control, &fixture.scope, 64)
            .await
            .expect("already refreshed")
            .reviewed
            == 0
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn revoked_start_key_stops_lost_acknowledgement_and_fences_a_delayed_first_start() {
    for start_before_revocation in [false, true] {
        let fixture = Fixture::new().await;
        fixture.route("Cem, selam").await;
        let lease = fixture.lease(Duration::from_secs(60)).await;
        let authority = authorized(
            fixture
                .control
                .authorize_dispatch(&fixture.scope, &lease)
                .await
                .expect("authorize"),
        );
        let PrepareOutcome::Prepared(prepared) = fixture
            .control
            .prepare_run(&fixture.scope, &authority)
            .await
            .expect("prepare")
        else {
            panic!("not prepared")
        };
        let run_id = prepared.run_id;
        let spec = authority.run_spec(run_id).expect("pinned spec");
        let lost_receipt = if start_before_revocation {
            Some(
                fixture
                    .adapter
                    .start_run(&spec)
                    .await
                    .expect("start with lost acknowledgement"),
            )
        } else {
            None
        };
        assert!(fixture.run(run_id).await.runtime_run_ref.is_none());
        revoke(&fixture).await;
        let report = reconcile_runtime(
            &fixture.control,
            &fixture.adapter,
            &fixture.scope,
            &fixture.config(),
            64,
        )
        .await
        .expect("stop stable start key");
        assert_eq!((report.revocations, report.stop_attempts), (1, 1));
        assert_eq!(fixture.run(run_id).await.status, "cancelled");
        assert_eq!(fixture.outbox(lease.id).await.state, "delivered");
        assert!(fixture.outbox(lease.id).await.lease_token.is_none());
        match lost_receipt {
            Some(receipt) => {
                assert_eq!(
                    fixture.run(run_id).await.runtime_run_ref,
                    Some(receipt.runtime_run_ref.0.clone())
                );
                assert_eq!(
                    fixture
                        .adapter
                        .start_run(&spec)
                        .await
                        .expect("replay returns same terminated run"),
                    receipt
                );
                assert!(
                    fixture
                        .adapter
                        .next_events(&receipt.runtime_run_ref, None, 64)
                        .await
                        .expect("stopped")
                        .terminal
                );
            }
            None => {
                assert!(
                    matches!(
                        fixture.adapter.start_run(&spec).await,
                        Err(RuntimeError::InvalidSpec { .. })
                    ),
                    "cancellation tombstone must refuse a delayed first external start"
                );
                assert!(fixture
                    .adapter
                    .lookup_start(&spec.idempotency_key)
                    .await
                    .expect("no runtime was started")
                    .is_none());
            }
        }
        assert_eq!(
            fixture
                .events(run_id)
                .await
                .iter()
                .filter(|event| event.1 == "run.cancelled")
                .count(),
            1
        );
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn runtime_dispatch_claim_leaves_other_adapters_attempt_budget_untouched() {
    let fixture = Fixture::new().await;
    let decision_id = fixture.route("Cem, selam").await;
    let outbox_id: Uuid = sqlx::query_scalar("SELECT id FROM outbox WHERE company_id=$1 AND routing_decision_id=$2 AND kind='run_dispatch'")
        .bind(fixture.scope.company_id()).bind(decision_id).fetch_one(&fixture.pool).await.expect("dispatch outbox");
    assert!(fixture
        .control
        .claim_runtime_dispatches(
            &fixture.scope,
            "other-runtime",
            "other-worker",
            Duration::from_secs(60),
            64
        )
        .await
        .expect("wrong adapter claim")
        .is_empty());
    assert_eq!(fixture.outbox(outbox_id).await.attempt_count, 0);
    let leases = fixture
        .control
        .claim_runtime_dispatches(
            &fixture.scope,
            "fake-runtime",
            "fake-worker",
            Duration::from_secs(60),
            64,
        )
        .await
        .expect("matching adapter claim");
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].id, outbox_id);
    assert_eq!(leases[0].attempt_count, 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn canonical_fact_mismatch_is_revoked_instead_of_poisoning_the_stop_worker() {
    let fixture = Fixture::new().await;
    let (run_id, reference, _) = fixture.started().await;
    let message = fixture.run(run_id).await.message_id;
    sqlx::query("UPDATE events SET pubkey=$3 WHERE community_id=$1 AND id=$2")
        .bind(fixture.community_id)
        .bind(message)
        .bind([8u8; 32].as_slice())
        .execute(&fixture.pool)
        .await
        .expect("canonical author differs from decided inbox");
    let report = reconcile_runtime(
        &fixture.control,
        &fixture.adapter,
        &fixture.scope,
        &fixture.config(),
        64,
    )
    .await
    .expect("permanent canonical error queues a stop instead of exiting");
    assert_eq!((report.revocations, report.stop_attempts), (1, 1));
    assert_eq!(fixture.run(run_id).await.status, "cancelled");
    assert!(
        fixture
            .adapter
            .next_events(&reference, None, 64)
            .await
            .expect("actual stop")
            .terminal
    );
    assert_eq!(
        reconcile_runtime(
            &fixture.control,
            &fixture.adapter,
            &fixture.scope,
            &fixture.config(),
            64
        )
        .await
        .expect("restart no poisoned oldest row")
        .stop_attempts,
        0
    );
}
