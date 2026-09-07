//! Signed production SSE. Root must integrate/apply the notification fragment;
//! these cases never install or bypass its triggers themselves.
use super::*;
use futures_util::StreamExt;
use std::time::Duration;

use crate::direct_channel_support as direct_support;

fn stream_path(channel: Uuid, message: EventId) -> String {
    format!("{}/stream", path(channel, message))
}

async fn subscribe(
    app: &Router,
    key: &Keys,
    channel: Uuid,
    message: EventId,
) -> axum::body::BodyDataStream {
    let response = app
        .clone()
        .oneshot(signed(
            key,
            "GET",
            &stream_path(channel, message),
            "",
            false,
        ))
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
        .expect("notification must push before five-second heartbeat")
        .expect("open stream")
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(
        !text.contains("id:"),
        "message snapshots have no invented history cursor"
    );
    assert!(text.len() <= 65_664);
    text
}

fn page(text: &str) -> Value {
    assert!(text.contains("event: routing"), "{text}");
    let page: Value = serde_json::from_str(
        text.lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap(),
    )
    .unwrap();
    for hidden in [
        CANARY,
        "Private source content",
        "credential://",
        "hidden-private",
        "scorer_usage",
        "candidate_revision_ids",
    ] {
        assert!(
            !text.contains(hidden),
            "private metadata escaped projection"
        );
    }
    page
}

