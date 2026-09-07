use super::*;

#[tokio::test]
#[ignore = "requires Postgres"]
async fn identity_and_signer_come_from_the_run_and_its_verified_binding() {
    let fixture = Fixture::new().await;
    let company_id = fixture.scope.company_id();

    // The caller's draft names only company, run, and message; the authorized
    // publish carries the employee and revision of the run row and the signer
    // and key of that revision's verified binding.
    let authorized = fixture.enqueue().await;
    assert_eq!(authorized.employee_id().as_str(), "cem");
    assert_eq!(authorized.employee_revision_id(), fixture.revision_id);
    assert_eq!(authorized.binding_id(), fixture.binding_id);
    assert_eq!(authorized.signer_ref().as_str(), SIGNER_REF);
    assert_eq!(*authorized.public_key(), fixture.public_key());
    assert_eq!(authorized.run_id(), fixture.run_id);
    assert_eq!(authorized.intent().kind, KIND_STREAM_MESSAGE);
    assert_eq!(authorized.intent().content, "Merhaba from Cem");
    let payload = authorized.payload();
    assert_eq!(payload.employee_id.as_str(), "cem");
    assert_eq!(payload.employee_revision_id, fixture.revision_id);
    assert_eq!(payload.public_key, fixture.public_key());
    // Replay is idempotent and yields the identical canonical object.
    fixture.replay(&authorized).await;
    assert_eq!(fixture.outbox_rows().await, 1);

    // A run that is not completed, or completed silently, cannot publish.
    for (status, intent) in [
        ("running", None),
        ("waiting", None),
        ("completed", Some("silent")),
        ("failed", None),
    ] {
        let run_id = insert_run(
            &fixture.pool,
            company_id,
            "cem",
            fixture.revision_id,
            status,
            intent,
        )
        .await;
        let error = fixture
            .control
            .enqueue_office_publish(&fixture.scope, &fixture.draft_for(run_id).await)
            .await
            .unwrap_err();
        assert!(
            matches!(&error, OfficeDeliveryError::RunNotPublishable { run_id: found, .. } if *found == run_id),
            "{status} / {intent:?}: {error:?}"
        );
    }

    // An unknown run and a run of another company are refused.
    let unknown = Uuid::new_v4();
    assert!(matches!(
        fixture
            .control
            .enqueue_office_publish(&fixture.scope, &fixture.draft_for(unknown).await)
            .await
            .unwrap_err(),
        OfficeDeliveryError::UnknownRun { run_id } if run_id == unknown
    ));
    let other = Fixture::new().await;
    assert!(matches!(
        fixture
            .control
            .enqueue_office_publish(&fixture.scope, &fixture.draft_for(other.run_id).await)
            .await
            .unwrap_err(),
        OfficeDeliveryError::UnknownRun { run_id } if run_id == other.run_id
    ));

    // Unverified and retired bindings fail closed.
    let (_, unverified_run) =
        employee_with_binding(&fixture, "zeynep", BindingShape::Unverified).await;
    assert_eq!(
        binding_rejection(
            fixture
                .control
                .enqueue_office_publish(&fixture.scope, &fixture.draft_for(unverified_run).await)
                .await
                .unwrap_err()
        ),
        BindingRejection::Unverified
    );
    let (_, retired_run) = employee_with_binding(&fixture, "ali", BindingShape::Retired).await;
    assert_eq!(
        binding_rejection(
            fixture
                .control
                .enqueue_office_publish(&fixture.scope, &fixture.draft_for(retired_run).await)
                .await
                .unwrap_err()
        ),
        BindingRejection::Retired
    );

    // A run pinned to a revision whose key has no binding, and one pinned to
    // a revision that names another employee's key, are refused.
    let unbound_key = FakeOfficeSigner::new()
        .with_generated_signer("credential://office/unbound")
        .public_key("credential://office/unbound")
        .expect("generated");
    let unbound_revision = insert_revision(
        &fixture.pool,
        company_id,
        "cem",
        2,
        &unbound_key,
        SIGNER_REF,
    )
    .await;
    let unbound_run = insert_run(
        &fixture.pool,
        company_id,
        "cem",
        unbound_revision,
        "completed",
        Some("channel"),
    )
    .await;
    assert_eq!(
        binding_rejection(
            fixture
                .control
                .enqueue_office_publish(&fixture.scope, &fixture.draft_for(unbound_run).await)
                .await
                .unwrap_err()
        ),
        BindingRejection::Missing
    );
    let zeynep_key: Vec<u8> = sqlx::query(
        "SELECT public_key FROM employee_office_bindings WHERE company_id = $1 AND employee_id = 'zeynep'",
    )
    .bind(company_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("zeynep binding")
    .try_get("public_key")
    .expect("key");
    let zeynep_key = OfficePublicKey::parse_hex(&hex::encode(zeynep_key)).expect("key");
    let borrowed_revision =
        insert_revision(&fixture.pool, company_id, "cem", 3, &zeynep_key, SIGNER_REF).await;
    let borrowed_run = insert_run(
        &fixture.pool,
        company_id,
        "cem",
        borrowed_revision,
        "completed",
        Some("reply"),
    )
    .await;
    assert_eq!(
        binding_rejection(
            fixture
                .control
                .enqueue_office_publish(&fixture.scope, &fixture.draft_for(borrowed_run).await)
                .await
                .unwrap_err()
        ),
        BindingRejection::WrongEmployee
    );

    // Kind policy: a profile event never reaches the outbox.
    let mut profile = fixture.draft_for(unbound_run).await;
    profile.kind = 0;
    assert!(matches!(
        fixture
            .control
            .enqueue_office_publish(&fixture.scope, &profile)
            .await
            .unwrap_err(),
        OfficeDeliveryError::Event(OfficeEventError::KindNotAllowed { kind: 0 })
    ));

    // None of the refusals created rows, signed, or published.
    assert_eq!(fixture.outbox_rows().await, 1);
    fixture.assert_untouched(authorized.outbox_id()).await;

    // A binding retired after enqueue is refused at delivery, before signing,
    // even though the authorized object was valid when it was issued.
    sqlx::query(
        "UPDATE employee_office_bindings SET valid_until = now()
          WHERE company_id = $1 AND id = $2",
    )
    .bind(company_id)
    .bind(fixture.binding_id)
    .execute(&fixture.pool)
    .await
    .expect("retire binding");
    let lease = fixture.claim(Duration::from_secs(30)).await;
    assert_eq!(
        binding_rejection(
            fixture
                .service(Duration::from_secs(30))
                .deliver(&fixture.scope, &lease, &authorized)
                .await
                .unwrap_err()
        ),
        BindingRejection::Retired
    );
    fixture.assert_untouched(authorized.outbox_id()).await;
}
