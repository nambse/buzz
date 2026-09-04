//! PostgreSQL implementation of [`ActivityQueries`] on the existing
//! [`PgControlPlane`], reading only `runs` and `run_events` (migration 0045)
//! plus the run-list index (migration 0046).
//!
//! Query budget, all company-scoped:
//!
//! - `list_runs`: one statement. Keyset predicate on `(queued_at, id)`
//!   served by `idx_runs_company_queued` (or `idx_runs_employee_status`
//!   under an employee filter); the newest event per run comes from a
//!   `LATERAL` probe on the `run_events` primary key, never a per-run
//!   round trip.
//! - `run_detail`: two statements, the header and one aggregate over the
//!   run's events.
//! - `run_events`: two statements, an existence check that fails closed
//!   and one ordered, bounded read on the primary key.
//!
//! Column contents that must not leave the server (`runtime_run_ref`,
//! `runtime_cursor`) are projected to booleans inside SQL.

use chrono::{DateTime, Utc};
use ortak_control::run_event::RunEventType;
use ortak_control::{CompanyScope, MessageId, PgControlPlane};
use ortak_domain::EmployeeId;
use sqlx::postgres::PgRow;
use sqlx::Row;
use uuid::Uuid;

use crate::error::{ActivityError, Result};
use crate::model::{
    FileSummary, LastEventSummary, RunDetail, RunHeader, RunProvenance, RunTiming,
    RuntimeReference, TerminalSummary, ToolSummary, UsageTotals,
};
use crate::projection::{
    assemble_event_page, bound_row_text, derive_outcome, parse_run_status, RunEventRecord,
    SummaryFacts, MAX_ROW_TEXT_BYTES,
};
use crate::query::{RunEventPage, RunEventsQuery, RunListCursor, RunListPage, RunListQuery};
use crate::repository::ActivityQueries;

// The header column list and the newest-event LATERAL probe are repeated
// verbatim in LIST_SQL and DETAIL_SQL: sqlx 0.9 accepts only `'static`
// statements, so the two cannot be assembled at runtime.
const LIST_SQL: &str = "SELECT r.id, r.employee_id, r.employee_revision_id, r.routing_decision_id,
            r.message_id, r.root_message_id, r.work_item_id, r.runtime_adapter,
            (r.runtime_run_ref IS NOT NULL) AS has_runtime_run_ref,
            r.status, r.delivery_intent, r.error_code,
            r.queued_at, r.started_at, r.finished_at, r.updated_at,
            le.sequence AS last_sequence, le.event_type AS last_event_type,
            le.occurred_at AS last_occurred_at, le.recorded_at AS last_recorded_at
       FROM runs r
       LEFT JOIN LATERAL (
            SELECT e.sequence, e.event_type, e.occurred_at, e.recorded_at
              FROM run_events e
             WHERE e.company_id = r.company_id AND e.run_id = r.id
             ORDER BY e.sequence DESC
             LIMIT 1) le ON true
      WHERE r.company_id = $1
        AND ($2::text IS NULL OR r.employee_id = $2::text)
        AND ($3::text[] IS NULL OR r.status = ANY($3::text[]))
        AND ($4::timestamptz IS NULL OR r.queued_at >= $4::timestamptz)
        AND ($5::timestamptz IS NULL OR r.queued_at < $5::timestamptz)
        AND ($6::timestamptz IS NULL OR (r.queued_at, r.id) < ($6::timestamptz, $7::uuid))
      ORDER BY r.queued_at DESC, r.id DESC
      LIMIT $8";

const DETAIL_SQL: &str =
    "SELECT r.id, r.employee_id, r.employee_revision_id, r.routing_decision_id,
            r.message_id, r.root_message_id, r.work_item_id, r.runtime_adapter,
            (r.runtime_run_ref IS NOT NULL) AS has_runtime_run_ref,
            r.status, r.delivery_intent, r.error_code,
            r.queued_at, r.started_at, r.finished_at, r.updated_at,
            le.sequence AS last_sequence, le.event_type AS last_event_type,
            le.occurred_at AS last_occurred_at, le.recorded_at AS last_recorded_at,
            r.error_message, r.cancel_reason
       FROM runs r
       LEFT JOIN LATERAL (
            SELECT e.sequence, e.event_type, e.occurred_at, e.recorded_at
              FROM run_events e
             WHERE e.company_id = r.company_id AND e.run_id = r.id
             ORDER BY e.sequence DESC
             LIMIT 1) le ON true
      WHERE r.company_id = $1 AND r.id = $2";

