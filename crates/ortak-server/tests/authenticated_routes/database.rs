//! The API's real database pool must bound locks while retaining its auth fence.
use super::*;
use std::time::Duration;

use ortak_server::connect_private_database;

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL on localhost:55432"]
async fn authenticated_cancel_lock_timeout_releases_pool_and_retries() {
    let url = std::env::var("ORTAK_TEST_DATABASE_URL").expect("explicit disposable database URL");
    let options: sqlx::postgres::PgConnectOptions = url.parse().expect("database URL");
    assert!(matches!(options.get_host(), "localhost" | "127.0.0.1") && options.get_port() == 55432);
    let f = Fixture::new().await;
    let run = f.run(f.channel).await;
    let pool = connect_private_database(&url)
        .await
        .expect("production API database connector");
    let app = product_router(
        PgControlPlane::new(pool.clone()),
        config(f.community, &f.operator, f.channel),
        Arc::new(Replay::default()),
    )
    .expect("production authenticated router");
    for (setting, expected) in [
        ("statement_timeout", "5s"),
        ("lock_timeout", "500ms"),
        ("idle_in_transaction_session_timeout", "10s"),
    ] {
        let actual: String = sqlx::query_scalar("SELECT current_setting($1)")
            .bind(setting)
            .fetch_one(&pool)
            .await
            .expect("actual API session bound");
        assert_eq!(actual, expected);
    }
    assert_eq!(pool.options().get_max_connections(), 8);
    let mut blocker = f
        .pool
        .begin()
        .await
        .expect("independent blocking transaction");
    sqlx::query("SELECT id FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.company)
        .bind(run)
        .fetch_one(&mut *blocker)
        .await
        .expect("hold only this fixture's run row");
    let path = format!("/api/v1/runs/{run}/cancel");
    let request = signed(&f.operator, "POST", &path, "{}", true);
    let request_app = app.clone();
    let task = tokio::spawn(async move { response(&request_app, request).await });

    // Observe the real middleware fence before the handler waits for the row.
    // The setup pool remains independent so it cannot consume API capacity.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let fenced: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM pg_locks
                   WHERE locktype='advisory' AND mode='ShareLock' AND granted
                     AND database=(SELECT oid FROM pg_database WHERE datname=current_database())
                     AND objsubid=1
                     AND classid::bigint=((ortak_office_company_lock_key($1) >> 32) & 4294967295::bigint)
                     AND objid::bigint=(ortak_office_company_lock_key($1) & 4294967295::bigint))",
            )
            .bind(f.company)
            .fetch_one(&f.pool)
            .await
            .expect("observe authority fence without contending with authentication");
            if fenced {
                break;
            }
            assert!(!task.is_finished(), "request must pass authentication and hold its fence");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("request must reach fenced handler before its lock deadline");
    let (status, body) = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("database lock timeout must precede the 15-second HTTP timeout")
        .expect("request task");
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body, json!({"error":{"code":"service_unavailable"}}));
    for query in [
        "SELECT count(*) FROM run_cancel_requests WHERE company_id=$1 AND run_id=$2",
        "SELECT count(*) FROM ortak_api_audit WHERE company_id=$1 AND requested_run_id=$2 AND outcome='requested'",
    ] {
        let count: i64 = sqlx::query_scalar(query)
            .bind(f.company)
            .bind(run)
            .fetch_one(&f.pool)
            .await
            .expect("no partial cancellation or audit");
        assert_eq!(count, 0);
    }

    // Acquire every slot together: a leaked authority or handler transaction
    // would strand at least one slot and exceed this test deadline. SELECT also
    // proves returned connections completed rollback and remain usable.
    let connections = tokio::time::timeout(Duration::from_secs(3), async {
        let mut connections = Vec::new();
        for _ in 0..8 {
            let mut connection = pool.acquire().await.expect("released API connection");
            let value: i32 = sqlx::query_scalar("SELECT 1")
                .fetch_one(&mut *connection)
                .await
                .expect("connection is usable after failed request");
            assert_eq!(value, 1);
            connections.push(connection);
        }
        connections
    })
    .await
    .expect("all eight API connections must be available after failure");
    drop(connections);
    let mut probe = f.pool.begin().await.expect("released authority probe");
    let available: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(ortak_office_company_lock_key($1))")
            .bind(f.company)
            .fetch_one(&mut *probe)
            .await
            .expect("authority lock released after response");
    assert!(
        available,
        "failed HTTP request must not retain its Office fence"
    );
    probe.rollback().await.expect("release authority probe");
    blocker.rollback().await.expect("release run row");

    // A retry gets a new NIP-98 event, preserving replay protection while using
    // the same logical cancellation and pool after the contention is removed.
    let (status, body) = tokio::time::timeout(
        Duration::from_secs(3),
        response(&app, signed(&f.operator, "POST", &path, "{}", true)),
    )
    .await
    .expect("retry must complete promptly");
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["status"], "pending");
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM run_cancel_requests WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.company)
    .bind(run)
    .fetch_one(&f.pool)
    .await
    .expect("durable retried cancellation");
    assert_eq!(count, 1);
    pool.close().await;
}
