//! Production-seam Postgres tests for the Activity read slice.
//!
//! Run with a disposable local database that can receive the embedded
//! migrations:
//! `ORTAK_TEST_DATABASE_URL=postgres://... cargo test -p ortak-observability -- --ignored`

use chrono::{DateTime, Duration, Utc};
use ortak_control::ports::{CompanyDirectory, RunEventRepository};
use ortak_control::run_event::{
    BoundedText, DeliveryIntentKind, FileChangeKind, RedactionPolicy, RunEvent, RunEventPayload,
    RunEventType, TerminalStream, UsageTelemetry, REDACTED,
};
use ortak_control::{CompanyScope, PgControlPlane};
use ortak_domain::{EmployeeId, RoutingPolicy};
use ortak_observability::projection::RunEventRecord;
use ortak_observability::{
    Activity, ActivityError, ActivityQueries, LifecyclePhase, RunEventsQuery, RunListCursor,
    RunListQuery, RunOutcome, RunStatus, SummaryFacts,
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const DEFAULT_DATABASE_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1 -- local test-only credentials
const FIXTURE_RUNTIME_REF: &str = "fake-runtime-run-ref-0001";
const FIXTURE_SECRET: &str = "fixture-literal-secret-value-0001";

fn database_url() -> String {
    std::env::var("ORTAK_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("BUZZ_TEST_DATABASE_URL"))
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned())
}

async fn setup_pool() -> PgPool {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to test database");
    buzz_db::migration::run_migrations(&pool)
        .await
        .expect("apply migrations");
    pool
}

fn employee(value: &str) -> EmployeeId {
    EmployeeId::parse(value).expect("valid employee id")
}

/// One company with two employees whose revisions runs can pin.
struct Company {
    control: PgControlPlane,
    pool: PgPool,
    scope: CompanyScope,
    revisions: Vec<(EmployeeId, Uuid)>,
}

impl Company {
    async fn new(pool: &PgPool) -> Self {
        let control = PgControlPlane::new(pool.clone());
        let slug = format!("co-{}", Uuid::new_v4().simple());
        let company_id: Uuid = sqlx::query(
            "INSERT INTO companies (slug, display_name, routing_policy)
             VALUES ($1, 'Ortak activity test', $2) RETURNING id",
        )
        .bind(&slug)
        .bind(serde_json::to_value(RoutingPolicy::default()).expect("policy json"))
        .fetch_one(pool)
        .await
        .expect("insert company")
        .try_get("id")
        .expect("company id");
        let mut revisions = Vec::new();
        for id in ["cem", "zeynep"] {
            let employee_id = employee(id);
            sqlx::query("INSERT INTO employees (company_id, id, status) VALUES ($1, $2, 'draft')")
                .bind(company_id)
                .bind(employee_id.as_str())
                .execute(pool)
                .await
                .expect("insert employee");
            let manifest = serde_json::json!({ "fixture": id });
            let revision_id: Uuid = sqlx::query(
                "INSERT INTO employee_revisions
                     (company_id, employee_id, revision_number, manifest,
                      manifest_fingerprint, provisioning_mode)
                 VALUES ($1, $2, 1, $3, $4, 'adopt') RETURNING id",
            )
            .bind(company_id)
            .bind(employee_id.as_str())
            .bind(&manifest)
            .bind(Sha256::digest(manifest.to_string().as_bytes()).to_vec())
            .fetch_one(pool)
            .await
            .expect("insert revision")
            .try_get("id")
            .expect("revision id");
            revisions.push((employee_id, revision_id));
        }
        let scope = control
            .resolve_company_by_slug(&slug)
            .await
            .expect("resolve scope");
        Self {
            control,
            pool: pool.clone(),
            scope,
            revisions,
        }
    }

    fn revision(&self, employee_id: &str) -> Uuid {
        self.revisions
            .iter()
            .find(|(id, _)| id.as_str() == employee_id)
            .map(|(_, revision)| *revision)
            .expect("known employee")
    }

    /// Inserts a queued run and its opening `run.queued` event through the
    /// production append path.
    async fn queue_run(&self, employee_id: &str, queued_at: DateTime<Utc>) -> Uuid {
        let run_id: Uuid = sqlx::query(
            "INSERT INTO runs
                 (company_id, employee_id, employee_revision_id, runtime_adapter, status, queued_at)
             VALUES ($1, $2, $3, 'fake-runtime', 'queued', $4) RETURNING id",
        )
        .bind(self.scope.company_id())
        .bind(employee_id)
        .bind(self.revision(employee_id))
        .bind(queued_at)
        .fetch_one(&self.pool)
        .await
        .expect("insert run")
        .try_get("id")
        .expect("run id");
        self.append(run_id, &[RunEventPayload::RunQueued]).await;
        run_id
    }

    /// Normalizes and appends payloads with cursors; returns the sequences.
    async fn append(&self, run_id: Uuid, payloads: &[RunEventPayload]) -> Vec<RunEvent> {
        let policy = RedactionPolicy::new().with_literal_secrets([FIXTURE_SECRET]);
        let events: Vec<RunEvent> = payloads
            .iter()
            .map(|payload| {
                RunEvent::normalize(
                    run_id,
                    Utc::now(),
                    Some(format!("cursor-{}", Uuid::new_v4().simple())),
                    payload,
                    &policy,
                )
                .expect("normalize")
            })
            .collect();
        let appended = self
            .control
            .append_run_events(&self.scope, run_id, &events)
            .await
            .expect("append events");
        assert_eq!(appended.sequences.len(), events.len());
        assert!(appended.duplicate_cursors.is_empty());
        events
            .into_iter()
            .zip(appended.sequences)
            .map(|(mut event, sequence)| {
                event.sequence = Some(sequence);
                event
            })
            .collect()
    }

    /// Writes terminal columns directly (the supervisor owns this in
    /// production; here only the read side is under test).
    async fn finish(
        &self,
        run_id: Uuid,
        status: &str,
        delivery_intent: Option<&str>,
        error: Option<(&str, &str)>,
        cancel_reason: Option<&str>,
    ) {
        sqlx::query(
            "UPDATE runs
                SET status = $3, delivery_intent = $4, error_code = $5, error_message = $6,
                    cancel_reason = $7, runtime_run_ref = $8,
                    started_at = now(), finished_at = now(), updated_at = now()
              WHERE company_id = $1 AND id = $2",
        )
        .bind(self.scope.company_id())
        .bind(run_id)
        .bind(status)
        .bind(delivery_intent)
        .bind(error.map(|(code, _)| code))
        .bind(error.map(|(_, message)| message))
        .bind(cancel_reason)
        .bind(FIXTURE_RUNTIME_REF)
        .execute(&self.pool)
        .await
        .expect("finish run");
    }
}

fn records_of(events: &[RunEvent]) -> Vec<RunEventRecord> {
    events
        .iter()
        .map(|event| {
            RunEventRecord::from_stored(
                event.sequence.expect("assigned sequence"),
                event.event_type().as_str(),
                event.occurred_at,
                event.occurred_at,
                event.runtime_cursor.is_some(),
                event.artifact_ref.clone(),
                event.payload_json().expect("payload json"),
            )
            .expect("record")
        })
        .collect()
}

fn work_timeline() -> Vec<RunEventPayload> {
    vec![
        RunEventPayload::RunStarted {
            runtime_run_ref: FIXTURE_RUNTIME_REF.to_owned(),
        },
        RunEventPayload::AssistantDelta {
            turn: 1,
            delta: BoundedText::raw("Bakıyorum"),
        },
        RunEventPayload::ToolCallStarted {
            call_id: "call-1".to_owned(),
            tool: "shell".to_owned(),
            arguments: BoundedText::raw(format!("api_key={FIXTURE_SECRET}")),
        },
        RunEventPayload::ToolCallCompleted {
            call_id: "call-1".to_owned(),
            result: BoundedText::raw("ok"),
        },
        RunEventPayload::ToolCallStarted {
            call_id: "call-2".to_owned(),
            tool: "edit".to_owned(),
            arguments: BoundedText::raw("{}"),
        },
        RunEventPayload::ToolCallFailed {
            call_id: "call-2".to_owned(),
            error: BoundedText::raw("denied"),
        },
        RunEventPayload::TerminalStarted {
            command_id: "cmd-1".to_owned(),
            command: BoundedText::raw("cargo test"),
            cwd: None,
        },
        RunEventPayload::TerminalOutput {
            command_id: "cmd-1".to_owned(),
            stream: TerminalStream::Stdout,
            chunk: BoundedText::raw("running 3 tests"),
        },
        RunEventPayload::TerminalOutput {
            command_id: "cmd-1".to_owned(),
            stream: TerminalStream::Stderr,
            chunk: BoundedText::raw("1 failed"),
        },
        RunEventPayload::TerminalCompleted {
            command_id: "cmd-1".to_owned(),
            exit_code: Some(101),
        },
        RunEventPayload::TerminalStarted {
            command_id: "cmd-2".to_owned(),
            command: BoundedText::raw("cargo fmt"),
            cwd: None,
        },
        RunEventPayload::TerminalCompleted {
            command_id: "cmd-2".to_owned(),
            exit_code: Some(0),
        },
        RunEventPayload::TerminalStarted {
            command_id: "cmd-3".to_owned(),
            command: BoundedText::raw("sleep 100"),
            cwd: None,
        },
        RunEventPayload::TerminalCompleted {
            command_id: "cmd-3".to_owned(),
            exit_code: None,
        },
        RunEventPayload::FileChanged {
            path: "src/lib.rs".to_owned(),
            change: FileChangeKind::Modified,
            summary: BoundedText::raw("+3 -1"),
            bytes: Some(2048),
        },
        RunEventPayload::FileChanged {
            path: "src/new.rs".to_owned(),
            change: FileChangeKind::Created,
            summary: BoundedText::raw("+40"),
            bytes: Some(900),
        },
        RunEventPayload::UsageRecorded {
            usage: UsageTelemetry {
                model: Some("fixture-model".to_owned()),
                input_tokens: Some(1200),
                output_tokens: Some(300),
                cached_input_tokens: Some(100),
                reasoning_tokens: None,
            },
        },
        RunEventPayload::UsageRecorded {
            usage: UsageTelemetry {
                model: Some("fixture-model".to_owned()),
                input_tokens: Some(800),
                output_tokens: Some(50),
                cached_input_tokens: None,
                reasoning_tokens: Some(7),
            },
        },
        RunEventPayload::ErrorRaised {
            code: "provider_timeout".to_owned(),
            message: BoundedText::raw("retrying"),
            retryable: true,
        },
        RunEventPayload::DeliveryIntent {
            intent: DeliveryIntentKind::Silent,
            target_ref: None,
        },
        RunEventPayload::RunCompleted {
            delivery_intent: DeliveryIntentKind::Silent,
        },
    ]
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn run_list_pages_deterministically_and_isolates_filters_and_companies() {
    let pool = setup_pool().await;
    let company = Company::new(&pool).await;
    let other = Company::new(&pool).await;

    // Five runs queued in the same microsecond: only the id can break ties.
    let same_instant = Utc::now() - Duration::minutes(1);
    let mut expected = Vec::new();
    for employee_id in ["cem", "cem", "zeynep", "cem", "zeynep"] {
        expected.push((
            same_instant,
            company.queue_run(employee_id, same_instant).await,
        ));
    }
    let older = same_instant - Duration::hours(2);
    let older_run = company.queue_run("zeynep", older).await;
    expected.push((older, older_run));
    company
        .finish(
            older_run,
            "failed",
            None,
            Some(("boom", "old failure")),
            None,
        )
        .await;
    // Noise in another company that must never appear.
    other.queue_run("cem", same_instant).await;
    other
        .queue_run("zeynep", same_instant + Duration::seconds(1))
        .await;

    expected.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let expected_ids: Vec<Uuid> = expected.iter().map(|(_, id)| *id).collect();

    // Walk pages of two through the encoded cursor, as a transport would.
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let query = RunListQuery {
            limit: Some(2),
            cursor: cursor
                .as_deref()
                .map(|value| RunListCursor::decode(value).expect("decode cursor")),
            ..RunListQuery::default()
        };
        let page = company
            .control
            .list_runs(&company.scope, &query)
            .await
            .expect("list runs");
        pages += 1;
        assert!(page.runs.len() <= 2);
        seen.extend(page.runs.iter().map(|run| run.run_id));
        assert_eq!(page.has_more, page.next_cursor.is_some());
        match page.next_cursor {
            Some(next) => cursor = Some(next.encode()),
            None => break,
        }
    }
    assert_eq!(pages, 3);
    assert_eq!(seen, expected_ids, "stable order under equal timestamps");

    // Every header is the run's own facts; no runtime reference leaks.
    let all = company
        .control
        .list_runs(&company.scope, &RunListQuery::default())
        .await
        .expect("list runs");
    assert!(!all.has_more);
    assert_eq!(all.runs.len(), 6);
    let failed = all
        .runs
        .iter()
        .find(|run| run.run_id == older_run)
        .expect("older run listed");
    assert_eq!(failed.status, RunStatus::Failed);
    assert_eq!(
        failed.outcome,
        RunOutcome::Failed {
            code: Some("boom".to_owned())
        }
    );
    assert!(failed.runtime.has_run_ref);
    assert_eq!(failed.runtime.adapter, "fake-runtime");
    assert_eq!(failed.employee_id.as_str(), "zeynep");
    assert_eq!(failed.employee_revision_id, company.revision("zeynep"));
    assert_eq!(failed.provenance.routing_decision_id, None);
    let last = failed.last_event.expect("last event");
    assert_eq!(
        (last.sequence, last.event_type),
        (0, RunEventType::RunQueued)
    );
    let rendered = serde_json::to_string(&all).expect("serialize page");
    assert!(!rendered.contains(FIXTURE_RUNTIME_REF));

    // Employee, status, and time filters are independent and additive.
    let cem_only = company
        .control
        .list_runs(
            &company.scope,
            &RunListQuery {
                employee_id: Some(employee("cem")),
                ..RunListQuery::default()
            },
        )
        .await
        .expect("employee filter");
    assert_eq!(cem_only.runs.len(), 3);
    assert!(cem_only
        .runs
        .iter()
        .all(|run| run.employee_id.as_str() == "cem"));

    let failed_only = company
        .control
        .list_runs(
            &company.scope,
            &RunListQuery {
                statuses: vec![RunStatus::Failed, RunStatus::Cancelled],
                ..RunListQuery::default()
            },
        )
        .await
        .expect("status filter");
    assert_eq!(
        failed_only
            .runs
            .iter()
            .map(|run| run.run_id)
            .collect::<Vec<_>>(),
        vec![older_run]
    );

    let recent = company
        .control
        .list_runs(
            &company.scope,
            &RunListQuery {
                queued_from: Some(same_instant),
                queued_until: Some(same_instant + Duration::seconds(1)),
                ..RunListQuery::default()
            },
        )
        .await
        .expect("time filter");
    assert_eq!(recent.runs.len(), 5);
    assert!(recent.runs.iter().all(|run| run.run_id != older_run));

    // The other company sees only its own two runs, never this company's.
    let foreign = other
        .control
        .list_runs(&other.scope, &RunListQuery::default())
        .await
        .expect("foreign list");
    assert_eq!(foreign.runs.len(), 2);
    assert!(foreign
        .runs
        .iter()
        .all(|run| !expected_ids.contains(&run.run_id)));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn unknown_and_cross_company_runs_are_indistinguishably_not_found() {
    let pool = setup_pool().await;
    let company = Company::new(&pool).await;
    let other = Company::new(&pool).await;
    let run_id = company.queue_run("cem", Utc::now()).await;

    let not_found = |error: ActivityError, expected: Uuid| match error {
        ActivityError::RunNotFound { run_id } => assert_eq!(run_id, expected),
        other => panic!("expected RunNotFound, got {other:?}"),
    };
    let query = RunEventsQuery::default();

    let foreign_detail = other.control.run_detail(&other.scope, run_id).await;
    not_found(foreign_detail.expect_err("cross-company detail"), run_id);
    let foreign_events = other.control.run_events(&other.scope, run_id, &query).await;
    not_found(foreign_events.expect_err("cross-company events"), run_id);

    let missing = Uuid::new_v4();
    let unknown_detail = company.control.run_detail(&company.scope, missing).await;
    not_found(unknown_detail.expect_err("unknown detail"), missing);
    let unknown_events = company
        .control
        .run_events(&company.scope, missing, &query)
        .await;
    not_found(unknown_events.expect_err("unknown events"), missing);

    let detail = company
        .control
        .run_detail(&company.scope, run_id)
        .await
        .expect("own detail");
    assert_eq!(detail.run.run_id, run_id);
    assert_eq!(detail.run.status, RunStatus::Queued);
    assert_eq!(detail.summary.event_count, 1);
    let page = company
        .control
        .run_events(&company.scope, run_id, &query)
        .await
        .expect("own events");
    assert_eq!(page.entries.len(), 1);
    assert!(!page.has_more);
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn events_page_incrementally_in_order_and_summaries_match_the_fold() {
    let pool = setup_pool().await;
    let company = Company::new(&pool).await;
    let run_id = company.queue_run("cem", Utc::now()).await;
    let mut events = company
        .control
        .run_events_after(&company.scope, run_id, -1, 10)
        .await
        .expect("opening event");
    events.extend(company.append(run_id, &work_timeline()).await);
    let total = events.len() as i64;
    assert_eq!(total, 22);
    company
        .finish(run_id, "completed", Some("silent"), None, None)
        .await;

    // Incremental pages of three: dense, ordered, no duplicate, no gap.
    let mut cursor = None;
    let mut sequences = Vec::new();
    let mut pages = 0;
    loop {
        let page = company
            .control
            .run_events(
                &company.scope,
                run_id,
                &RunEventsQuery {
                    after_sequence: cursor,
                    limit: Some(3),
                    include_raw: false,
                },
            )
            .await
            .expect("events page");
        pages += 1;
        assert_eq!(page.gap, None);
        assert!(page.entries.len() <= 3);
        for entry in &page.entries {
            assert!(entry.has_runtime_cursor || entry.sequence == 0);
            assert!(entry.raw.is_none());
            assert!(entry.recorded_at >= entry.occurred_at - Duration::seconds(5));
        }
        sequences.extend(page.entries.iter().map(|entry| entry.sequence));
        cursor = page.next_after_sequence;
        if !page.has_more {
            break;
        }
    }
    assert_eq!(pages, 8);
    assert_eq!(sequences, (0..total).collect::<Vec<_>>());
    assert_eq!(cursor, Some(total - 1));

    // Polling after the newest sequence is empty and keeps the cursor.
    let idle = company
        .control
        .run_events(
            &company.scope,
            run_id,
            &RunEventsQuery {
                after_sequence: cursor,
                limit: None,
                include_raw: false,
            },
        )
        .await
        .expect("idle poll");
    assert!(idle.entries.is_empty());
    assert!(!idle.has_more);
    assert_eq!(idle.next_after_sequence, cursor);

    // Typed projections and the raw view on demand; nothing sensitive leaks.
    let full = company
        .control
        .run_events(
            &company.scope,
            run_id,
            &RunEventsQuery {
                after_sequence: None,
                limit: Some(100),
                include_raw: true,
            },
        )
        .await
        .expect("full page");
    assert_eq!(full.entries.len(), total as usize);
    assert!(matches!(
        full.entries[1].activity,
        Activity::Lifecycle {
            phase: LifecyclePhase::Started {
                has_runtime_run_ref: true
            }
        }
    ));
    assert!(full.entries[3].redacted, "tool arguments carry the marker");
    assert!(full.entries.iter().all(|entry| entry.raw.is_some()));
    let rendered = serde_json::to_string(&full).expect("serialize page");
    assert!(!rendered.contains(FIXTURE_RUNTIME_REF));
    assert!(!rendered.contains(FIXTURE_SECRET));
    assert!(!rendered.contains("cursor-"));
    assert!(rendered.contains(REDACTED));

    // The SQL aggregate equals the pure fold over the same events.
    let detail = company
        .control
        .run_detail(&company.scope, run_id)
        .await
        .expect("detail");
    let folded = SummaryFacts::fold(&records_of(&events));
    let summary = &detail.summary;
    assert_eq!(summary.event_count, folded.event_count);
    assert_eq!(summary.tools, folded.tools);
    assert_eq!(summary.terminal, folded.terminal);
    assert_eq!(summary.files, folded.files);
    assert_eq!(summary.assistant_fragments, folded.assistant_fragments);
    assert_eq!(summary.waits, folded.waits);
    assert_eq!(summary.errors_raised, folded.errors_raised);
    assert_eq!(summary.usage, Some(folded.usage));
    assert_eq!(summary.tools.started, 2);
    assert_eq!(summary.tools.failed, 1);
    assert_eq!(summary.terminal.commands, 3);
    assert_eq!(summary.terminal.nonzero_exits, 1);
    assert_eq!(summary.terminal.abnormal_exits, 1);
    assert_eq!(summary.terminal.output_chunks, 2);
    assert_eq!(summary.files.changes, 2);
    assert_eq!(summary.usage.expect("usage").input_tokens, 2000);
    assert_eq!(summary.usage.expect("usage").reasoning_tokens, 7);
    let last = summary.last_event.expect("last event");
    assert_eq!(last.sequence, total - 1);
    assert_eq!(last.event_type, RunEventType::RunCompleted);
    assert_eq!(detail.run.last_event, Some(last));
    let terminal = summary.terminal_state.clone().expect("terminal state");
    assert_eq!(terminal.status, RunStatus::Completed);
    assert_eq!(
        terminal.outcome,
        RunOutcome::Completed {
            delivery_intent: DeliveryIntentKind::Silent
        }
    );
    assert!(terminal.finished_at.is_some());
    assert!(detail.run.runtime.has_run_ref);
    assert_eq!(detail.error_message, None);
    let rendered = serde_json::to_string(&detail).expect("serialize detail");
    assert!(!rendered.contains(FIXTURE_RUNTIME_REF));

    // Legacy failure text on the row is bounded and redacted when read.
    let failed_run = company.queue_run("zeynep", Utc::now()).await;
    let legacy_message = format!("token={FIXTURE_SECRET} {}", "z".repeat(1900));
    company
        .finish(
            failed_run,
            "failed",
            None,
            Some(("runtime_crash", &legacy_message)),
            None,
        )
        .await;
    let failed = company
        .control
        .run_detail(&company.scope, failed_run)
        .await
        .expect("failed detail");
    let message = failed.error_message.expect("error message");
    assert!(!message.contains(FIXTURE_SECRET));
    assert!(message.contains(REDACTED));
    assert!(message.len() <= 2048);
    assert_eq!(
        failed.run.outcome,
        RunOutcome::Failed {
            code: Some("runtime_crash".to_owned())
        }
    );
    assert_eq!(failed.summary.event_count, 1);
    assert!(failed.summary.usage.is_none());
}
