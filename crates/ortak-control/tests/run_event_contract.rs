//! Normalized run-event contract: bounded payloads, secret redaction, and the
//! closed event-type vocabulary that `run_events.event_type` accepts.

use chrono::Utc;
use ortak_control::run_event::{
    strip_control_characters, BoundedText, DeliveryIntentKind, RedactionPolicy, RunEvent,
    RunEventError, RunEventPayload, RunEventType, TerminalStream, UsageTelemetry,
    DEFAULT_MAX_TEXT_BYTES, MAX_PAYLOAD_BYTES, REDACTED,
};
use uuid::Uuid;

const FAKE_LITERAL_SECRET: &str = "fixture-literal-secret-value-0001";

fn normalized(payload: RunEventPayload, policy: &RedactionPolicy) -> RunEvent {
    RunEvent::normalize(
        Uuid::new_v4(),
        Utc::now(),
        Some("c1".to_owned()),
        &payload,
        policy,
    )
    .expect("normalize")
}

fn terminal_output(text: &str) -> RunEventPayload {
    RunEventPayload::TerminalOutput {
        command_id: "cmd-1".to_owned(),
        stream: TerminalStream::Stdout,
        chunk: BoundedText::raw(text),
    }
}

fn chunk_text(event: &RunEvent) -> &BoundedText {
    match &event.payload {
        RunEventPayload::TerminalOutput { chunk, .. } => chunk,
        other => panic!("unexpected payload {other:?}"),
    }
}

#[test]
fn secret_shaped_material_is_redacted_from_persisted_text() {
    let policy = RedactionPolicy::new().with_literal_secrets([FAKE_LITERAL_SECRET]);
    let corpus = [
        ("Authorization: Bearer abc.def.ghi-XYZ", "abc.def.ghi-XYZ"),
        (
            "export OPENAI_API_KEY=sk-fixture-not-a-real-key-000",
            "sk-fixture-not-a-real-key-000",
        ),
        (
            "nostr key nsec1fixturefixturefixture000",
            "nsec1fixturefixturefixture000",
        ),
        (
            "jwt eyJfixture-header.eyJfixture-claims.fixture-sig",
            "eyJfixture-header",
        ),
        ("{\"password\": \"hunter2-fixture\"}", "hunter2-fixture"),
        (
            "client_secret=fixture-client-secret",
            "fixture-client-secret",
        ),
        ("token: fixture-token-value", "fixture-token-value"),
        (
            "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBg\n-----END PRIVATE KEY-----",
            "MIIEvQIBADANBg",
        ),
        (
            &format!("tool output leaked {FAKE_LITERAL_SECRET} into stdout"),
            FAKE_LITERAL_SECRET,
        ),
        (
            "github ghp_fixture_not_a_real_token_000",
            "ghp_fixture_not_a_real_token_000",
        ),
    ];
    for (input, must_be_absent) in corpus {
        let event = normalized(terminal_output(input), &policy);
        let json = serde_json::to_string(&event.payload).expect("json");
        assert!(
            !json.contains(must_be_absent),
            "{must_be_absent:?} survived redaction in {json}"
        );
        assert!(
            json.contains(REDACTED),
            "no placeholder written for {input:?}"
        );
    }
}

#[test]
fn ordinary_text_survives_redaction() {
    let policy = RedactionPolicy::new();
    let inputs = [
        "the token count is 12 and max_tokens=4000",
        "GET http://localhost:8080/health returned 200 at 12:30:05",
        "credential://ortak-runtime/cem/codex-oauth resolved",
        "a basic explanation of the bearer-of-news idiom",
        "Cem, selam nasılsın?",
    ];
    for input in inputs {
        let event = normalized(terminal_output(input), &policy);
        assert_eq!(chunk_text(&event).text, input, "{input:?} was altered");
    }
}

