use super::*;

#[tokio::test]
#[ignore = "requires Postgres"]
async fn stale_lease_cannot_freeze_or_publish() {
    let fixture = Fixture::new().await;
    let authorized = fixture.enqueue().await;
    let outbox_id = authorized.outbox_id();
    let service = fixture.service(Duration::from_secs(30));

    let stale = fixture.claim(Duration::ZERO).await;
    let current = fixture.claim(Duration::from_secs(30)).await;
    assert_ne!(stale.lease_token, current.lease_token);

    assert_eq!(
        service
            .deliver(&fixture.scope, &stale, &authorized)
            .await
            .expect("deliver"),
        DeliveryOutcome::StaleLease
    );
    fixture.assert_untouched(outbox_id).await;

    // Even a correctly signed event cannot be frozen under the stale token.
    let signing = authorized
        .signing_request(Utc::now())
        .expect("signing request");
    let signed = fixture.signer.sign(&signing).await.expect("sign");
    assert_eq!(
        fixture
            .control
            .freeze_signed_event(&fixture.scope, &stale, &signed)
            .await
            .expect("stale freeze"),
        FreezeOutcome::StaleLease
    );
    let row = fixture.row(outbox_id).await;
    assert!(row.signed_event_bytes.is_none());
    assert!(fixture.publisher.published().is_empty());

    // The current holder delivers normally and signs exactly once more.
    let outcome = service
        .deliver(&fixture.scope, &current, &authorized)
        .await
        .expect("deliver");
    assert!(
        matches!(
            outcome,
            DeliveryOutcome::Delivered {
                signed_now: true,
                receipt: PublishReceipt::Accepted,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(fixture.signer.sign_calls(), 2);
    assert_eq!(fixture.publisher.published().len(), 1);
    assert_eq!(fixture.row(outbox_id).await.state, "delivered");
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn cross_company_wrong_run_wrong_kind_and_mismatched_intent_fail_closed() {
    let fixture = Fixture::new().await;
    let other = Fixture::new().await;
    let authorized = fixture.enqueue().await;
    let outbox_id = authorized.outbox_id();
    let service = fixture.service(Duration::from_secs(30));
    let lease = fixture.claim(Duration::from_secs(30)).await;

    // A draft scoped to another company is refused before any lookup.
    let mut foreign = fixture.draft().await;
    foreign.company_id = other.scope.company_id();
    assert!(matches!(
        fixture
            .control
            .enqueue_office_publish(&fixture.scope, &foreign)
            .await
            .unwrap_err(),
        OfficeDeliveryError::CompanyMismatch { .. }
    ));

    // Another company's authorized publish cannot use this lease: it names
    // another run, and even a lease edited to name that run is a different
    // row than the one the publish was issued for. This company's publish
    // is refused under the other scope before any lookup.
    let through_other = other.enqueue().await;
    assert!(matches!(
        other
            .service(Duration::from_secs(30))
            .deliver(&other.scope, &lease, &through_other)
            .await
            .unwrap_err(),
        OfficeDeliveryError::WrongRun { .. }
    ));
    let mut renamed = lease.clone();
    renamed.run_id = Some(other.run_id);
    assert!(matches!(
        other
            .control
            .frozen_event(&other.scope, &renamed, &through_other)
            .await
            .unwrap_err(),
        OfficeDeliveryError::WrongRow { expected, found }
            if expected == through_other.outbox_id() && found == lease.id
    ));
    assert!(matches!(
        other
            .service(Duration::from_secs(30))
            .deliver(&other.scope, &lease, &authorized)
            .await
            .unwrap_err(),
        OfficeDeliveryError::CompanyMismatch { .. }
    ));

    // Wrong run: another completed run of the same employee has its own row.
    let second_run = insert_canonical_run(
        &fixture.pool,
        fixture.scope.company_id(),
        fixture.revision_id,
        "Something else",
    )
    .await;
    let mut second_draft = fixture.draft_for(second_run).await;
    second_draft.content = "Something else".to_owned();
    let second = fixture
        .control
        .enqueue_office_publish(&fixture.scope, &second_draft)
        .await
        .expect("enqueue second run")
        .into_authorized();
    assert!(matches!(
        service
            .deliver(&fixture.scope, &lease, &second)
            .await
            .unwrap_err(),
        OfficeDeliveryError::WrongRun { .. }
    ));

    // Wrong outbox kind.
    let mut wrong_kind = lease.clone();
    wrong_kind.kind = OutboxKind::RunDispatch;
    assert!(matches!(
        service
            .deliver(&fixture.scope, &wrong_kind, &authorized)
            .await
            .unwrap_err(),
        OfficeDeliveryError::WrongKind { .. }
    ));

    // The frozen canonical draft refuses different content before the outbox
    // fingerprint check, so no changed intent can become authorized.
    let mut different = fixture.draft().await;
    different.content = "Something else".to_owned();
    assert!(matches!(
        fixture
            .control
            .enqueue_office_publish(&fixture.scope, &different)
            .await
            .unwrap_err(),
        OfficeDeliveryError::Control(ortak_control::ControlError::InvalidData(message))
            if message == "office delivery authority is no longer valid"
    ));

    // A signed event for another run cannot be frozen into this row.
    let signing = second.signing_request(Utc::now()).expect("signing request");
    let foreign_event = fixture.signer.sign(&signing).await.expect("sign");
    assert!(matches!(
        fixture
            .control
            .freeze_signed_event(&fixture.scope, &lease, &foreign_event)
            .await
            .unwrap_err(),
        OfficeDeliveryError::WrongRun { .. }
    ));

    let row = fixture.row(outbox_id).await;
    assert_eq!(row.state, "pending");
    assert!(row.signed_event_bytes.is_none());
    assert!(fixture.publisher.published().is_empty());
    assert!(other.publisher.published().is_empty());

    // The matching authorized publish still delivers under the same lease.
    assert!(matches!(
        service
            .deliver(&fixture.scope, &lease, &authorized)
            .await
            .expect("deliver"),
        DeliveryOutcome::Delivered {
            signed_now: true,
            ..
        }
    ));
    assert_eq!(fixture.row(outbox_id).await.state, "delivered");
}
