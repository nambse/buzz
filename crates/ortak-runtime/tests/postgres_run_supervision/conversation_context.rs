//! Real normalizer → authority → supervisor → snapshot → delivery gates.
use super::*;
use ortak_control::conversation_context::ConversationContext;
use ortak_runtime::memory_context::RunContextRepository;
use ortak_runtime::office_output::schedule_office_outputs;

async fn source(
    f: &Fixture,
    channel: Uuid,
    key: &[u8],
    text: &str,
    seconds: i64,
    root: Option<(MessageId, chrono::DateTime<Utc>)>,
) -> (MessageId, chrono::DateTime<Utc>) {
    let id = message_id();
    let at = Utc::now() - chrono::Duration::seconds(seconds);
    sqlx::query("INSERT INTO events(community_id,id,pubkey,created_at,received_at,kind,tags,content,sig,channel_id) VALUES($1,$2,$3,$4,$4,9,'[]',$5,$6,$7)")
        .bind(f.community_id).bind(id.as_bytes().as_slice()).bind(key).bind(at).bind(text)
        .bind([9u8;64].as_slice()).bind(channel).execute(&f.pool).await.expect("prior canonical source");
    if let Some((root, root_at)) = root {
        sqlx::query("INSERT INTO thread_metadata(community_id,event_id,event_created_at,channel_id,parent_event_id,parent_event_created_at,root_event_id,root_event_created_at,depth) VALUES($1,$2,$3,$4,$5,$6,$5,$6,1)")
            .bind(f.community_id).bind(id.as_bytes().as_slice()).bind(at).bind(channel)
            .bind(root.as_bytes().as_slice()).bind(root_at).execute(&f.pool).await.expect("source thread");
    }
    (id, at)
}

