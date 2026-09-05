//! PostgreSQL implementation of [`RunDispatchRepository`] on the existing
//! [`PgControlPlane`], using only migration 0045 tables plus the signed
//! `events` row the inbox handoff points at.
//!
//! Provenance is derived, never accepted:
//!
//! - `outbox` (company, id) supplies kind, state, lease token/expiry, and the
//!   decision/employee the routing commit wrote; the lease's copies must
//!   agree with it and `payload` is never read.
//! - `routing_decisions` and `routing_recipients` supply message, root, the
//!   `wake` action, and the pinned `employee_revision_id`.
//! - `delivery_chain_visits` must hold the recipient's reservation.
//! - `office_inbox` must be `decided`; its `(event_created_at, event_id)`
//!   locate the signed `events` row through the company's community binding,
//!   which supplies the message text. The inbox kind must be a supported
//!   plaintext channel kind and channel-scoped, and must agree with the
//!   canonical event's kind and channel, before the content is read as text.
//! - `employees.status` must be `active`; `employee_revisions.manifest` and
//!   the validated `employee_runtime_bindings` row of the pinned revision
//!   supply the runtime binding and permission policy from that same manifest.

use std::collections::BTreeMap;

use chrono::Utc;
use ortak_control::adapter::truncate_at_char_boundary;
use ortak_control::inbox::is_supported_channel_kind;
use ortak_control::outbox::{OutboxKind, OutboxLease};
use ortak_control::run_event::{RunEvent, RunEventPayload};
use ortak_control::runtime::{RunStartReceipt, RuntimeCursor, RuntimeRunRef};
use ortak_control::{CompanyScope, ControlError, MessageId, PgControlPlane};
use ortak_domain::{EmployeeId, EmployeeStatus};
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::authority::{
    bound_message_text, validate_pinned_revision, DispatchAuthority, DispatchRefusal, RunInput,
    StoredRuntimeBinding,
};
use crate::error::{Result, RunSupervisionError};
use crate::repository::{
    AppendOutcome, CorrelationOutcome, DispatchAuthorization, PrepareOutcome, PreparedRun,
    RunCursorState, RunDispatchRepository,
};
use crate::state::{fold_status, RunStatus, TerminalRecord};

/// Ceiling for `runs.error_code`.
const MAX_ERROR_CODE_BYTES: usize = 64;
/// Ceiling for `runs.error_message` and `runs.cancel_reason`.
const MAX_ROW_TEXT_BYTES: usize = 2048;

const AUTHORITY_SQL: &str = "SELECT o.kind, o.state, o.dedup_key, o.lease_token,
            (o.lease_expires_at > now()) AS lease_live,
            o.routing_decision_id, o.employee_id,
            d.id AS decision_id, d.message_id, d.root_message_id,
            rr.action AS recipient_action, rr.employee_revision_id AS pinned_revision_id,
            v.batch_hop,
            e.status AS employee_status,
            rev.id AS revision_id, rev.manifest,
            rb.adapter AS binding_adapter, rb.profile_ref AS binding_profile_ref,
            rb.model AS binding_model, rb.workspace_ref AS binding_workspace_ref,
            rb.credential_refs AS binding_credential_refs, rb.options AS binding_options,
            rb.validated_at AS binding_validated_at,
            i.state AS inbox_state, i.event_kind, i.channel_id,
            ev.kind AS message_kind, ev.channel_id AS message_channel_id,
            ev.content AS message_content, ev.deleted_at AS message_deleted_at
       FROM outbox o
       LEFT JOIN routing_decisions d
         ON d.company_id = o.company_id AND d.id = o.routing_decision_id
       LEFT JOIN routing_recipients rr
         ON rr.company_id = o.company_id
        AND rr.routing_decision_id = o.routing_decision_id
        AND rr.employee_id = o.employee_id
       LEFT JOIN delivery_chain_visits v
         ON v.company_id = o.company_id
        AND v.root_message_id = d.root_message_id
        AND v.employee_id = o.employee_id
        AND v.routing_decision_id = d.id
       LEFT JOIN employees e
         ON e.company_id = o.company_id AND e.id = o.employee_id
       LEFT JOIN employee_revisions rev
         ON rev.company_id = o.company_id
        AND rev.employee_id = o.employee_id
        AND rev.id = rr.employee_revision_id
       LEFT JOIN employee_runtime_bindings rb
         ON rb.company_id = o.company_id
        AND rb.employee_id = o.employee_id
        AND rb.revision_id = rev.id
       LEFT JOIN office_inbox i
         ON i.company_id = o.company_id AND i.event_id = d.message_id
       LEFT JOIN office_company_bindings ocb
         ON ocb.company_id = o.company_id
       LEFT JOIN events ev
         ON ev.community_id = ocb.community_id
        AND ev.created_at = i.event_created_at
        AND ev.id = d.message_id
      WHERE o.company_id = $1 AND o.id = $2";

