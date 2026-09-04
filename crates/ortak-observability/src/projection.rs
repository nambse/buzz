//! Pure projection from durable rows to Activity read models.
//!
//! Everything here is deterministic and database-free so it can be tested
//! without infrastructure and reused by any storage adapter:
//!
//! - [`RunEventRecord`] is the closed view of one `run_events` row. Building
//!   it fails when the event type is outside the vocabulary, when the
//!   payload does not deserialize as the normalized
//!   [`RunEventPayload`], or when the column type and the payload tag
//!   disagree.
//! - [`project_entry`] turns a record into a typed [`ActivityEntry`],
//!   preserving order metadata, cursor/artifact presence, and
//!   redaction/truncation markers, and never surfacing the runtime run
//!   reference.
//! - [`assemble_event_page`] applies the bounded page and detects gaps.
//! - [`SummaryFacts`] is the aggregate the SQL adapter computes in one
//!   statement; [`SummaryFacts::fold`] is the same aggregate over records,
//!   which keeps the two computations checkable against each other.

use chrono::{DateTime, Utc};
use ortak_control::adapter::truncate_at_char_boundary;
use ortak_control::run_event::{
    strip_control_characters, DeliveryIntentKind, FileChangeKind, RedactionPolicy, RunEventPayload,
    RunEventType, REDACTED,
};
use ortak_runtime::RunStatus;

use crate::error::{ActivityError, Result};
use crate::model::{
    contains_redaction, Activity, ActivityEntry, ActivityText, FileSummary, LastEventSummary,
    LifecyclePhase, RunHeader, RunOutcome, RunSummary, TerminalPhase, TerminalState,
    TerminalSummary, ToolCallPhase, ToolSummary, UsageTotals,
};
use crate::query::{RunEventPage, RunEventsQuery, SequenceGap};

/// Read-time ceiling for `runs.error_message` and `runs.cancel_reason`.
pub const MAX_ROW_TEXT_BYTES: usize = 2048;
/// Read-time ceiling for `runs.error_code`.
pub const MAX_ERROR_CODE_BYTES: usize = 64;

/// Bounds and redacts free text stored on the run row itself. The writer
/// already bounded it, but a row written by an older path is not trusted:
/// control characters are stripped, secret-shaped material is redacted, and
/// the result is clamped to `max_bytes`.
pub fn bound_row_text(value: &str, max_bytes: usize) -> String {
    let redacted = RedactionPolicy::new().redact(&strip_control_characters(value));
    truncate_at_char_boundary(&redacted, max_bytes).to_owned()
}

/// Parses `runs.delivery_intent`; fails closed on any other value.
pub fn parse_delivery_intent(value: &str) -> Result<DeliveryIntentKind> {
    [
        DeliveryIntentKind::Reply,
        DeliveryIntentKind::Channel,
        DeliveryIntentKind::Silent,
    ]
    .into_iter()
    .find(|candidate| candidate.as_str() == value)
    .ok_or_else(|| ActivityError::InvalidRecord {
        detail: format!("runs.delivery_intent holds {value:?}"),
    })
}

/// Parses `runs.status`; fails closed on any other value.
pub fn parse_run_status(value: &str) -> Result<RunStatus> {
    RunStatus::parse(value).ok_or_else(|| ActivityError::InvalidRecord {
        detail: format!("runs.status holds {value:?}"),
    })
}

/// Derives the typed outcome from the run's terminal columns. A completed
/// run without a delivery intent violates the schema and fails closed.
pub fn derive_outcome(
    status: RunStatus,
    delivery_intent: Option<&str>,
    error_code: Option<&str>,
) -> Result<RunOutcome> {
    Ok(match status {
        RunStatus::Completed => {
            let intent = delivery_intent.ok_or_else(|| ActivityError::InvalidRecord {
                detail: "completed run has no delivery intent".to_owned(),
            })?;
            RunOutcome::Completed {
                delivery_intent: parse_delivery_intent(intent)?,
            }
        }
        RunStatus::Failed => RunOutcome::Failed {
            code: error_code.map(|code| bound_row_text(code, MAX_ERROR_CODE_BYTES)),
        },
        RunStatus::Cancelled => RunOutcome::Cancelled,
        RunStatus::Queued | RunStatus::Running | RunStatus::Waiting => RunOutcome::Pending,
    })
}

/// Terminal facts of a header, `None` while the run is live.
pub fn terminal_state(header: &RunHeader) -> Option<TerminalState> {
    header.status.is_terminal().then(|| TerminalState {
        status: header.status,
        outcome: header.outcome.clone(),
        finished_at: header.timing.finished_at,
    })
}

