//! Immutable pre-start snapshots. Recall/provider work never runs in this module.

use std::time::Duration;

use ortak_control::outbox::OutboxLease;
use ortak_control::postgres::lock_office_authority_on;
use ortak_control::{CompanyScope, PgControlPlane};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use super::{authority, invalid};
use crate::authority::{DispatchAuthority, DispatchRefusal};
use crate::memory_context::{FreezeSnapshotOutcome, FrozenRunSnapshot, RunContextRepository};
use crate::repository::DispatchAuthorization;
use crate::{Result, RunSupervisionError};

async fn bounds(connection: &mut PgConnection) -> Result<()> {
    sqlx::query("SELECT set_config('lock_timeout','500ms',true),set_config('statement_timeout','2s',true),set_config('idle_in_transaction_session_timeout','5s',true)")
        .execute(connection).await?;
    Ok(())
}

impl RunContextRepository for PgControlPlane {
    async fn load_run_snapshot(
        &self,
        scope: &CompanyScope,
        authority: &DispatchAuthority,
        run_id: Uuid,
    ) -> Result<Option<FrozenRunSnapshot>> {
        tokio::time::timeout(Duration::from_secs(5), load(self, scope, authority, run_id))
            .await
            .map_err(|_| invalid("run snapshot load timed out".to_owned()))?
    }

    async fn freeze_run_snapshot(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        original: &DispatchAuthority,
        run_id: Uuid,
        candidate: &FrozenRunSnapshot,
    ) -> Result<FreezeSnapshotOutcome> {
        tokio::time::timeout(
            Duration::from_secs(5),
            freeze(self, scope, lease, original, run_id, candidate),
        )
        .await
        .map_err(|_| invalid("run snapshot admission timed out".to_owned()))?
    }
}

async fn load(
    control: &PgControlPlane,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    run_id: Uuid,
) -> Result<Option<FrozenRunSnapshot>> {
    if authority.company_id() != scope.company_id() {
        return Err(invalid("snapshot scope mismatch".to_owned()));
    }
    let mut tx = control.pool().begin().await?;
    bounds(&mut tx).await?;
    let row=sqlx::query("SELECT s.spec_bytes,s.spec_hash FROM run_context_snapshots s JOIN runs r ON r.company_id=s.company_id AND r.id=s.run_id WHERE s.company_id=$1 AND s.run_id=$2 AND r.employee_id=$3 AND r.employee_revision_id=$4 AND r.routing_decision_id IS NOT DISTINCT FROM $5 AND r.message_id IS NOT DISTINCT FROM $6 AND r.root_message_id IS NOT DISTINCT FROM $7 AND r.runtime_adapter=$8 AND r.work_item_id IS NOT DISTINCT FROM $9")
        .bind(scope.company_id()).bind(run_id).bind(authority.employee_id().as_str())
        .bind(authority.employee_revision_id()).bind(authority.routing_decision_id())
        .bind(authority.message_id().map(|id| id.as_bytes().to_vec())).bind(authority.root_message_id().map(|id| id.as_bytes().to_vec()))
        .bind(&authority.binding().adapter).bind(authority.work_origin().map(|work|work.work_item_id)).fetch_optional(&mut *tx).await?;
    let result = row
        .as_ref()
        .map(|row| decode(row, authority, run_id))
        .transpose()?;
    if result.as_ref().is_some_and(|s| {
        s.reviewed().is_some()
            || s.conversation().is_some()
            || s.employee().is_some()
            || s.spec().context.conversation_context.is_some()
    }) {
        let current: bool = sqlx::query_scalar("SELECT ortak_run_reviewed_memory_current($1,$2)")
            .bind(scope.company_id())
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await?;
        if !current {
            return Err(invalid(
                "stored reviewed context no longer authorized".into(),
            ));
        }
    }
    tx.commit().await?;
    Ok(result)
}

fn decode(
    row: &sqlx::postgres::PgRow,
    authority: &DispatchAuthority,
    run_id: Uuid,
) -> Result<FrozenRunSnapshot> {
    let bytes: Vec<u8> = row.try_get("spec_bytes")?;
    let hash: Vec<u8> = row.try_get("spec_hash")?;
    if Sha256::digest(&bytes).as_slice() != hash.as_slice() {
        return Err(invalid("stored run snapshot digest mismatch".to_owned()));
    }
    FrozenRunSnapshot::decode(&bytes, authority, run_id)
}