fn invalid(detail: String) -> RunSupervisionError {
    RunSupervisionError::Control(ControlError::InvalidData(detail))
}

fn parse_status(value: &str) -> Result<RunStatus> {
    RunStatus::parse(value).ok_or_else(|| invalid(format!("runs.status holds {value:?}")))
}

fn parse_employee_status(value: &str) -> Result<EmployeeStatus> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|_| invalid(format!("employees.status holds {value:?}")))
}

fn stored_binding(row: &PgRow) -> Result<Option<StoredRuntimeBinding>> {
    let adapter: Option<String> = row.try_get("binding_adapter")?;
    let Some(adapter) = adapter else {
        return Ok(None);
    };
    let credential_refs: serde_json::Value = row.try_get("binding_credential_refs")?;
    let options: serde_json::Value = row.try_get("binding_options")?;
    let validated_at: Option<chrono::DateTime<Utc>> = row.try_get("binding_validated_at")?;
    Ok(Some(StoredRuntimeBinding {
        adapter,
        profile_ref: row.try_get("binding_profile_ref")?,
        model: row.try_get("binding_model")?,
        workspace_ref: row.try_get("binding_workspace_ref")?,
        credential_refs: serde_json::from_value::<Vec<String>>(credential_refs)
            .map_err(|_| invalid("employee_runtime_bindings.credential_refs".to_owned()))?,
        options: serde_json::from_value::<BTreeMap<String, String>>(options)
            .map_err(|_| invalid("employee_runtime_bindings.options".to_owned()))?,
        validated: validated_at.is_some(),
    }))
}

/// Reads the run row under lock. `None` when it does not exist in the scope.
async fn lock_run(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    run_id: Uuid,
) -> Result<Option<(RunStatus, Option<RuntimeRunRef>)>> {
    let row = sqlx::query(
        "SELECT status, runtime_run_ref FROM runs
          WHERE company_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let status: String = row.try_get("status")?;
    let runtime_run_ref: Option<String> = row.try_get("runtime_run_ref")?;
    Ok(Some((
        parse_status(&status)?,
        runtime_run_ref.map(RuntimeRunRef),
    )))
}

/// Completes the leased outbox row in the caller's transaction; false when
/// the lease token no longer matches.
async fn complete_lease(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    outbox_id: Uuid,
    lease_token: Uuid,
) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE outbox
            SET state = 'delivered', delivered_at = now(),
                lease_owner = NULL, lease_token = NULL, lease_expires_at = NULL,
                updated_at = now()
          WHERE company_id = $1 AND id = $2 AND lease_token = $3 AND state = 'pending'",
    )
    .bind(scope.company_id())
    .bind(outbox_id)
    .bind(lease_token)
    .execute(&mut *connection)
    .await?;
    Ok(result.rows_affected() == 1)
}