/// Closed view of one durable `run_events` row.
#[derive(Clone, Debug, PartialEq)]
pub struct RunEventRecord {
    /// Dense per-run sequence.
    pub sequence: i64,
    /// When the runtime observed the event.
    pub occurred_at: DateTime<Utc>,
    /// When the row became durable.
    pub recorded_at: DateTime<Utc>,
    /// True when the row carries a runtime cursor. The cursor itself is
    /// never loaded into a record.
    pub has_runtime_cursor: bool,
    /// Object-storage reference for offloaded content.
    pub artifact_ref: Option<String>,
    /// Normalized payload as persisted.
    pub payload: RunEventPayload,
}

impl RunEventRecord {
    /// Builds a record from stored column values, failing closed on an
    /// unknown event type, an unreadable payload, or a type/payload mismatch.
    pub fn from_stored(
        sequence: i64,
        event_type: &str,
        occurred_at: DateTime<Utc>,
        recorded_at: DateTime<Utc>,
        has_runtime_cursor: bool,
        artifact_ref: Option<String>,
        payload: serde_json::Value,
    ) -> Result<Self> {
        let declared =
            RunEventType::parse(event_type).ok_or_else(|| ActivityError::InvalidRecord {
                detail: format!(
                    "run_events.event_type holds {event_type:?} at sequence {sequence}"
                ),
            })?;
        let payload: RunEventPayload =
            serde_json::from_value(payload).map_err(|_| ActivityError::InvalidRecord {
                detail: format!(
                    "run_events.payload is not a normalized {declared} at sequence {sequence}"
                ),
            })?;
        if payload.event_type() != declared {
            return Err(ActivityError::InvalidRecord {
                detail: format!(
                    "run_events.event_type {declared} disagrees with payload {} at sequence {sequence}",
                    payload.event_type()
                ),
            });
        }
        Ok(Self {
            sequence,
            occurred_at,
            recorded_at,
            has_runtime_cursor,
            artifact_ref,
            payload,
        })
    }

    /// Closed event type of the payload.
    pub fn event_type(&self) -> RunEventType {
        self.payload.event_type()
    }

    /// Compact summary of this record as the newest event.
    pub fn last_event_summary(&self) -> LastEventSummary {
        LastEventSummary {
            sequence: self.sequence,
            event_type: self.event_type(),
            occurred_at: self.occurred_at,
            recorded_at: self.recorded_at,
        }
    }
}

/// The normalized payload as a client may see it on demand: identical to
/// the persisted payload except that the runtime run reference is replaced
/// by the redaction placeholder, so no adapter-side correlation id leaves
/// the server.
pub fn raw_view(payload: &RunEventPayload) -> RunEventPayload {
    match payload {
        RunEventPayload::RunStarted { .. } => RunEventPayload::RunStarted {
            runtime_run_ref: REDACTED.to_owned(),
        },
        other => other.clone(),
    }
}

/// Projects one record into typed Activity semantics.
pub fn project_entry(record: &RunEventRecord, include_raw: bool) -> ActivityEntry {
    let (activity, markers) = project_activity(&record.payload);
    ActivityEntry {
        sequence: record.sequence,
        event_type: record.event_type(),
        occurred_at: record.occurred_at,
        recorded_at: record.recorded_at,
        has_runtime_cursor: record.has_runtime_cursor,
        artifact_ref: record.artifact_ref.clone(),
        redacted: markers.redacted,
        truncated: markers.truncated,
        activity,
        raw: include_raw.then(|| raw_view(&record.payload)),
    }
}

#[derive(Default)]
struct Markers {
    redacted: bool,
    truncated: bool,
}

impl Markers {
    fn text(&mut self, text: &ActivityText) {
        self.redacted |= text.redacted;
        self.truncated |= text.truncated;
    }

    fn short(&mut self, value: &str) {
        self.redacted |= contains_redaction(value);
    }

    fn optional(&mut self, value: Option<&str>) {
        if let Some(value) = value {
            self.short(value);
        }
    }
}

