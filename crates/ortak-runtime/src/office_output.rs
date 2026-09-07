//! Durable completion-to-Office publication scheduling. Runtime destination
//! hints never determine a channel, thread, kind, or signing identity.

use std::time::Duration;

use ortak_control::postgres::lock_office_authority_on;
use ortak_control::{CompanyScope, ControlError, PgControlPlane};
use ortak_office::repository::OfficePublishDraft;
use sqlx::postgres::PgRow;
use sqlx::Row;
use uuid::Uuid;

use crate::Result;

mod canonical;
mod database;
mod sql;

const JOB_TIMEOUT: Duration = Duration::from_secs(5);
const PASS_TIMEOUT: Duration = Duration::from_secs(10);

/// Counts from one bounded completion-publication pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OfficeOutputReport {
    /// Jobs claimed, at most 64.
    pub attempted: usize,
    /// Drafts atomically connected to their durable Office outbox row.
    pub enqueued: usize,
    /// Terminal failures, including expired final attempts.
    pub failed: usize,
    /// Transient failures with durable bounded backoff.
    pub retrying: usize,
}

#[derive(Debug)]
enum OutputFailure {
    Permanent(&'static str),
    Control(ControlError),
    Office(ortak_office::OfficeDeliveryError),
    TimedOut,
    Stale,
}

impl From<ControlError> for OutputFailure {
    fn from(error: ControlError) -> Self {
        Self::Control(error)
    }
}
impl From<sqlx::Error> for OutputFailure {
    fn from(error: sqlx::Error) -> Self {
        Self::Control(error.into())
    }
}
impl From<ortak_office::OfficeDeliveryError> for OutputFailure {
    fn from(error: ortak_office::OfficeDeliveryError) -> Self {
        Self::Office(error)
    }
}

impl OutputFailure {
    fn record(&self) -> (&'static str, bool) {
        match self {
            Self::Permanent(code) => (code, true),
            Self::Office(ortak_office::OfficeDeliveryError::Control(ControlError::Database(_)))
            | Self::Control(ControlError::Database(_)) => ("office_output_database_retry", false),
            Self::Control(_) => ("office_output_source_invalid", true),
            Self::Office(_) => ("office_output_enqueue_refused", true),
            Self::TimedOut => ("office_output_timeout", false),
            Self::Stale => ("office_output_stale", false),
        }
    }
}

struct OutputLease {
    run_id: Uuid,
    token: Uuid,
}

/// Processes up to 64 completion jobs. Every crash leaves either a pending job
/// or an idempotent Office outbox row; no signer or publisher is called here.
///
/// A first transaction freezes canonical text and tags. A second revalidates
/// current Office authority and atomically enqueues those exact fields. Leases
/// expire after 60 seconds; failures back off to 300 seconds with twenty total
/// attempts. Permanent content or authority failures remain visible on the job.
/// Jobs are claimed immediately before use, at most once per pass. Database
/// locks wait at most 500ms, statements at most 2s, and one job at most 5s.
/// The whole pass returns within 10s so runtime cancellation can run again.
/// A pass timeout propagates; any unfinished lease remains durably retryable.
pub async fn schedule_office_outputs(
    control: &PgControlPlane,
    scope: &CompanyScope,
    limit: usize,
) -> Result<OfficeOutputReport> {
    tokio::time::timeout(PASS_TIMEOUT, schedule_pass(control, scope, limit))
        .await
        .map_err(|_| ControlError::InvalidData("Office output scheduling timed out".to_owned()))?
}

async fn schedule_pass(
    control: &PgControlPlane,
    scope: &CompanyScope,
    limit: usize,
) -> Result<OfficeOutputReport> {
    let cap = limit.min(64);
    let failed = database::exhausted(control, scope, cap).await?;
    let mut report = OfficeOutputReport {
        failed,
        ..OfficeOutputReport::default()
    };
    let mut attempted = Vec::with_capacity(cap);
    for _ in 0..cap {
        let Some(lease) = database::claim(control, scope, &attempted).await? else {
            break;
        };
        attempted.push(lease.run_id);
        report.attempted += 1;
        let attempt = tokio::time::timeout(JOB_TIMEOUT, async {
            process_phase(control, scope, &lease, false).await?;
            process_phase(control, scope, &lease, true).await
        })
        .await
        .unwrap_or(Err(OutputFailure::TimedOut));
        match attempt {
            Ok(()) => report.enqueued += 1,
            Err(OutputFailure::Stale) => {}
            Err(failure) => {
                let (code, permanent) = failure.record();
                let state = database::fail(control, scope, &lease, code, permanent).await?;
                match state.as_deref() {
                    Some("failed") => report.failed += 1,
                    Some("pending") => report.retrying += 1,
                    _ => {}
                }
            }
        }
    }
    Ok(report)
}

/// Reloads an immutable enqueued draft after restart. Its only destination is
/// the original authorized source channel. The Office delivery repository must
/// reauthorize it again before signing or reusing a frozen signed event.
pub async fn office_output_draft(
    control: &PgControlPlane,
    scope: &CompanyScope,
    run_id: Uuid,
) -> Result<Option<OfficePublishDraft>> {
    let row = sqlx::query(
        "SELECT draft_kind,draft_tags,draft_content,source_facts FROM runtime_office_outputs
        WHERE company_id=$1 AND run_id=$2 AND state='enqueued'",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .fetch_optional(control.pool())
    .await?;
    row.as_ref()
        .map(|row| {
            read_draft(row, scope, run_id).map_err(|_| {
                ControlError::InvalidData("persisted Office output draft is invalid".to_owned())
                    .into()
            })
        })
        .transpose()
}

/// Revalidates an acknowledged output before its content can enter memory.
/// The caller holds the shared authority fence and commits that admission before
/// external I/O. Reusing the exact draft does not publish another Office event.
pub(crate) async fn revalidate_delivered_output_on(
    connection: &mut sqlx::PgConnection,
    scope: &CompanyScope,
    run_id: Uuid,
) -> std::result::Result<(), ControlError> {
    let target =
        canonical::target(connection, scope, run_id)
            .await
            .map_err(|failure| match failure {
                OutputFailure::Control(error) => error,
                _ => ControlError::InvalidData("memory source Office authority changed".to_owned()),
            })?;
    let row = sqlx::query(
        "SELECT j.draft_kind,j.draft_tags,j.draft_content,j.source_facts,j.outbox_id
         FROM runtime_office_outputs j JOIN outbox o
           ON o.company_id=j.company_id AND o.id=j.outbox_id AND o.run_id=j.run_id
         WHERE j.company_id=$1 AND j.run_id=$2 AND j.state='enqueued'
           AND o.kind='office_publish' AND o.state='delivered' AND o.signed_event_bytes IS NOT NULL",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(|| ControlError::InvalidData("memory source Office output is not delivered".to_owned()))?;
    let draft = read_draft(&row, scope, run_id).map_err(|_| {
        ControlError::InvalidData("memory source Office draft is invalid".to_owned())
    })?;
    if draft.kind != target.kind
        || draft.tags != target.tags
        || row.try_get::<serde_json::Value, _>("source_facts")? != target.source_facts
    {
        return Err(ControlError::InvalidData(
            "memory source Office facts changed".to_owned(),
        ));
    }
    let expected: Uuid = row.try_get("outbox_id")?;
    let enqueued = ortak_office::postgres::enqueue_office_publish_on(connection, scope, &draft)
        .await
        .map_err(|error| match error {
            ortak_office::OfficeDeliveryError::Control(error) => error,
            _ => ControlError::InvalidData("memory source Office publication refused".to_owned()),
        })?;
    if enqueued.authorized().outbox_id() != expected {
        return Err(ControlError::InvalidData(
            "memory source Office receipt changed".to_owned(),
        ));
    }
    Ok(())
}

async fn process_phase(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &OutputLease,
    enqueue: bool,
) -> std::result::Result<(), OutputFailure> {
    let mut tx = database::begin(control).await?;
    // Shared authority comes before run→job→outbox locks. Completion/cancel
    // paths also lock the run first; claims lock only the job.
    let witness = lock_office_authority_on(&mut tx, scope).await?;
    let target = canonical::target(&mut tx, scope, lease.run_id).await?;
    let row = sqlx::query(
        "SELECT draft_kind,draft_tags,draft_content,source_facts FROM runtime_office_outputs
        WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3
          AND lease_expires_at>clock_timestamp() FOR UPDATE",
    )
    .bind(scope.company_id())
    .bind(lease.run_id)
    .bind(lease.token)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(OutputFailure::Stale)?;
    let draft = if row.try_get::<Option<i32>, _>("draft_kind")?.is_some() {
        let draft = read_draft(&row, scope, lease.run_id)?;
        if draft.kind != target.kind
            || draft.tags != target.tags
            || row.try_get::<serde_json::Value, _>("source_facts")? != target.source_facts
        {
            return Err(OutputFailure::Permanent("office_output_target_changed"));
        }
        draft
    } else {
        if enqueue {
            return Err(OutputFailure::Permanent("office_output_draft_missing"));
        }
        let content = canonical::final_text(&mut tx, scope, lease.run_id).await?;
        let draft = OfficePublishDraft {
            company_id: scope.company_id(),
            run_id: lease.run_id,
            kind: target.kind,
            tags: target.tags,
            content,
        };
        draft
            .validate()
            .map_err(|_| OutputFailure::Permanent("office_output_invalid_text"))?;
        let changed = sqlx::query(
            "UPDATE runtime_office_outputs SET draft_kind=$4,draft_tags=$5,draft_content=$6,
            draft_created_at=clock_timestamp(),office_authority_generation=$7,
            office_authority_valid_before=$8,office_authority_token=$9,source_facts=$10
            WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3
              AND lease_expires_at>clock_timestamp()",
        )
        .bind(scope.company_id())
        .bind(lease.run_id)
        .bind(lease.token)
        .bind(i32::from(draft.kind))
        .bind(serde_json::json!(draft.tags))
        .bind(&draft.content)
        .bind(witness.generation())
        .bind(witness.valid_before())
        .bind(Uuid::new_v4())
        .bind(target.source_facts)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(OutputFailure::Stale);
        }
        draft
    };
    if enqueue {
        let enqueued =
            ortak_office::postgres::enqueue_office_publish_on(&mut tx, scope, &draft).await?;
        let changed=sqlx::query("UPDATE runtime_office_outputs SET state='enqueued',outbox_id=$4,
            enqueued_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL,last_error_code=NULL,
            office_authority_generation=$5,office_authority_valid_before=$6,office_authority_token=$7
            WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3
              AND lease_expires_at>clock_timestamp()")
            .bind(scope.company_id()).bind(lease.run_id).bind(lease.token).bind(enqueued.outbox_id())
            .bind(witness.generation()).bind(witness.valid_before()).bind(Uuid::new_v4())
            .execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(OutputFailure::Stale);
        }
    }
    tx.commit().await?;
    Ok(())
}

fn read_draft(
    row: &PgRow,
    scope: &CompanyScope,
    run_id: Uuid,
) -> std::result::Result<OfficePublishDraft, OutputFailure> {
    let kind = u16::try_from(row.try_get::<i32, _>("draft_kind")?)
        .map_err(|_| OutputFailure::Permanent("office_output_draft_invalid"))?;
    let tags = serde_json::from_value(row.try_get("draft_tags")?)
        .map_err(|_| OutputFailure::Permanent("office_output_draft_invalid"))?;
    let draft = OfficePublishDraft {
        company_id: scope.company_id(),
        run_id,
        kind,
        tags,
        content: row.try_get("draft_content")?,
    };
    draft
        .validate()
        .map_err(|_| OutputFailure::Permanent("office_output_draft_invalid"))?;
    Ok(draft)
}
