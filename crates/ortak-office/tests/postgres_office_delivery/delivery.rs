use super::*;

#[tokio::test]
#[ignore = "requires Postgres"]
async fn publish_failure_after_freeze_retries_identical_bytes_without_resigning() {
    let fixture = Fixture::new().await;
    let authorized = fixture.enqueue().await;
    let outbox_id = authorized.outbox_id();
    let service = fixture.service(Duration::ZERO);

    // First attempt: signed and frozen, then the Office is unreachable.
    fixture.publisher.fail_next(1);
    let first = fixture.claim(Duration::from_secs(30)).await;
    let outcome = service
        .deliver(&fixture.scope, &first, &authorized)
        .await
        .expect("deliver");
    let DeliveryOutcome::Retrying {
        event_id: Some(event_id),
        ..
    } = outcome
    else {
        panic!("expected a retrying outcome with a frozen id, got {outcome:?}");
    };
    assert_eq!(fixture.signer.sign_calls(), 1);
    assert!(fixture.publisher.published().is_empty());
    let frozen = fixture.row(outbox_id).await;
    assert_eq!(frozen.state, "pending");
    assert_eq!(frozen.attempt_count, 1);
    assert_eq!(
        frozen.signed_event_id.as_deref(),
        Some(event_id.as_bytes().as_slice())
    );
    let frozen_bytes = frozen
        .signed_event_bytes
        .expect("bytes frozen before first publish");
    assert!(frozen.last_error.is_some());

    // Retry under a new lease from a replayed enqueue (a fresh process):
    // same bytes, same id, signer not invoked.
    let replayed = fixture.replay(&authorized).await;
    let second = fixture.claim(Duration::from_secs(30)).await;
    assert_ne!(second.lease_token, first.lease_token);
    assert_eq!(
        service
            .deliver(&fixture.scope, &second, &replayed)
            .await
            .expect("deliver"),
        DeliveryOutcome::Delivered {
            event_id,
            signed_now: false,
            receipt: PublishReceipt::Accepted,
        }
    );
    assert_eq!(fixture.signer.sign_calls(), 1);
    let published = fixture.publisher.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].event_id, event_id);
    assert_eq!(published[0].signed_bytes, frozen_bytes);
    let delivered = fixture.row(outbox_id).await;
    assert_eq!(delivered.state, "delivered");
    assert_eq!(
        delivered.signed_event_bytes.as_deref(),
        Some(frozen_bytes.as_slice())
    );
    fixture.nothing_due().await;
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn crash_after_freeze_is_recovered_by_the_next_lease_holder_without_resigning() {
    let fixture = Fixture::new().await;
    let authorized = fixture.enqueue().await;
    let outbox_id = authorized.outbox_id();

    // Worker A signs and freezes under a live lease, then crashes before
    // publishing. Expire that lease only after the freeze has committed.
    let crashed = fixture.claim(Duration::from_secs(30)).await;
    let signing = authorized
        .signing_request(Utc::now())
        .expect("signing request");
    let signed = fixture.signer.sign(&signing).await.expect("sign");
    let FreezeOutcome::Frozen(frozen) = fixture
        .control
        .freeze_signed_event(&fixture.scope, &crashed, &signed)
        .await
        .expect("freeze")
    else {
        panic!("lease is current, freeze must succeed");
    };
    assert_eq!(*frozen, signed);
    assert!(fixture.publisher.published().is_empty());
    let expired = sqlx::query(
        "UPDATE outbox SET lease_expires_at=clock_timestamp()-interval '1 second'
         WHERE company_id=$1 AND id=$2 AND lease_token=$3",
    )
    .bind(fixture.scope.company_id())
    .bind(outbox_id)
    .bind(crashed.lease_token)
    .execute(&fixture.pool)
    .await
    .expect("expire crashed worker lease after freeze");
    assert_eq!(expired.rows_affected(), 1);

    // Worker B reclaims and publishes exactly the frozen event.
    let service = fixture.service(Duration::from_secs(30));
    let recovered = fixture.claim(Duration::from_secs(30)).await;
    assert_ne!(recovered.lease_token, crashed.lease_token);
    assert_eq!(
        service
            .deliver(&fixture.scope, &recovered, &authorized)
            .await
            .expect("deliver"),
        DeliveryOutcome::Delivered {
            event_id: signed.event_id(),
            signed_now: false,
            receipt: PublishReceipt::Accepted,
        }
    );
    assert_eq!(fixture.signer.sign_calls(), 1);
    let published = fixture.publisher.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].signed_bytes, signed.signed_bytes());
    assert_eq!(fixture.row(outbox_id).await.state, "delivered");

    // The crashed worker can no longer freeze anything into the row.
    assert_eq!(
        fixture
            .control
            .freeze_signed_event(&fixture.scope, &crashed, &signed)
            .await
            .expect("stale freeze"),
        FreezeOutcome::StaleLease
    );
}
