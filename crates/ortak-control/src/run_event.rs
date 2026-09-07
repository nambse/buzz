//! Normalized run events (Architecture v0 §4.6).
//!
//! Runtime adapters emit provider-specific streams; this module defines the
//! provider-neutral shape that is persisted in `run_events`. Persistence goes
//! through [`RunEventPayload::normalize`], which bounds every text field,
//! redacts secret-like material, and guarantees the serialized payload fits
//! the column check. Raw adapter output never reaches the database directly.

mod final_text;
pub use final_text::{assemble_final_text, FinalTextRefusal};

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::adapter::truncate_at_char_boundary;

/// Hard ceiling for one serialized payload. The column allows 65536 bytes of
/// `jsonb::text`, which renders with extra whitespace, so the compact form is
/// capped lower to leave margin.
pub const MAX_PAYLOAD_BYTES: usize = 32 * 1024;

/// Default ceiling for one text field inside a payload.
pub const DEFAULT_MAX_TEXT_BYTES: usize = 8 * 1024;

/// Placeholder written in place of redacted material.
pub const REDACTED: &str = "[redacted]";

/// Removes characters PostgreSQL `text`/`jsonb` cannot store or that have no
/// place in a persisted transcript: NUL (rejected outright by the database)
/// and every other control character except newline, carriage return, and
/// tab. Applied before redaction so a secret split by control bytes is still
/// recognized, and before truncation so the ceiling applies to stored bytes.
pub fn strip_control_characters(value: &str) -> String {
    if !value.chars().any(is_disallowed_control) {
        return value.to_owned();
    }
    value
        .chars()
        .filter(|character| !is_disallowed_control(*character))
        .collect()
}

fn is_disallowed_control(character: char) -> bool {
    character.is_control() && !matches!(character, '\n' | '\r' | '\t')
}

/// Stable, closed event-type vocabulary stored in `run_events.event_type`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventType {
    /// Run accepted and waiting for a runtime.
    RunQueued,
    /// Runtime started executing the run.
    RunStarted,
    /// Run is blocked on approval, input, or an external dependency.
    RunWaiting,
    /// Run finished with a typed delivery intent.
    RunCompleted,
    /// Run finished with an error.
    RunFailed,
    /// Run was cancelled.
    RunCancelled,
    /// Streamed assistant text fragment.
    AssistantDelta,
    /// Tool invocation began.
    ToolCallStarted,
    /// Tool invocation returned a result.
    ToolCallCompleted,
    /// Tool invocation failed.
    ToolCallFailed,
    /// Terminal command started.
    TerminalStarted,
    /// Terminal output chunk.
    TerminalOutput,
    /// Terminal command exited.
    TerminalCompleted,
    /// A workspace file was read, created, modified, or deleted.
    FileChanged,
    /// Model usage telemetry.
    UsageRecorded,
    /// Runtime, tool, or provider error that did not (yet) terminate the run.
    ErrorRaised,
    /// Typed delivery intent chosen by the runtime.
    DeliveryIntent,
}

impl RunEventType {
    /// Column value; matches the `^[a-z][a-z0-9_.]{0,63}$` check.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunQueued => "run.queued",
            Self::RunStarted => "run.started",
            Self::RunWaiting => "run.waiting",
            Self::RunCompleted => "run.completed",
            Self::RunFailed => "run.failed",
            Self::RunCancelled => "run.cancelled",
            Self::AssistantDelta => "assistant.delta",
            Self::ToolCallStarted => "tool_call.started",
            Self::ToolCallCompleted => "tool_call.completed",
            Self::ToolCallFailed => "tool_call.failed",
            Self::TerminalStarted => "terminal.started",
            Self::TerminalOutput => "terminal.output",
            Self::TerminalCompleted => "terminal.completed",
            Self::FileChanged => "file.changed",
            Self::UsageRecorded => "usage.recorded",
            Self::ErrorRaised => "error.raised",
            Self::DeliveryIntent => "delivery.intent",
        }
    }

    /// Parses a column value.
    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::RunQueued,
            Self::RunStarted,
            Self::RunWaiting,
            Self::RunCompleted,
            Self::RunFailed,
            Self::RunCancelled,
            Self::AssistantDelta,
            Self::ToolCallStarted,
            Self::ToolCallCompleted,
            Self::ToolCallFailed,
            Self::TerminalStarted,
            Self::TerminalOutput,
            Self::TerminalCompleted,
            Self::FileChanged,
            Self::UsageRecorded,
            Self::ErrorRaised,
            Self::DeliveryIntent,
        ]
        .into_iter()
        .find(|candidate| candidate.as_str() == value)
    }

    /// True for the states that end a run.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::RunCompleted | Self::RunFailed | Self::RunCancelled
        )
    }
}

