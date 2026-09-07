//! Direct production persistence/admission regressions; supervisor recall and
//! lost-start workflows are covered separately in memory_context.rs.
use super::*;
use ortak_control::memory::MemoryRecall;
use ortak_runtime::memory_context::{
    FreezeSnapshotOutcome, FrozenRunSnapshot, RunContextRepository,
};

pub(super) fn empty_snapshot(
    authority: &ortak_runtime::DispatchAuthority,
    run_id: Uuid,
) -> FrozenRunSnapshot {
    let wire = serde_json::json!({
        "version":1,"company_id":authority.company_id(),
        "routing_decision_id":authority.routing_decision_id(),
        "message_id":authority.message_id().unwrap().to_hex(),
        "root_message_id":authority.root_message_id().unwrap().to_hex(),
        "event_kind":authority.input().event_kind,
        "input_truncated":authority.input().truncated,
        "memory_binding":authority.memory_binding(),
        "recall":MemoryRecall::default(),"spec":authority.run_spec(run_id).expect("spec"),
    });
    FrozenRunSnapshot::decode(&serde_json::to_vec(&wire).expect("wire"), authority, run_id)
        .expect("public bounded snapshot decoder")
}

async fn prepared(
    f: &Fixture,
) -> (
    OutboxLease,
    ortak_runtime::DispatchAuthority,
    Uuid,
    FrozenRunSnapshot,
) {
    f.route("Cem, remember this run context.").await;
    let lease = f.lease(Duration::from_secs(60)).await;
    let authority = authorized(
        f.control
            .authorize_dispatch(&f.scope, &lease)
            .await
            .expect("authorize"),
    );
    let run = match f
        .control
        .prepare_run(&f.scope, &authority)
        .await
        .expect("durable run")
    {
        PrepareOutcome::Prepared(run) => run,
        other => panic!("expected prepared run: {other:?}"),
    };
    let snapshot = empty_snapshot(&authority, run.run_id);
    (lease, authority, run.run_id, snapshot)
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn snapshot_first_writer_is_immutable_and_reloaded_under_a_new_dispatch_lease() {
    let f = Fixture::new().await;
    let (lease, authority, run_id, candidate) = prepared(&f).await;
    assert!(f
        .control
        .load_run_snapshot(&f.scope, &authority, run_id)
        .await
        .expect("initial load")
        .is_none());
    let winner = match f
        .control
        .freeze_run_snapshot(&f.scope, &lease, &authority, run_id, &candidate)
        .await
        .expect("freeze")
    {
        FreezeSnapshotOutcome::Ready(snapshot) => snapshot,
        other => panic!("expected durable winner: {other:?}"),
    };
    let mut without_office_context = winner.spec().clone();
    assert!(without_office_context
        .context
        .conversation_context
        .take()
        .is_some());
    assert_eq!(
        &without_office_context,
        candidate.spec(),
        "recall candidate is retained alongside canonical Office selection"
    );
    assert!(
        sqlx::query(
            "UPDATE run_context_snapshots SET spec_bytes=$3 WHERE company_id=$1 AND run_id=$2"
        )
        .bind(f.scope.company_id())
        .bind(run_id)
        .bind(b"replacement".as_slice())
        .execute(&f.pool)
        .await
        .is_err(),
        "database rejects rewriting persisted input"
    );
    f.control
        .fail(&f.scope, &lease, "lost start acknowledgement", Utc::now())
        .await
        .expect("durable retry");
    let retry = f.lease(Duration::from_secs(60)).await;
    assert_ne!(retry.lease_token, lease.lease_token);
    let fresh = authorized(
        f.control
            .authorize_dispatch(&f.scope, &retry)
            .await
            .expect("fresh authority"),
    );
    let loaded = f
        .control
        .load_run_snapshot(&f.scope, &fresh, run_id)
        .await
        .expect("reload")
        .expect("persisted winner");
    assert_eq!(
        loaded.encode().expect("bytes"),
        winner.encode().expect("original bytes")
    );
    let repeated = f
        .control
        .freeze_run_snapshot(&f.scope, &retry, &fresh, run_id, &loaded)
        .await
        .expect("renewed final admission");
    assert!(matches!(repeated,FreezeSnapshotOutcome::Ready(value) if value==winner));
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn snapshot_commit_refuses_dispatch_lease_that_expires_while_unchanged_outbox_row_is_locked()
{
    let f = Fixture::new().await;
    let (lease, authority, run_id, candidate) = prepared(&f).await;
    let mut blocker = f.pool.begin().await.expect("blocker");
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .expect("blocker pid");
    let deadline:chrono::DateTime<Utc>=sqlx::query_scalar("UPDATE outbox SET lease_expires_at=clock_timestamp()+interval '350 milliseconds' WHERE company_id=$1 AND id=$2 RETURNING lease_expires_at")
        .bind(f.scope.company_id()).bind(lease.id).fetch_one(&f.pool).await.expect("short authoritative lease");
    sqlx::query("SELECT id FROM outbox WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.scope.company_id())
        .bind(lease.id)
        .fetch_one(&mut *blocker)
        .await
        .expect("hold without changing row");
    let control = f.control.clone();
    let scope = f.scope.clone();
    let admission = tokio::spawn(async move {
        control
            .freeze_run_snapshot(&scope, &lease, &authority, run_id, &candidate)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2),async {
        loop {
            let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(blocker_pid).fetch_one(&f.pool).await.expect("observe actual blocked statement");
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("snapshot reached final outbox lock");
    // Read the DB clock and release before the production 500ms lock timeout;
    // the tuple remains unchanged, so the deferred deadline is the final guard.
    sqlx::query("SELECT pg_sleep(greatest(0,extract(epoch FROM $1::timestamptz-clock_timestamp()))::double precision+0.01)")
        .bind(deadline).execute(&mut *blocker).await.expect("cross exact lease boundary");
    blocker.commit().await.expect("release unchanged tuple");
    let error = tokio::time::timeout(Duration::from_secs(2), admission)
        .await
        .expect("bounded admission")
        .expect("task")
        .expect_err("expired lease cannot commit a ready snapshot");
    match error {
        RunSupervisionError::Control(ortak_control::ControlError::Database(error)) => {
            assert_eq!(
                error.as_database_error().and_then(|e| e.code()).as_deref(),
                Some("40001")
            );
        }
        other => panic!("expected deferred lease expiry: {other:?}"),
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company_id())
    .bind(run_id)
    .fetch_one(&f.pool)
    .await
    .expect("rollback is atomic");
    assert_eq!(count, 0);
}
