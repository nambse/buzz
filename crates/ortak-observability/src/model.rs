//! Stable Activity read models (Architecture v0 §4.6, §9, §11).
//!
//! These are the shapes a client renders. They carry semantics first: closed
//! vocabularies, typed outcomes, bounded/redacted text with explicit
//! markers, and *presence* of adapter references and runtime cursors rather
//! than their contents. Nothing here contains a credential reference, a
//! runtime cursor, a runtime run reference, or a Buzz community id.

use chrono::{DateTime, Utc};
use ortak_control::run_event::{
    BoundedText, DeliveryIntentKind, FileChangeKind, RunEventPayload, RunEventType, TerminalStream,
    UsageTelemetry, REDACTED,
};
use ortak_domain::EmployeeId;
use ortak_runtime::RunStatus;
use serde::Serialize;
use uuid::Uuid;

/// One row of the company-scoped run list; also the header of a run detail.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RunHeader {
    /// Durable run id.
    pub run_id: Uuid,
    /// Employee that executed the run.
    pub employee_id: EmployeeId,
    /// Immutable revision the run pinned when it was created.
    pub employee_revision_id: Uuid,
    /// Dispatch provenance, each part present only when recorded.
    pub provenance: RunProvenance,
    /// Runtime adapter and whether a runtime correlation exists.
    pub runtime: RuntimeReference,
    /// Durable lifecycle status.
    pub status: RunStatus,
    /// Typed outcome derived from the status and terminal columns.
    pub outcome: RunOutcome,
    /// Lifecycle timestamps.
    pub timing: RunTiming,
    /// Newest durable event, when the run has any.
    pub last_event: Option<LastEventSummary>,
}

/// Where a run came from. Conversational runs carry the decision that woke
/// them plus its message and thread root; Work-originated runs carry the
/// work item instead.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RunProvenance {
    /// Dispatching routing decision.
    pub routing_decision_id: Option<Uuid>,
    /// Lowercase hex id of the Office message that woke the run.
    pub message_id: Option<String>,
    /// Lowercase hex id of the thread root the run belongs to.
    pub root_message_id: Option<String>,
    /// Attached work item.
    pub work_item_id: Option<Uuid>,
}

/// Runtime adapter identity plus correlation *presence*. The adapter-side
/// run reference itself is never part of a read model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuntimeReference {
    /// Runtime adapter name (`^[a-z][a-z0-9_-]{0,63}$`).
    pub adapter: String,
    /// True once the run is correlated with a runtime run.
    pub has_run_ref: bool,
}

/// Typed completion state of a run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunOutcome {
    /// The run has not reached a terminal status.
    Pending,
    /// Completed with the runtime's typed delivery intent.
    Completed {
        /// Delivery intent chosen by the runtime.
        delivery_intent: DeliveryIntentKind,
    },
    /// Failed; the bounded message is available on the run detail only.
    Failed {
        /// Stable error code, bounded again at read time.
        code: Option<String>,
    },
    /// Cancelled; the bounded reason is available on the run detail only.
    Cancelled,
}

/// Lifecycle timestamps of a run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RunTiming {
    /// Row creation.
    pub queued_at: DateTime<Utc>,
    /// Runtime correlation.
    pub started_at: Option<DateTime<Utc>>,
    /// Terminal transition.
    pub finished_at: Option<DateTime<Utc>>,
    /// Last durable change to the run row.
    pub updated_at: DateTime<Utc>,
}

/// Compact view of the newest durable event of a run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct LastEventSummary {
    /// Dense per-run sequence.
    pub sequence: i64,
    /// Closed event type.
    pub event_type: RunEventType,
    /// When the runtime observed it.
    pub occurred_at: DateTime<Utc>,
    /// When it became durable.
    pub recorded_at: DateTime<Utc>,
}

/// One run with its terminal text and aggregate summary.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RunDetail {
    /// Header shared with the run list.
    pub run: RunHeader,
    /// Bounded, redacted failure message when the run failed.
    pub error_message: Option<String>,
    /// Bounded, redacted cancellation reason when the run was cancelled.
    pub cancel_reason: Option<String>,
    /// Aggregate summary of the run's durable events.
    pub summary: RunSummary,
}