impl fmt::Display for RunEventType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Typed completion chosen by the runtime (Architecture v0 §4.7).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryIntentKind {
    /// Reply to the triggering message or thread.
    Reply,
    /// Publish a new channel message with explicit context.
    Channel,
    /// Complete without publishing.
    Silent,
}

impl DeliveryIntentKind {
    /// Column value stored in `runs.delivery_intent`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reply => "reply",
            Self::Channel => "channel",
            Self::Silent => "silent",
        }
    }
}

/// Which terminal stream a chunk came from.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalStream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Kind of file change observed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    /// File was read.
    Read,
    /// File was created.
    Created,
    /// File was modified.
    Modified,
    /// File was deleted.
    Deleted,
}

/// Bounded text plus truncation metadata. Once normalized, `text` is at most
/// the configured field ceiling and has been redacted.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct BoundedText {
    /// Retained (possibly truncated and redacted) text.
    pub text: String,
    /// True when the original exceeded the ceiling.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    /// Byte length of the original text when truncated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_bytes: Option<u64>,
    /// SHA-256 of the original text (hex) when truncated, so an offloaded
    /// artifact can be matched to this event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_sha256: Option<String>,
}

impl BoundedText {
    /// Wraps raw text without bounding; [`RunEventPayload::normalize`] bounds it.
    pub fn raw(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            truncated: false,
            original_bytes: None,
            original_sha256: None,
        }
    }

    fn normalize(&self, policy: &RedactionPolicy) -> Self {
        let redacted = policy.redact(&strip_control_characters(&self.text));
        if redacted.len() <= policy.max_text_bytes {
            return Self {
                text: redacted,
                truncated: self.truncated,
                original_bytes: self.original_bytes,
                original_sha256: self.original_sha256.clone(),
            };
        }
        let kept = truncate_at_char_boundary(&redacted, policy.max_text_bytes).to_owned();
        Self {
            text: kept,
            truncated: true,
            original_bytes: Some(
                self.original_bytes
                    .unwrap_or(u64::try_from(self.text.len()).unwrap_or(u64::MAX)),
            ),
            original_sha256: Some(
                self.original_sha256
                    .clone()
                    .unwrap_or_else(|| hex::encode(Sha256::digest(self.text.as_bytes()))),
            ),
        }
    }
}

/// Bounded, secret-free usage telemetry. Never authoritative for billing.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct UsageTelemetry {
    /// Model reference reported by the runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Input tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Output tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Cached input tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    /// Reasoning tokens where the provider reports them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