fn project_activity(payload: &RunEventPayload) -> (Activity, Markers) {
    let mut markers = Markers::default();
    let mut text = |bounded| {
        let text = ActivityText::from_bounded(bounded);
        markers.text(&text);
        text
    };
    let activity = match payload {
        RunEventPayload::RunQueued => Activity::Lifecycle {
            phase: LifecyclePhase::Queued,
        },
        RunEventPayload::RunStarted { runtime_run_ref } => Activity::Lifecycle {
            phase: LifecyclePhase::Started {
                has_runtime_run_ref: !runtime_run_ref.is_empty(),
            },
        },
        RunEventPayload::RunWaiting { reason, detail } => {
            let detail = text(detail);
            markers.short(reason);
            Activity::Lifecycle {
                phase: LifecyclePhase::Waiting {
                    reason: reason.clone(),
                    detail,
                },
            }
        }
        RunEventPayload::RunCompleted { delivery_intent } => Activity::Lifecycle {
            phase: LifecyclePhase::Completed {
                delivery_intent: *delivery_intent,
            },
        },
        RunEventPayload::RunFailed { code, message } => {
            let message = text(message);
            markers.short(code);
            Activity::Lifecycle {
                phase: LifecyclePhase::Failed {
                    code: code.clone(),
                    message,
                },
            }
        }
        RunEventPayload::RunCancelled { reason } => {
            let reason = text(reason);
            Activity::Lifecycle {
                phase: LifecyclePhase::Cancelled { reason },
            }
        }
        RunEventPayload::AssistantDelta { turn, delta } => {
            let delta = text(delta);
            Activity::AssistantOutput {
                turn: *turn,
                text: delta,
            }
        }
        RunEventPayload::ToolCallStarted {
            call_id,
            tool,
            arguments,
        } => {
            let arguments = text(arguments);
            markers.short(call_id);
            markers.short(tool);
            Activity::ToolCall {
                call_id: call_id.clone(),
                phase: ToolCallPhase::Started {
                    tool: tool.clone(),
                    arguments,
                },
            }
        }
        RunEventPayload::ToolCallCompleted { call_id, result } => {
            let result = text(result);
            markers.short(call_id);
            Activity::ToolCall {
                call_id: call_id.clone(),
                phase: ToolCallPhase::Completed { result },
            }
        }
        RunEventPayload::ToolCallFailed { call_id, error } => {
            let error = text(error);
            markers.short(call_id);
            Activity::ToolCall {
                call_id: call_id.clone(),
                phase: ToolCallPhase::Failed { error },
            }
        }
        RunEventPayload::TerminalStarted {
            command_id,
            command,
            cwd,
        } => {
            let command = text(command);
            markers.short(command_id);
            markers.optional(cwd.as_deref());
            Activity::Terminal {
                command_id: command_id.clone(),
                phase: TerminalPhase::Started {
                    command,
                    cwd: cwd.clone(),
                },
            }
        }
        RunEventPayload::TerminalOutput {
            command_id,
            stream,
            chunk,
        } => {
            let chunk = text(chunk);
            markers.short(command_id);
            Activity::Terminal {
                command_id: command_id.clone(),
                phase: TerminalPhase::Output {
                    stream: *stream,
                    chunk,
                },
            }
        }
        RunEventPayload::TerminalCompleted {
            command_id,
            exit_code,
        } => {
            markers.short(command_id);
            Activity::Terminal {
                command_id: command_id.clone(),
                phase: TerminalPhase::Completed {
                    exit_code: *exit_code,
                },
            }
        }
        RunEventPayload::FileChanged {
            path,
            change,
            summary,
            bytes,
        } => {
            let summary = text(summary);
            markers.short(path);
            Activity::FileChange {
                path: path.clone(),
                change: *change,
                summary,
                bytes: *bytes,
            }
        }
        RunEventPayload::UsageRecorded { usage } => {
            markers.optional(usage.model.as_deref());
            Activity::Usage {
                usage: usage.clone(),
            }
        }
        RunEventPayload::ErrorRaised {
            code,
            message,
            retryable,
        } => {
            let message = text(message);
            markers.short(code);
            Activity::Error {
                code: code.clone(),
                message,
                retryable: *retryable,
            }
        }
        RunEventPayload::DeliveryIntent { intent, target_ref } => {
            markers.optional(target_ref.as_deref());
            Activity::DeliveryIntent {
                intent: *intent,
                target_ref: target_ref.clone(),
            }
        }
    };
    (activity, markers)
}

/// Builds one event page from records read in ascending sequence order
/// with `sequence > query.start_after()`.
///
/// The adapter fetches at most `page_size + 1` records; the extra record
/// only signals `has_more`. Entries must be dense from the cursor: at the
/// first hole or reorder the page stops, `gap` names the discontinuity, and
/// `has_more` is true because durable events exist beyond the returned
/// prefix.
pub fn assemble_event_page(query: &RunEventsQuery, records: Vec<RunEventRecord>) -> RunEventPage {
    let page_size = query.page_size() as usize;
    let mut has_more = records.len() > page_size;
    let mut entries = Vec::with_capacity(records.len().min(page_size));
    let mut gap = None;
    let mut expected = query.start_after().saturating_add(1);
    for record in records.into_iter().take(page_size) {
        if record.sequence != expected {
            gap = Some(SequenceGap {
                expected,
                found: record.sequence,
            });
            has_more = true;
            break;
        }
        entries.push(project_entry(&record, query.include_raw));
        expected = expected.saturating_add(1);
    }
    let next_after_sequence = entries
        .last()
        .map(|entry| entry.sequence)
        .or(query.after_sequence);
    RunEventPage {
        entries,
        next_after_sequence,
        has_more,
        gap,
    }
}