async fn commit_decision(f: &Fixture, message: EventId, template: EventId) {
    // Same inert historical fixture as routing_read::record, committed as one
    // decision+recipient transaction so NOTIFY cannot expose a partial record.
    let mut tx = f.pool.begin().await.unwrap();
    let id:Uuid=sqlx::query_scalar("INSERT INTO routing_decisions(company_id,message_id,root_message_id,inbox_claim_generation,origin_type,mode,summary_reason,policy_version,policy_fingerprint,input_hash,excluded_targets,scorer_adapter,scorer_model,scorer_prompt_version,scorer_version,scorer_latency_ms,scorer_usage,wake_count,hop_consumed)
        SELECT company_id,$3,$3,inbox_claim_generation,origin_type,mode,summary_reason,policy_version,policy_fingerprint,input_hash,excluded_targets,scorer_adapter,scorer_model,scorer_prompt_version,scorer_version,scorer_latency_ms,scorer_usage,wake_count,hop_consumed
        FROM routing_decisions WHERE company_id=$1 AND message_id=$2 RETURNING id")
        .bind(f.company).bind(template.to_bytes().as_slice()).bind(message.to_bytes().as_slice())
        .fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO routing_recipients(company_id,routing_decision_id,employee_id,position,action,reason,score,evidence)
        SELECT r.company_id,$3,r.employee_id,r.position,r.action,r.reason,r.score,r.evidence
        FROM routing_recipients r JOIN routing_decisions d ON d.company_id=r.company_id AND d.id=r.routing_decision_id
        WHERE d.company_id=$1 AND d.message_id=$2")
        .bind(f.company).bind(template.to_bytes().as_slice()).bind(id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable port55432 Postgres and routing notification fragment"]
async fn routing_stream_overlaps_listen_current_read_and_push_then_reconnects_without_cursor() {
    let f = Fixture::new().await;
    let template = record(&f, f.channel, false).await;
    let message = source(&f, f.channel).await;
    let mut stream = subscribe(&f.app, &f.operator, f.channel, message).await;
    assert!(page(&frame(&mut stream).await)["decision"].is_null());
    commit_decision(&f, message, template).await;
    let pushed = page(&frame(&mut stream).await);
    assert_eq!(pushed["decision"]["mode"], "silent");
    assert_eq!(
        pushed["decision"]["recipients"].as_array().unwrap().len(),
        1
    );
    assert_eq!(pushed["decision"]["recipients"][0]["action"], "drop");
    let current = read(&f.app, &f.operator, f.channel, message).await;
    assert_eq!(
        pushed, current.1,
        "HTTP and SSE use exactly the same current projection"
    );
    drop(stream);
    let mut resumed = subscribe(&f.app, &f.operator, f.channel, message).await;
    assert_eq!(page(&frame(&mut resumed).await), pushed);
    drop(resumed);
    // A commit after LISTEN but before the body is polled belongs to the first
    // current snapshot. There is no backfill/subscribe gap or client cursor.
    let before_poll = source(&f, f.channel).await;
    let mut delayed = subscribe(&f.app, &f.operator, f.channel, before_poll).await;
    commit_decision(&f, before_poll, template).await;
    assert_eq!(
        page(&frame(&mut delayed).await)["decision"]["mode"],
        "silent"
    );
    drop(delayed);
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE company_id=$1")
        .bind(f.company)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        runs, 0,
        "routing read/stream must not create employee execution"
    );
}

#[tokio::test]
#[ignore = "requires disposable port55432 Postgres and routing notification fragment"]
async fn routing_stream_authority_hint_revokes_and_foreign_hints_cannot_select_content() {
    let f = Fixture::new().await;
    let message = record(&f, f.channel, true).await;
    let hidden = record(&f, f.hidden, false).await;
    for (channel, message) in [(f.hidden, hidden), (f.channel, hidden)] {
        assert_eq!(
            response(
                &f.app,
                signed(
                    &f.operator,
                    "GET",
                    &stream_path(channel, message),
                    "",
                    false
                )
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    let other = Fixture::new().await;
    assert_eq!(
        response(
            &other.app,
            signed(
                &other.operator,
                "GET",
                &stream_path(other.channel, message),
                "",
                false
            )
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut stream = subscribe(&f.app, &f.operator, f.channel, message).await;
    page(&frame(&mut stream).await);
    sqlx::query("SELECT pg_notify('ortak_routing_v1',$1)")
        .bind(json!({"company_id":other.company,"message_id":message.to_hex()}).to_string())
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), stream.next())
            .await
            .is_err()
    );
    let mut held = f.pool.begin().await.unwrap();
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(f.operator.public_key().to_bytes().as_slice())
        .execute(&mut *held).await.unwrap();
    // An early/forged same-company hint cannot read through the held canonical
    // authority fence. The commit's real generation notification follows.
    sqlx::query("SELECT pg_notify('ortak_routing_v1',$1)")
        .bind(json!({"company_id":f.company,"message_id":message.to_hex()}).to_string())
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), stream.next())
            .await
            .is_err()
    );
    held.commit().await.unwrap();
    let revoked = frame(&mut stream).await;
    assert!(revoked.contains("\"code\":\"revoked\""), "{revoked}");
    assert!(!revoked.contains("decision"));
    assert!(stream.next().await.is_none());
}

#[tokio::test]
#[ignore = "requires disposable port55432 Postgres and routing notification fragment"]
async fn routing_stream_dm_revalidates_exact_pair_and_source_pins_never_become_absence() {
    let f = Fixture::new().await;
    let employee = Keys::generate().public_key().to_bytes();
    sqlx::query("INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,verified_at) VALUES($1,'cem',$2,'adopt',$3,'credential://synthetic/routing-stream-dm',clock_timestamp())")
        .bind(f.company).bind(f.revision).bind(employee.as_slice()).execute(&f.pool).await.unwrap();
    let channel = direct_support::create(
        &f.pool,
        f.community,
        &[f.operator.public_key().to_bytes(), employee],
    )
    .await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    direct_support::select(
        &f.control,
        &scope,
        channel,
        &EmployeeId::parse("cem").unwrap(),
    )
    .await;
    let mut cfg = config(f.community, &f.operator, channel);
    cfg.humans[0]
        .employee_ids
        .push(EmployeeId::parse("hidden-private").unwrap());
    let mut outsider = cfg.humans[0].clone();
    outsider.public_key = f.reader.public_key().to_hex();
    cfg.humans.push(outsider);
    let app = product_router(f.control.clone(), cfg, Arc::new(Replay::default())).unwrap();
    let message = record(&f, channel, true).await;
    assert_eq!(
        response(
            &app,
            signed(&f.reader, "GET", &stream_path(channel, message), "", false)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut stream = subscribe(&app, &f.operator, channel, message).await;
    let first = page(&frame(&mut stream).await);
    assert_eq!(first["decision"]["recipients"].as_array().unwrap().len(), 1);
    assert_eq!(first["decision"]["recipients"][0]["employee_id"], "cem");
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(channel).bind(employee.as_slice()).execute(&f.pool).await.unwrap();
    // Employee absence blocks new execution, not the current human's retained
    // Activity read. SSE must use the same canonical read predicate as HTTP.
    let retained = frame(&mut stream).await;
    assert_eq!(page(&retained), first, "{retained}");
    let current = read(&app, &f.operator, channel, message).await;
    assert_eq!(current.0, StatusCode::OK, "{current:?}");
    assert_eq!(current.1, first);
    // A third retained key invalidates the exact pair even though the removed
    // employee row still exists. This is actual read-authority revocation.
    sqlx::query("INSERT INTO channel_members(community_id,channel_id,pubkey) VALUES($1,$2,$3)")
        .bind(f.community)
        .bind(channel)
        .bind(f.reader.public_key().to_bytes().as_slice())
        .execute(&f.pool)
        .await
        .unwrap();
    let current = read(&app, &f.operator, channel, message).await;
    assert_eq!(current.0, StatusCode::NOT_FOUND, "{current:?}");
    let revoked = frame(&mut stream).await;
    assert!(revoked.contains("\"code\":\"revoked\""), "{revoked}");
    assert!(!revoked.contains("decision"), "{revoked}");
    assert!(tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("revoked stream must close")
        .is_none());
    drop(stream);
    let visible = record(&f, f.channel, false).await;
    let mut stream = subscribe(&f.app, &f.operator, f.channel, visible).await;
    page(&frame(&mut stream).await);
    sqlx::query("UPDATE office_inbox SET event_kind=40002 WHERE company_id=$1 AND event_id=$2")
        .bind(f.company)
        .bind(visible.to_bytes().as_slice())
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("SELECT pg_notify('ortak_routing_v1',$1)")
        .bind(json!({"company_id":f.company,"message_id":visible.to_hex()}).to_string())
        .execute(&f.pool)
        .await
        .unwrap();
    let refused = frame(&mut stream).await;
    assert!(refused.contains("\"code\":\"retry\""), "{refused}");
    assert!(
        !refused.contains("decision"),
        "broken source cannot claim undecided"
    );
}

#[tokio::test]
#[ignore = "requires disposable port55432 Postgres; checks real shared four slots and45s absolute deadline"]
async fn routing_stream_unpolled_bodies_release_shared_activity_capacity_at_deadline() {
    let f = Fixture::new().await;
    let message = record(&f, f.channel, false).await;
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
    let mut held = Vec::new();
    for _ in 0..3 {
        held.push(subscribe(&app, &f.operator, f.channel, message).await);
    }
    let activity_path = format!("/api/v1/runs/{run}/stream");
    let activity = app
        .clone()
        .oneshot(signed(&f.operator, "GET", &activity_path, "", false))
        .await
        .unwrap();
    assert_eq!(activity.status(), StatusCode::OK);
    let activity = activity.into_body().into_data_stream();
    assert_eq!(
        response(
            &app,
            signed(
                &f.operator,
                "GET",
                &stream_path(f.channel, message),
                "",
                false
            )
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        read(&app, &f.operator, f.channel, message).await.0,
        StatusCode::OK,
        "listeners do not exhaust the two-connection query pool"
    );
    tokio::time::sleep(Duration::from_secs(46)).await;
    let mut replacement = subscribe(&app, &f.operator, f.channel, message).await;
    page(&frame(&mut replacement).await);
    drop(replacement);
    drop(activity);
    drop(held);
    let mut replacement = subscribe(&app, &f.operator, f.channel, message).await;
    page(&frame(&mut replacement).await);
}