/// Normalized event body. The serde tag equals [`RunEventType::as_str`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "event_type")]
pub enum RunEventPayload {
    /// Run accepted.
    #[serde(rename = "run.queued")]
    RunQueued,
    /// Runtime started the run.
    #[serde(rename = "run.started")]
    RunStarted {
        /// Adapter-side run reference.
        runtime_run_ref: String,
    },
    /// Run is waiting.
    #[serde(rename = "run.waiting")]
    RunWaiting {
        /// Stable reason code such as `approval` or `input`.
        reason: String,
        /// Bounded detail.
        detail: BoundedText,
    },
    /// Run completed.
    #[serde(rename = "run.completed")]
    RunCompleted {
        /// Delivery intent chosen by the runtime.
        delivery_intent: DeliveryIntentKind,
    },
    /// Run failed.
    #[serde(rename = "run.failed")]
    RunFailed {
        /// Stable error code.
        code: String,
        /// Bounded, redacted message.
        message: BoundedText,
    },
    /// Run cancelled.
    #[serde(rename = "run.cancelled")]
    RunCancelled {
        /// Bounded cancellation reason.
        reason: BoundedText,
    },
    /// Assistant text fragment.
    #[serde(rename = "assistant.delta")]
    AssistantDelta {
        /// Turn index within the run.
        turn: u32,
        /// Text fragment.
        delta: BoundedText,
    },
    /// Tool call started.
    #[serde(rename = "tool_call.started")]
    ToolCallStarted {
        /// Adapter-side call correlation id.
        call_id: String,
        /// Tool name.
        tool: String,
        /// Bounded argument summary (JSON or text).
        arguments: BoundedText,
    },
    /// Tool call completed.
    #[serde(rename = "tool_call.completed")]
    ToolCallCompleted {
        /// Adapter-side call correlation id.
        call_id: String,
        /// Bounded result summary.
        result: BoundedText,
    },
    /// Tool call failed.
    #[serde(rename = "tool_call.failed")]
    ToolCallFailed {
        /// Adapter-side call correlation id.
        call_id: String,
        /// Bounded error.
        error: BoundedText,
    },
    /// Terminal command started.
    #[serde(rename = "terminal.started")]
    TerminalStarted {
        /// Adapter-side command correlation id.
        command_id: String,
        /// Bounded command line.
        command: BoundedText,
        /// Working directory reference, if reported.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    /// Terminal output chunk.
    #[serde(rename = "terminal.output")]
    TerminalOutput {
        /// Adapter-side command correlation id.
        command_id: String,
        /// Which stream.
        stream: TerminalStream,
        /// Bounded chunk.
        chunk: BoundedText,
    },
    /// Terminal command exited.
    #[serde(rename = "terminal.completed")]
    TerminalCompleted {
        /// Adapter-side command correlation id.
        command_id: String,
        /// Exit code, if the process exited normally.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },
    /// File change.
    #[serde(rename = "file.changed")]
    FileChanged {
        /// Workspace-relative or absolute path as reported.
        path: String,
        /// Kind of change.
        change: FileChangeKind,
        /// Bounded diff or content summary.
        summary: BoundedText,
        /// Size of the resulting file, if known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes: Option<u64>,
    },
    /// Usage telemetry.
    #[serde(rename = "usage.recorded")]
    UsageRecorded {
        /// Telemetry.
        usage: UsageTelemetry,
    },
    /// Non-terminal error.
    #[serde(rename = "error.raised")]
    ErrorRaised {
        /// Stable error code.
        code: String,
        /// Bounded, redacted message.
        message: BoundedText,
        /// Whether the runtime intends to retry.
        retryable: bool,
    },
    /// Delivery intent.
    #[serde(rename = "delivery.intent")]
    DeliveryIntent {
        /// Intent.
        intent: DeliveryIntentKind,
        /// Explicit target reference for `channel` intents.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_ref: Option<String>,
    },
}

impl RunEventPayload {
    /// Event type of this payload.
    pub fn event_type(&self) -> RunEventType {
        match self {
            Self::RunQueued => RunEventType::RunQueued,
            Self::RunStarted { .. } => RunEventType::RunStarted,
            Self::RunWaiting { .. } => RunEventType::RunWaiting,
            Self::RunCompleted { .. } => RunEventType::RunCompleted,
            Self::RunFailed { .. } => RunEventType::RunFailed,
            Self::RunCancelled { .. } => RunEventType::RunCancelled,
            Self::AssistantDelta { .. } => RunEventType::AssistantDelta,
            Self::ToolCallStarted { .. } => RunEventType::ToolCallStarted,
            Self::ToolCallCompleted { .. } => RunEventType::ToolCallCompleted,
            Self::ToolCallFailed { .. } => RunEventType::ToolCallFailed,
            Self::TerminalStarted { .. } => RunEventType::TerminalStarted,
            Self::TerminalOutput { .. } => RunEventType::TerminalOutput,
            Self::TerminalCompleted { .. } => RunEventType::TerminalCompleted,
            Self::FileChanged { .. } => RunEventType::FileChanged,
            Self::UsageRecorded { .. } => RunEventType::UsageRecorded,
            Self::ErrorRaised { .. } => RunEventType::ErrorRaised,
            Self::DeliveryIntent { .. } => RunEventType::DeliveryIntent,
        }
    }

    /// Applies bounds and redaction to every free-text field, returning a
    /// payload that satisfies the persistence contract.
    ///
    /// Identifier-like fields (`call_id`, `tool`, `path`, `code`, ...) are
    /// also redacted and clamped so an adapter cannot smuggle a secret
    /// through a short field.
    pub fn normalize(&self, policy: &RedactionPolicy) -> Self {
        let short = |value: &str| policy.short(value);
        let opt_short = |value: &Option<String>| value.as_deref().map(short);
        match self {
            Self::RunQueued => Self::RunQueued,
            Self::RunStarted { runtime_run_ref } => Self::RunStarted {
                runtime_run_ref: short(runtime_run_ref),
            },
            Self::RunWaiting { reason, detail } => Self::RunWaiting {
                reason: short(reason),
                detail: detail.normalize(policy),
            },
            Self::RunCompleted { delivery_intent } => Self::RunCompleted {
                delivery_intent: *delivery_intent,
            },
            Self::RunFailed { code, message } => Self::RunFailed {
                code: short(code),
                message: message.normalize(policy),
            },
            Self::RunCancelled { reason } => Self::RunCancelled {
                reason: reason.normalize(policy),
            },
            Self::AssistantDelta { turn, delta } => Self::AssistantDelta {
                turn: *turn,
                delta: delta.normalize(policy),
            },
            Self::ToolCallStarted {
                call_id,
                tool,
                arguments,
            } => Self::ToolCallStarted {
                call_id: short(call_id),
                tool: short(tool),
                arguments: arguments.normalize(policy),
            },
            Self::ToolCallCompleted { call_id, result } => Self::ToolCallCompleted {
                call_id: short(call_id),
                result: result.normalize(policy),
            },
            Self::ToolCallFailed { call_id, error } => Self::ToolCallFailed {
                call_id: short(call_id),
                error: error.normalize(policy),
            },
            Self::TerminalStarted {
                command_id,
                command,
                cwd,
            } => Self::TerminalStarted {
                command_id: short(command_id),
                command: command.normalize(policy),
                cwd: opt_short(cwd),
            },
            Self::TerminalOutput {
                command_id,
                stream,
                chunk,
            } => Self::TerminalOutput {
                command_id: short(command_id),
                stream: *stream,
                chunk: chunk.normalize(policy),
            },
            Self::TerminalCompleted {
                command_id,
                exit_code,
            } => Self::TerminalCompleted {
                command_id: short(command_id),
                exit_code: *exit_code,
            },
            Self::FileChanged {
                path,
                change,
                summary,
                bytes,
            } => Self::FileChanged {
                path: short(path),
                change: *change,
                summary: summary.normalize(policy),
                bytes: *bytes,
            },
            Self::UsageRecorded { usage } => Self::UsageRecorded {
                usage: UsageTelemetry {
                    model: opt_short(&usage.model),
                    ..usage.clone()
                },
            },
            Self::ErrorRaised {
                code,
                message,
                retryable,
            } => Self::ErrorRaised {
                code: short(code),
                message: message.normalize(policy),
                retryable: *retryable,
            },
            Self::DeliveryIntent { intent, target_ref } => Self::DeliveryIntent {
                intent: *intent,
                target_ref: opt_short(target_ref),
            },
        }
    }
}

/// Redaction and bounding rules applied before persistence.
///
/// `literal_secrets` holds resolved credential values supplied at runtime by
/// the credential resolver so that a value that leaks into tool output is
/// scrubbed; it is never serialized, logged, or persisted.
#[derive(Clone, Default)]
pub struct RedactionPolicy {
    max_text_bytes: usize,
    literal_secrets: Vec<String>,
}

impl fmt::Debug for RedactionPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RedactionPolicy")
            .field("max_text_bytes", &self.max_text_bytes)
            .field("literal_secrets", &self.literal_secrets.len())
            .finish()
    }
}

