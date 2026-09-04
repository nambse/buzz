//! Caller-supplied filters and cursors, and the pages they produce.
//!
//! A query never names a company: the company boundary is the
//! server-resolved [`CompanyScope`](ortak_control::CompanyScope) passed
//! beside it. Every page size is clamped to a hard ceiling here so no
//! caller can request an unbounded result.

use chrono::{DateTime, Utc};
use ortak_domain::EmployeeId;
use ortak_runtime::RunStatus;
use serde::{Serialize, Serializer};
use uuid::Uuid;

use crate::error::{ActivityError, Result};
use crate::model::{ActivityEntry, RunHeader};

/// Hard ceiling for one run-list page.
pub const MAX_RUN_PAGE_SIZE: u32 = 100;
/// Run-list page size when the caller gives none.
pub const DEFAULT_RUN_PAGE_SIZE: u32 = 25;
/// Hard ceiling for one event page.
pub const MAX_EVENT_PAGE_SIZE: u32 = 500;
/// Event page size when the caller gives none.
pub const DEFAULT_EVENT_PAGE_SIZE: u32 = 100;

/// Filters and keyset cursor for the company-scoped run list.
///
/// Runs are ordered newest first by `(queued_at DESC, run_id DESC)`; the
/// run id breaks ties between runs queued in the same microsecond so a page
/// boundary is stable even under equal timestamps.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunListQuery {
    /// Only runs of this employee.
    pub employee_id: Option<EmployeeId>,
    /// Only runs in one of these statuses; empty means every status.
    pub statuses: Vec<RunStatus>,
    /// Only runs queued at or after this instant.
    pub queued_from: Option<DateTime<Utc>>,
    /// Only runs queued strictly before this instant.
    pub queued_until: Option<DateTime<Utc>>,
    /// Continue after this position (from a previous page's `next_cursor`).
    pub cursor: Option<RunListCursor>,
    /// Requested page size; clamped to `1..=`[`MAX_RUN_PAGE_SIZE`].
    pub limit: Option<u32>,
}

impl RunListQuery {
    /// Effective page size.
    pub fn page_size(&self) -> u32 {
        self.limit
            .unwrap_or(DEFAULT_RUN_PAGE_SIZE)
            .clamp(1, MAX_RUN_PAGE_SIZE)
    }

    /// Rejects an empty time window.
    pub fn validate(&self) -> Result<()> {
        if let (Some(from), Some(until)) = (self.queued_from, self.queued_until) {
            if from >= until {
                return Err(ActivityError::InvalidQuery(
                    "queued_from must be before queued_until",
                ));
            }
        }
        Ok(())
    }

    /// Deduplicated status column values, `None` when unfiltered.
    pub fn status_filter(&self) -> Option<Vec<&'static str>> {
        if self.statuses.is_empty() {
            return None;
        }
        let mut values: Vec<&'static str> =
            self.statuses.iter().map(|status| status.as_str()).collect();
        values.sort_unstable();
        values.dedup();
        Some(values)
    }
}

/// Keyset position in the run list: the `(queued_at, run_id)` of the last
/// run on the previous page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunListCursor {
    queued_at: DateTime<Utc>,
    run_id: Uuid,
}

impl RunListCursor {
    /// Position after the given run.
    pub fn after(run: &RunHeader) -> Self {
        Self {
            queued_at: run.timing.queued_at,
            run_id: run.run_id,
        }
    }

    /// Queue time component.
    pub fn queued_at(&self) -> DateTime<Utc> {
        self.queued_at
    }

    /// Run id component.
    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    /// Opaque transport form: `<unix microseconds>:<run id>`. PostgreSQL
    /// stores `timestamptz` at microsecond precision, so the round trip is
    /// exact.
    pub fn encode(&self) -> String {
        format!(
            "{}:{}",
            self.queued_at.timestamp_micros(),
            self.run_id.as_simple()
        )
    }

    /// Parses the transport form; any malformed input is an invalid query.
    pub fn decode(value: &str) -> Result<Self> {
        let invalid = || ActivityError::InvalidQuery("run list cursor is malformed");
        if value.len() > 64 {
            return Err(invalid());
        }
        let (micros, run_id) = value.split_once(':').ok_or_else(invalid)?;
        let micros: i64 = micros.parse().map_err(|_| invalid())?;
        let queued_at = DateTime::<Utc>::from_timestamp_micros(micros).ok_or_else(invalid)?;
        let run_id = Uuid::try_parse(run_id).map_err(|_| invalid())?;
        Ok(Self { queued_at, run_id })
    }
}

impl Serialize for RunListCursor {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.encode())
    }
}

/// One page of the run list.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RunListPage {
    /// Runs, newest first.
    pub runs: Vec<RunHeader>,
    /// Cursor for the next page; `None` when this page is the last.
    pub next_cursor: Option<RunListCursor>,
    /// True when more runs match beyond this page.
    pub has_more: bool,
}

/// Sequence-based incremental read of one run's events.
///
/// A client keeps the last sequence it rendered and asks for everything
/// after it; a reconnect replays from the same cursor without duplicates
/// or reordering because sequences are dense and append-only.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunEventsQuery {
    /// Return events with a sequence strictly greater than this; `None`
    /// starts from the first event.
    pub after_sequence: Option<i64>,
    /// Requested page size; clamped to `1..=`[`MAX_EVENT_PAGE_SIZE`].
    pub limit: Option<u32>,
    /// Include the bounded, redacted normalized payload on each entry.
    pub include_raw: bool,
}

impl RunEventsQuery {
    /// Effective page size.
    pub fn page_size(&self) -> u32 {
        self.limit
            .unwrap_or(DEFAULT_EVENT_PAGE_SIZE)
            .clamp(1, MAX_EVENT_PAGE_SIZE)
    }

    /// Rejects a negative cursor.
    pub fn validate(&self) -> Result<()> {
        if self.after_sequence.is_some_and(|after| after < 0) {
            return Err(ActivityError::InvalidQuery(
                "after_sequence must be zero or positive",
            ));
        }
        Ok(())
    }

    /// Exclusive lower bound used in SQL: `-1` when reading from the start.
    pub fn start_after(&self) -> i64 {
        self.after_sequence.unwrap_or(-1)
    }
}

/// One page of a run's events.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RunEventPage {
    /// Entries in ascending, dense sequence order.
    pub entries: Vec<ActivityEntry>,
    /// Cursor to pass as `after_sequence` next time: the last sequence on
    /// this page, or the request's cursor when the page is empty.
    pub next_after_sequence: Option<i64>,
    /// True when more events exist beyond this page.
    pub has_more: bool,
    /// Set when the durable sequence was not dense from the cursor. The
    /// page stops before the gap; a client should resynchronize from the
    /// start rather than advance.
    pub gap: Option<SequenceGap>,
}

/// A hole or reorder observed in the per-run sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SequenceGap {
    /// Sequence the reader expected next.
    pub expected: i64,
    /// Sequence actually found.
    pub found: i64,
}
