//! Pure projection contract: typed Activity entries, markers, raw-view
//! scrubbing, bounded incremental paging with gap detection, summary
//! folding, closed-vocabulary parsing, and cursor/limit bounds. No database.

use chrono::{Duration, TimeZone, Utc};
use ortak_control::run_event::{
    BoundedText, DeliveryIntentKind, FileChangeKind, RedactionPolicy, RunEvent, RunEventPayload,
    RunEventType, TerminalStream, UsageTelemetry, REDACTED,
};
use ortak_observability::projection::{
    assemble_event_page, bound_row_text, derive_outcome, project_entry, raw_view, RunEventRecord,
    SummaryFacts, MAX_ERROR_CODE_BYTES,
};
use ortak_observability::{
    Activity, ActivityError, LifecyclePhase, RunEventsQuery, RunHeader, RunListCursor,
    RunListQuery, RunOutcome, RunProvenance, RunStatus, RunTiming, RuntimeReference, SequenceGap,
    TerminalPhase, ToolCallPhase, MAX_EVENT_PAGE_SIZE, MAX_RUN_PAGE_SIZE,
};
use uuid::Uuid;

const FIXTURE_RUNTIME_REF: &str = "fake-runtime-run-ref-0001";
const FIXTURE_SECRET: &str = "fixture-literal-secret-value-0001";

fn policy() -> RedactionPolicy {
    RedactionPolicy::new()
        .with_literal_secrets([FIXTURE_SECRET])
        .with_max_text_bytes(64)
}

/// Normalizes through the production contract, then reads back through the
/// stored-row constructor exactly as the SQL adapter does.
fn record(sequence: i64, payload: RunEventPayload) -> RunEventRecord {
    let event = RunEvent::normalize(
        Uuid::new_v4(),
        Utc.with_ymd_and_hms(2026, 9, 4, 12, 0, 0)
            .single()
            .expect("timestamp")
            + Duration::seconds(sequence),
        Some(format!("cursor-{sequence}")),
        &payload,
        &policy(),
    )
    .expect("normalize");
    RunEventRecord::from_stored(
        sequence,
        event.event_type().as_str(),
        event.occurred_at,
        event.occurred_at + Duration::milliseconds(7),
        event.runtime_cursor.is_some(),
        (sequence == 4).then(|| "artifact://run/4".to_owned()),
        event.payload_json().expect("payload json"),
    )
    .expect("stored record")
}

fn timeline() -> Vec<RunEventRecord> {
    let payloads = vec![
        RunEventPayload::RunQueued,
        RunEventPayload::RunStarted {
            runtime_run_ref: FIXTURE_RUNTIME_REF.to_owned(),
        },
        RunEventPayload::AssistantDelta {
            turn: 1,
            delta: BoundedText::raw("Merhaba"),
        },
        RunEventPayload::ToolCallStarted {
            call_id: "call-1".to_owned(),
            tool: "shell".to_owned(),
            arguments: BoundedText::raw(format!("{{\"token\": \"{FIXTURE_SECRET}\"}}")),
        },
        RunEventPayload::ToolCallCompleted {
            call_id: "call-1".to_owned(),
            result: BoundedText::raw("x".repeat(200)),
        },
        RunEventPayload::ToolCallFailed {
            call_id: "call-2".to_owned(),
            error: BoundedText::raw("boom"),
        },
        RunEventPayload::TerminalStarted {
            command_id: "cmd-1".to_owned(),
            command: BoundedText::raw("cargo test"),
            cwd: Some("/workspace".to_owned()),
        },
        RunEventPayload::TerminalOutput {
            command_id: "cmd-1".to_owned(),
            stream: TerminalStream::Stderr,
            chunk: BoundedText::raw("error: failed"),
        },
        RunEventPayload::TerminalCompleted {
            command_id: "cmd-1".to_owned(),
            exit_code: Some(1),
        },
        RunEventPayload::TerminalStarted {
            command_id: "cmd-2".to_owned(),
            command: BoundedText::raw("ls"),
            cwd: None,
        },
        RunEventPayload::TerminalCompleted {
            command_id: "cmd-2".to_owned(),
            exit_code: None,
        },
        RunEventPayload::FileChanged {
            path: "src/lib.rs".to_owned(),
            change: FileChangeKind::Modified,
            summary: BoundedText::raw("+1 -1"),
            bytes: Some(1024),
        },
        RunEventPayload::FileChanged {
            path: "README.md".to_owned(),
            change: FileChangeKind::Read,
            summary: BoundedText::raw(""),
            bytes: None,
        },
        RunEventPayload::UsageRecorded {
            usage: UsageTelemetry {
                model: Some("fixture-model".to_owned()),
                input_tokens: Some(10),
                output_tokens: Some(20),
                cached_input_tokens: Some(5),
                reasoning_tokens: None,
            },
        },
        RunEventPayload::UsageRecorded {
            usage: UsageTelemetry {
                model: None,
                input_tokens: Some(1),
                output_tokens: Some(2),
                cached_input_tokens: None,
                reasoning_tokens: Some(3),
            },
        },
        RunEventPayload::ErrorRaised {
            code: "provider_timeout".to_owned(),
            message: BoundedText::raw("retrying"),
            retryable: true,
        },
        RunEventPayload::RunWaiting {
            reason: "approval".to_owned(),
            detail: BoundedText::raw("needs a human"),
        },
        RunEventPayload::DeliveryIntent {
            intent: DeliveryIntentKind::Channel,
            target_ref: Some("channel:general".to_owned()),
        },
        RunEventPayload::RunCompleted {
            delivery_intent: DeliveryIntentKind::Channel,
        },
    ];
    payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| record(index as i64, payload))
        .collect()
}

