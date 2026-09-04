//! Durable run lifecycle states and the typed-event transition table.
//!
//! `runs.status` moves only through this table, driven by normalized
//! [`RunEventPayload`] values. A terminal event is the only way to reach a
//! terminal status, and no event may follow one.

use std::fmt;

use ortak_control::run_event::{DeliveryIntentKind, RunEvent, RunEventPayload, RunEventType};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// `runs.status` vocabulary (migration 0045).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Durable row exists; no runtime run yet.
    Queued,
    /// Correlated with a runtime run that is executing.
    Running,
    /// Blocked on approval, input, or an external dependency.
    Waiting,
    /// Finished with a typed delivery intent.
    Completed,
    /// Finished with an error.
    Failed,
    /// Cancelled before completion.
    Cancelled,
}

impl RunStatus {
    /// Column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses the column value.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "waiting" => Some(Self::Waiting),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    /// True once the run can accept no further events.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Terminal facts written onto the run row together with the terminal event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalRecord {
    /// Completed with the typed delivery intent the runtime chose.
    Completed {
        /// Delivery intent.
        delivery_intent: DeliveryIntentKind,
    },
    /// Failed with a stable code and a bounded, redacted message.
    Failed {
        /// Stable error code.
        code: String,
        /// Bounded message.
        message: String,
    },
    /// Cancelled with a bounded reason.
    Cancelled {
        /// Bounded reason.
        reason: String,
    },
}

impl TerminalRecord {
    /// Extracts the terminal facts of a terminal payload; `None` otherwise.
    pub fn from_payload(payload: &RunEventPayload) -> Option<Self> {
        match payload {
            RunEventPayload::RunCompleted { delivery_intent } => Some(Self::Completed {
                delivery_intent: *delivery_intent,
            }),
            RunEventPayload::RunFailed { code, message } => Some(Self::Failed {
                code: code.clone(),
                message: message.text.clone(),
            }),
            RunEventPayload::RunCancelled { reason } => Some(Self::Cancelled {
                reason: reason.text.clone(),
            }),
            _ => None,
        }
    }

    /// Status this record ends the run in.
    pub fn status(&self) -> RunStatus {
        match self {
            Self::Completed { .. } => RunStatus::Completed,
            Self::Failed { .. } => RunStatus::Failed,
            Self::Cancelled { .. } => RunStatus::Cancelled,
        }
    }
}

/// A typed event that is not a valid transition from the current status.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
#[error("event {event} is not a valid transition from run status {from}")]
pub struct InvalidTransition {
    /// Status before the event.
    pub from: RunStatus,
    /// Offending event type.
    pub event: RunEventType,
}

/// Status after applying one typed event to `current`.
///
/// - A terminal status accepts nothing.
/// - `run.queued` is only valid while queued (it opens the run).
/// - `run.started` and every work event (`assistant.delta`, tool, terminal,
///   and file events) move the run to `running`, including out of `waiting`.
/// - `run.waiting` moves to `waiting`.
/// - Telemetry, non-terminal errors, and `delivery.intent` keep the status.
/// - `run.completed`, `run.failed`, and `run.cancelled` are terminal.
pub fn status_after(
    current: RunStatus,
    event: RunEventType,
) -> std::result::Result<RunStatus, InvalidTransition> {
    let invalid = || InvalidTransition {
        from: current,
        event,
    };
    if current.is_terminal() {
        return Err(invalid());
    }
    Ok(match event {
        RunEventType::RunQueued => {
            if current == RunStatus::Queued {
                RunStatus::Queued
            } else {
                return Err(invalid());
            }
        }
        RunEventType::RunStarted
        | RunEventType::AssistantDelta
        | RunEventType::ToolCallStarted
        | RunEventType::ToolCallCompleted
        | RunEventType::ToolCallFailed
        | RunEventType::TerminalStarted
        | RunEventType::TerminalOutput
        | RunEventType::TerminalCompleted
        | RunEventType::FileChanged => RunStatus::Running,
        RunEventType::RunWaiting => RunStatus::Waiting,
        RunEventType::UsageRecorded | RunEventType::ErrorRaised | RunEventType::DeliveryIntent => {
            current
        }
        RunEventType::RunCompleted => RunStatus::Completed,
        RunEventType::RunFailed => RunStatus::Failed,
        RunEventType::RunCancelled => RunStatus::Cancelled,
    })
}

/// Folds a batch of events over `current`, returning the final status and
/// the terminal record when the batch ends the run.
pub fn fold_status(
    current: RunStatus,
    events: &[RunEvent],
) -> std::result::Result<(RunStatus, Option<TerminalRecord>), InvalidTransition> {
    let mut status = current;
    let mut terminal = None;
    for event in events {
        status = status_after(status, event.event_type())?;
        terminal = TerminalRecord::from_payload(&event.payload);
    }
    Ok((status, terminal))
}

#[cfg(test)]
mod tests {
    use ortak_control::run_event::{BoundedText, DeliveryIntentKind, RunEventType};

    use super::{status_after, RunStatus, TerminalRecord};
    use ortak_control::run_event::RunEventPayload;

    #[test]
    fn terminal_states_accept_nothing() {
        for terminal in [
            RunStatus::Completed,
            RunStatus::Failed,
            RunStatus::Cancelled,
        ] {
            assert!(terminal.is_terminal());
            assert!(status_after(terminal, RunEventType::AssistantDelta).is_err());
            assert!(status_after(terminal, RunEventType::RunCompleted).is_err());
        }
    }

    #[test]
    fn waiting_and_work_events_move_between_running_and_waiting() {
        assert_eq!(
            status_after(RunStatus::Running, RunEventType::RunWaiting),
            Ok(RunStatus::Waiting)
        );
        assert_eq!(
            status_after(RunStatus::Waiting, RunEventType::ToolCallStarted),
            Ok(RunStatus::Running)
        );
        assert_eq!(
            status_after(RunStatus::Waiting, RunEventType::UsageRecorded),
            Ok(RunStatus::Waiting)
        );
        assert_eq!(
            status_after(RunStatus::Running, RunEventType::RunQueued).ok(),
            None
        );
    }

    #[test]
    fn terminal_records_carry_typed_facts() {
        let completed = TerminalRecord::from_payload(&RunEventPayload::RunCompleted {
            delivery_intent: DeliveryIntentKind::Silent,
        })
        .expect("terminal");
        assert_eq!(completed.status(), RunStatus::Completed);
        let failed = TerminalRecord::from_payload(&RunEventPayload::RunFailed {
            code: "boom".to_owned(),
            message: BoundedText::raw("bounded"),
        })
        .expect("terminal");
        assert_eq!(failed.status(), RunStatus::Failed);
        assert!(TerminalRecord::from_payload(&RunEventPayload::RunQueued).is_none());
    }
}