impl RedactionPolicy {
    /// Default policy: pattern redaction with the default text ceiling.
    pub fn new() -> Self {
        Self {
            max_text_bytes: DEFAULT_MAX_TEXT_BYTES,
            literal_secrets: Vec::new(),
        }
    }

    /// Overrides the per-field ceiling; clamped to [16, [`MAX_PAYLOAD_BYTES`] / 2].
    pub fn with_max_text_bytes(mut self, max_text_bytes: usize) -> Self {
        self.max_text_bytes = max_text_bytes.clamp(16, MAX_PAYLOAD_BYTES / 2);
        self
    }

    /// Adds literal values that must never appear in a persisted payload.
    /// Values shorter than 8 bytes are ignored to avoid scrubbing common text.
    pub fn with_literal_secrets<I, S>(mut self, secrets: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.literal_secrets.extend(
            secrets
                .into_iter()
                .map(Into::into)
                .filter(|secret| secret.len() >= 8),
        );
        self
    }

    /// Per-field text ceiling.
    pub fn max_text_bytes(&self) -> usize {
        self.max_text_bytes
    }

    /// Strips control characters, redacts, and clamps an identifier-like field.
    fn short(&self, value: &str) -> String {
        truncate_at_char_boundary(&self.redact(&strip_control_characters(value)), 256).to_owned()
    }