const SUMMARY_SQL: &str = "SELECT count(*) AS event_count,
            count(*) FILTER (WHERE event_type = 'assistant.delta') AS assistant_fragments,
            count(*) FILTER (WHERE event_type = 'run.waiting') AS waits,
            count(*) FILTER (WHERE event_type = 'error.raised') AS errors_raised,
            count(*) FILTER (WHERE event_type = 'tool_call.started') AS tools_started,
            count(*) FILTER (WHERE event_type = 'tool_call.completed') AS tools_completed,
            count(*) FILTER (WHERE event_type = 'tool_call.failed') AS tools_failed,
            count(*) FILTER (WHERE event_type = 'terminal.started') AS terminal_commands,
            count(*) FILTER (WHERE event_type = 'terminal.completed') AS terminal_completed,
            count(*) FILTER (WHERE event_type = 'terminal.completed'
                               AND (CASE WHEN jsonb_typeof(payload->'exit_code') = 'number'
                                         THEN (payload->>'exit_code')::numeric END) <> 0)
                AS terminal_nonzero_exits,
            count(*) FILTER (WHERE event_type = 'terminal.completed'
                               AND jsonb_typeof(payload->'exit_code') IS DISTINCT FROM 'number')
                AS terminal_abnormal_exits,
            count(*) FILTER (WHERE event_type = 'terminal.output') AS terminal_output_chunks,
            count(*) FILTER (WHERE event_type = 'file.changed') AS file_changes,
            count(*) FILTER (WHERE event_type = 'file.changed' AND payload->>'change' = 'read')
                AS files_read,
            count(*) FILTER (WHERE event_type = 'file.changed' AND payload->>'change' = 'created')
                AS files_created,
            count(*) FILTER (WHERE event_type = 'file.changed' AND payload->>'change' = 'modified')
                AS files_modified,
            count(*) FILTER (WHERE event_type = 'file.changed' AND payload->>'change' = 'deleted')
                AS files_deleted,
            count(*) FILTER (WHERE event_type = 'usage.recorded') AS usage_records,
            least(coalesce(sum(CASE WHEN event_type = 'usage.recorded'
                                     AND jsonb_typeof(payload->'usage'->'input_tokens') = 'number'
                                    THEN (payload->'usage'->>'input_tokens')::numeric END), 0),
                  9223372036854775807)::bigint AS input_tokens,
            least(coalesce(sum(CASE WHEN event_type = 'usage.recorded'
                                     AND jsonb_typeof(payload->'usage'->'output_tokens') = 'number'
                                    THEN (payload->'usage'->>'output_tokens')::numeric END), 0),
                  9223372036854775807)::bigint AS output_tokens,
            least(coalesce(sum(CASE WHEN event_type = 'usage.recorded'
                                     AND jsonb_typeof(payload->'usage'->'cached_input_tokens') = 'number'
                                    THEN (payload->'usage'->>'cached_input_tokens')::numeric END), 0),
                  9223372036854775807)::bigint AS cached_input_tokens,
            least(coalesce(sum(CASE WHEN event_type = 'usage.recorded'
                                     AND jsonb_typeof(payload->'usage'->'reasoning_tokens') = 'number'
                                    THEN (payload->'usage'->>'reasoning_tokens')::numeric END), 0),
                  9223372036854775807)::bigint AS reasoning_tokens
       FROM run_events
      WHERE company_id = $1 AND run_id = $2";

const EVENTS_SQL: &str = "SELECT sequence, event_type, occurred_at, recorded_at,
            (runtime_cursor IS NOT NULL) AS has_runtime_cursor, artifact_ref, payload
       FROM run_events
      WHERE company_id = $1 AND run_id = $2 AND sequence > $3
      ORDER BY sequence
      LIMIT $4";

