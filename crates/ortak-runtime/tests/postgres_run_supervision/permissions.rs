use super::*;

#[tokio::test]
#[ignore = "requires Postgres"]
async fn lifecycle_and_binding_refusals_record_bounded_retry_before_any_runtime_call() {
    let fixture = Fixture::new().await;
    fixture.route("Cem, selam").await;
    let supervisor = fixture.supervisor(fixture.config());

    sqlx::query("UPDATE employees SET status = 'disabled' WHERE company_id = $1 AND id = 'cem'")
        .bind(fixture.scope.company_id())
        .execute(&fixture.pool)
        .await
        .expect("disable");
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let outcome = supervisor
        .dispatch(&fixture.scope, &lease)
        .await
        .expect("dispatch");
    assert_eq!(
        outcome,
        DispatchOutcome::Refused {
            refusal: DispatchRefusal::EmployeeLifecycleChanged,
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
    let row = fixture.outbox(lease.id).await;
    assert_eq!((row.state.as_str(), row.attempt_count), ("pending", 1));
    assert!(row.lease_token.is_none());
    assert!(row
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("lifecycle")));

    // A separate current epoch isolates runtime binding validation. Disabled
    // work is permanently fenced; the lifecycle suite covers sealed re-enable.
    let fixture = Fixture::new().await;
    fixture.route("Cem, current binding").await;
    let supervisor = fixture.supervisor(fixture.config());
    sqlx::query(
        "UPDATE employee_runtime_bindings SET validated_at = NULL
          WHERE company_id = $1 AND revision_id = $2",
    )
    .bind(fixture.scope.company_id())
    .bind(fixture.revision_id)
    .execute(&fixture.pool)
    .await
    .expect("unvalidate");
    let lease = fixture.lease(Duration::from_secs(60)).await;
    assert_eq!(lease.attempt_count, 1);
    assert_eq!(
        supervisor
            .dispatch(&fixture.scope, &lease)
            .await
            .expect("dispatch"),
        DispatchOutcome::Refused {
            refusal: DispatchRefusal::RuntimeBindingUnvalidated,
            retry: OutboxFailOutcome::Retrying,
        }
    );
    assert_eq!(fixture.run_rows().await, 0);
    assert!(fixture.adapter.start_specs().is_empty());

    // A later revision does not matter: the decision pins the revision it
    // routed against, so re-validating that binding lets the dispatch through.
    sqlx::query(
        "UPDATE employee_runtime_bindings SET validated_at = now()
          WHERE company_id = $1 AND revision_id = $2",
    )
    .bind(fixture.scope.company_id())
    .bind(fixture.revision_id)
    .execute(&fixture.pool)
    .await
    .expect("revalidate");
    let mut newer = fixture_employee();
    newer.title = "Co-Founder (updated)".to_owned();
    newer.permissions = PermissionPolicy::default();
    let newer_revision =
        activate_employee(&fixture.pool, fixture.scope.company_id(), &newer, true).await;
    assert_ne!(newer_revision, fixture.revision_id);
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let run_id = match supervisor
        .dispatch(&fixture.scope, &lease)
        .await
        .expect("dispatch")
    {
        DispatchOutcome::Started { run_id, .. } => run_id,
        other => panic!("expected a started run, got {other:?}"),
    };
    assert_eq!(
        fixture.run(run_id).await.employee_revision_id,
        fixture.revision_id
    );
    let starts = fixture.adapter.start_specs();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].revision_id, fixture.revision_id);
    assert_eq!(starts[0].permissions, fixture_employee().permissions);
    assert_ne!(starts[0].permissions, newer.permissions);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn invalid_permission_policies_refuse_before_any_runtime_call() {
    for permissions in [
        PermissionPolicy {
            allowed_networks: vec!["service://invalid-policy\n".to_owned()],
            ..PermissionPolicy::default()
        },
        PermissionPolicy {
            approval_required: vec![ApprovalRequirement::ExternalPublish; 2],
            ..PermissionPolicy::default()
        },
    ] {
        let mut fixture = Fixture::new().await;
        let mut employee = fixture_employee();
        employee.permissions = permissions;
        // Seed a malformed immutable revision without weakening table guards.
        fixture.revision_id =
            activate_employee(&fixture.pool, fixture.scope.company_id(), &employee, true).await;
        fixture.route("Cem, selam").await;
        let lease = fixture.lease(Duration::from_secs(60)).await;
        assert_eq!(
            fixture
                .supervisor(fixture.config())
                .dispatch(&fixture.scope, &lease)
                .await
                .expect("durable refusal"),
            DispatchOutcome::Refused {
                refusal: DispatchRefusal::ManifestUnreadable,
                retry: OutboxFailOutcome::Retrying,
            }
        );
        assert!(fixture.adapter.start_specs().is_empty());
        assert_eq!(fixture.run_rows().await, 0);
        let outbox = fixture.outbox(lease.id).await;
        assert_eq!(
            (outbox.state.as_str(), outbox.attempt_count),
            ("pending", 1)
        );
        assert!(outbox.lease_token.is_none());
        assert_eq!(
            outbox.last_error.as_deref(),
            Some("dispatch refused: pinned revision manifest unreadable")
        );
    }
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn retry_after_lost_acknowledgement_converges_and_a_stale_lease_cannot_overwrite() {
    let fixture = Fixture::new().await;
    fixture.route("Cem, selam").await;
    let supervisor = fixture.supervisor(fixture.config());

    // Worker A: authorize, create the run, start it at the runtime, then
    // crash before the acknowledgement is recorded.
    let lease_a = fixture.lease(Duration::from_millis(50)).await;
    let authority = authorized(
        fixture
            .control
            .authorize_dispatch(&fixture.scope, &lease_a)
            .await
            .expect("authorize"),
    );
    let run_id = match fixture
        .control
        .prepare_run(&fixture.scope, &authority)
        .await
        .expect("prepare")
    {
        PrepareOutcome::Prepared(prepared) => {
            assert!(prepared.created);
            assert_eq!(prepared.status, RunStatus::Queued);
            prepared.run_id
        }
        PrepareOutcome::StaleLease => panic!("lease is live"),
        PrepareOutcome::Refused(reason) => panic!("unexpected refusal: {reason}"),
    };
    let spec = authority.run_spec(run_id).expect("spec");
    let receipt_a = fixture
        .adapter
        .start_run(&spec)
        .await
        .expect("external start");
    assert_eq!(fixture.run(run_id).await.status, "queued");

    let mut newer = fixture_employee();
    newer.permissions = PermissionPolicy::default();
    let newer_revision =
        activate_employee(&fixture.pool, fixture.scope.company_id(), &newer, true).await;
    assert_ne!(newer_revision, fixture.revision_id);

    // Worker B reclaims after the lease expires and retries with the same
    // durable run and idempotency key: the runtime returns the same run.
    tokio::time::sleep(Duration::from_millis(120)).await;
    let lease_b = fixture.lease(Duration::from_secs(60)).await;
    assert_ne!(lease_b.lease_token, lease_a.lease_token);
    assert_eq!(lease_b.attempt_count, 2);
    assert_eq!(
        supervisor
            .dispatch(&fixture.scope, &lease_b)
            .await
            .expect("dispatch"),
        DispatchOutcome::Started {
            run_id,
            runtime_run_ref: receipt_a.runtime_run_ref.clone(),
        }
    );
    assert!(matches!(
        fixture
            .adapter
            .next_events(&RuntimeRunRef("fake-run-2".to_owned()), None, 1)
            .await,
        Err(RuntimeError::UnknownRun { .. })
    ));
    assert_eq!(fixture.run_rows().await, 1);
    assert_eq!(fixture.outbox(lease_a.id).await.state, "delivered");
    let starts = fixture.adapter.start_specs();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0], starts[1]);
    assert_eq!(starts[1].permissions, fixture_employee().permissions);
    assert_eq!(starts[1].revision_id, fixture.revision_id);

    // Worker A wakes up: its lease is stale at every step and nothing changes.
    assert_eq!(
        supervisor
            .dispatch(&fixture.scope, &lease_a)
            .await
            .expect("stale dispatch"),
        DispatchOutcome::StaleLease
    );
    assert_eq!(
        fixture
            .control
            .prepare_run(&fixture.scope, &authority)
            .await
            .expect("stale prepare"),
        PrepareOutcome::StaleLease
    );
    // Even a direct correlation attempt with another runtime reference is
    // refused by the compare-and-set.
    let forged = RunStartReceipt {
        runtime_run_ref: RuntimeRunRef("fake-run-forged".to_owned()),
        started_at: Utc::now(),
    };
    assert_eq!(
        fixture
            .control
            .correlate_run(&fixture.scope, &authority, run_id, &forged)
            .await
            .expect("correlate"),
        CorrelationOutcome::RefConflict {
            durable: receipt_a.runtime_run_ref.clone()
        }
    );
    let run = fixture.run(run_id).await;
    assert_eq!(run.status, "running");
    assert_eq!(run.runtime_run_ref, Some(receipt_a.runtime_run_ref.0));
    assert_eq!(fixture.run_rows().await, 1);
    assert_eq!(fixture.adapter.start_specs().len(), 2);
}