    /// Returns `value` with literal secrets and secret-like tokens replaced.
    pub fn redact(&self, value: &str) -> String {
        let mut text = value.to_owned();
        for secret in &self.literal_secrets {
            if text.contains(secret.as_str()) {
                text = text.replace(secret.as_str(), REDACTED);
            }
        }
        let text = redact_pem_blocks(&text);
        let text = redact_assignments(&text);
        redact_tokens(&text)
    }
}

/// Replaces `-----BEGIN ... PRIVATE KEY-----` blocks (and any other PEM body)
/// with the placeholder.
fn redact_pem_blocks(text: &str) -> String {
    const BEGIN: &str = "-----BEGIN";
    const END: &str = "-----END";
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(BEGIN) {
        output.push_str(&rest[..start]);
        let after_begin = &rest[start..];
        match after_begin.find(END) {
            Some(end_offset) => {
                let tail = &after_begin[end_offset..];
                let close = tail.find("-----").map(|first| {
                    tail[first + 5..]
                        .find("-----")
                        .map(|second| first + 5 + second + 5)
                        .unwrap_or(tail.len())
                });
                output.push_str(REDACTED);
                rest = &tail[close.unwrap_or(tail.len())..];
            }
            None => {
                output.push_str(REDACTED);
                rest = "";
            }
        }
    }
    output.push_str(rest);
    output
}

const SECRET_KEY_MARKERS: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "access_token",
    "refresh_token",
    "auth_token",
    "bearer_token",
    "client_secret",
    "private_key",
    "privatekey",
    "password",
    "passwd",
    "secret",
    "token",
    "authorization",
    "cookie",
    "nsec",
];

fn is_secret_key(key: &str) -> bool {
    let normalized = key
        .trim_matches(|character: char| !character.is_ascii_alphanumeric())
        .to_ascii_lowercase();
    if normalized.is_empty() || normalized.len() > 64 {
        return false;
    }
    SECRET_KEY_MARKERS
        .iter()
        .any(|marker| normalized.ends_with(marker) || normalized == *marker)
}