fn header(status: RunStatus, queued_at: chrono::DateTime<Utc>) -> RunHeader {
    RunHeader {
        run_id: Uuid::new_v4(),
        employee_id: ortak_domain::EmployeeId::parse("cem").expect("employee id"),
        employee_revision_id: Uuid::new_v4(),
        provenance: RunProvenance::default(),
        runtime: RuntimeReference {
            adapter: "fake-runtime".to_owned(),
            has_run_ref: true,
        },
        status,
        outcome: RunOutcome::Pending,
        timing: RunTiming {
            queued_at,
            started_at: None,
            finished_at: None,
            updated_at: queued_at,
        },
        last_event: None,
    }
}

#[test]
fn every_payload_projects_to_typed_activity_with_markers() {
    let entries: Vec<_> = timeline()
        .iter()
        .map(|record| project_entry(record, false))
        .collect();
    assert_eq!(entries.len(), 19);
    for (index, entry) in entries.iter().enumerate() {
        assert_eq!(entry.sequence, index as i64);
        assert!(entry.has_runtime_cursor, "cursor presence at {index}");
        assert!(entry.raw.is_none(), "raw is opt-in at {index}");
        assert_eq!(
            entry.recorded_at - entry.occurred_at,
            Duration::milliseconds(7)
        );
    }

    assert!(matches!(
        entries[0].activity,
        Activity::Lifecycle {
            phase: LifecyclePhase::Queued
        }
    ));
    assert!(matches!(
        entries[1].activity,
        Activity::Lifecycle {
            phase: LifecyclePhase::Started {
                has_runtime_run_ref: true
            }
        }
    ));
    match &entries[2].activity {
        Activity::AssistantOutput { turn: 1, text } => assert_eq!(text.text, "Merhaba"),
        other => panic!("unexpected {other:?}"),
    }
    match &entries[3].activity {
        Activity::ToolCall {
            call_id,
            phase: ToolCallPhase::Started { tool, arguments },
        } => {
            assert_eq!(call_id, "call-1");
            assert_eq!(tool, "shell");
            assert!(!arguments.text.contains(FIXTURE_SECRET));
            assert!(arguments.redacted);
            assert!(entries[3].redacted);
            assert!(!entries[3].truncated);
        }
        other => panic!("unexpected {other:?}"),
    }
    match &entries[4].activity {
        Activity::ToolCall {
            phase: ToolCallPhase::Completed { result },
            ..
        } => {
            assert!(result.truncated);
            assert_eq!(result.original_bytes, Some(200));
            assert!(result.original_sha256.is_some());
            assert!(entries[4].truncated);
            assert!(!entries[4].redacted);
            assert_eq!(entries[4].artifact_ref.as_deref(), Some("artifact://run/4"));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(
        &entries[5].activity,
        Activity::ToolCall {
            phase: ToolCallPhase::Failed { .. },
            ..
        }
    ));
    match &entries[6].activity {
        Activity::Terminal {
            command_id,
            phase: TerminalPhase::Started { command, cwd },
        } => {
            assert_eq!(command_id, "cmd-1");
            assert_eq!(command.text, "cargo test");
            assert_eq!(cwd.as_deref(), Some("/workspace"));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(
        &entries[7].activity,
        Activity::Terminal {
            phase: TerminalPhase::Output {
                stream: TerminalStream::Stderr,
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        &entries[8].activity,
        Activity::Terminal {
            phase: TerminalPhase::Completed { exit_code: Some(1) },
            ..
        }
    ));
    assert!(matches!(
        &entries[10].activity,
        Activity::Terminal {
            phase: TerminalPhase::Completed { exit_code: None },
            ..
        }
    ));
    match &entries[11].activity {
        Activity::FileChange {
            path,
            change: FileChangeKind::Modified,
            bytes: Some(1024),
            ..
        } => assert_eq!(path, "src/lib.rs"),
        other => panic!("unexpected {other:?}"),
    }
    match &entries[13].activity {
        Activity::Usage { usage } => assert_eq!(usage.input_tokens, Some(10)),
        other => panic!("unexpected {other:?}"),
    }
    match &entries[15].activity {
        Activity::Error {
            code,
            retryable: true,
            ..
        } => assert_eq!(code, "provider_timeout"),
        other => panic!("unexpected {other:?}"),
    }
    match &entries[16].activity {
        Activity::Lifecycle {
            phase: LifecyclePhase::Waiting { reason, detail },
        } => {
            assert_eq!(reason, "approval");
            assert_eq!(detail.text, "needs a human");
        }
        other => panic!("unexpected {other:?}"),
    }
    match &entries[17].activity {
        Activity::DeliveryIntent {
            intent: DeliveryIntentKind::Channel,
            target_ref,
        } => assert_eq!(target_ref.as_deref(), Some("channel:general")),
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(
        &entries[18].activity,
        Activity::Lifecycle {
            phase: LifecyclePhase::Completed {
                delivery_intent: DeliveryIntentKind::Channel
            }
        }
    ));
    assert_eq!(entries[18].event_type, RunEventType::RunCompleted);

    // The rendered surface never carries the runtime run reference, a
    // cursor, or the fixture secret.
    let rendered = serde_json::to_string(&entries).expect("serialize entries");
    assert!(!rendered.contains(FIXTURE_RUNTIME_REF));
    assert!(!rendered.contains("cursor-"));
    assert!(!rendered.contains(FIXTURE_SECRET));
}

#[test]
fn raw_view_is_opt_in_and_scrubs_the_runtime_run_ref() {
    let started = record(
        1,
        RunEventPayload::RunStarted {
            runtime_run_ref: FIXTURE_RUNTIME_REF.to_owned(),
        },
    );
    let entry = project_entry(&started, true);
    let raw = entry.raw.as_ref().expect("raw requested");
    assert_eq!(
        raw,
        &RunEventPayload::RunStarted {
            runtime_run_ref: REDACTED.to_owned()
        }
    );
    let rendered = serde_json::to_string(&entry).expect("serialize entry");
    assert!(!rendered.contains(FIXTURE_RUNTIME_REF));

    // Other payloads pass through unchanged: they are already bounded and
    // redacted by the persistence contract.
    let output = record(
        2,
        RunEventPayload::TerminalOutput {
            command_id: "cmd".to_owned(),
            stream: TerminalStream::Stdout,
            chunk: BoundedText::raw(format!("token={FIXTURE_SECRET}")),
        },
    );
    assert_eq!(raw_view(&output.payload), output.payload);
    let rendered = serde_json::to_string(&project_entry(&output, true)).expect("serialize");
    assert!(!rendered.contains(FIXTURE_SECRET));
    assert!(rendered.contains(REDACTED));
}

#[test]
fn event_pages_are_bounded_dense_and_report_gaps() {
    let records = timeline();
    let page_of = |after: Option<i64>, limit: u32| {
        let query = RunEventsQuery {
            after_sequence: after,
            limit: Some(limit),
            include_raw: false,
        };
        let window: Vec<_> = records
            .iter()
            .filter(|record| record.sequence > query.start_after())
            .take(query.page_size() as usize + 1)
            .cloned()
            .collect();
        assemble_event_page(&query, window)
    };

    let first = page_of(None, 5);
    assert_eq!(
        first
            .entries
            .iter()
            .map(|entry| entry.sequence)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );
    assert!(first.has_more);
    assert_eq!(first.next_after_sequence, Some(4));
    assert_eq!(first.gap, None);

    let mut cursor = first.next_after_sequence;
    let mut seen: Vec<i64> = first.entries.iter().map(|entry| entry.sequence).collect();
    loop {
        let page = page_of(cursor, 5);
        seen.extend(page.entries.iter().map(|entry| entry.sequence));
        assert_eq!(page.gap, None);
        cursor = page.next_after_sequence;
        if !page.has_more {
            break;
        }
    }
    assert_eq!(seen, (0..19).collect::<Vec<_>>());
    assert_eq!(cursor, Some(18));

    let empty = page_of(Some(18), 5);
    assert!(empty.entries.is_empty());
    assert!(!empty.has_more);
    assert_eq!(empty.next_after_sequence, Some(18));

    // A hole after the cursor stops the page before the hole and signals it.
    let holed = vec![
        records[5].clone(),
        records[6].clone(),
        records[8].clone(),
        records[9].clone(),
    ];
    let query = RunEventsQuery {
        after_sequence: Some(4),
        limit: Some(10),
        include_raw: false,
    };
    let page = assemble_event_page(&query, holed);
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.sequence)
            .collect::<Vec<_>>(),
        vec![5, 6]
    );
    assert_eq!(
        page.gap,
        Some(SequenceGap {
            expected: 7,
            found: 8
        })
    );
    assert!(page.has_more);
    assert_eq!(page.next_after_sequence, Some(6));

    // A first record that is not the cursor's successor yields no entries
    // and keeps the caller's cursor.
    let query = RunEventsQuery::default();
    let page = assemble_event_page(&query, vec![records[1].clone()]);
    assert!(page.entries.is_empty());
    assert_eq!(
        page.gap,
        Some(SequenceGap {
            expected: 0,
            found: 1
        })
    );
    assert_eq!(page.next_after_sequence, None);

    // Page sizes are clamped to the hard ceiling.
    let oversized = RunEventsQuery {
        limit: Some(u32::MAX),
        ..RunEventsQuery::default()
    };
    assert_eq!(oversized.page_size(), MAX_EVENT_PAGE_SIZE);
    assert_eq!(
        RunEventsQuery {
            limit: Some(0),
            ..RunEventsQuery::default()
        }
        .page_size(),
        1
    );
    assert!(matches!(
        RunEventsQuery {
            after_sequence: Some(-1),
            ..RunEventsQuery::default()
        }
        .validate(),
        Err(ActivityError::InvalidQuery(_))
    ));
}

#[test]
fn summary_fold_counts_tools_terminal_files_and_usage() {
    let records = timeline();
    let facts = SummaryFacts::fold(&records);
    assert_eq!(facts.event_count, 19);
    let last = facts.last_event.expect("last event");
    assert_eq!(last.sequence, 18);
    assert_eq!(last.event_type, RunEventType::RunCompleted);

    assert_eq!(facts.tools.started, 1);
    assert_eq!(facts.tools.completed, 1);
    assert_eq!(facts.tools.failed, 1);
    assert_eq!(facts.tools.open(), 0);

    assert_eq!(facts.terminal.commands, 2);
    assert_eq!(facts.terminal.completed, 2);
    assert_eq!(facts.terminal.nonzero_exits, 1);
    assert_eq!(facts.terminal.abnormal_exits, 1);
    assert_eq!(facts.terminal.output_chunks, 1);

    assert_eq!(facts.files.changes, 2);
    assert_eq!(facts.files.modified, 1);
    assert_eq!(facts.files.read, 1);
    assert_eq!(facts.files.created, 0);

    assert_eq!(facts.assistant_fragments, 1);
    assert_eq!(facts.waits, 1);
    assert_eq!(facts.errors_raised, 1);

    assert_eq!(facts.usage.records, 2);
    assert_eq!(facts.usage.input_tokens, 11);
    assert_eq!(facts.usage.output_tokens, 22);
    assert_eq!(facts.usage.cached_input_tokens, 5);
    assert_eq!(facts.usage.reasoning_tokens, 3);

    let live = header(RunStatus::Running, Utc::now());
    let summary = facts.clone().into_summary(&live);
    assert!(summary.usage.is_some());
    assert!(summary.terminal_state.is_none());

    let mut finished = header(RunStatus::Completed, Utc::now());
    finished.outcome = RunOutcome::Completed {
        delivery_intent: DeliveryIntentKind::Channel,
    };
    let summary = facts.into_summary(&finished);
    let terminal = summary.terminal_state.expect("terminal state");
    assert_eq!(terminal.status, RunStatus::Completed);
    assert_eq!(terminal.outcome, finished.outcome);

    let none = SummaryFacts::fold(&[]).into_summary(&live);
    assert_eq!(none.event_count, 0);
    assert!(none.usage.is_none());
    assert!(none.last_event.is_none());
}

#[test]
fn stored_rows_outside_the_closed_vocabulary_fail_closed() {
    let now = Utc::now();
    let queued = serde_json::json!({ "event_type": "run.queued" });
    assert!(
        RunEventRecord::from_stored(0, "run.queued", now, now, false, None, queued.clone()).is_ok()
    );

    let unknown_type =
        RunEventRecord::from_stored(0, "run.exploded", now, now, false, None, queued.clone());
    assert!(matches!(
        unknown_type,
        Err(ActivityError::InvalidRecord { .. })
    ));

    let mismatch = RunEventRecord::from_stored(0, "run.started", now, now, false, None, queued);
    assert!(matches!(mismatch, Err(ActivityError::InvalidRecord { .. })));

    let garbage = RunEventRecord::from_stored(
        3,
        "tool_call.started",
        now,
        now,
        false,
        None,
        serde_json::json!({ "event_type": "tool_call.started", "call_id": 7 }),
    );
    assert!(matches!(garbage, Err(ActivityError::InvalidRecord { .. })));

    assert!(matches!(
        derive_outcome(RunStatus::Completed, None, None),
        Err(ActivityError::InvalidRecord { .. })
    ));
    assert!(matches!(
        derive_outcome(RunStatus::Completed, Some("broadcast"), None),
        Err(ActivityError::InvalidRecord { .. })
    ));
    assert_eq!(
        derive_outcome(RunStatus::Completed, Some("reply"), None).expect("outcome"),
        RunOutcome::Completed {
            delivery_intent: DeliveryIntentKind::Reply
        }
    );
    assert_eq!(
        derive_outcome(RunStatus::Running, Some("reply"), Some("ignored")).expect("outcome"),
        RunOutcome::Pending
    );
}

#[test]
fn legacy_row_text_is_bounded_and_redacted_at_read_time() {
    let dirty = format!(
        "boom\u{0}\u{7} api_key={FIXTURE_SECRET} {}",
        "y".repeat(4000)
    );
    let bounded = bound_row_text(&dirty, 2048);
    assert!(bounded.len() <= 2048);
    assert!(!bounded.contains(FIXTURE_SECRET));
    assert!(!bounded.contains('\u{0}'));
    assert!(bounded.contains(REDACTED));

    let long_code = "e".repeat(500);
    match derive_outcome(RunStatus::Failed, None, Some(&long_code)).expect("outcome") {
        RunOutcome::Failed { code: Some(code) } => assert_eq!(code.len(), MAX_ERROR_CODE_BYTES),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn run_list_cursor_round_trips_and_query_bounds_hold() {
    let queued_at = Utc
        .with_ymd_and_hms(2026, 9, 4, 8, 30, 15)
        .single()
        .expect("timestamp")
        + Duration::microseconds(123_456);
    let run = header(RunStatus::Queued, queued_at);
    let cursor = RunListCursor::after(&run);
    let encoded = cursor.encode();
    assert_eq!(RunListCursor::decode(&encoded).expect("decode"), cursor);
    assert_eq!(cursor.queued_at(), queued_at);
    assert_eq!(cursor.run_id(), run.run_id);
    assert_eq!(
        serde_json::to_value(cursor).expect("serialize"),
        serde_json::Value::String(encoded)
    );

    for garbage in [
        "",
        "abc",
        "12:not-a-uuid",
        "x:00000000000000000000000000000000",
    ] {
        assert!(matches!(
            RunListCursor::decode(garbage),
            Err(ActivityError::InvalidQuery(_))
        ));
    }

    let query = RunListQuery {
        limit: Some(10_000),
        ..RunListQuery::default()
    };
    assert_eq!(query.page_size(), MAX_RUN_PAGE_SIZE);
    assert_eq!(RunListQuery::default().validate().ok(), Some(()));
    let inverted = RunListQuery {
        queued_from: Some(queued_at),
        queued_until: Some(queued_at),
        ..RunListQuery::default()
    };
    assert!(matches!(
        inverted.validate(),
        Err(ActivityError::InvalidQuery(_))
    ));
    let filtered = RunListQuery {
        statuses: vec![RunStatus::Failed, RunStatus::Queued, RunStatus::Failed],
        ..RunListQuery::default()
    };
    assert_eq!(filtered.status_filter(), Some(vec!["failed", "queued"]));
    assert_eq!(RunListQuery::default().status_filter(), None);
}
