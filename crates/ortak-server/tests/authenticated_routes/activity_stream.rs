use super::*;
use futures_util::StreamExt;
use std::time::Duration;

async fn subscribe(
    app: &Router,
    key: &Keys,
    run: Uuid,
    cursor: Option<i64>,
) -> axum::body::BodyDataStream {
    let path = format!(
        "/api/v1/runs/{run}/stream{}",
        cursor
            .map(|v| format!("?after_sequence={v}"))
            .unwrap_or_default()
    );
    let response = app
        .clone()
        .oneshot(signed(key, "GET", &path, "", false))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    assert_eq!(response.headers()["cache-control"], "no-store");
    response.into_body().into_data_stream()
}

async fn frame(stream: &mut axum::body::BodyDataStream) -> String {
    let bytes = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("push before five-second authority heartbeat")
        .expect("open stream")
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}
fn activity(frame: &str) -> Value {
    assert!(frame.contains("event: activity"), "{frame}");
    serde_json::from_str(
        frame
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap(),
    )
    .unwrap()
}
async fn append(f: &Fixture, run: Uuid) {
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let event = RunEvent::normalize(
        run,
        Utc::now(),
        None,
        &RunEventPayload::RunQueued,
        &RedactionPolicy::new(),
    )
    .unwrap();
    f.control
        .append_run_events(&scope, run, &[event])
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn signed_stream_overlaps_backfill_push_and_durable_reconnect() {
    let f = Fixture::new().await;
    let run = f.run(f.channel).await;
    let mut stream = subscribe(&f.app, &f.operator, run, None).await;
    // Write after LISTEN but before the response body takes its first snapshot.
    append(&f, run).await;
    let first = activity(&frame(&mut stream).await);
    assert_eq!(first["page"]["entries"].as_array().unwrap().len(), 2);
    assert_eq!(first["page"]["next_after_sequence"], 1);
    // Consume the queued wake hint, then prove committed changes push without
    // advancing a clock or issuing another HTTP request.
    let _ = frame(&mut stream).await;
    append(&f, run).await;
    let next = activity(&frame(&mut stream).await);
    assert_eq!(next["page"]["entries"][0]["sequence"], 2);
    drop(stream);
    append(&f, run).await;
    let mut reconnect = subscribe(&f.app, &f.operator, run, Some(2)).await;
    let resumed = activity(&frame(&mut reconnect).await);
    assert_eq!(resumed["page"]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(resumed["page"]["entries"][0]["sequence"], 3);
    assert_eq!(resumed["page"]["next_after_sequence"], 3);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn live_stream_revocation_fences_every_payload_and_other_audiences() {
    let f = Fixture::new().await;
    let run = f.run(f.channel).await;
    let hidden = f.run(f.hidden).await;
    let foreign = Fixture::new().await;
    let foreign_run = foreign.run(foreign.channel).await;
    for denied in [hidden, foreign_run] {
        let path = format!("/api/v1/runs/{denied}/stream");
        assert_eq!(
            response(&f.app, signed(&f.operator, "GET", &path, "", false))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    let mut stream = subscribe(&f.app, &f.operator, run, None).await;
    activity(&frame(&mut stream).await);
    // Even a forged UUID hint can only cause a fresh scoped DB read.
    sqlx::query("SELECT pg_notify('ortak_activity_v1',$1)")
        .bind(json!({"company_id":foreign.company,"run_id":foreign_run}).to_string())
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), stream.next())
            .await
            .is_err()
    );
    sqlx::query("UPDATE channel_members SET removed_at=now() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(f.operator.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap();
    let revoked = frame(&mut stream).await;
    assert!(revoked.contains("\"code\":\"revoked\""), "{revoked}");
    assert!(!revoked.contains("entries"));
    assert!(stream.next().await.is_none());
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn min_query_pool_serves_streams_and_http_and_drop_releases_capacity() {
    let f = Fixture::new().await;
    let run = f.run(f.channel).await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with((*f.pool.connect_options()).clone())
        .await
        .unwrap();
    let app = product_router(
        PgControlPlane::new(pool),
        config(f.community, &f.operator, f.channel),
        Arc::new(Replay::default()),
    )
    .unwrap();
    let mut first = subscribe(&app, &f.operator, run, None).await;
    let mut second = subscribe(&app, &f.operator, run, None).await;
    let path = format!("/api/v1/runs/{run}");
    let (a, b, http) = tokio::join!(
        frame(&mut first),
        frame(&mut second),
        response(&app, signed(&f.operator, "GET", &path, "", false))
    );
    activity(&a);
    activity(&b);
    assert_eq!(http.0, StatusCode::OK);
    let third = subscribe(&app, &f.operator, run, None).await;
    let fourth = subscribe(&app, &f.operator, run, None).await;
    let path = format!("/api/v1/runs/{run}/stream");
    assert_eq!(
        response(&app, signed(&f.operator, "GET", &path, "", false))
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    drop(first);
    drop(second);
    drop(third);
    drop(fourth);
    let mut replacement = subscribe(&app, &f.operator, run, None).await;
    activity(&frame(&mut replacement).await);
    drop(replacement);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn terminal_run_pushes_late_office_status() {
    let f = Fixture::new().await;
    let run = f.run(f.channel).await;
    // Office output scheduling requires a retained routing decision; an
    // otherwise minimal run is deliberately not an Office delivery source.
    super::memory::snapshot(&f, run, f.channel).await;
    sqlx::query("UPDATE runs SET status='completed',delivery_intent='reply',finished_at=now() WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(run).execute(&f.pool).await.unwrap();
    let mut stream = subscribe(&f.app, &f.operator, run, None).await;
    let initial = activity(&frame(&mut stream).await);
    assert_eq!(initial["detail"]["office_delivery"]["status"], "pending");
    sqlx::query("UPDATE runtime_office_outputs SET state='failed',last_error_code='office_output_source_invalid' WHERE company_id=$1 AND run_id=$2")
        .bind(f.company).bind(run).execute(&f.pool).await.unwrap();
    let late = activity(&frame(&mut stream).await);
    assert_eq!(late["detail"]["detail"]["run"]["status"], "completed");
    assert_eq!(late["detail"]["office_delivery"]["status"], "failed");
    assert_eq!(late["page"]["entries"], json!([]));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn terminal_memory_receipt_pushes_without_new_event_and_resumes_current_detail() {
    let f = Fixture::new().await;
    let run = f.run(f.channel).await;
    let wire = super::memory::snapshot(&f, run, f.channel).await;
    super::memory::store(&f, run, &wire, true).await;
    super::memory::pending_write(&f, run, &wire, f.channel).await;
    let mut stream = subscribe(&f.app, &f.operator, run, None).await;
    let initial = activity(&frame(&mut stream).await);
    assert_eq!(initial["detail"]["memory"]["write"]["status"], "pending");
    assert!(!initial.to_string().contains("sk-live-"));
    sqlx::query("UPDATE runtime_memory_writes SET state='acknowledged',receipt=$3,acknowledged_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2")
        .bind(f.company).bind(run).bind(json!({"receipt_ref":"receipt:stream-fixture","written":1})).execute(&f.pool).await.unwrap();
    let receipt = activity(&frame(&mut stream).await);
    assert_eq!(
        receipt["detail"]["memory"]["write"]["status"],
        "acknowledged"
    );
    assert_eq!(
        receipt["detail"]["memory"]["write"]["receipt"]["written"],
        1
    );
    assert_eq!(receipt["page"]["entries"], json!([]));
    drop(stream);
    let mut reconnect = subscribe(&f.app, &f.operator, run, Some(0)).await;
    let current = activity(&frame(&mut reconnect).await);
    assert_eq!(
        current["detail"]["memory"]["write"]["status"],
        "acknowledged"
    );
    assert_eq!(current["page"]["next_after_sequence"], 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres; checks real 45s deadline"]
async fn unpolled_streams_release_listener_capacity_at_the_absolute_deadline() {
    let f = Fixture::new().await;
    let run = f.run(f.channel).await;
    let mut held = Vec::new();
    for _ in 0..4 {
        held.push(subscribe(&f.app, &f.operator, run, None).await);
    }
    // The bodies remain owned but completely unpolled. Their deadline tasks,
    // rather than client reads or drop, must release the four listener slots.
    tokio::time::sleep(Duration::from_secs(46)).await;
    let mut replacement = subscribe(&f.app, &f.operator, run, None).await;
    activity(&frame(&mut replacement).await);
    assert_eq!(held.len(), 4);
}
