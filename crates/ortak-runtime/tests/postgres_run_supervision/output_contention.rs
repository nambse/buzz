use super::office_output::{complete, output_count};
use super::*;
use ortak_runtime::cancellation::{CancellationReason, RuntimeCancellationRepository};
use ortak_runtime::office_output::schedule_office_outputs;
use ortak_runtime::reconciliation::reconcile_runtime;

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn blocked_output_does_not_lease_later_jobs_or_prevent_cancellation_recovery() {
    let fixture = Fixture::new().await;
    let (blocked_run, first_ref, _) = fixture.started().await;
    complete(
        &fixture,
        blocked_run,
        &first_ref,
        DeliveryIntentKind::Reply,
        vec![BoundedText::raw("first answer")],
    )
    .await;
    let (ready_run, second_ref, _) = fixture.started().await;
    complete(
        &fixture,
        ready_run,
        &second_ref,
        DeliveryIntentKind::Reply,
        vec![BoundedText::raw("second answer")],
    )
    .await;
    // Fix ordering without sleeping so the first claim must encounter the held
    // row. Job creation times remain immutable.
    sqlx::query(
        "UPDATE runtime_office_outputs SET next_attempt_at=clock_timestamp()-
        CASE WHEN run_id=$2 THEN interval '2 seconds' ELSE interval '1 second' END
        WHERE company_id=$1",
    )
    .bind(fixture.scope.company_id())
    .bind(blocked_run)
    .execute(&fixture.pool)
    .await
    .expect("deterministic due order");
    let (cancel_run, _, _) = fixture.started().await;
    fixture
        .control
        .enqueue_cancellation(
            &fixture.scope,
            cancel_run,
            CancellationReason::HumanRequested,
        )
        .await
        .expect("durable stop before output pass");

    let mut blocker = fixture.pool.begin().await.expect("run blocker");
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .expect("blocker pid");
    sqlx::query("SELECT id FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(fixture.scope.company_id())
        .bind(blocked_run)
        .fetch_one(&mut *blocker)
        .await
        .expect("hold first completed run");

    let observe_wait =
        async {
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let waiting: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE datname=current_database()
                     AND $1=ANY(pg_blocking_pids(pid)))",
                ).bind(blocker_pid).fetch_one(&fixture.pool).await.expect("observe row wait");
                    if waiting {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("production source query must reach the held run row");
            let row = sqlx::query(
                "SELECT attempt_count,lease_token FROM runtime_office_outputs
            WHERE company_id=$1 AND run_id=$2",
            )
            .bind(fixture.scope.company_id())
            .bind(ready_run)
            .fetch_one(&fixture.pool)
            .await
            .expect("later job remains untouched while first waits");
            assert_eq!(
                row.get::<i32, _>("attempt_count"),
                0,
                "no batch lease before use"
            );
            assert!(row.get::<Option<Uuid>, _>("lease_token").is_none());
        };
    let (report, ()) = tokio::time::timeout(Duration::from_secs(4), async {
        tokio::join!(
            schedule_office_outputs(&fixture.control, &fixture.scope, 2),
            observe_wait
        )
    })
    .await
    .expect("database lock timeout must release the output scheduler");
    let report = report.expect("lock contention records a durable retry");
    assert_eq!(
        (
            report.attempted,
            report.enqueued,
            report.retrying,
            report.failed
        ),
        (2, 1, 1, 0)
    );
    assert_eq!(output_count(&fixture, blocked_run).await, 0);
    assert_eq!(output_count(&fixture, ready_run).await, 1);
    let blocked = sqlx::query(
        "SELECT attempt_count,lease_token,last_error_code FROM runtime_office_outputs
        WHERE company_id=$1 AND run_id=$2",
    )
    .bind(fixture.scope.company_id())
    .bind(blocked_run)
    .fetch_one(&fixture.pool)
    .await
    .expect("blocked job retry is durable");
    assert_eq!(blocked.get::<i32, _>("attempt_count"), 1);
    assert!(blocked.get::<Option<Uuid>, _>("lease_token").is_none());
    assert_eq!(
        blocked.get::<String, _>("last_error_code"),
        "office_output_database_retry"
    );

    // The original run lock is STILL held: a later worker step must be able to
    // stop an unrelated runtime now, without requiring that lock to be released.
    let stopped = tokio::time::timeout(
        Duration::from_secs(3),
        reconcile_runtime(
            &fixture.control,
            &fixture.adapter,
            &fixture.scope,
            &fixture.config(),
            64,
        ),
    )
    .await
    .expect("cancellation regains control")
    .expect("acknowledge durable stop");
    assert_eq!(stopped.stop_attempts, 1);
    assert_eq!(fixture.run(cancel_run).await.status, "cancelled");
    blocker.rollback().await.expect("release first run row");

    sqlx::query(
        "UPDATE runtime_office_outputs SET next_attempt_at=clock_timestamp()
        WHERE company_id=$1 AND run_id=$2",
    )
    .bind(fixture.scope.company_id())
    .bind(blocked_run)
    .execute(&fixture.pool)
    .await
    .expect("retry first job now");
    let retry = schedule_office_outputs(&fixture.control, &fixture.scope, 64)
        .await
        .expect("retry after lock release");
    assert_eq!((retry.attempted, retry.enqueued, retry.failed), (1, 1, 0));
    assert_eq!(output_count(&fixture, blocked_run).await, 1);
}