impl RunDispatchRepository for PgControlPlane {
    async fn authorize_dispatch(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
    ) -> Result<DispatchAuthorization> {
        let row = sqlx::query(AUTHORITY_SQL)
            .bind(scope.company_id())
            .bind(lease.id)
            .fetch_optional(self.pool())
            .await?;
        let Some(row) = row else {
            return Err(RunSupervisionError::UnknownOutboxRow {
                outbox_id: lease.id,
            });
        };

        // 1. The row itself: kind, then the lease's own copies of the routing
        //    hints must agree with the row before anything else is derived.
        let kind_raw: String = row.try_get("kind")?;
        let kind = OutboxKind::parse(&kind_raw)
            .ok_or_else(|| invalid(format!("outbox.kind holds {kind_raw:?}")))?;
        if kind != OutboxKind::RunDispatch {
            return Err(RunSupervisionError::WrongKind { found: kind });
        }
        let row_decision: Option<Uuid> = row.try_get("routing_decision_id")?;
        let row_employee: Option<String> = row.try_get("employee_id")?;
        let row_dedup: String = row.try_get("dedup_key")?;
        if lease.routing_decision_id != row_decision
            || lease.employee_id != row_employee
            || lease.dedup_key != row_dedup
        {
            return Err(RunSupervisionError::LeaseInconsistent {
                outbox_id: lease.id,
            });
        }

        // 2. Lease fence at the database clock.
        let state: String = row.try_get("state")?;
        let lease_token: Option<Uuid> = row.try_get("lease_token")?;
        let lease_live: Option<bool> = row.try_get("lease_live")?;
        if state != "pending" || lease_token != Some(lease.lease_token) || lease_live != Some(true)
        {
            return Ok(DispatchAuthorization::StaleLease);
        }

        // 3. Routing provenance.
        let refused = |refusal| Ok(DispatchAuthorization::Refused(refusal));
        let Some(routing_decision_id) = row.try_get::<Option<Uuid>, _>("decision_id")? else {
            return refused(DispatchRefusal::DecisionMissing);
        };
        let Some(employee_id_raw) = row_employee else {
            return refused(DispatchRefusal::RecipientMissing);
        };
        let employee_id = EmployeeId::parse(employee_id_raw.as_str())
            .map_err(|error| invalid(format!("outbox.employee_id: {error}")))?;
        let message_bytes: Vec<u8> = row.try_get("message_id")?;
        let root_bytes: Vec<u8> = row.try_get("root_message_id")?;
        let message_id = MessageId::try_from_slice(&message_bytes)?;
        let root_message_id = MessageId::try_from_slice(&root_bytes)?;
        match row.try_get::<Option<String>, _>("recipient_action")? {
            None => return refused(DispatchRefusal::RecipientMissing),
            Some(action) if action != "wake" => {
                return refused(DispatchRefusal::RecipientNotWake { action })
            }
            Some(_) => {}
        }
        let Some(employee_revision_id) = row.try_get::<Option<Uuid>, _>("pinned_revision_id")?
        else {
            return refused(DispatchRefusal::RecipientRevisionUnpinned);
        };
        if row.try_get::<Option<i16>, _>("batch_hop")?.is_none() {
            return refused(DispatchRefusal::VisitMissing);
        }
        let inbox_state: Option<String> = row.try_get("inbox_state")?;
        if inbox_state.as_deref() != Some("decided") {
            return refused(DispatchRefusal::InboxNotDecided { state: inbox_state });
        }

        // 4. Employee lifecycle and the pinned revision's validated binding.
        let Some(employee_status) = row.try_get::<Option<String>, _>("employee_status")? else {
            return refused(DispatchRefusal::EmployeeMissing);
        };
        let employee_status = parse_employee_status(&employee_status)?;
        let Some(revision_id) = row.try_get::<Option<Uuid>, _>("revision_id")? else {
            if employee_status != EmployeeStatus::Active {
                return refused(DispatchRefusal::EmployeeNotActive {
                    status: employee_status,
                });
            }
            return refused(DispatchRefusal::RevisionMissing);
        };
        if revision_id != employee_revision_id {
            return Err(invalid(
                "pinned revision join returned another revision".to_owned(),
            ));
        }
        let manifest: serde_json::Value = row.try_get("manifest")?;
        let stored = stored_binding(&row)?;
        let configuration = match validate_pinned_revision(
            &employee_id,
            employee_status,
            &manifest,
            stored.as_ref(),
        ) {
            Ok(configuration) => configuration,
            Err(refusal) => return refused(refusal),
        };

        // 5. Last-mile channel-kind guard, before any content is read as
        //    text. A stale or hand-seeded dispatch for a gift wrap (1059) or
        //    any other non-channel kind is refused here even if it somehow
        //    reached a `wake` recipient row, and the inbox copy of kind and
        //    channel must agree with the canonical signed event.
        let event_kind: Option<i32> = row.try_get("event_kind")?;
        let channel_id: Option<Uuid> = row.try_get("channel_id")?;
        let Some(event_kind) = event_kind else {
            return refused(DispatchRefusal::MessageUnavailable);
        };
        if !is_supported_channel_kind(event_kind) {
            return refused(DispatchRefusal::UnsupportedMessageKind { kind: event_kind });
        }
        let Some(channel_id) = channel_id else {
            return refused(DispatchRefusal::MessageChannelMissing);
        };
        let Some(content) = row.try_get::<Option<String>, _>("message_content")? else {
            return refused(DispatchRefusal::MessageUnavailable);
        };
        if row.try_get::<Option<i32>, _>("message_kind")? != Some(event_kind) {
            return refused(DispatchRefusal::MessageProvenanceMismatch { field: "kind" });
        }
        if row.try_get::<Option<Uuid>, _>("message_channel_id")? != Some(channel_id) {
            return refused(DispatchRefusal::MessageProvenanceMismatch { field: "channel" });
        }
        if row
            .try_get::<Option<chrono::DateTime<Utc>>, _>("message_deleted_at")?
            .is_some()
        {
            return refused(DispatchRefusal::MessageDeleted);
        }
        let (body, truncated) = match bound_message_text(&content) {
            Ok(bounded) => bounded,
            Err(refusal) => return refused(refusal),
        };
        let input = RunInput {
            body,
            truncated,
            channel_id: Some(channel_id),
            event_kind,
        };

        Ok(DispatchAuthorization::Authorized(Box::new(
            DispatchAuthority::new(
                scope.company_id(),
                lease.id,
                lease.lease_token,
                routing_decision_id,
                employee_id,
                employee_revision_id,
                message_id,
                root_message_id,
                configuration,
                input,
            ),
        )))
    }