const RUN_EXISTS_SQL: &str = "SELECT 1 FROM runs WHERE company_id = $1 AND id = $2";

fn invalid(detail: impl Into<String>) -> ActivityError {
    ActivityError::InvalidRecord {
        detail: detail.into(),
    }
}

fn message_hex(bytes: Option<Vec<u8>>) -> Result<Option<String>> {
    bytes
        .map(|bytes| MessageId::try_from_slice(&bytes).map(|id| id.to_hex()))
        .transpose()
        .map_err(Into::into)
}

fn header_from_row(row: &PgRow) -> Result<RunHeader> {
    let run_id: Uuid = row.try_get("id")?;
    let employee_raw: String = row.try_get("employee_id")?;
    let employee_id = EmployeeId::parse(employee_raw).map_err(|_| {
        invalid(format!(
            "runs.employee_id of run {run_id} is not an employee id"
        ))
    })?;
    let status_raw: String = row.try_get("status")?;
    let status = parse_run_status(&status_raw)?;
    let delivery_intent: Option<String> = row.try_get("delivery_intent")?;
    let error_code: Option<String> = row.try_get("error_code")?;
    let outcome = derive_outcome(status, delivery_intent.as_deref(), error_code.as_deref())?;

    let last_sequence: Option<i64> = row.try_get("last_sequence")?;
    let last_event = match last_sequence {
        Some(sequence) => {
            let event_type_raw: String = row.try_get("last_event_type")?;
            let event_type = RunEventType::parse(&event_type_raw).ok_or_else(|| {
                invalid(format!(
                    "run_events.event_type holds {event_type_raw:?} at sequence {sequence}"
                ))
            })?;
            Some(LastEventSummary {
                sequence,
                event_type,
                occurred_at: row.try_get("last_occurred_at")?,
                recorded_at: row.try_get("last_recorded_at")?,
            })
        }
        None => None,
    };

    Ok(RunHeader {
        run_id,
        employee_id,
        employee_revision_id: row.try_get("employee_revision_id")?,
        provenance: RunProvenance {
            routing_decision_id: row.try_get("routing_decision_id")?,
            message_id: message_hex(row.try_get("message_id")?)?,
            root_message_id: message_hex(row.try_get("root_message_id")?)?,
            work_item_id: row.try_get("work_item_id")?,
        },
        runtime: RuntimeReference {
            adapter: row.try_get("runtime_adapter")?,
            has_run_ref: row.try_get("has_runtime_run_ref")?,
        },
        status,
        outcome,
        timing: RunTiming {
            queued_at: row.try_get("queued_at")?,
            started_at: row.try_get("started_at")?,
            finished_at: row.try_get("finished_at")?,
            updated_at: row.try_get("updated_at")?,
        },
        last_event,
    })
}

fn count(row: &PgRow, column: &str) -> Result<u64> {
    let value: i64 = row.try_get(column)?;
    Ok(u64::try_from(value).unwrap_or(0))
}

fn facts_from_row(row: &PgRow, last_event: Option<LastEventSummary>) -> Result<SummaryFacts> {
    Ok(SummaryFacts {
        event_count: count(row, "event_count")?,
        last_event,
        tools: ToolSummary {
            started: count(row, "tools_started")?,
            completed: count(row, "tools_completed")?,
            failed: count(row, "tools_failed")?,
        },
        terminal: TerminalSummary {
            commands: count(row, "terminal_commands")?,
            completed: count(row, "terminal_completed")?,
            nonzero_exits: count(row, "terminal_nonzero_exits")?,
            abnormal_exits: count(row, "terminal_abnormal_exits")?,
            output_chunks: count(row, "terminal_output_chunks")?,
        },
        files: FileSummary {
            changes: count(row, "file_changes")?,
            read: count(row, "files_read")?,
            created: count(row, "files_created")?,
            modified: count(row, "files_modified")?,
            deleted: count(row, "files_deleted")?,
        },
        assistant_fragments: count(row, "assistant_fragments")?,
        waits: count(row, "waits")?,
        errors_raised: count(row, "errors_raised")?,
        usage: UsageTotals {
            records: count(row, "usage_records")?,
            input_tokens: count(row, "input_tokens")?,
            output_tokens: count(row, "output_tokens")?,
            cached_input_tokens: count(row, "cached_input_tokens")?,
            reasoning_tokens: count(row, "reasoning_tokens")?,
        },
    })
}