/// Aggregate counts over a run's durable events, computed with a fixed
/// number of company-scoped queries (never per event).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct RunSummary {
    /// Number of durable events.
    pub event_count: u64,
    /// Newest event.
    pub last_event: Option<LastEventSummary>,
    /// Tool-call counts.
    pub tools: ToolSummary,
    /// Terminal-command counts.
    pub terminal: TerminalSummary,
    /// File-change counts.
    pub files: FileSummary,
    /// Assistant text fragments.
    pub assistant_fragments: u64,
    /// `run.waiting` transitions.
    pub waits: u64,
    /// Non-terminal `error.raised` events.
    pub errors_raised: u64,
    /// Usage totals, present when at least one usage record exists. Never
    /// authoritative for billing.
    pub usage: Option<UsageTotals>,
    /// Terminal facts, present once the run reached a terminal status.
    pub terminal_state: Option<TerminalState>,
}

/// Tool-call counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ToolSummary {
    /// `tool_call.started` events.
    pub started: u64,
    /// `tool_call.completed` events.
    pub completed: u64,
    /// `tool_call.failed` events.
    pub failed: u64,
}

impl ToolSummary {
    /// Calls that started and have neither completed nor failed.
    pub fn open(&self) -> u64 {
        self.started
            .saturating_sub(self.completed)
            .saturating_sub(self.failed)
    }
}

/// Terminal-command counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct TerminalSummary {
    /// `terminal.started` events.
    pub commands: u64,
    /// `terminal.completed` events.
    pub completed: u64,
    /// Completed commands whose exit code was reported and non-zero.
    pub nonzero_exits: u64,
    /// Completed commands without an exit code (signal or runtime loss).
    pub abnormal_exits: u64,
    /// `terminal.output` chunks.
    pub output_chunks: u64,
}

/// File-change counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct FileSummary {
    /// All `file.changed` events.
    pub changes: u64,
    /// Reads.
    pub read: u64,
    /// Creations.
    pub created: u64,
    /// Modifications.
    pub modified: u64,
    /// Deletions.
    pub deleted: u64,
}

/// Summed usage telemetry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct UsageTotals {
    /// `usage.recorded` events.
    pub records: u64,
    /// Input tokens.
    pub input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
    /// Cached input tokens.
    pub cached_input_tokens: u64,
    /// Reasoning tokens.
    pub reasoning_tokens: u64,
}

/// Terminal facts of a finished run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TerminalState {
    /// Terminal status.
    pub status: RunStatus,
    /// Typed outcome.
    pub outcome: RunOutcome,
    /// When the run finished.
    pub finished_at: Option<DateTime<Utc>>,
}

/// One durable event projected into typed Activity semantics.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ActivityEntry {
    /// Dense per-run sequence; the incremental paging cursor.
    pub sequence: i64,
    /// Closed event type.
    pub event_type: RunEventType,
    /// When the runtime observed the event.
    pub occurred_at: DateTime<Utc>,
    /// When the event became durable.
    pub recorded_at: DateTime<Utc>,
    /// True when the event carries a runtime cursor (contents never shown).
    pub has_runtime_cursor: bool,
    /// Object-storage reference for offloaded large content.
    pub artifact_ref: Option<String>,
    /// True when any text in the payload was redacted before persistence.
    pub redacted: bool,
    /// True when any text in the payload was truncated before persistence.
    pub truncated: bool,
    /// Typed semantics.
    pub activity: Activity,
    /// The bounded, redacted normalized payload, present only when the
    /// caller asked for raw events. The runtime run reference is scrubbed.
    pub raw: Option<RunEventPayload>,
}

