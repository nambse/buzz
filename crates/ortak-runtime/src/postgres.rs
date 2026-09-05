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

use chrono::Utc;
use ortak_control::adapter::truncate_at_char_boundary;
use ortak_control::outbox::OutboxLease;
use ortak_control::postgres::office_authority_matches_on;
use ortak_control::run_event::{RunEvent, RunEventPayload};
use ortak_control::runtime::{RunStartReceipt, RuntimeCursor, RuntimeRunRef};
use ortak_control::{CompanyScope, ControlError, PgControlPlane};
use ortak_domain::EmployeeId;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::authority::{DispatchAuthority, DispatchRefusal};
use crate::error::{Result, RunSupervisionError};
use crate::repository::{
    AppendOutcome, CorrelationOutcome, DispatchAuthorization, PrepareOutcome, PreparedRun,
    RunCursorState, RunDispatchRepository,
};
use crate::state::{fold_status, RunStatus, TerminalRecord};

mod authority;
mod memory_context;

/// Ceiling for `runs.error_code`.
const MAX_ERROR_CODE_BYTES: usize = 64;
/// Ceiling for `runs.error_message` and `runs.cancel_reason`.
const MAX_ROW_TEXT_BYTES: usize = 2048;

fn invalid(detail: String) -> RunSupervisionError {
    RunSupervisionError::Control(ControlError::InvalidData(detail))
}

fn parse_status(value: &str) -> Result<RunStatus> {
    RunStatus::parse(value).ok_or_else(|| invalid(format!("runs.status holds {value:?}")))
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
        authority::authorize(self, scope, lease).await
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

        // An unlocked fast refusal preserves the stale-lease outcome. The
        // final conditional write below is still the authoritative lease fence.
        let lease_live: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM outbox WHERE company_id = $1 AND id = $2
             AND lease_token = $3 AND state = 'pending' AND lease_expires_at > clock_timestamp())",
        )
        .bind(company_id)
        .bind(authority.outbox_id())
        .bind(authority.lease_token())
        .fetch_one(&mut *tx)
        .await?;
        if !lease_live {
            return Ok(PrepareOutcome::StaleLease);
        }

        let Some(witness) = authority.office_authority() else {
            return Ok(PrepareOutcome::Refused(
                DispatchRefusal::OfficeAuthorityChanged,
            ));
        };
        if !office_authority_matches_on(&mut tx, scope, witness).await? {
            return Ok(PrepareOutcome::Refused(
                DispatchRefusal::OfficeAuthorityChanged,
            ));
        }

        let inserted = sqlx::query(
            "INSERT INTO runs
                 (company_id, employee_id, employee_revision_id, routing_decision_id,
                  message_id, root_message_id, runtime_adapter, status,
                  office_admission_generation, office_admission_valid_before, office_admission_token)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'queued', $8, $9, $10)
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
        .bind(witness.generation())
        .bind(witness.valid_before())
        .bind(Uuid::new_v4())
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

        if sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2)
                 OR EXISTS (SELECT 1 FROM run_cancel_requests WHERE company_id=$1 AND run_id=$2)",
        )
        .bind(company_id)
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?
        {
            return Ok(PrepareOutcome::Refused(
                DispatchRefusal::CancellationRequested,
            ));
        }

        // Reconciliation after a lost start acknowledgement may renew a
        // witness only after the canonical input hash was revalidated. The
        // pinned employee revision and stable idempotency key remain intact.
        sqlx::query(
            "UPDATE runs SET office_admission_generation = $3,
                    office_admission_valid_before = $4, office_admission_token = $5
                    WHERE company_id = $1 AND id = $2",
        )
        .bind(company_id)
        .bind(run_id)
        .bind(witness.generation())
        .bind(witness.valid_before())
        .bind(Uuid::new_v4())
        .execute(&mut *tx)
        .await?;

        let fenced = sqlx::query(
            "UPDATE outbox SET run_id = $4, updated_at = now()
              WHERE company_id = $1 AND id = $2 AND lease_token = $3 AND state = 'pending'
                AND (run_id IS NULL OR run_id = $4)
                AND lease_expires_at > clock_timestamp()
                AND kind = 'run_dispatch' AND routing_decision_id = $5 AND employee_id = $6",
        )
        .bind(company_id)
        .bind(authority.outbox_id())
        .bind(authority.lease_token())
        .bind(run_id)
        .bind(authority.routing_decision_id())
        .bind(authority.employee_id().as_str())
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

pub(crate) async fn refresh_run_office_authority(
    control: &PgControlPlane,
    scope: &CompanyScope,
    run_id: Uuid,
) -> Result<bool> {
    authority::refresh_admission(control, scope, run_id).await
}