/// Redacts values of `key=value`, `key: value`, and `"key": "value"` pairs
/// whose key looks like a secret. Unquoted values end at whitespace, quotes,
/// commas, semicolons, or closing braces; quoted values end at the matching
/// quote, so `"Authorization": "Bearer x"` loses the whole value.
fn redact_assignments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut copied = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'=' && bytes[index] != b':' {
            index += 1;
            continue;
        }
        let mut key_end = index;
        if key_end > 0 && matches!(bytes[key_end - 1], b'"' | b'\'') {
            key_end -= 1;
        }
        let mut key_start = key_end;
        while key_start > copied
            && (bytes[key_start - 1].is_ascii_alphanumeric()
                || matches!(bytes[key_start - 1], b'_' | b'-' | b'.'))
        {
            key_start -= 1;
        }
        if !is_secret_key(&text[key_start..key_end]) {
            index += 1;
            continue;
        }
        let mut value_start = index + 1;
        while value_start < bytes.len() && matches!(bytes[value_start], b' ' | b'\t') {
            value_start += 1;
        }
        let quote = (value_start < bytes.len() && matches!(bytes[value_start], b'"' | b'\''))
            .then(|| bytes[value_start]);
        let content_start = value_start + usize::from(quote.is_some());
        let mut value_end = content_start;
        while value_end < bytes.len() {
            let current = bytes[value_end];
            let stop = match quote {
                Some(quote) => current == quote,
                None => matches!(
                    current,
                    b' ' | b'\t' | b'\n' | b'\r' | b',' | b';' | b'}' | b'"' | b'\''
                ),
            };
            if stop {
                break;
            }
            value_end += 1;
        }
        // `Authorization: Bearer <token>`: the scheme word alone is not the
        // secret; extend an unquoted value to cover the following token.
        if quote.is_none() {
            let scheme = &text[content_start..value_end];
            if scheme.eq_ignore_ascii_case("bearer") || scheme.eq_ignore_ascii_case("basic") {
                let mut next_start = value_end;
                while next_start < bytes.len() && matches!(bytes[next_start], b' ' | b'\t') {
                    next_start += 1;
                }
                let mut next_end = next_start;
                while next_end < bytes.len()
                    && !matches!(
                        bytes[next_end],
                        b' ' | b'\t' | b'\n' | b'\r' | b',' | b';' | b'}' | b'"' | b'\''
                    )
                {
                    next_end += 1;
                }
                if next_end > next_start {
                    value_end = next_end;
                }
            }
        }
        if value_end == content_start {
            index += 1;
            continue;
        }
        output.push_str(&text[copied..content_start]);
        output.push_str(REDACTED);
        copied = value_end;
        index = value_end;
    }
    output.push_str(&text[copied..]);
    output
}

/// True when a whitespace-delimited token has the shape of a bearer token,
/// API key, Nostr secret key, or JWT.
fn looks_like_secret_token(token: &str) -> bool {
    let trimmed = token.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | ',' | ';' | ')' | '(' | '[' | ']' | '{' | '}' | '<' | '>'
        )
    });
    if trimmed.len() < 12 {
        return trimmed.starts_with("nsec1") && trimmed.len() > 5;
    }
    let lower = trimmed.to_ascii_lowercase();
    const PREFIXES: &[&str] = &[
        "sk-",
        "sk_live_",
        "sk_test_",
        "rk_live_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xapp-",
        "nsec1",
        "akia",
        "aiza",
        "ya29.",
        "glpat-",
        "npm_",
        "shpat_",
        "sq0atp-",
        "hf_",
    ];
    if PREFIXES.iter().any(|prefix| lower.starts_with(prefix)) {
        return true;
    }
    // JWT: three base64url segments starting with `eyJ`.
    if trimmed.starts_with("eyJ") && trimmed.split('.').count() == 3 {
        return true;
    }
    false
}