#[test]
fn identifier_fields_are_redacted_and_clamped_too() {
    let policy = RedactionPolicy::new();
    let event = normalized(
        RunEventPayload::ToolCallStarted {
            call_id: "x".repeat(1_000),
            tool: "sk-fixture-not-a-real-key-000".to_owned(),
            arguments: BoundedText::raw("{\"api_key\": \"fixture-api-key-value\"}"),
        },
        &policy,
    );
    let RunEventPayload::ToolCallStarted {
        call_id,
        tool,
        arguments,
    } = &event.payload
    else {
        panic!("unexpected payload");
    };
    assert_eq!(call_id.len(), 256);
    assert_eq!(tool, REDACTED);
    assert!(!arguments.text.contains("fixture-api-key-value"));
}

#[test]
fn oversized_output_is_truncated_with_hash_metadata_and_fits_the_column() {
    let policy = RedactionPolicy::new();
    let big = "é".repeat(100 * 1024);
    let event = normalized(terminal_output(&big), &policy);
    let chunk = chunk_text(&event);
    assert!(chunk.truncated);
    assert!(chunk.text.len() <= DEFAULT_MAX_TEXT_BYTES);
    assert!(chunk.text.chars().all(|character| character == 'é'));
    assert_eq!(chunk.original_bytes, Some(big.len() as u64));
    assert_eq!(chunk.original_sha256.as_deref().map(str::len), Some(64));
    let json = serde_json::to_string(&event.payload).expect("json");
    assert!(json.len() <= MAX_PAYLOAD_BYTES);
    assert_eq!(event.validate(), Ok(()));
}

#[test]
fn validation_rejects_unbounded_cursor_and_oversized_payload() {
    let policy = RedactionPolicy::new();
    let mut event = normalized(terminal_output("ok"), &policy);
    event.runtime_cursor = Some("c".repeat(513));
    assert_eq!(event.validate(), Err(RunEventError::InvalidCursor));

    // Bypass normalization to prove the size check is enforced on its own.
    let raw = RunEvent {
        run_id: Uuid::new_v4(),
        sequence: None,
        occurred_at: Utc::now(),
        runtime_cursor: None,
        artifact_ref: None,
        payload: terminal_output(&"x".repeat(MAX_PAYLOAD_BYTES + 1)),
    };
    assert!(matches!(
        raw.validate(),
        Err(RunEventError::PayloadTooLarge { .. })
    ));
}