fn record_from_row(row: &PgRow) -> Result<RunEventRecord> {
    let event_type: String = row.try_get("event_type")?;
    let occurred_at: DateTime<Utc> = row.try_get("occurred_at")?;
    let recorded_at: DateTime<Utc> = row.try_get("recorded_at")?;
    RunEventRecord::from_stored(
        row.try_get("sequence")?,
        &event_type,
        occurred_at,
        recorded_at,
        row.try_get("has_runtime_cursor")?,
        row.try_get("artifact_ref")?,
        row.try_get("payload")?,
    )
}

impl ActivityQueries for PgControlPlane {
    async fn list_runs(&self, scope: &CompanyScope, query: &RunListQuery) -> Result<RunListPage> {
        query.validate()?;
        let page_size = query.page_size();
        let statuses = query
            .status_filter()
            .map(|values| values.into_iter().map(str::to_owned).collect::<Vec<_>>());
        let rows = sqlx::query(LIST_SQL)
            .bind(scope.company_id())
            .bind(query.employee_id.as_ref().map(|id| id.as_str().to_owned()))
            .bind(statuses)
            .bind(query.queued_from)
            .bind(query.queued_until)
            .bind(query.cursor.map(|cursor| cursor.queued_at()))
            .bind(query.cursor.map(|cursor| cursor.run_id()))
            .bind(i64::from(page_size) + 1)
            .fetch_all(self.pool())
            .await?;
        let has_more = rows.len() > page_size as usize;
        let runs = rows
            .iter()
            .take(page_size as usize)
            .map(header_from_row)
            .collect::<Result<Vec<_>>>()?;
        let next_cursor = has_more
            .then(|| runs.last().map(RunListCursor::after))
            .flatten();
        Ok(RunListPage {
            runs,
            next_cursor,
            has_more,
        })
    }

    async fn run_detail(&self, scope: &CompanyScope, run_id: Uuid) -> Result<RunDetail> {
        let row = sqlx::query(DETAIL_SQL)
            .bind(scope.company_id())
            .bind(run_id)
            .fetch_optional(self.pool())
            .await?;
        let Some(row) = row else {
            return Err(ActivityError::RunNotFound { run_id });
        };
        let run = header_from_row(&row)?;
        let error_message: Option<String> = row.try_get("error_message")?;
        let cancel_reason: Option<String> = row.try_get("cancel_reason")?;

        let aggregate = sqlx::query(SUMMARY_SQL)
            .bind(scope.company_id())
            .bind(run_id)
            .fetch_one(self.pool())
            .await?;
        let facts = facts_from_row(&aggregate, run.last_event)?;
        let summary = facts.into_summary(&run);
        Ok(RunDetail {
            error_message: error_message.map(|text| bound_row_text(&text, MAX_ROW_TEXT_BYTES)),
            cancel_reason: cancel_reason.map(|text| bound_row_text(&text, MAX_ROW_TEXT_BYTES)),
            run,
            summary,
        })
    }

    async fn run_events(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        query: &RunEventsQuery,
    ) -> Result<RunEventPage> {
        query.validate()?;
        let exists = sqlx::query(RUN_EXISTS_SQL)
            .bind(scope.company_id())
            .bind(run_id)
            .fetch_optional(self.pool())
            .await?;
        if exists.is_none() {
            return Err(ActivityError::RunNotFound { run_id });
        }
        let rows = sqlx::query(EVENTS_SQL)
            .bind(scope.company_id())
            .bind(run_id)
            .bind(query.start_after())
            .bind(i64::from(query.page_size()) + 1)
            .fetch_all(self.pool())
            .await?;
        let records = rows
            .iter()
            .map(record_from_row)
            .collect::<Result<Vec<_>>>()?;
        Ok(assemble_event_page(query, records))
    }
}