    async fn prepare_run(
        &self,
        scope: &CompanyScope,
        authority: &DispatchAuthority,
    ) -> Result<PrepareOutcome> {
        if authority.company_id() != scope.company_id() {
            return Err(invalid(
                "dispatch authority company does not match the scope".to_owned(),
            ));
        }
        let company_id = scope.company_id();
        let mut tx = self.pool().begin().await?;

        let inserted = sqlx::query(
            "INSERT INTO runs
                 (company_id, employee_id, employee_revision_id, routing_decision_id,
                  message_id, root_message_id, runtime_adapter, status)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'queued')
             ON CONFLICT (company_id, routing_decision_id, employee_id) DO NOTHING
             RETURNING id",
        )
        .bind(company_id)
        .bind(authority.employee_id().as_str())
        .bind(authority.employee_revision_id())
        .bind(authority.routing_decision_id())
        .bind(authority.message_id().as_bytes().as_slice())
        .bind(authority.root_message_id().as_bytes().as_slice())
        .bind(&authority.binding().adapter)
        .fetch_optional(&mut *tx)
        .await?;
        let created = if let Some(row) = inserted {
            let run_id: Uuid = row.try_get("id")?;
            let opened = RunEvent {
                run_id,
                sequence: Some(0),
                occurred_at: Utc::now(),
                runtime_cursor: None,
                artifact_ref: None,
                payload: RunEventPayload::RunQueued,
            };
            sqlx::query(
                "INSERT INTO run_events
                     (company_id, run_id, sequence, event_type, occurred_at, payload)
                 VALUES ($1, $2, 0, $3, $4, $5)",
            )
            .bind(company_id)
            .bind(run_id)
            .bind(opened.event_type().as_str())
            .bind(opened.occurred_at)
            .bind(opened.payload_json()?)
            .execute(&mut *tx)
            .await?;
            true
        } else {
            false
        };

        let run = sqlx::query(
            "SELECT id, status, runtime_run_ref, employee_revision_id, message_id, root_message_id,
                    runtime_adapter
               FROM runs
              WHERE company_id = $1 AND routing_decision_id = $2 AND employee_id = $3
              FOR UPDATE",
        )
        .bind(company_id)
        .bind(authority.routing_decision_id())
        .bind(authority.employee_id().as_str())
        .fetch_one(&mut *tx)
        .await?;
        let run_id: Uuid = run.try_get("id")?;
        let pinned_revision: Uuid = run.try_get("employee_revision_id")?;
        let pinned_message: Option<Vec<u8>> = run.try_get("message_id")?;
        let pinned_root: Option<Vec<u8>> = run.try_get("root_message_id")?;
        let pinned_adapter: String = run.try_get("runtime_adapter")?;
        if pinned_revision != authority.employee_revision_id()
            || pinned_message.as_deref() != Some(authority.message_id().as_bytes().as_slice())
            || pinned_root.as_deref() != Some(authority.root_message_id().as_bytes().as_slice())
            || pinned_adapter != authority.binding().adapter
        {
            tx.rollback().await?;
            return Err(RunSupervisionError::RunPinnedDifferently { run_id });
        }
        let status: String = run.try_get("status")?;
        let runtime_run_ref: Option<String> = run.try_get("runtime_run_ref")?;