/// Typed Activity semantics of one event.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Activity {
    /// Run lifecycle transition.
    Lifecycle {
        /// Phase reached.
        phase: LifecyclePhase,
    },
    /// Assistant text fragment.
    AssistantOutput {
        /// Turn index within the run.
        turn: u32,
        /// Fragment.
        text: ActivityText,
    },
    /// Tool invocation.
    ToolCall {
        /// Adapter-side call correlation id.
        call_id: String,
        /// Phase.
        phase: ToolCallPhase,
    },
    /// Terminal command.
    Terminal {
        /// Adapter-side command correlation id.
        command_id: String,
        /// Phase.
        phase: TerminalPhase,
    },
    /// Workspace file change.
    FileChange {
        /// Path as reported by the runtime (already bounded and redacted).
        path: String,
        /// Kind of change.
        change: FileChangeKind,
        /// Diff or content summary.
        summary: ActivityText,
        /// Resulting size, if known.
        bytes: Option<u64>,
    },
    /// Usage telemetry.
    Usage {
        /// Telemetry.
        usage: UsageTelemetry,
    },
    /// Typed delivery intent.
    DeliveryIntent {
        /// Intent.
        intent: DeliveryIntentKind,
        /// Explicit target reference for `channel` intents.
        target_ref: Option<String>,
    },
    /// Non-terminal error.
    Error {
        /// Stable error code.
        code: String,
        /// Message.
        message: ActivityText,
        /// Whether the runtime intends to retry.
        retryable: bool,
    },
}

/// Lifecycle phase carried by a lifecycle event.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum LifecyclePhase {
    /// Run accepted.
    Queued,
    /// Runtime started the run.
    Started {
        /// True when the runtime reported a run reference (never shown).
        has_runtime_run_ref: bool,
    },
    /// Run is waiting.
    Waiting {
        /// Stable reason code such as `approval` or `input`.
        reason: String,
        /// Detail.
        detail: ActivityText,
    },
    /// Run completed.
    Completed {
        /// Delivery intent.
        delivery_intent: DeliveryIntentKind,
    },
    /// Run failed.
    Failed {
        /// Stable error code.
        code: String,
        /// Message.
        message: ActivityText,
    },
    /// Run cancelled.
    Cancelled {
        /// Reason.
        reason: ActivityText,
    },
}

/// Tool-call phase.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum ToolCallPhase {
    /// Invocation began.
    Started {
        /// Tool name.
        tool: String,
        /// Argument summary.
        arguments: ActivityText,
    },
    /// Invocation returned.
    Completed {
        /// Result summary.
        result: ActivityText,
    },
    /// Invocation failed.
    Failed {
        /// Error.
        error: ActivityText,
    },
}

/// Terminal-command phase.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum TerminalPhase {
    /// Command started.
    Started {
        /// Command line.
        command: ActivityText,
        /// Working directory reference, if reported.
        cwd: Option<String>,
    },
    /// Output chunk.
    Output {
        /// Stream.
        stream: TerminalStream,
        /// Chunk.
        chunk: ActivityText,
    },
    /// Command exited.
    Completed {
        /// Exit code when the process exited normally.
        exit_code: Option<i32>,
    },
}

/// Bounded, redacted text with explicit markers.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ActivityText {
    /// Retained text.
    pub text: String,
    /// True when the original exceeded the persistence ceiling.
    pub truncated: bool,
    /// Byte length of the original when truncated.
    pub original_bytes: Option<u64>,
    /// SHA-256 (hex) of the original when truncated.
    pub original_sha256: Option<String>,
    /// True when secret-like material was replaced before persistence.
    pub redacted: bool,
}

impl ActivityText {
    /// Views persisted bounded text, marking redaction.
    pub fn from_bounded(bounded: &BoundedText) -> Self {
        Self {
            text: bounded.text.clone(),
            truncated: bounded.truncated,
            original_bytes: bounded.original_bytes,
            original_sha256: bounded.original_sha256.clone(),
            redacted: contains_redaction(&bounded.text),
        }
    }
}

/// True when text carries the persistence redaction placeholder.
pub fn contains_redaction(text: &str) -> bool {
    text.contains(REDACTED)
}
