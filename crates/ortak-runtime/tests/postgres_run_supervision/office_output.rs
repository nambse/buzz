use super::*;
use ortak_runtime::cancellation::{CancellationReason, RuntimeCancellationRepository};
use ortak_runtime::office_output::{office_output_draft, schedule_office_outputs};

pub(super) async fn complete(
    fixture: &Fixture,
    run_id: Uuid,
    reference: &RuntimeRunRef,
    intent: DeliveryIntentKind,
    deltas: Vec<BoundedText>,
) {
    let mut payloads = vec![RunEventPayload::AssistantDelta {
        turn: 0,
        delta: BoundedText::raw("intermediate text"),
    }];
    payloads.extend(
        deltas
            .into_iter()
            .map(|delta| RunEventPayload::AssistantDelta { turn: 1, delta }),
    );
    payloads.push(RunEventPayload::DeliveryIntent {
        intent,
        target_ref: Some("foreign-channel-or-thread".to_owned()),
    });
    payloads.push(RunEventPayload::RunCompleted {
        delivery_intent: intent,
    });
    let events = payloads
        .iter()
        .map(|payload| {
            RunEvent::normalize(run_id, Utc::now(), None, payload, &RedactionPolicy::new())
                .expect("normalize output")
        })
        .collect::<Vec<_>>();
    fixture
        .control
        .append_supervised_events(&fixture.scope, run_id, reference, &events)
        .await
        .expect("complete with durable job");
}

