use super::*;

#[tokio::test]
#[ignore = "requires Postgres"]
async fn event_pump_resumes_from_the_durable_cursor_and_commits_terminal_state_atomically() {
    let fixture = Fixture::new().await;
    let (run_id, runtime_run_ref, _) = fixture.started().await;
    let supervisor = fixture.supervisor(SupervisorConfig {
        event_batch_limit: 2,
        ..fixture.config()
    });

    let secret = "sk-live-abcdef1234567890";
    for payload in [
        RunEventPayload::AssistantDelta {
            turn: 0,
            delta: BoundedText::raw(format!("thinking with {secret}")),
        },
        RunEventPayload::ToolCallStarted {
            call_id: "call-1".to_owned(),
            tool: "files".to_owned(),
            arguments: BoundedText::raw("{\"path\": \"README.md\"}"),
        },
        RunEventPayload::RunWaiting {
            reason: "approval".to_owned(),
            detail: BoundedText::raw("external publish requires approval"),
        },
        RunEventPayload::ToolCallCompleted {
            call_id: "call-1".to_owned(),
            result: BoundedText::raw("ok"),
        },
        RunEventPayload::DeliveryIntent {
            intent: DeliveryIntentKind::Reply,
            target_ref: None,
        },
        RunEventPayload::RunCompleted {
            delivery_intent: DeliveryIntentKind::Reply,
        },
        RunEventPayload::AssistantDelta {
            turn: 1,
            delta: BoundedText::raw("after the end"),
        },
    ] {
        fixture.adapter.push_event(&runtime_run_ref, payload);
    }

    // Batches of two, resuming from the stored cursor each time; the
    // runtime's own run.started is the first streamed event.
    let mut statuses = Vec::new();
    loop {
        match supervisor.pump(&fixture.scope, run_id).await.expect("pump") {
            PumpOutcome::Appended { status, .. } => statuses.push(status),
            PumpOutcome::Terminal { status } => {
                statuses.push(status);
                break;
            }
            other => panic!("unexpected pump outcome {other:?}"),
        }
        if statuses.len() > 8 {
            panic!("pump did not terminate: {statuses:?}");
        }
    }
    assert_eq!(
        statuses,
        vec![
            RunStatus::Running,
            RunStatus::Waiting,
            RunStatus::Running,
            RunStatus::Completed,
            RunStatus::Completed,
        ]
    );

    let run = fixture.run(run_id).await;
    assert_eq!(run.status, "completed");
    assert_eq!(run.delivery_intent.as_deref(), Some("reply"));
    assert!(run.finished_at.is_some());
    let events = fixture.events(run_id).await;
    let types = events
        .iter()
        .map(|(sequence, event_type, _, _)| (*sequence, event_type.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        types,
        vec![
            (0, "run.queued"),
            (1, "run.started"),
            (2, "assistant.delta"),
            (3, "tool_call.started"),
            (4, "run.waiting"),
            (5, "tool_call.completed"),
            (6, "delivery.intent"),
            (7, "run.completed"),
        ]
    );
    let stored = events
        .iter()
        .map(|(_, _, _, payload)| payload.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!stored.contains(secret), "secret leaked into run_events");
    assert!(stored.contains("[redacted]"));
    let cursors = events
        .iter()
        .filter_map(|(_, _, cursor, _)| cursor.clone())
        .collect::<Vec<_>>();
    assert_eq!(cursors.len(), 7);
    assert_eq!(
        cursors
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        7
    );

    // A replayed cursor is deduplicated rather than appended, and a
    // terminal run accepts nothing more.
    let replay = RunEvent::normalize(
        run_id,
        Utc::now(),
        Some(cursors[1].clone()),
        &RunEventPayload::AssistantDelta {
            turn: 0,
            delta: BoundedText::raw("replayed"),
        },
        &RedactionPolicy::new(),
    )
    .expect("normalize");
    assert_eq!(
        fixture
            .control
            .append_supervised_events(&fixture.scope, run_id, &runtime_run_ref, &[replay])
            .await
            .expect("append"),
        AppendOutcome::RunTerminal {
            status: RunStatus::Completed
        }
    );
    assert_eq!(fixture.events(run_id).await.len(), 8);

    // Mid-stream replay on a second run: the duplicate cursor is skipped
    // and the sequence stays dense.
    let (second, second_ref, _) = fixture.started().await;
    assert!(matches!(
        supervisor.pump(&fixture.scope, second).await.expect("pump"),
        PumpOutcome::Appended { appended: 1, .. }
    ));
    let first_cursor = fixture.events(second).await[1].2.clone().expect("cursor");
    let replay = RunEvent::normalize(
        second,
        Utc::now(),
        Some(first_cursor.clone()),
        &RunEventPayload::RunStarted {
            runtime_run_ref: second_ref.0.clone(),
        },
        &RedactionPolicy::new(),
    )
    .expect("normalize");
    assert_eq!(
        fixture
            .control
            .append_supervised_events(&fixture.scope, second, &second_ref, &[replay])
            .await
            .expect("append"),
        AppendOutcome::Appended {
            sequences: Vec::new(),
            duplicate_cursors: vec![first_cursor],
            status: RunStatus::Running,
        }
    );
    assert_eq!(fixture.events(second).await.len(), 2);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn cancellation_is_supervised_idempotent_and_requires_a_correlated_run() {
    let fixture = Fixture::new().await;
    let (run_id, runtime_run_ref, _) = fixture.started().await;
    let supervisor = fixture.supervisor(fixture.config());

    let outcome = supervisor
        .cancel(
            &fixture.scope,
            run_id,
            "operator stop; token Bearer abc123456789",
        )
        .await
        .expect("cancel");
    assert_eq!(outcome, CancellationOutcome::Cancelled { run_id });
    let run = fixture.run(run_id).await;
    assert_eq!(run.status, "cancelled");
    assert!(run.finished_at.is_some());
    assert!(run.error_code.is_none());
    let reason = run.cancel_reason.expect("reason recorded");
    assert!(reason.contains("operator stop"));
    assert!(!reason.contains("abc123456789"));
    let events = fixture.events(run_id).await;
    let last = events.last().expect("terminal event");
    assert_eq!(last.1, "run.cancelled");
    assert!(
        last.2.is_none(),
        "synthesized cancellation carries no runtime cursor"
    );
    assert!(!last.3.to_string().contains("abc123456789"));

    // Replays settle without touching the run again.
    assert_eq!(
        supervisor
            .cancel(&fixture.scope, run_id, "again")
            .await
            .expect("replay cancel"),
        CancellationOutcome::AlreadyTerminal {
            run_id,
            status: RunStatus::Cancelled
        }
    );
    assert_eq!(
        supervisor.pump(&fixture.scope, run_id).await.expect("pump"),
        PumpOutcome::Terminal {
            status: RunStatus::Cancelled
        }
    );
    assert_eq!(fixture.events(run_id).await.len(), events.len());
    // The runtime's own cancellation event stays behind the durable one.
    assert!(matches!(
        fixture.adapter.cancel_run(&runtime_run_ref, "again").await,
        Ok(ortak_control::runtime::CancelOutcome::AlreadyTerminal)
    ));

    // A run that was never correlated cannot be addressed at the runtime.
    fixture.route("Cem, bir daha").await;
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let authority = authorized(
        fixture
            .control
            .authorize_dispatch(&fixture.scope, &lease)
            .await
            .expect("authorize"),
    );
    let queued = match fixture
        .control
        .prepare_run(&fixture.scope, &authority)
        .await
        .expect("prepare")
    {
        PrepareOutcome::Prepared(prepared) => prepared.run_id,
        PrepareOutcome::StaleLease => panic!("lease is live"),
        PrepareOutcome::Refused(reason) => panic!("unexpected refusal: {reason}"),
    };
    assert!(matches!(
        supervisor.cancel(&fixture.scope, queued, "too early").await,
        Err(RunSupervisionError::NotCorrelated { run_id, status: RunStatus::Queued }) if run_id == queued
    ));
    assert_eq!(fixture.run(queued).await.status, "queued");
}