async fn freeze(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &OutboxLease,
    original: &DispatchAuthority,
    run_id: Uuid,
    candidate: &FrozenRunSnapshot,
) -> Result<FreezeSnapshotOutcome> {
    if original.company_id() != scope.company_id()
        || original.outbox_id() != lease.id
        || original.lease_token() != lease.lease_token
    {
        return Err(invalid(
            "snapshot admission scope or lease mismatch".to_owned(),
        ));
    }
    candidate.validate_for(original, run_id)?;
    let mut tx = control.pool().begin().await?;
    bounds(&mut tx).await?;
    let witness = lock_office_authority_on(&mut tx, scope).await?;
    let fresh = match authority::authorize_on(&mut tx, scope, lease, witness.clone()).await? {
        DispatchAuthorization::Authorized(authority) => authority,
        DispatchAuthorization::Refused(reason) => {
            return Ok(FreezeSnapshotOutcome::Refused(reason));
        }
        DispatchAuthorization::StaleLease => return Ok(FreezeSnapshotOutcome::StaleLease),
    };
    candidate.validate_for(&fresh, run_id)?;
    let existing: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2)",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .fetch_one(&mut *tx)
    .await?;
    let mut selected = candidate.clone();
    if !existing && selected.spec().context.conversation_context.is_none() {
        if let Some(context) =
            super::conversation_context::select(&mut tx, scope, &fresh, run_id).await?
        {
            selected = selected.with_conversation_context(&fresh, context)?;
        }
    }
    let candidate = &selected;
    if !super::reviewed_memory::validate_candidate(&mut tx, scope, &fresh, candidate).await? {
        return Ok(FreezeSnapshotOutcome::Refused(
            DispatchRefusal::MemoryContextRejected,
        ));
    }
    let run=sqlx::query("SELECT status,employee_id,employee_revision_id,routing_decision_id,message_id,root_message_id,runtime_adapter,work_item_id FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company_id()).bind(run_id).fetch_optional(&mut *tx).await?
        .ok_or(RunSupervisionError::UnknownRun {run_id})?;
    if run.try_get::<String, _>("employee_id")? != fresh.employee_id().as_str()
        || run.try_get::<Uuid, _>("employee_revision_id")? != fresh.employee_revision_id()
        || run.try_get::<Option<Uuid>, _>("routing_decision_id")? != fresh.routing_decision_id()
        || run.try_get::<Option<Vec<u8>>, _>("message_id")?.as_deref()
            != fresh
                .message_id()
                .map(|id| id.as_bytes().to_vec())
                .as_deref()
        || run
            .try_get::<Option<Vec<u8>>, _>("root_message_id")?
            .as_deref()
            != fresh
                .root_message_id()
                .map(|id| id.as_bytes().to_vec())
                .as_deref()
        || run.try_get::<String, _>("runtime_adapter")? != fresh.binding().adapter
        || run.try_get::<Option<Uuid>, _>("work_item_id")?
            != fresh.work_origin().map(|work| work.work_item_id)
        || fresh
            .work_origin()
            .is_some_and(|work| work.run_id != run_id)
    {
        return Err(RunSupervisionError::RunPinnedDifferently { run_id });
    }
    let cancelled:bool=sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2) OR EXISTS (SELECT 1 FROM run_cancel_requests WHERE company_id=$1 AND run_id=$2)")
        .bind(scope.company_id()).bind(run_id).fetch_one(&mut *tx).await?;
    if cancelled {
        return Ok(FreezeSnapshotOutcome::Refused(
            DispatchRefusal::CancellationRequested,
        ));
    }
    if run.try_get::<String, _>("status")? != "queued" {
        return Ok(FreezeSnapshotOutcome::Refused(
            DispatchRefusal::OfficeAuthorityChanged,
        ));
    }
    // Final lease check also locks outbox in the established run→outbox order.
    let lease_deadline: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar("UPDATE outbox SET updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND lease_token=$3 AND state='pending' AND kind=$7 AND lease_expires_at>clock_timestamp() AND run_id=$4 AND routing_decision_id IS NOT DISTINCT FROM $5 AND employee_id=$6 RETURNING lease_expires_at")
        .bind(scope.company_id()).bind(lease.id).bind(lease.lease_token).bind(run_id)
        .bind(fresh.routing_decision_id()).bind(fresh.employee_id().as_str())
        .bind(if fresh.work_origin().is_some() {"work_run_dispatch"} else {"run_dispatch"}).fetch_optional(&mut *tx).await?;
    let Some(lease_deadline) = lease_deadline else {
        return Ok(FreezeSnapshotOutcome::StaleLease);
    };
    // The existing deferred run witness also fences lease expiry at COMMIT.
    let admission_deadline = witness
        .valid_before()
        .map_or(lease_deadline, |office| office.min(lease_deadline));
    let bytes = candidate.encode()?;
    sqlx::query("INSERT INTO run_context_snapshots(company_id,run_id,spec_bytes,spec_hash) VALUES ($1,$2,$3,$4) ON CONFLICT (company_id,run_id) DO NOTHING")
        .bind(scope.company_id()).bind(run_id).bind(&bytes).bind(Sha256::digest(&bytes).as_slice())
        .execute(&mut *tx).await?;
    let row = sqlx::query(
        "SELECT spec_bytes,spec_hash FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .fetch_one(&mut *tx)
    .await?;
    let winner = decode(&row, &fresh, run_id)?;
    // A concurrent different winner may contain fact rows absent when this
    // transaction derived and locked its candidate. Retry under that winner's
    // current locks instead of admitting text protected only by our stale set.
    if (winner.reviewed().is_some()
        || winner.conversation().is_some()
        || winner.employee().is_some()
        || winner.spec().context.conversation_context.is_some())
        && winner.encode()? != bytes
    {
        return Ok(FreezeSnapshotOutcome::Refused(
            DispatchRefusal::MemoryContextRejected,
        ));
    }
    super::reviewed_memory::persist(&mut tx, scope, run_id, &winner).await?;
    if fresh.work_origin().is_some() {
        super::work::renew(&mut tx, scope, &fresh, run_id, Some(lease_deadline)).await?;
    } else {
        sqlx::query("UPDATE runs SET office_admission_generation=$3,office_admission_valid_before=$4,office_admission_token=$5 WHERE company_id=$1 AND id=$2")
        .bind(scope.company_id()).bind(run_id).bind(witness.generation()).bind(admission_deadline).bind(Uuid::new_v4())
        .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(FreezeSnapshotOutcome::Ready(Box::new(winner)))
}
