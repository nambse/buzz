use super::*;
use ortak_office::publisher::{OfficePublishError, OfficePublisher, PublishReceipt};
use ortak_runtime::memory_output::schedule_memory_output;
use ortak_runtime::office_delivery::deliver_one_office_output;
use ortak_runtime::office_output::schedule_office_outputs;

async fn completed(c: &ConversationFixture) -> Uuid {
    let (run, reference) = c.start_office().await;
    c.complete(run, &reference).await;
    assert_eq!(
        schedule_office_outputs(&c.x.f.control, &c.x.scope, 1)
            .await
            .unwrap()
            .enqueued,
        1
    );
    run
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with conversation76"]
async fn conversation_runtime_nested_office_reply_uses_thread_root_and_retries_exact_bytes() {
    let c = ConversationFixture::new().await;
    // Real human routing starts a fresh delivery chain at a native direct
    // reply. The employee's response is consequently a nested Office reply.
    let run = completed(&c).await;
    let (message, chain_root): (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT message_id,root_message_id FROM runs WHERE company_id=$1 AND id=$2")
            .bind(c.x.f.company)
            .bind(run)
            .fetch_one(&c.x.f.pool)
            .await
            .unwrap();
    assert_eq!(message, chain_root);
    assert_ne!(hex::encode(&chain_root), c.x.source);
    let draft = ortak_runtime::office_output::office_output_draft(&c.x.f.control, &c.x.scope, run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        draft.tags,
        vec![
            vec!["h".to_owned(), c.x.f.channel.to_string()],
            vec![
                "e".to_owned(),
                c.x.source.clone(),
                String::new(),
                "root".to_owned(),
            ],
            vec![
                "e".to_owned(),
                hex::encode(&message),
                String::new(),
                "reply".to_owned(),
            ],
        ],
        "the relay checks the immediate parent's canonical thread root"
    );
    let publisher = FakeOfficePublisher::new();
    publisher.fail_next(1);
    let service = OfficeDeliveryService::new(
        c.x.f.control.clone(),
        &c.signer,
        &publisher,
        DeliveryConfig::default(),
    );
    assert!(
        deliver_one_office_output(&c.x.f.control, &c.x.scope, "nested-first", &service)
            .await
            .unwrap()
    );
    assert_eq!(
        c.signer.sign_calls(),
        1,
        "delivery must accept the canonical tags"
    );
    assert_eq!(publisher.publish_calls(), 1);
    let frozen: Vec<u8> = sqlx::query_scalar("SELECT signed_event_bytes FROM outbox WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'")
        .bind(c.x.f.company).bind(run).fetch_one(&c.x.f.pool).await.unwrap();
    let signed: Value = serde_json::from_slice(&frozen).unwrap();
    assert_eq!(signed["tags"], json!(draft.tags));
    sqlx::query("UPDATE outbox SET retry_after=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'")
        .bind(c.x.f.company).bind(run).execute(&c.x.f.pool).await.unwrap();
    assert!(
        deliver_one_office_output(&c.x.f.control, &c.x.scope, "nested-retry", &service)
            .await
            .unwrap()
    );
    let (state, after): (String, Vec<u8>) = sqlx::query_as("SELECT state,signed_event_bytes FROM outbox WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'")
        .bind(c.x.f.company).bind(run).fetch_one(&c.x.f.pool).await.unwrap();
    assert_eq!(state, "delivered");
    assert_eq!(c.signer.sign_calls(), 1);
    assert_eq!(publisher.publish_calls(), 2);
    assert_eq!(after, frozen, "retry preserves the first signed payload");
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with conversation76"]
async fn conversation_runtime_frozen_office_retry_cannot_publish_after_revocation() {
    let c = ConversationFixture::new().await;
    let run = completed(&c).await;
    let publisher = FakeOfficePublisher::new();
    publisher.fail_next(1);
    let service = OfficeDeliveryService::new(
        c.x.f.control.clone(),
        &c.signer,
        &publisher,
        DeliveryConfig::default(),
    );
    assert!(
        deliver_one_office_output(&c.x.f.control, &c.x.scope, "v4-frozen-first", &service)
            .await
            .unwrap()
    );
    assert_eq!(c.signer.sign_calls(), 1);
    assert_eq!(publisher.publish_calls(), 1);
    let frozen: Vec<u8> = sqlx::query_scalar("SELECT signed_event_bytes FROM outbox WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'")
        .bind(c.x.f.company).bind(run).fetch_one(&c.x.f.pool).await.unwrap();
    c.opt_out().await;
    sqlx::query("UPDATE outbox SET retry_after=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'")
        .bind(c.x.f.company).bind(run).execute(&c.x.f.pool).await.unwrap();
    assert!(
        deliver_one_office_output(&c.x.f.control, &c.x.scope, "v4-frozen-retry", &service)
            .await
            .unwrap()
    );
    assert_eq!(c.signer.sign_calls(), 1, "no signing authority was renewed");
    assert_eq!(
        publisher.publish_calls(),
        1,
        "frozen bytes are not a publication grant"
    );
    let row = sqlx::query("SELECT state,last_error,signed_event_bytes,lease_token FROM outbox WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'")
        .bind(c.x.f.company).bind(run).fetch_one(&c.x.f.pool).await.unwrap();
    assert_eq!(row.get::<String, _>("state"), "pending");
    assert_eq!(
        row.get::<String, _>("last_error"),
        "office_delivery_authority_refused"
    );
    assert_eq!(row.get::<Vec<u8>, _>("signed_event_bytes"), frozen);
    assert!(
        row.get::<Option<Uuid>, _>("lease_token").is_none(),
        "failed attempt remains durably retryable"
    );
}

struct RevokeAfterPublish<'a> {
    publisher: &'a FakeOfficePublisher,
    fixture: &'a ConversationFixture,
}
impl OfficePublisher for RevokeAfterPublish<'_> {
    async fn publish(
        &self,
        scope: &CompanyScope,
        event: &ortak_office::FrozenSignedEvent,
    ) -> Result<PublishReceipt, OfficePublishError> {
        let receipt = self.publisher.publish(scope, event).await?;
        // The remote has accepted the exact event. Lose use authority before
        // returning its ACK: local correlation must survive without new output.
        self.fixture.opt_out().await;
        Ok(receipt)
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with conversation76"]
async fn conversation_runtime_late_office_ack_survives_but_memory_write_and_text_are_withheld() {
    let c = ConversationFixture::new().await;
    let run = completed(&c).await;
    let before = c.bytes(run).await;
    let publisher = FakeOfficePublisher::new();
    let revoking = RevokeAfterPublish {
        publisher: &publisher,
        fixture: &c,
    };
    let service = OfficeDeliveryService::new(
        c.x.f.control.clone(),
        &c.signer,
        &revoking,
        DeliveryConfig::default(),
    );
    assert!(
        deliver_one_office_output(&c.x.f.control, &c.x.scope, "v4-post-ack", &service)
            .await
            .unwrap()
    );
    assert_eq!(publisher.published().len(), 1);
    let delivered: bool = sqlx::query_scalar("SELECT state='delivered' FROM outbox WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'")
        .bind(c.x.f.company).bind(run).fetch_one(&c.x.f.pool).await.unwrap();
    assert!(
        delivered,
        "receipt-only correlation persists after source revocation"
    );
    let report = schedule_memory_output(&c.x.f.control, &c.memory, &c.x.scope)
        .await
        .unwrap();
    assert_eq!(
        (
            report.attempted,
            report.acknowledged,
            report.failed_attempts
        ),
        (1, 0, 1)
    );
    let stored: (String, String, Option<Value>) = sqlx::query_as(
        "SELECT state,content,receipt FROM runtime_memory_writes WHERE company_id=$1 AND run_id=$2",
    )
    .bind(c.x.f.company)
    .bind(run)
    .fetch_one(&c.x.f.pool)
    .await
    .unwrap();
    assert_eq!(stored.0, "failed");
    assert_eq!(stored.1, ANSWER);
    assert!(stored.2.is_none());
    let (status, body) = c.read(run).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["memory"]["write"]["withheld"], true);
    assert_eq!(body["memory"]["write"]["status"], "failed");
    assert_eq!(body["memory"]["write"]["content"]["text"], "");
    assert_eq!(body["memory"]["recall"]["withheld"], true);
    for canary in [FACT, SCRATCH, ANSWER] {
        assert!(!body["memory"].to_string().contains(canary));
    }
    assert_eq!(c.bytes(run).await, before);
}
