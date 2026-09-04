#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Ortak Activity read foundation (Architecture v0 §4.6, §9, §11;
//! Implementation Plan Milestone 4).
//!
//! This crate answers three company-scoped questions over the durable
//! `runs` and `run_events` relations without any mutation:
//!
//! - which runs exist ([`ActivityQueries::list_runs`]): deterministic keyset
//!   pages ordered by `(queued_at DESC, run_id DESC)`, hard-capped, with
//!   optional employee/status/time filters;
//! - what one run is ([`ActivityQueries::run_detail`]): header, bounded
//!   terminal text, and an aggregate summary of tools, terminal commands,
//!   file changes, usage, and terminal state computed in one statement;
//! - what happened, in order ([`ActivityQueries::run_events`]): typed
//!   [`ActivityEntry`] values after a sequence cursor with `has_more`, the
//!   next cursor, and gap detection, suitable for reconnect and polling.
//!
//! Guarantees enforced here:
//!
//! - **Company scope is not a filter.** Every read takes the server-resolved
//!   [`CompanyScope`](ortak_control::CompanyScope) beside the caller's
//!   filters; a query type has no company field. An unknown run and a run
//!   of another company are the same [`ActivityError::RunNotFound`].
//! - **Closed vocabularies fail closed.** Status, event type, delivery
//!   intent, and the normalized payload are parsed against the existing
//!   `ortak-control`/`ortak-runtime` types; an unreadable row is an error,
//!   never a partial render.
//! - **Presence, not contents.** Runtime run references and runtime cursors
//!   are reduced to booleans inside SQL. The optional raw view is the
//!   already-bounded, redacted normalized payload with the run reference
//!   scrubbed; nothing else on the row is exposed.
//! - **Bounded again at read time.** Legacy `error_message`, `cancel_reason`,
//!   and `error_code` text is control-stripped, redacted, and clamped when
//!   read. Every page size is clamped to a hard ceiling.
//! - **No N+1.** The list is one statement with a `LATERAL` last-event
//!   probe; detail and events are two statements each.
//!
//! Not in this crate: desktop rendering, HTTP/WebSocket transport, realtime
//! push, retry/cancel actions, and an operator-only raw model. Those attach
//! to this seam in later Milestone 4 slices.

mod error;
pub mod model;
pub mod postgres;
pub mod projection;
pub mod query;
pub mod repository;

pub use error::{ActivityError, Result};
pub use model::{
    Activity, ActivityEntry, ActivityText, FileSummary, LastEventSummary, LifecyclePhase,
    RunDetail, RunHeader, RunOutcome, RunProvenance, RunSummary, RunTiming, RuntimeReference,
    TerminalPhase, TerminalState, TerminalSummary, ToolCallPhase, ToolSummary, UsageTotals,
};
pub use ortak_runtime::RunStatus;
pub use projection::{RunEventRecord, SummaryFacts};
pub use query::{
    RunEventPage, RunEventsQuery, RunListCursor, RunListPage, RunListQuery, SequenceGap,
    DEFAULT_EVENT_PAGE_SIZE, DEFAULT_RUN_PAGE_SIZE, MAX_EVENT_PAGE_SIZE, MAX_RUN_PAGE_SIZE,
};
pub use repository::ActivityQueries;
