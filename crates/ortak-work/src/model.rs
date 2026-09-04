//! Read models, list queries, and cursors.
//!
//! A query never names a company: the boundary is the server-resolved
//! [`CompanyScope`](ortak_control::CompanyScope) passed beside it. Page
//! sizes are clamped to hard ceilings here so no caller can request an
//! unbounded result.

use chrono::{DateTime, Utc};
use ortak_domain::{Project, WorkActor, WorkEvent, WorkItem, WorkPriority, WorkState};
use serde::Serialize;
use uuid::Uuid;

use crate::error::{Result, WorkError};

/// Hard ceiling for one project work-list page.
pub const MAX_WORK_PAGE_SIZE: u32 = 100;
/// Work-list page size when the caller gives none.
pub const DEFAULT_WORK_PAGE_SIZE: u32 = 25;
/// Ceiling on history rows returned with one aggregate (oldest first).
pub const MAX_WORK_HISTORY_ROWS: i64 = 500;

/// A project with its durable bookkeeping.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectRecord {
    /// Domain aggregate.
    pub project: Project,
    /// Who created it.
    pub created_by: WorkActor,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last mutation time.
    pub updated_at: DateTime<Utc>,
    /// Archive time, when archived.
    pub archived_at: Option<DateTime<Utc>>,
}

/// One append-only history event of a work item.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkHistoryRecord {
    /// Dense sequence from 0.
    pub sequence: i64,
    /// Item version after this event (`sequence + 1`).
    pub version: i64,
    /// Who acted.
    pub actor: WorkActor,
    /// Typed, bounded event.
    pub event: WorkEvent,
    /// When it was committed.
    pub recorded_at: DateTime<Utc>,
}

/// A work item with its children, bookkeeping, and history.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkItemAggregate {
    /// Domain aggregate.
    pub item: WorkItem,
    /// Dispatching decision of the source message, when one existed.
    pub source_routing_decision_id: Option<Uuid>,
    /// Who created it.
    pub created_by: WorkActor,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last mutation time.
    pub updated_at: DateTime<Utc>,
    /// Completion time, when completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// Cancellation time, when cancelled.
    pub cancelled_at: Option<DateTime<Utc>>,
    /// Oldest-first history, at most [`MAX_WORK_HISTORY_ROWS`] rows.
    pub history: Vec<WorkHistoryRecord>,
    /// True when the history was cut at the ceiling.
    pub history_truncated: bool,
}

/// One row of a project's work list.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkSummary {
    /// Work item id.
    pub id: Uuid,
    /// Owning project.
    pub project_id: Uuid,
    /// Title.
    pub title: String,
    /// Priority.
    pub priority: WorkPriority,
    /// State.
    pub state: WorkState,
    /// Current version.
    pub version: i64,
    /// Source Office message when promoted (lowercase hex).
    pub source_message_id: Option<String>,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last mutation time.
    pub updated_at: DateTime<Utc>,
}

/// Filters and keyset cursor for one project's work list, ordered newest
/// first by `(created_at DESC, id DESC)`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkListQuery {
    /// Only items in one of these states; empty means every state.
    pub states: Vec<WorkState>,
    /// Continue after this position (from a previous page's `next_cursor`).
    pub cursor: Option<WorkListCursor>,
    /// Requested page size; clamped to `1..=`[`MAX_WORK_PAGE_SIZE`].
    pub limit: Option<u32>,
}

impl WorkListQuery {
    /// Effective page size.
    pub fn page_size(&self) -> u32 {
        self.limit
            .unwrap_or(DEFAULT_WORK_PAGE_SIZE)
            .clamp(1, MAX_WORK_PAGE_SIZE)
    }

    /// Deduplicated state column values, `None` when unfiltered.
    pub fn state_filter(&self) -> Option<Vec<&'static str>> {
        if self.states.is_empty() {
            return None;
        }
        let mut values: Vec<&'static str> =
            self.states.iter().map(|state| state.as_str()).collect();
        values.sort_unstable();
        values.dedup();
        Some(values)
    }
}

/// Keyset position in a work list: the `(created_at, id)` of the last item
/// on the previous page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkListCursor {
    created_at: DateTime<Utc>,
    id: Uuid,
}

impl WorkListCursor {
    /// Position after the given summary.
    pub fn after(summary: &WorkSummary) -> Self {
        Self {
            created_at: summary.created_at,
            id: summary.id,
        }
    }

    /// Creation time component.
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Item id component.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Opaque transport form: `<unix microseconds>:<item id>`.
    pub fn encode(&self) -> String {
        format!(
            "{}:{}",
            self.created_at.timestamp_micros(),
            self.id.as_simple()
        )
    }

    /// Parses the transport form; any malformed input is an invalid query.
    pub fn decode(value: &str) -> Result<Self> {
        let invalid = || WorkError::InvalidQuery("work list cursor is malformed");
        let (micros, id) = value.split_once(':').ok_or_else(invalid)?;
        let micros: i64 = micros.parse().map_err(|_| invalid())?;
        let created_at = DateTime::<Utc>::from_timestamp_micros(micros).ok_or_else(invalid)?;
        let id = Uuid::parse_str(id).map_err(|_| invalid())?;
        Ok(Self { created_at, id })
    }
}

/// One page of a project's work list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkListPage {
    /// Items in `(created_at DESC, id DESC)` order.
    pub items: Vec<WorkSummary>,
    /// Cursor for the next page, `None` on the last page.
    pub next_cursor: Option<WorkListCursor>,
}
