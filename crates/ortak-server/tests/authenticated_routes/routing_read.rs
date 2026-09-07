//! Signed read seam over retained decision records; not a dispatch/provider fixture.
use super::*;
#[path = "routing_read/fixture.rs"]
pub(crate) mod fixture;
use fixture::*;
#[path = "routing_read/direct.rs"]
mod direct;
#[path = "routing_read/stream.rs"]
mod stream;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn routing_read_distinguishes_missing_record_silence_and_wake_without_raw_metadata() {
    let f = Fixture::new().await;
    let pending = source(&f, f.channel).await;
    let result = read(&f.app, &f.operator, f.channel, pending).await;
    assert_eq!(result.0, StatusCode::OK);
    assert!(result.1["decision"].is_null());
    for wake in [false, true] {
        let message = record(&f, f.channel, wake).await;
        let result = read(&f.app, &f.operator, f.channel, message).await;
        assert_eq!(result.0, StatusCode::OK);
        let decision = &result.1["decision"];
        assert_eq!(decision["mode"], if wake { "semantic" } else { "silent" });
        assert_eq!(decision["recipients"].as_array().unwrap().len(), 1);
        assert_eq!(
            decision["recipients"][0]["action"],
            if wake { "wake" } else { "drop" }
        );
        assert_eq!(
            decision["recipients"][0]["evidence"],
            json!(["domain_match"])
        );
        assert_eq!(decision["scorer"]["reasoning_effort"], "high");
        assert_eq!(decision["scorer"]["input_tokens"], 20);
        assert!(decision["scorer"]["output_tokens"].is_null());
        assert!(decision["scorer"]["failure_code"].is_null());
        let wire = result.1.to_string();
        for private in [
            CANARY,
            "credential://",
            "hidden-private",
            "input_hash",
            "candidate_revision_ids",
            "excluded_targets",
            "scorer_usage",
            "manifest",
            "office_input_hash",
        ] {
            assert!(!wire.contains(private), "private field/canary leaked");
        }
        let after: i64 =
            sqlx::query_scalar("SELECT count(*) FROM runs WHERE company_id=$1 AND message_id=$2")
                .bind(f.company)
                .bind(message.to_bytes().as_slice())
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_eq!(after, 0, "read must not create a run");
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn routing_read_uses_current_company_channel_source_and_employee_grants() {
    let f = Fixture::new().await;
    let visible = record(&f, f.channel, true).await;
    let hidden = record(&f, f.hidden, false).await;
    assert_eq!(
        read(&f.app, &f.operator, f.hidden, hidden).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        read(&f.app, &f.operator, f.channel, hidden).await.0,
        StatusCode::NOT_FOUND
    );
    let other = Fixture::new().await;
    assert_eq!(
        read(&other.app, &other.operator, other.channel, visible)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let mut cfg = config(f.community, &f.operator, f.channel);
    cfg.humans[0].employee_ids = vec![EmployeeId::parse("another-granted-employee").unwrap()];
    let restricted = product_router(f.control.clone(), cfg, Arc::new(Replay::default())).unwrap();
    let result = read(&restricted, &f.operator, f.channel, visible).await;
    assert_eq!(result.0, StatusCode::OK);
    assert_eq!(result.1["decision"]["recipients"], json!([]));
    assert_eq!(result.1["decision"]["mode"], "semantic");
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(visible.to_bytes().as_slice())
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        read(&f.app, &f.operator, f.channel, visible).await.0,
        StatusCode::NOT_FOUND
    );
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM ortak_api_audit WHERE company_id=$1 AND action='access' AND outcome='not_found'")
        .bind(f.company).fetch_one(&f.pool).await.unwrap();
    assert!(audit >= 3);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn routing_read_inconsistent_persisted_source_is_unavailable_not_absent() {
    let f = Fixture::new().await;
    let message = record(&f, f.channel, false).await;
    assert_eq!(
        read(&f.app, &f.operator, f.channel, message).await.0,
        StatusCode::OK
    );
    // Preserve the visible canonical event and decision but corrupt one inbox
    // pin. Reads must not silently normalize this retained inconsistency.
    sqlx::query("UPDATE office_inbox SET event_kind=40002 WHERE company_id=$1 AND event_id=$2")
        .bind(f.company)
        .bind(message.to_bytes().as_slice())
        .execute(&f.pool)
        .await
        .unwrap();
    let result = read(&f.app, &f.operator, f.channel, message).await;
    assert_eq!(result.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(result.1, json!({"error":{"code":"service_unavailable"}}));
    let retained: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM routing_decisions WHERE company_id=$1 AND message_id=$2",
    )
    .bind(f.company)
    .bind(message.to_bytes().as_slice())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(retained, 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn routing_read_waits_for_authority_then_refuses_revoked_channel_membership() {
    let f = Fixture::new().await;
    let message = record(&f, f.channel, false).await;
    let mut authority = f.pool.begin().await.unwrap();
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(f.operator.public_key().to_bytes().as_slice())
        .execute(&mut *authority).await.unwrap();
    let app = f.app.clone();
    let request = signed(&f.operator, "GET", &path(f.channel, message), "", false);
    let task = tokio::spawn(async move { response(&app, request).await });
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    assert!(!task.is_finished(), "read escaped the current Office fence");
    authority.commit().await.unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.0, StatusCode::NOT_FOUND);
}