/// Redacts secret-shaped tokens and `Bearer <token>` pairs.
fn redact_tokens(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut previous_was_bearer = false;
    let mut rest = text;
    while !rest.is_empty() {
        let token_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let token = &rest[..token_end];
        if token.is_empty() {
            let whitespace_end = rest
                .find(|character: char| !character.is_whitespace())
                .unwrap_or(rest.len());
            output.push_str(&rest[..whitespace_end]);
            rest = &rest[whitespace_end..];
            continue;
        }
        if previous_was_bearer || looks_like_secret_token(token) {
            output.push_str(REDACTED);
        } else {
            output.push_str(token);
        }
        previous_was_bearer = token.eq_ignore_ascii_case("bearer");
        rest = &rest[token_end..];
    }
    output
}

/// A normalized event ready for `run_events`, with its dense sequence.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunEvent {
    /// Run this event belongs to.
    pub run_id: Uuid,
    /// Dense per-run sequence; `None` until the repository assigns it.
    pub sequence: Option<i64>,
    /// When the runtime observed the event.
    pub occurred_at: DateTime<Utc>,
    /// Adapter cursor for resume-from-cursor ingestion; unique per run.
    pub runtime_cursor: Option<String>,
    /// Object-storage reference for offloaded large content.
    pub artifact_ref: Option<String>,
    /// Normalized, bounded, redacted payload.
    pub payload: RunEventPayload,
}

/// Why an event cannot be persisted even after normalization.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RunEventError {
    /// Serialized payload still exceeds [`MAX_PAYLOAD_BYTES`].
    #[error("run event payload is {bytes} bytes, above the persistence ceiling")]
    PayloadTooLarge {
        /// Serialized size.
        bytes: usize,
    },
    /// Runtime cursor is empty, oversized, or contains NUL.
    #[error("runtime cursor must be 1..=512 bytes without NUL")]
    InvalidCursor,
    /// Artifact reference is empty, oversized, or contains NUL.
    #[error("artifact ref must be 1..=1024 bytes without NUL")]
    InvalidArtifactRef,
    /// A payload text field still contains NUL, which PostgreSQL `jsonb`
    /// rejects; the event was not normalized.
    #[error("run event payload contains a NUL character")]
    NulInPayload,
    /// The payload could not be serialized.
    #[error("run event payload could not be serialized")]
    Serialization,
}

impl RunEvent {
    /// Builds a normalized event from raw adapter output.
    pub fn normalize(
        run_id: Uuid,
        occurred_at: DateTime<Utc>,
        runtime_cursor: Option<String>,
        payload: &RunEventPayload,
        policy: &RedactionPolicy,
    ) -> Result<Self, RunEventError> {
        let event = Self {
            run_id,
            sequence: None,
            occurred_at,
            runtime_cursor,
            artifact_ref: None,
            payload: payload.normalize(policy),
        };
        event.validate()?;
        Ok(event)
    }

    /// Serializes the payload as the compact JSON object stored in the row.
    pub fn payload_json(&self) -> Result<serde_json::Value, RunEventError> {
        serde_json::to_value(&self.payload).map_err(|_| RunEventError::Serialization)
    }

    /// Checks the persistence contract: payload size, absence of NUL (which
    /// PostgreSQL `jsonb` and `text` reject, failing the whole batch on
    /// every replay), and cursor and artifact bounds.
    pub fn validate(&self) -> Result<(), RunEventError> {
        let json =
            serde_json::to_string(&self.payload).map_err(|_| RunEventError::Serialization)?;
        let bytes = json.len();
        if bytes > MAX_PAYLOAD_BYTES {
            return Err(RunEventError::PayloadTooLarge { bytes });
        }
        if json.contains("\\u0000") {
            return Err(RunEventError::NulInPayload);
        }
        if self
            .runtime_cursor
            .as_deref()
            .is_some_and(|cursor| cursor.is_empty() || cursor.len() > 512 || cursor.contains('\0'))
        {
            return Err(RunEventError::InvalidCursor);
        }
        if self.artifact_ref.as_deref().is_some_and(|artifact| {
            artifact.is_empty() || artifact.len() > 1024 || artifact.contains('\0')
        }) {
            return Err(RunEventError::InvalidArtifactRef);
        }
        Ok(())
    }

    /// Event type column value.
    pub fn event_type(&self) -> RunEventType {
        self.payload.event_type()
    }
}