pub(super) async fn output_count(fixture: &Fixture, run_id: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM outbox WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'",
    )
    .bind(fixture.scope.company_id())
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("publication count")
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn completed_reply_and_channel_freeze_only_last_turn_at_the_canonical_source() {
    for intent in [DeliveryIntentKind::Reply, DeliveryIntentKind::Channel] {
        let fixture = Fixture::new().await;
        let (run_id, reference, _) = fixture.started().await;
        complete(
            &fixture,
            run_id,
            &reference,
            intent,
            vec![BoundedText::raw("Hello "), BoundedText::raw("world")],
        )
        .await;
        assert_eq!(
            output_count(&fixture, run_id).await,
            0,
            "completion persists job before worker"
        );
        let pending: String = sqlx::query_scalar(
            "SELECT state FROM runtime_office_outputs WHERE company_id=$1 AND run_id=$2",
        )
        .bind(fixture.scope.company_id())
        .bind(run_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("crash recoverable completion job");
        assert_eq!(pending, "pending");
        let report = schedule_office_outputs(&fixture.control, &fixture.scope, 64)
            .await
            .expect("schedule output");
        assert_eq!(
            (
                report.attempted,
                report.enqueued,
                report.failed,
                report.retrying
            ),
            (1, 1, 0, 0)
        );
        let draft = office_output_draft(&fixture.control, &fixture.scope, run_id)
            .await
            .expect("stored draft")
            .expect("enqueued");
        assert_eq!(draft.content, "Hello world");
        assert_eq!(draft.kind, 9);
        let channel:Uuid=sqlx::query_scalar("SELECT i.channel_id FROM office_inbox i JOIN runs r ON r.company_id=i.company_id AND r.message_id=i.event_id WHERE r.company_id=$1 AND r.id=$2")
            .bind(fixture.scope.company_id()).bind(run_id).fetch_one(&fixture.pool).await.expect("source channel");
        let mut expected = vec![vec!["h".to_owned(), channel.to_string()]];
        if intent == DeliveryIntentKind::Reply {
            expected.push(vec![
                "e".to_owned(),
                hex::encode(fixture.run(run_id).await.message_id),
                String::new(),
                "reply".to_owned(),
            ]);
        }
        assert_eq!(
            draft.tags, expected,
            "runtime target hint must never become a tag"
        );
        assert_eq!(output_count(&fixture, run_id).await, 1);
        assert_eq!(
            schedule_office_outputs(&fixture.control, &fixture.scope, 64)
                .await
                .expect("replay")
                .attempted,
            0
        );
        assert_eq!(
            office_output_draft(&fixture.control, &fixture.scope, run_id)
                .await
                .expect("reload"),
            Some(draft)
        );
        let error=sqlx::query("UPDATE runtime_office_outputs SET draft_content='changed' WHERE company_id=$1 AND run_id=$2")
            .bind(fixture.scope.company_id()).bind(run_id).execute(&fixture.pool).await.expect_err("draft is immutable");
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("55000")
        );
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn invalid_final_text_and_post_completion_revocation_leave_visible_failed_jobs() {
    let mut truncated = BoundedText::raw("partial");
    truncated.truncated = true;
    for (deltas, expected) in [
        (vec![BoundedText::raw(" \n")], "office_output_empty"),
        (vec![truncated], "office_output_truncated"),
        (
            vec![BoundedText::raw("x".repeat(4096)); 9],
            "office_output_oversized",
        ),
    ] {
        let fixture = Fixture::new().await;
        let (run_id, reference, _) = fixture.started().await;
        complete(
            &fixture,
            run_id,
            &reference,
            DeliveryIntentKind::Reply,
            deltas,
        )
        .await;
        let report = schedule_office_outputs(&fixture.control, &fixture.scope, 64)
            .await
            .expect("reject invalid final output");
        assert_eq!((report.failed, report.enqueued), (1, 0));
        let error: String = sqlx::query_scalar(
            "SELECT last_error_code FROM runtime_office_outputs WHERE company_id=$1 AND run_id=$2",
        )
        .bind(fixture.scope.company_id())
        .bind(run_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("visible failure");
        assert_eq!(error, expected);
        assert_eq!(output_count(&fixture, run_id).await, 0);
    }
    for cancel in [false, true] {
        let fixture = Fixture::new().await;
        let (run_id, reference, _) = fixture.started().await;
        complete(
            &fixture,
            run_id,
            &reference,
            DeliveryIntentKind::Reply,
            vec![BoundedText::raw("answer")],
        )
        .await;
        if cancel {
            fixture
                .control
                .enqueue_cancellation(&fixture.scope, run_id, CancellationReason::OfficeRevoked)
                .await
                .expect("pending stop");
        } else {
            sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND pubkey=$2")
                .bind(fixture.community_id).bind(hex::decode(fixture_employee().office.public_key).expect("key"))
                .execute(&fixture.pool).await.expect("revoke source membership");
        }
        let report = schedule_office_outputs(&fixture.control, &fixture.scope, 64)
            .await
            .expect("refuse revoked output");
        assert_eq!((report.failed, report.enqueued), (1, 0));
        assert_eq!(output_count(&fixture, run_id).await, 0);
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn silent_failed_and_cancelled_runs_never_create_publication_jobs() {
    for payload in [
        RunEventPayload::RunCompleted {
            delivery_intent: DeliveryIntentKind::Silent,
        },
        RunEventPayload::RunFailed {
            code: "test_failure".to_owned(),
            message: BoundedText::raw("failure"),
        },
        RunEventPayload::RunCancelled {
            reason: BoundedText::raw("human_requested"),
        },
    ] {
        let fixture = Fixture::new().await;
        let (run_id, reference, _) = fixture.started().await;
        let event =
            RunEvent::normalize(run_id, Utc::now(), None, &payload, &RedactionPolicy::new())
                .expect("terminal event");
        fixture
            .control
            .append_supervised_events(&fixture.scope, run_id, &reference, &[event])
            .await
            .expect("terminal status");
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM runtime_office_outputs WHERE company_id=$1 AND run_id=$2",
        )
        .bind(fixture.scope.company_id())
        .bind(run_id)
        .fetch_one(&fixture.pool)
        .await
        .expect("no job");
        assert_eq!(count, 0);
        assert_eq!(
            schedule_office_outputs(&fixture.control, &fixture.scope, 64)
                .await
                .expect("nothing due")
                .attempted,
            0
        );
        assert_eq!(output_count(&fixture, run_id).await, 0);
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn enqueue_failure_retries_the_previously_frozen_draft_after_restart() {
    let fixture = Fixture::new().await;
    let (run_id, reference, _) = fixture.started().await;
    complete(
        &fixture,
        run_id,
        &reference,
        DeliveryIntentKind::Reply,
        vec![BoundedText::raw("original answer")],
    )
    .await;
    // Dynamic SQL contains only a constant prefix and generated typed UUIDs.
    let name = format!("output_test_{}", Uuid::new_v4().simple());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test enqueue interruption' USING ERRCODE='serialization_failure'; END $$;
        CREATE TRIGGER {name} BEFORE INSERT ON outbox FOR EACH ROW WHEN (NEW.run_id='{run_id}'::uuid AND NEW.kind='office_publish') EXECUTE FUNCTION {name}();")))
        .execute(&fixture.pool).await.expect("inject scoped enqueue interruption");
    let report = schedule_office_outputs(&fixture.control, &fixture.scope, 64)
        .await
        .expect("durable retry");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON outbox; DROP FUNCTION {name}();"
    )))
    .execute(&fixture.pool)
    .await
    .expect("remove scoped interruption");
    assert_eq!((report.retrying, report.enqueued), (1, 0));
    let frozen: String = sqlx::query_scalar(
        "SELECT draft_content FROM runtime_office_outputs WHERE company_id=$1 AND run_id=$2",
    )
    .bind(fixture.scope.company_id())
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("first phase survives failure");
    assert_eq!(frozen, "original answer");
    assert_eq!(output_count(&fixture, run_id).await, 0);
    // Even an illicit late fragment cannot make retry reconstruction change the
    // already-frozen output. Ordinary supervised appends refuse terminal runs.
    let late = serde_json::to_value(RunEventPayload::AssistantDelta {
        turn: 1,
        delta: BoundedText::raw("changed later"),
    })
    .expect("late fixture");
    sqlx::query("INSERT INTO run_events(company_id,run_id,sequence,event_type,occurred_at,payload)
        SELECT $1,$2,max(sequence)+1,'assistant.delta',clock_timestamp(),$3 FROM run_events WHERE company_id=$1 AND run_id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).bind(late).execute(&fixture.pool).await.expect("late fixture append");
    sqlx::query("UPDATE runtime_office_outputs SET next_attempt_at=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND run_id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).execute(&fixture.pool).await.expect("retry due");
    let restarted = PgControlPlane::new(fixture.pool.clone());
    let retried = schedule_office_outputs(&restarted, &fixture.scope, 64)
        .await
        .expect("restarted scheduler");
    assert_eq!((retried.enqueued, retried.failed), (1, 0));
    assert_eq!(
        office_output_draft(&restarted, &fixture.scope, run_id)
            .await
            .expect("reload")
            .expect("draft")
            .content,
        "original answer"
    );
    assert_eq!(output_count(&fixture, run_id).await, 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn frozen_channel_output_cannot_be_rebound_to_another_decision_in_the_same_channel() {
    let fixture = Fixture::new().await;
    let (run_id, reference, _) = fixture.started().await;
    complete(
        &fixture,
        run_id,
        &reference,
        DeliveryIntentKind::Channel,
        vec![BoundedText::raw("old prompt answer")],
    )
    .await;
    // Dynamic SQL contains only a constant prefix and generated typed UUIDs.
    let name = format!("source_test_{}", Uuid::new_v4().simple());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test enqueue interruption' USING ERRCODE='serialization_failure'; END $$;
        CREATE TRIGGER {name} BEFORE INSERT ON outbox FOR EACH ROW WHEN (NEW.run_id='{run_id}'::uuid AND NEW.kind='office_publish') EXECUTE FUNCTION {name}();")))
        .execute(&fixture.pool).await.expect("interrupt after draft freeze");
    let first = schedule_office_outputs(&fixture.control, &fixture.scope, 64)
        .await
        .expect("frozen retry");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON outbox; DROP FUNCTION {name}();"
    )))
    .execute(&fixture.pool)
    .await
    .expect("remove interruption");
    assert_eq!(first.retrying, 1);
    let channel:Uuid=sqlx::query_scalar("SELECT i.channel_id FROM office_inbox i JOIN runs r ON r.company_id=i.company_id AND r.message_id=i.event_id WHERE r.company_id=$1 AND r.id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).fetch_one(&fixture.pool).await.expect("original channel");
    let new_decision = fixture
        .route_kind(9, Some(channel), "Cem, a different prompt")
        .await;
    sqlx::query("UPDATE runs r SET routing_decision_id=d.id,message_id=d.message_id,root_message_id=d.root_message_id
        FROM routing_decisions d WHERE r.company_id=$1 AND r.id=$2 AND d.company_id=r.company_id AND d.id=$3")
        .bind(fixture.scope.company_id()).bind(run_id).bind(new_decision).execute(&fixture.pool).await.expect("retarget run to other valid decision");
    sqlx::query("UPDATE runtime_office_outputs SET next_attempt_at=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND run_id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).execute(&fixture.pool).await.expect("retry due");
    let result = schedule_office_outputs(&fixture.control, &fixture.scope, 64)
        .await
        .expect("source pin rejects retarget");
    assert_eq!((result.failed, result.enqueued), (1, 0));
    let row=sqlx::query("SELECT last_error_code,draft_content FROM runtime_office_outputs WHERE company_id=$1 AND run_id=$2")
        .bind(fixture.scope.company_id()).bind(run_id).fetch_one(&fixture.pool).await.expect("original output preserved");
    assert_eq!(
        row.get::<String, _>("last_error_code"),
        "office_output_authority_changed"
    );
    assert_eq!(row.get::<String, _>("draft_content"), "old prompt answer");
    assert_eq!(output_count(&fixture, run_id).await, 0);
}