#[test]
fn event_types_match_the_column_grammar_and_serde_tags() {
    let payloads = vec![
        RunEventPayload::RunQueued,
        RunEventPayload::RunStarted {
            runtime_run_ref: "r".to_owned(),
        },
        RunEventPayload::RunWaiting {
            reason: "approval".to_owned(),
            detail: BoundedText::raw("waiting"),
        },
        RunEventPayload::RunCompleted {
            delivery_intent: DeliveryIntentKind::Reply,
        },
        RunEventPayload::RunFailed {
            code: "boom".to_owned(),
            message: BoundedText::raw("failed"),
        },
        RunEventPayload::RunCancelled {
            reason: BoundedText::raw("operator"),
        },
        RunEventPayload::AssistantDelta {
            turn: 1,
            delta: BoundedText::raw("hi"),
        },
        RunEventPayload::ToolCallStarted {
            call_id: "c".to_owned(),
            tool: "web".to_owned(),
            arguments: BoundedText::raw("{}"),
        },
        RunEventPayload::ToolCallCompleted {
            call_id: "c".to_owned(),
            result: BoundedText::raw("ok"),
        },
        RunEventPayload::ToolCallFailed {
            call_id: "c".to_owned(),
            error: BoundedText::raw("no"),
        },
        RunEventPayload::TerminalStarted {
            command_id: "t".to_owned(),
            command: BoundedText::raw("ls"),
            cwd: None,
        },
        terminal_output("x"),
        RunEventPayload::TerminalCompleted {
            command_id: "t".to_owned(),
            exit_code: Some(0),
        },
        RunEventPayload::FileChanged {
            path: "a.rs".to_owned(),
            change: ortak_control::run_event::FileChangeKind::Modified,
            summary: BoundedText::raw("+1 -1"),
            bytes: Some(10),
        },
        RunEventPayload::UsageRecorded {
            usage: UsageTelemetry::default(),
        },
        RunEventPayload::ErrorRaised {
            code: "e".to_owned(),
            message: BoundedText::raw("m"),
            retryable: true,
        },
        RunEventPayload::DeliveryIntent {
            intent: DeliveryIntentKind::Silent,
            target_ref: None,
        },
    ];
    let mut seen = std::collections::BTreeSet::new();
    for payload in payloads {
        let event_type = payload.event_type();
        let name = event_type.as_str();
        assert!(seen.insert(name), "duplicate event type {name}");
        let mut chars = name.chars();
        assert!(chars.next().is_some_and(|c| c.is_ascii_lowercase()));
        assert!(name.len() <= 64);
        assert!(chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.'));
        assert_eq!(RunEventType::parse(name), Some(event_type));
        let json = serde_json::to_value(&payload).expect("json");
        assert_eq!(
            json["event_type"], name,
            "serde tag must equal the column value"
        );
        let back: RunEventPayload = serde_json::from_value(json).expect("round trip");
        assert_eq!(back, payload);
    }
    assert_eq!(seen.len(), 17);
}

#[test]
fn nul_and_control_bytes_are_stripped_before_persistence() {
    let policy = RedactionPolicy::new();
    // PostgreSQL jsonb rejects \u0000 outright; other C0/C1 controls have no
    // place in a transcript. Newline, carriage return, and tab survive.
    let raw = "a\0b\u{1b}[31mc\td\r\ne\u{7f}f\u{85}g";
    assert_eq!(strip_control_characters(raw), "ab[31mc\td\r\nefg");
    assert_eq!(strip_control_characters("plain"), "plain");

    let event = normalized(terminal_output(raw), &policy);
    assert_eq!(chunk_text(&event).text, "ab[31mc\td\r\nefg");
    assert!(!chunk_text(&event).truncated);
    let json = serde_json::to_string(&event.payload).expect("json");
    assert!(!json.contains("\\u0000"), "{json}");
    assert_eq!(event.validate(), Ok(()));

    // Identifier-like fields go through the same sanitizer, and a secret
    // split by NUL bytes is still recognized once they are removed.
    let event = normalized(
        RunEventPayload::ToolCallStarted {
            call_id: "call\0-1".to_owned(),
            tool: "we\u{0}b".to_owned(),
            arguments: BoundedText::raw("sk-fix\0ture-not-a-real-key-000 rest"),
        },
        &policy,
    );
    let RunEventPayload::ToolCallStarted {
        call_id,
        tool,
        arguments,
    } = &event.payload
    else {
        panic!("unexpected payload");
    };
    assert_eq!(call_id, "call-1");
    assert_eq!(tool, "web");
    assert_eq!(arguments.text, format!("{REDACTED} rest"));
}

#[test]
fn validation_rejects_nul_that_bypassed_normalization() {
    let raw = RunEvent {
        run_id: Uuid::new_v4(),
        sequence: None,
        occurred_at: Utc::now(),
        runtime_cursor: None,
        artifact_ref: None,
        payload: terminal_output("with\0nul"),
    };
    assert_eq!(raw.validate(), Err(RunEventError::NulInPayload));

    let policy = RedactionPolicy::new();
    let mut event = normalized(terminal_output("ok"), &policy);
    event.runtime_cursor = Some("cursor\0".to_owned());
    assert_eq!(event.validate(), Err(RunEventError::InvalidCursor));
    event.runtime_cursor = Some("cursor".to_owned());
    event.artifact_ref = Some("blob\0".to_owned());
    assert_eq!(event.validate(), Err(RunEventError::InvalidArtifactRef));

    // The normalizing constructor refuses a NUL cursor up front rather than
    // letting the batch fail at the database on every replay.
    let refused = RunEvent::normalize(
        Uuid::new_v4(),
        Utc::now(),
        Some("c\0".to_owned()),
        &terminal_output("ok"),
        &policy,
    );
    assert_eq!(refused.err(), Some(RunEventError::InvalidCursor));
}