/// Aggregate counters over a run's events. The SQL adapter computes these
/// in one statement; [`SummaryFacts::fold`] computes them from records.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SummaryFacts {
    /// Number of events.
    pub event_count: u64,
    /// Newest event.
    pub last_event: Option<LastEventSummary>,
    /// Tool-call counts.
    pub tools: ToolSummary,
    /// Terminal-command counts.
    pub terminal: TerminalSummary,
    /// File-change counts.
    pub files: FileSummary,
    /// `assistant.delta` events.
    pub assistant_fragments: u64,
    /// `run.waiting` events.
    pub waits: u64,
    /// `error.raised` events.
    pub errors_raised: u64,
    /// Usage totals; `records == 0` means none were recorded.
    pub usage: UsageTotals,
}

impl SummaryFacts {
    /// Folds records in any order; `last_event` is the highest sequence.
    pub fn fold<'a>(records: impl IntoIterator<Item = &'a RunEventRecord>) -> Self {
        let mut facts = Self::default();
        for record in records {
            facts.event_count += 1;
            if facts
                .last_event
                .is_none_or(|last| record.sequence > last.sequence)
            {
                facts.last_event = Some(record.last_event_summary());
            }
            match &record.payload {
                RunEventPayload::AssistantDelta { .. } => facts.assistant_fragments += 1,
                RunEventPayload::RunWaiting { .. } => facts.waits += 1,
                RunEventPayload::ErrorRaised { .. } => facts.errors_raised += 1,
                RunEventPayload::ToolCallStarted { .. } => facts.tools.started += 1,
                RunEventPayload::ToolCallCompleted { .. } => facts.tools.completed += 1,
                RunEventPayload::ToolCallFailed { .. } => facts.tools.failed += 1,
                RunEventPayload::TerminalStarted { .. } => facts.terminal.commands += 1,
                RunEventPayload::TerminalOutput { .. } => facts.terminal.output_chunks += 1,
                RunEventPayload::TerminalCompleted { exit_code, .. } => {
                    facts.terminal.completed += 1;
                    match exit_code {
                        Some(0) => {}
                        Some(_) => facts.terminal.nonzero_exits += 1,
                        None => facts.terminal.abnormal_exits += 1,
                    }
                }
                RunEventPayload::FileChanged { change, .. } => {
                    facts.files.changes += 1;
                    match change {
                        FileChangeKind::Read => facts.files.read += 1,
                        FileChangeKind::Created => facts.files.created += 1,
                        FileChangeKind::Modified => facts.files.modified += 1,
                        FileChangeKind::Deleted => facts.files.deleted += 1,
                    }
                }
                RunEventPayload::UsageRecorded { usage } => {
                    let totals = &mut facts.usage;
                    totals.records += 1;
                    let add = |total: &mut u64, value: Option<u64>| {
                        *total = total.saturating_add(value.unwrap_or(0));
                    };
                    add(&mut totals.input_tokens, usage.input_tokens);
                    add(&mut totals.output_tokens, usage.output_tokens);
                    add(&mut totals.cached_input_tokens, usage.cached_input_tokens);
                    add(&mut totals.reasoning_tokens, usage.reasoning_tokens);
                }
                RunEventPayload::RunQueued
                | RunEventPayload::RunStarted { .. }
                | RunEventPayload::RunCompleted { .. }
                | RunEventPayload::RunFailed { .. }
                | RunEventPayload::RunCancelled { .. }
                | RunEventPayload::DeliveryIntent { .. } => {}
            }
        }
        facts
    }

    /// Combines the aggregate with the run header into the client summary.
    pub fn into_summary(self, header: &RunHeader) -> RunSummary {
        RunSummary {
            event_count: self.event_count,
            last_event: self.last_event,
            tools: self.tools,
            terminal: self.terminal,
            files: self.files,
            assistant_fragments: self.assistant_fragments,
            waits: self.waits,
            errors_raised: self.errors_raised,
            usage: (self.usage.records > 0).then_some(self.usage),
            terminal_state: terminal_state(header),
        }
    }
}
