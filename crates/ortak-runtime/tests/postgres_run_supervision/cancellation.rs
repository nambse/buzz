use super::*;
use ortak_runtime::cancellation::{
    CancellationAckOutcome, CancellationFailOutcome, CancellationLease, CancellationReason,
    RuntimeCancellationRepository,
};

async fn claim(fixture: &Fixture) -> Vec<CancellationLease> {
    fixture
        .control
        .claim_cancellations(&fixture.scope, "fake-runtime", Duration::from_secs(60), 64)
        .await
        .expect("claim cancellations")
}

async fn human_request(fixture: &Fixture, run_id: Uuid) {
    sqlx::query(
        "INSERT INTO run_cancel_requests (company_id,run_id,requested_by,auth_event_id)
        VALUES ($1,$2,repeat('ab',32),decode(repeat('cd',32),'hex'))",
    )
    .bind(fixture.scope.company_id())
    .bind(run_id)
    .execute(&fixture.pool)
    .await
    .expect("human request");
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn human_stop_is_adapter_scoped_leased_and_acknowledged_atomically_after_suspension() {
    let fixture = Fixture::new().await;
    let (run_id, runtime_ref, _) = fixture.started().await;
    human_request(&fixture, run_id).await;
    assert_eq!(
        fixture
            .control
            .enqueue_human_cancellations(&fixture.scope, 1000)
            .await
            .expect("import"),
        1
    );
    assert_eq!(
        fixture
            .control
            .enqueue_human_cancellations(&fixture.scope, 64)
            .await
            .expect("reimport"),
        0
    );
    assert!(fixture
        .control
        .claim_cancellations(
            &fixture.scope,
            "different-adapter",
            Duration::from_secs(60),
            64
        )
        .await
        .expect("wrong adapter")
        .is_empty());
    let (first, second) = tokio::join!(claim(&fixture), claim(&fixture));
    let leases: Vec<_> = first.into_iter().chain(second).collect();
    assert_eq!(leases.len(), 1, "concurrent workers must not share a lease");
    let lease = &leases[0];
    assert_eq!(lease.reason, CancellationReason::HumanRequested);
    assert_eq!(lease.attempt_count, 1);
    let mut forged = lease.clone();
    forged.lease_token = Uuid::new_v4();
    assert_eq!(
        fixture
            .control
            .fail_cancellation(&fixture.scope, &forged, "runtime_cancel_failed")
            .await
            .expect("stale failure"),
        CancellationFailOutcome::StaleLease
    );
    for presented in [None, Some(RuntimeRunRef("wrong-ref".to_owned()))] {
        assert_eq!(
            fixture
                .control
                .acknowledge_cancellation(&fixture.scope, lease, presented.as_ref())
                .await
                .expect("wrong receipt"),
            CancellationAckOutcome::RuntimeRefMismatch {
                durable: Some(runtime_ref.clone())
            }
        );
    }
    sqlx::query("UPDATE companies SET status='suspended' WHERE id=$1")
        .bind(fixture.scope.company_id())
        .execute(&fixture.pool)
        .await
        .expect("suspend company");
    assert_eq!(
        fixture
            .control
            .acknowledge_cancellation(&fixture.scope, lease, Some(&runtime_ref))
            .await
            .expect("acknowledge actual stop"),
        CancellationAckOutcome::Acknowledged {
            status: RunStatus::Cancelled
        }
    );
    let run = fixture.run(run_id).await;
    assert_eq!(run.status, "cancelled");
    assert_eq!(run.cancel_reason.as_deref(), Some("human_requested"));
    let events = fixture.events(run_id).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.1 == "run.cancelled")
            .count(),
        1
    );
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.0, index as i64);
    }
    let human = sqlx::query("SELECT status,attempts,lease_token,acknowledged_at FROM run_cancel_requests WHERE company_id=$1 AND run_id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).fetch_one(&fixture.pool).await.expect("human state");
    assert_eq!(human.get::<String, _>("status"), "acknowledged");
    assert_eq!(human.get::<i32, _>("attempts"), 1);
    assert!(human.get::<Option<Uuid>, _>("lease_token").is_none());
    assert!(human
        .get::<Option<chrono::DateTime<Utc>>, _>("acknowledged_at")
        .is_some());
    assert!(!fixture
        .control
        .enqueue_cancellation(&fixture.scope, run_id, CancellationReason::OfficeRevoked)
        .await
        .expect("replay does not reset"));
    assert_eq!(
        fixture
            .control
            .acknowledge_cancellation(&fixture.scope, lease, Some(&runtime_ref))
            .await
            .expect("replay acknowledgement"),
        CancellationAckOutcome::StaleLease
    );
    assert_eq!(fixture.events(run_id).await.len(), events.len());
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn stop_can_acknowledge_an_uncorrelated_run_and_preserves_terminal_history() {
    for receipt in [
        None,
        Some(RuntimeRunRef("recovered-lost-receipt".to_owned())),
    ] {
        let fixture = Fixture::new().await;
        fixture.route("Cem, selam").await;
        let dispatch = fixture.lease(Duration::from_secs(60)).await;
        let authority = authorized(
            fixture
                .control
                .authorize_dispatch(&fixture.scope, &dispatch)
                .await
                .expect("authority"),
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
        assert!(fixture
            .control
            .enqueue_cancellation(&fixture.scope, run_id, CancellationReason::OfficeRevoked)
            .await
            .expect("enqueue"));
        let lease = claim(&fixture).await.remove(0);
        assert_eq!(
            fixture
                .control
                .acknowledge_cancellation(&fixture.scope, &lease, receipt.as_ref())
                .await
                .expect("stop receipt"),
            CancellationAckOutcome::Acknowledged {
                status: RunStatus::Cancelled
            }
        );
        let run = fixture.run(run_id).await;
        assert_eq!(run.runtime_run_ref, receipt.map(|reference| reference.0));
        assert_eq!(run.cancel_reason.as_deref(), Some("office_revoked"));
        assert_eq!(
            fixture.events(run_id).await.last().expect("cancel event").1,
            "run.cancelled"
        );
    }
    let fixture = Fixture::new().await;
    let (run_id, runtime_ref, _) = fixture.started().await;
    let event = RunEvent::normalize(
        run_id,
        Utc::now(),
        None,
        &RunEventPayload::RunCompleted {
            delivery_intent: DeliveryIntentKind::Silent,
        },
        &RedactionPolicy::new(),
    )
    .expect("normalize terminal");
    fixture
        .control
        .append_supervised_events(&fixture.scope, run_id, &runtime_ref, &[event])
        .await
        .expect("finish locally");
    let event_count = fixture.events(run_id).await.len();
    assert!(fixture
        .control
        .enqueue_cancellation(&fixture.scope, run_id, CancellationReason::OfficeRevoked)
        .await
        .expect("terminal still requires actual stop"));
    let lease = claim(&fixture).await.remove(0);
    assert_eq!(
        fixture
            .control
            .acknowledge_cancellation(&fixture.scope, &lease, Some(&runtime_ref))
            .await
            .expect("adapter terminal acknowledgement"),
        CancellationAckOutcome::Acknowledged {
            status: RunStatus::Completed
        }
    );
    assert_eq!(fixture.run(run_id).await.status, "completed");
    assert_eq!(fixture.events(run_id).await.len(), event_count);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn cancellation_retry_backoff_and_crashed_final_attempt_are_durable_and_bounded() {
    let fixture = Fixture::new().await;
    let (run_id, runtime_ref, _) = fixture.started().await;
    human_request(&fixture, run_id).await;
    fixture
        .control
        .enqueue_human_cancellations(&fixture.scope, 64)
        .await
        .expect("import");
    let mut lease = claim(&fixture).await.remove(0);
    // Forged hints must not override the durable attempt budget or attribution.
    lease.attempt_count = 20;
    lease.reason = CancellationReason::OfficeRevoked;
    assert_eq!(
        fixture
            .control
            .fail_cancellation(&fixture.scope, &lease, "runtime_cancel_failed")
            .await
            .expect("failure"),
        CancellationFailOutcome::Retrying
    );
    assert!(claim(&fixture).await.is_empty(), "backoff is enforced");
    let retry=sqlx::query("SELECT attempt_count,reason,last_error_code,next_attempt_at>clock_timestamp() AS backed_off,
        next_attempt_at<=clock_timestamp()+interval '301 seconds' AS bounded FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).fetch_one(&fixture.pool).await.expect("retry state");
    assert_eq!(retry.get::<i32, _>("attempt_count"), 1);
    assert_eq!(retry.get::<String, _>("reason"), "human_requested");
    assert_eq!(
        retry.get::<String, _>("last_error_code"),
        "runtime_cancel_failed"
    );
    assert!(retry.get::<bool, _>("backed_off") && retry.get::<bool, _>("bounded"));
    sqlx::query("UPDATE runtime_cancellations SET attempt_count=19,next_attempt_at=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND run_id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).execute(&fixture.pool).await.expect("final attempt fixture");
    let final_lease = claim(&fixture).await.remove(0);
    assert_eq!(final_lease.attempt_count, 20);
    sqlx::query("UPDATE runtime_cancellations SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND run_id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).execute(&fixture.pool).await.expect("worker crashed");
    assert_eq!(
        fixture
            .control
            .acknowledge_cancellation(&fixture.scope, &final_lease, Some(&runtime_ref))
            .await
            .expect("expired ack"),
        CancellationAckOutcome::StaleLease
    );
    assert!(claim(&fixture).await.is_empty());
    let state=sqlx::query("SELECT c.state,c.attempt_count,c.last_error_code,h.status AS human_status FROM runtime_cancellations c JOIN run_cancel_requests h USING(company_id,run_id) WHERE c.company_id=$1 AND c.run_id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).fetch_one(&fixture.pool).await.expect("terminal failure");
    assert_eq!(state.get::<String, _>("state"), "failed");
    assert_eq!(state.get::<i32, _>("attempt_count"), 20);
    assert_eq!(state.get::<String, _>("human_status"), "failed");
    assert_eq!(
        state.get::<String, _>("last_error_code"),
        "cancellation_lease_exhausted"
    );
    assert!(!fixture
        .control
        .enqueue_cancellation(&fixture.scope, run_id, CancellationReason::HumanRequested)
        .await
        .expect("cannot reset failed request"));
    assert_eq!(
        fixture.run(run_id).await.status,
        "running",
        "queue failure cannot invent a stop"
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn late_human_request_observes_an_already_failed_office_stop_without_resetting_it() {
    let fixture = Fixture::new().await;
    let (run_id, _, _) = fixture.started().await;
    fixture
        .control
        .enqueue_cancellation(&fixture.scope, run_id, CancellationReason::OfficeRevoked)
        .await
        .expect("office stop");
    sqlx::query(
        "UPDATE runtime_cancellations SET attempt_count=19 WHERE company_id=$1 AND run_id=$2",
    )
    .bind(fixture.scope.company_id())
    .bind(run_id)
    .execute(&fixture.pool)
    .await
    .expect("nearly exhausted fixture");
    let lease = claim(&fixture).await.remove(0);
    assert_eq!(
        fixture
            .control
            .fail_cancellation(&fixture.scope, &lease, "runtime_stop_failed")
            .await
            .expect("terminal failure"),
        CancellationFailOutcome::Failed
    );
    human_request(&fixture, run_id).await;
    assert_eq!(
        fixture
            .control
            .enqueue_human_cancellations(&fixture.scope, 64)
            .await
            .expect("import late human"),
        0
    );
    let row=sqlx::query("SELECT h.status,h.attempts,c.reason,c.state FROM run_cancel_requests h
        JOIN runtime_cancellations c USING(company_id,run_id) WHERE h.company_id=$1 AND h.run_id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).fetch_one(&fixture.pool).await.expect("converged request");
    assert_eq!(row.get::<String, _>("status"), "failed");
    assert_eq!(row.get::<i32, _>("attempts"), 20);
    assert_eq!(row.get::<String, _>("reason"), "office_revoked");
    assert_eq!(row.get::<String, _>("state"), "failed");
    assert!(claim(&fixture).await.is_empty());
}
