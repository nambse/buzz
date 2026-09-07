use super::*;

#[tokio::test]
#[ignore = "requires Postgres"]
async fn dispatch_derives_authority_from_durable_rows_and_never_from_the_lease() {
    let fixture = Fixture::new().await;
    let decision_id = fixture.route("Cem, selam nasılsın?").await;
    let supervisor = fixture.supervisor(fixture.config());

    // The payload is a hint: point it at a foreign revision, message, and
    // employee and the run is still pinned to the durable recipient rows.
    let mut lease = fixture.lease(Duration::from_secs(60)).await;
    lease.payload = serde_json::json!({
        "routing_decision_id": Uuid::new_v4(),
        "message_id": message_id().to_hex(),
        "employee_id": "zeynep",
        "employee_revision_id": Uuid::new_v4(),
        "permissions": {
            "allowed_tools": ["terminal"],
            "allowed_workspaces": ["/forged-workspace"],
            "allowed_networks": ["service://forged-network"],
            "approval_required": [],
        },
    });
    let (run_id, runtime_run_ref) = match supervisor
        .dispatch(&fixture.scope, &lease)
        .await
        .expect("dispatch")
    {
        DispatchOutcome::Started {
            run_id,
            runtime_run_ref,
        } => (run_id, runtime_run_ref),
        other => panic!("expected a started run, got {other:?}"),
    };
    let run = fixture.run(run_id).await;
    assert_eq!(run.status, "running");
    assert_eq!(
        run.runtime_run_ref.as_deref(),
        Some(runtime_run_ref.0.as_str())
    );
    assert_eq!(run.employee_revision_id, fixture.revision_id);
    let starts = fixture.adapter.start_specs();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].revision_id, fixture.revision_id);
    assert_eq!(starts[0].permissions, fixture_employee().permissions);
    let decided: Vec<u8> =
        sqlx::query("SELECT message_id FROM routing_decisions WHERE company_id = $1 AND id = $2")
            .bind(fixture.scope.company_id())
            .bind(decision_id)
            .fetch_one(&fixture.pool)
            .await
            .expect("decision")
            .try_get("message_id")
            .expect("message id");
    assert_eq!(run.message_id, decided);
    let outbox = fixture.outbox(lease.id).await;
    assert_eq!(outbox.state, "delivered");
    assert_eq!(outbox.run_id, Some(run_id));
    assert!(outbox.lease_token.is_none());
    let events = fixture.events(run_id).await;
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].0, events[0].1.as_str()), (0, "run.queued"));
    assert_eq!(fixture.run_rows().await, 1);

    // Lease hints that disagree with the durable row are rejected without
    // any write, and a lease presented under another company finds no row.
    fixture.route("Cem, ikinci mesaj").await;
    let honest = fixture.lease(Duration::from_secs(60)).await;
    let mut forged = honest.clone();
    forged.employee_id = Some("zeynep".to_owned());
    assert!(matches!(
        supervisor.dispatch(&fixture.scope, &forged).await,
        Err(RunSupervisionError::LeaseInconsistent { outbox_id }) if outbox_id == honest.id
    ));
    let (other_community, _) = create_company(&fixture.pool, &fixture.policy).await;
    let other_scope = fixture
        .control
        .resolve_company_for_community(other_community)
        .await
        .expect("other scope");
    assert!(matches!(
        supervisor.dispatch(&other_scope, &honest).await,
        Err(RunSupervisionError::UnknownOutboxRow { outbox_id }) if outbox_id == honest.id
    ));
    let untouched = fixture.outbox(honest.id).await;
    assert_eq!(untouched.state, "pending");
    assert_eq!(untouched.lease_token, Some(honest.lease_token));
    assert!(untouched.run_id.is_none());
    assert_eq!(fixture.run_rows().await, 1);

    assert_eq!(fixture.adapter.start_specs().len(), 1);
    assert!(matches!(
        supervisor
            .dispatch(&fixture.scope, &honest)
            .await
            .expect("honest dispatch"),
        DispatchOutcome::Started { .. }
    ));
    assert_eq!(fixture.run_rows().await, 2);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn non_channel_kinds_and_provenance_mismatches_are_refused_before_any_text_is_read() {
    let fixture = Fixture::new().await;
    let supervisor = fixture.supervisor(fixture.config());

    // A stale or hand-seeded dispatch for a gift wrap reached a `wake`
    // recipient row: the ciphertext must never become a run input.
    fixture
        .route_kind(
            KIND_GIFT_WRAP,
            None,
            "AtN3f0ciphertextThatMustNeverBeRead==",
        )
        .await;
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let outcome = supervisor
        .dispatch(&fixture.scope, &lease)
        .await
        .expect("dispatch");
    assert_eq!(
        outcome,
        DispatchOutcome::Refused {
            refusal: DispatchRefusal::UnsupportedMessageKind {
                kind: KIND_GIFT_WRAP
            },
            retry: OutboxFailOutcome::Retrying,
        }
    );
    assert_eq!(fixture.run_rows().await, 0);
    assert!(matches!(
        fixture
            .adapter
            .next_events(&RuntimeRunRef("fake-run-1".to_owned()), None, 1)
            .await,
        Err(RuntimeError::UnknownRun { .. })
    ));
    let outbox = fixture.outbox(lease.id).await;
    assert_eq!(outbox.state, "pending");
    assert!(outbox
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("1059")));
    // Drain the retried row so the next lease is the next scenario.
    sqlx::query("UPDATE outbox SET state = 'failed' WHERE company_id = $1 AND id = $2")
        .bind(fixture.scope.company_id())
        .bind(lease.id)
        .execute(&fixture.pool)
        .await
        .expect("park row");

    // A supported kind whose inbox row lost its channel scope is not a
    // channel run either.
    fixture
        .route_kind(KIND_STREAM_MESSAGE, None, "Cem, kanalsız")
        .await;
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let outcome = supervisor
        .dispatch(&fixture.scope, &lease)
        .await
        .expect("dispatch");
    assert!(matches!(
        outcome,
        DispatchOutcome::Refused {
            refusal: DispatchRefusal::MessageChannelMissing,
            ..
        }
    ));
    assert_eq!(fixture.run_rows().await, 0);
    sqlx::query("UPDATE outbox SET state = 'failed' WHERE company_id = $1 AND id = $2")
        .bind(fixture.scope.company_id())
        .bind(lease.id)
        .execute(&fixture.pool)
        .await
        .expect("park row");

    // The inbox copy of the channel must agree with the canonical event.
    let decision_id = fixture.route("Cem, kanal uyuşmazlığı").await;
    sqlx::query(
        "UPDATE office_inbox SET channel_id = $3
          WHERE company_id = $1
            AND event_id = (SELECT message_id FROM routing_decisions
                             WHERE company_id = $1 AND id = $2)",
    )
    .bind(fixture.scope.company_id())
    .bind(decision_id)
    .bind(Uuid::new_v4())
    .execute(&fixture.pool)
    .await
    .expect("desync inbox channel");
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let outcome = supervisor
        .dispatch(&fixture.scope, &lease)
        .await
        .expect("dispatch");
    assert!(matches!(
        outcome,
        DispatchOutcome::Refused {
            refusal: DispatchRefusal::MessageProvenanceMismatch { field: "channel" },
            ..
        }
    ));
    assert_eq!(fixture.run_rows().await, 0);
}