async fn setup() -> (Fixture, Uuid, Employee) {
    let mut bora = fixture_employee();
    bora.id = employee_id("bora");
    bora.name = "Bora".into();
    bora.title = "Writer".into();
    let f = Fixture::new_for_employee(bora).await;
    let channel = f
        .control
        .routing_cohort(&f.scope)
        .await
        .unwrap()
        .unwrap()
        .channel_ids[0];
    let mut ada = fixture_employee();
    ada.id = employee_id("ada");
    ada.name = "Ada".into();
    ada.title = "Product lead".into();
    ada.office.public_key = "44".repeat(32);
    ada.office.signer_ref =
        ortak_domain::CredentialRef::parse("credential://fixture/ada-signer").unwrap();
    activate_employee(&f.pool, f.scope.company_id(), &ada, true).await;
    for key in [
        hex::decode(&ada.office.public_key).unwrap(),
        hex::decode(&f.employee.office.public_key).unwrap(),
        vec![7u8; 32],
    ] {
        sqlx::query("INSERT INTO channel_members(community_id,channel_id,pubkey) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
            .bind(f.community_id).bind(channel).bind(key).execute(&f.pool).await.unwrap();
    }
    cohort_support::select_and_reconcile(
        &f.control,
        &f.scope,
        &[channel],
        &[f.employee.id.clone(), ada.id.clone()],
    )
    .await;
    (f, channel, ada)
}

async fn snapshot(f: &Fixture, run: Uuid) -> ConversationContext {
    let bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT spec_bytes FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company_id())
    .bind(run)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let wire: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    serde_json::from_value(wire["spec"]["context"]["conversation_context"].clone()).unwrap()
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn ada_to_bora_sources_roles_cutoff_and_retry_are_canonical() {
    let (f, channel, ada) = setup().await;
    let root = source(
        &f,
        channel,
        &[7; 32],
        "Bir ürün üzerinde çalışacağım, kim yardımcı olur?",
        10,
        None,
    )
    .await;
    let answer = source(
        &f,
        channel,
        &hex::decode(&ada.office.public_key).unwrap(),
        "1. Ürün hedefini netleştirelim.\n2. Bora metne yardımcı olur.",
        8,
        Some(root),
    )
    .await;
    f.route_kind(
        KIND_STREAM_MESSAGE,
        Some(channel),
        "Bora, bunu İngilizceye çevirir misin?",
    )
    .await;
    let lease = f.lease(Duration::from_secs(60)).await;
    let authority = authorized(
        f.control
            .authorize_dispatch(&f.scope, &lease)
            .await
            .unwrap(),
    );
    // An event created earlier but received after the trigger is not historical input.
    let late = source(
        &f,
        channel,
        &[7; 32],
        "late arrival must not appear",
        5,
        None,
    )
    .await;
    sqlx::query("UPDATE events SET received_at=clock_timestamp() WHERE community_id=$1 AND id=$2 AND created_at=$3")
        .bind(f.community_id).bind(late.0.as_bytes().as_slice()).bind(late.1).execute(&f.pool).await.unwrap();
    f.adapter.set_unavailable(true);
    let run = match f
        .supervisor(f.config())
        .dispatch(&f.scope, &lease)
        .await
        .unwrap()
    {
        DispatchOutcome::RuntimeFailed { run_id, .. } => run_id,
        other => panic!("expected retryable start failure: {other:?}"),
    };
    let selected = snapshot(&f, run).await;
    assert_eq!(selected.employee.name, "Bora");
    assert!(selected
        .teammates
        .iter()
        .any(|e| e.name == "Ada" && e.title == "Product lead"));
    assert_eq!(
        selected
            .messages
            .iter()
            .map(|m| m.message_id.clone())
            .collect::<Vec<_>>(),
        vec![root.0.to_hex(), answer.0.to_hex()]
    );
    assert_eq!(
        selected.messages[1].author_employee_id.as_ref(),
        Some(&ada.id)
    );
    assert!(selected.messages[1].content.contains("Ürün hedefini"));
    let first = f
        .control
        .load_run_snapshot(&f.scope, &authority, run)
        .await
        .unwrap()
        .unwrap()
        .encode()
        .unwrap();
    source(
        &f,
        channel,
        &[7; 32],
        "new turn cannot change frozen input",
        0,
        None,
    )
    .await;
    let retry = f.lease(Duration::from_secs(60)).await;
    f.adapter.set_unavailable(false);
    assert!(
        matches!(f.supervisor(f.config()).dispatch(&f.scope,&retry).await.unwrap(),DispatchOutcome::Started{run_id,..} if run_id==run)
    );
    let again = f
        .control
        .load_run_snapshot(&f.scope, &authority, run)
        .await
        .unwrap()
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(first, again);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn explicit_reply_ignores_other_thread_and_source_deletion_fences_delivery() {
    let (f, channel, ada) = setup().await;
    let key = hex::decode(&ada.office.public_key).unwrap();
    let root = source(&f, channel, &[7; 32], "First product", 20, None).await;
    let answer = source(
        &f,
        channel,
        &key,
        "1. First plan\n2. Second step",
        18,
        Some(root),
    )
    .await;
    let unrelated = source(&f, channel, &[7; 32], "Other product", 12, None).await;
    source(
        &f,
        channel,
        &key,
        "Unrelated newest reply",
        10,
        Some(unrelated),
    )
    .await;
    f.route_kind_with_reply(
        KIND_STREAM_MESSAGE,
        Some(channel),
        "Bora, ikinci maddeyi değiştir",
        Some((answer.0, answer.1, root.0, root.1)),
    )
    .await;
    let lease = f.lease(Duration::from_secs(60)).await;
    let (run, reference) = match f
        .supervisor(f.config())
        .dispatch(&f.scope, &lease)
        .await
        .unwrap()
    {
        DispatchOutcome::Started {
            run_id,
            runtime_run_ref,
        } => (run_id, runtime_run_ref),
        other => panic!("expected start: {other:?}"),
    };
    let selected = snapshot(&f, run).await;
    assert_eq!(selected.thread_root_message_id, Some(root.0.to_hex()));
    assert_eq!(selected.messages.len(), 2);
    assert_eq!(selected.messages[1].message_id, answer.0.to_hex());
    super::office_output::complete(
        &f,
        run,
        &reference,
        DeliveryIntentKind::Reply,
        vec![BoundedText::raw("Updated second step")],
    )
    .await;
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2 AND created_at=$3")
        .bind(f.community_id).bind(answer.0.as_bytes().as_slice()).bind(answer.1).execute(&f.pool).await.unwrap();
    let current: bool = sqlx::query_scalar("SELECT ortak_run_conversation_context_current($1,$2)")
        .bind(f.scope.company_id())
        .bind(run)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert!(!current);
    let report = schedule_office_outputs(&f.control, &f.scope, 64)
        .await
        .unwrap();
    assert_eq!(report.enqueued, 0, "deleted source must prevent delivery");
    assert_eq!(super::office_output::output_count(&f, run).await, 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn recipient_revocation_blocks_frozen_context_before_retry_start() {
    let (f, channel, ada) = setup().await;
    source(
        &f,
        channel,
        &hex::decode(&ada.office.public_key).unwrap(),
        "Ada's actual proposal",
        5,
        None,
    )
    .await;
    f.route_kind(
        KIND_STREAM_MESSAGE,
        Some(channel),
        "Bora, translate Ada's proposal",
    )
    .await;
    let lease = f.lease(Duration::from_secs(60)).await;
    let authority = authorized(
        f.control
            .authorize_dispatch(&f.scope, &lease)
            .await
            .unwrap(),
    );
    f.adapter.set_unavailable(true);
    let run = match f
        .supervisor(f.config())
        .dispatch(&f.scope, &lease)
        .await
        .unwrap()
    {
        DispatchOutcome::RuntimeFailed { run_id, .. } => run_id,
        other => panic!("expected start failure: {other:?}"),
    };
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community_id).bind(channel).bind(hex::decode(&f.employee.office.public_key).unwrap())
        .execute(&f.pool).await.unwrap();
    assert!(f
        .control
        .load_run_snapshot(&f.scope, &authority, run)
        .await
        .is_err());
    f.adapter.set_unavailable(false);
    let retry = f.lease(Duration::from_secs(60)).await;
    assert!(matches!(
        f.supervisor(f.config())
            .dispatch(&f.scope, &retry)
            .await
            .unwrap(),
        DispatchOutcome::Refused { .. }
    ));
    assert!(f.run(run).await.runtime_run_ref.is_none());
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn long_history_is_bounded_and_foreign_channel_and_deleted_text_are_absent() {
    let (f, channel, ada) = setup().await;
    let key = hex::decode(&ada.office.public_key).unwrap();
    for i in 0..40 {
        source(
            &f,
            channel,
            &key,
            &format!("source {i}: {}", "ü".repeat(5000)),
            100 - i,
            None,
        )
        .await;
    }
    let deleted = source(&f, channel, &key, "deleted-secret-fixture", 3, None).await;
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2 AND created_at=$3")
        .bind(f.community_id).bind(deleted.0.as_bytes().as_slice()).bind(deleted.1).execute(&f.pool).await.unwrap();
    let foreign:Uuid=sqlx::query_scalar("INSERT INTO channels(community_id,name,created_by) VALUES($1,'foreign-scope',$2) RETURNING id")
        .bind(f.community_id).bind([7u8;32].as_slice()).fetch_one(&f.pool).await.unwrap();
    source(&f, foreign, &key, "foreign-secret-fixture", 2, None).await;
    f.route_kind(
        KIND_STREAM_MESSAGE,
        Some(channel),
        "Bora, summarize the available context",
    )
    .await;
    let lease = f.lease(Duration::from_secs(60)).await;
    let run = match f
        .supervisor(f.config())
        .dispatch(&f.scope, &lease)
        .await
        .unwrap()
    {
        DispatchOutcome::Started { run_id, .. } => run_id,
        other => panic!("expected start: {other:?}"),
    };
    let selected = snapshot(&f, run).await;
    assert!(selected.omitted_history);
    assert!(selected.messages.len() <= 32);
    assert!(
        selected
            .messages
            .iter()
            .map(|m| m.content.len())
            .sum::<usize>()
            <= 48 * 1024
    );
    assert!(selected.messages.iter().all(|m| m.content.len() <= 8192));
    assert!(selected.messages.iter().any(|m| m.truncated));
    let encoded = serde_json::to_string(&selected).unwrap();
    assert!(encoded.len() <= 64 * 1024);
    assert!(!encoded.contains("secret-fixture"));
}