        let fenced = sqlx::query(
            "UPDATE outbox SET run_id = $4, updated_at = now()
              WHERE company_id = $1 AND id = $2 AND lease_token = $3 AND state = 'pending'
                AND (run_id IS NULL OR run_id = $4)",
        )
        .bind(company_id)
        .bind(authority.outbox_id())
        .bind(authority.lease_token())
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        if fenced.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(PrepareOutcome::StaleLease);
        }
        tx.commit().await?;

        Ok(PrepareOutcome::Prepared(PreparedRun {
            run_id,
            status: parse_status(&status)?,
            runtime_run_ref: runtime_run_ref.map(RuntimeRunRef),
            created,
        }))
    }

    async fn correlate_run(
        &self,
        scope: &CompanyScope,
        authority: &DispatchAuthority,
        run_id: Uuid,
        receipt: &RunStartReceipt,
    ) -> Result<CorrelationOutcome> {
        if authority.company_id() != scope.company_id() {
            return Err(invalid(
                "dispatch authority company does not match the scope".to_owned(),
            ));
        }
        let mut tx = self.pool().begin().await?;
        let Some((status, durable_ref)) = lock_run(&mut tx, scope, run_id).await? else {
            return Err(RunSupervisionError::UnknownRun { run_id });
        };

        enum Pending {
            Correlated,
            AlreadyCorrelated(RunStatus),
            Terminal(RunStatus),
        }
        let pending = match (status, durable_ref) {
            (RunStatus::Queued, None) => {
                sqlx::query(
                    "UPDATE runs
                        SET runtime_run_ref = $3, status = 'running', started_at = $4,
                            updated_at = now()
                      WHERE company_id = $1 AND id = $2 AND status = 'queued'
                        AND runtime_run_ref IS NULL",
                )
                .bind(scope.company_id())
                .bind(run_id)
                .bind(&receipt.runtime_run_ref.0)
                .bind(receipt.started_at)
                .execute(&mut *tx)
                .await?;
                Pending::Correlated
            }
            (status, Some(durable)) if durable == receipt.runtime_run_ref => {
                Pending::AlreadyCorrelated(status)
            }
            (_, Some(durable)) => {
                tx.rollback().await?;
                return Ok(CorrelationOutcome::RefConflict { durable });
            }
            (status, None) if status.is_terminal() => Pending::Terminal(status),
            (status, None) => {
                tx.rollback().await?;
                return Err(invalid(format!(
                    "run {run_id} is {status} without a runtime run reference"
                )));
            }
        };

        let lease_completed = complete_lease(
            &mut tx,
            scope,
            authority.outbox_id(),
            authority.lease_token(),
        )
        .await?;
        tx.commit().await?;
        Ok(match pending {
            Pending::Correlated => CorrelationOutcome::Correlated { lease_completed },
            Pending::AlreadyCorrelated(status) => CorrelationOutcome::AlreadyCorrelated {
                status,
                lease_completed,
            },
            Pending::Terminal(status) => CorrelationOutcome::Terminal {
                status,
                lease_completed,
            },
        })
    }

    async fn run_cursor_state(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
    ) -> Result<Option<RunCursorState>> {
        let row = sqlx::query(
            "SELECT r.employee_id, r.status, r.runtime_run_ref, r.updated_at,
                    (SELECT runtime_cursor FROM run_events
                      WHERE company_id = r.company_id AND run_id = r.id
                        AND runtime_cursor IS NOT NULL
                      ORDER BY sequence DESC LIMIT 1) AS last_cursor,
                    (SELECT count(*) FROM run_events
                      WHERE company_id = r.company_id AND run_id = r.id) AS event_count
               FROM runs r
              WHERE r.company_id = $1 AND r.id = $2",
        )
        .bind(scope.company_id())
        .bind(run_id)
        .fetch_optional(self.pool())
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let employee_id: String = row.try_get("employee_id")?;
        let status: String = row.try_get("status")?;
        let runtime_run_ref: Option<String> = row.try_get("runtime_run_ref")?;
        let last_cursor: Option<String> = row.try_get("last_cursor")?;
        Ok(Some(RunCursorState {
            run_id,
            employee_id: EmployeeId::parse(employee_id.as_str())
                .map_err(|error| invalid(format!("runs.employee_id: {error}")))?,
            status: parse_status(&status)?,
            runtime_run_ref: runtime_run_ref.map(RuntimeRunRef),
            last_cursor: last_cursor.map(RuntimeCursor),
            event_count: row.try_get("event_count")?,
            updated_at: row.try_get("updated_at")?,
        }))
    }

    async fn append_supervised_events(
        &self,
        scope: &CompanyScope,
        run_id: Uuid,
        expected_ref: &RuntimeRunRef,
        events: &[RunEvent],
    ) -> Result<AppendOutcome> {
        for event in events {
            event.validate()?;
            if event.run_id != run_id {
                return Err(RunSupervisionError::Control(
                    ControlError::RunEventRejected {
                        run_id,
                        detail: "event belongs to a different run",
                    },
                ));
            }
        }
        let company_id = scope.company_id();
        let mut tx = self.pool().begin().await?;
        let Some((current, durable_ref)) = lock_run(&mut tx, scope, run_id).await? else {
            return Err(RunSupervisionError::UnknownRun { run_id });
        };
        if current.is_terminal() {
            tx.rollback().await?;
            return Ok(AppendOutcome::RunTerminal { status: current });
        }
        if durable_ref.as_ref() != Some(expected_ref) {
            tx.rollback().await?;
            return Ok(AppendOutcome::RefMismatch {
                durable: durable_ref,
            });
        }

        let mut next: i64 = sqlx::query(
            "SELECT coalesce(max(sequence) + 1, 0) AS next
               FROM run_events WHERE company_id = $1 AND run_id = $2",
        )
        .bind(company_id)
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?
        .try_get("next")?;

        let mut sequences = Vec::with_capacity(events.len());
        let mut duplicate_cursors = Vec::new();
        let mut appended = Vec::with_capacity(events.len());
        for event in events {
            if let Some(cursor) = &event.runtime_cursor {
                let seen = sqlx::query(
                    "SELECT 1 FROM run_events
                      WHERE company_id = $1 AND run_id = $2 AND runtime_cursor = $3",
                )
                .bind(company_id)
                .bind(run_id)
                .bind(cursor)
                .fetch_optional(&mut *tx)
                .await?;
                if seen.is_some() {
                    duplicate_cursors.push(cursor.clone());
                    continue;
                }
            }
            sqlx::query(
                "INSERT INTO run_events
                     (company_id, run_id, sequence, event_type, occurred_at,
                      runtime_cursor, payload, artifact_ref)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(company_id)
            .bind(run_id)
            .bind(next)
            .bind(event.event_type().as_str())
            .bind(event.occurred_at)
            .bind(event.runtime_cursor.as_deref())
            .bind(event.payload_json()?)
            .bind(event.artifact_ref.as_deref())
            .execute(&mut *tx)
            .await?;
            sequences.push(next);
            next += 1;
            appended.push(event.clone());
        }

        let (status, terminal) = match fold_status(current, &appended) {
            Ok(folded) => folded,
            Err(invalid_transition) => {
                tx.rollback().await?;
                return Err(invalid_transition.into());
            }
        };
        match terminal {
            Some(record) => {
                let (delivery_intent, error_code, error_message, cancel_reason) = match &record {
                    TerminalRecord::Completed { delivery_intent } => {
                        (Some(delivery_intent.as_str()), None, None, None)
                    }
                    TerminalRecord::Failed { code, message } => (
                        None,
                        Some(truncate_at_char_boundary(code, MAX_ERROR_CODE_BYTES)),
                        Some(truncate_at_char_boundary(message, MAX_ROW_TEXT_BYTES)),
                        None,
                    ),
                    TerminalRecord::Cancelled { reason } => (
                        None,
                        None,
                        None,
                        Some(truncate_at_char_boundary(reason, MAX_ROW_TEXT_BYTES)),
                    ),
                };
                sqlx::query(
                    "UPDATE runs
                        SET status = $3, delivery_intent = $4, error_code = $5,
                            error_message = $6, cancel_reason = $7,
                            finished_at = now(), updated_at = now()
                      WHERE company_id = $1 AND id = $2",
                )
                .bind(company_id)
                .bind(run_id)
                .bind(record.status().as_str())
                .bind(delivery_intent)
                .bind(error_code)
                .bind(error_message)
                .bind(cancel_reason)
                .execute(&mut *tx)
                .await?;
            }
            None if status != current => {
                sqlx::query(
                    "UPDATE runs SET status = $3, updated_at = now()
                      WHERE company_id = $1 AND id = $2",
                )
                .bind(company_id)
                .bind(run_id)
                .bind(status.as_str())
                .execute(&mut *tx)
                .await?;
            }
            None => {}
        }
        tx.commit().await?;
        Ok(AppendOutcome::Appended {
            sequences,
            duplicate_cursors,
            status,
        })
    }
}
