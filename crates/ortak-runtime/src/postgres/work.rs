//! Current Work admission under Office → project → item → run → outbox locks.
mod sql;
use super::{authority, invalid, parse_status};
use crate::authority::{
    validate_pinned_revision, DispatchAuthority, DispatchRefusal, RunInput, WorkRunOrigin,
};
use crate::{DispatchAuthorization, PrepareOutcome, PreparedRun, Result, RunSupervisionError};
use ortak_control::office_authority::OfficeAuthority;
use ortak_control::outbox::{OutboxKind, OutboxLease};
use ortak_control::runtime::RuntimeRunRef;
use ortak_control::{CompanyScope, PgControlPlane};
use ortak_domain::{Employee, EmployeeId};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

fn refused() -> Result<DispatchAuthorization> {
    Ok(DispatchAuthorization::Refused(
        DispatchRefusal::WorkAuthorityChanged,
    ))
}

pub(super) async fn authorize_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    lease: &OutboxLease,
    witness: OfficeAuthority,
) -> Result<DispatchAuthorization> {
    let row = sqlx::query(
        "SELECT kind,state,lease_token,lease_expires_at>clock_timestamp() AS live,
        run_id,employee_id,routing_decision_id,dedup_key FROM outbox WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company_id())
    .bind(lease.id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(RunSupervisionError::UnknownOutboxRow {
        outbox_id: lease.id,
    })?;
    if row.try_get::<String, _>("kind")? != "work_run_dispatch"
        || lease.kind != OutboxKind::WorkRunDispatch
    {
        return Err(RunSupervisionError::WrongKind { found: lease.kind });
    }
    if row.try_get::<Option<Uuid>, _>("run_id")? != lease.run_id
        || row.try_get::<Option<String>, _>("employee_id")? != lease.employee_id
        || row
            .try_get::<Option<Uuid>, _>("routing_decision_id")?
            .is_some()
        || lease.routing_decision_id.is_some()
        || row.try_get::<String, _>("dedup_key")? != lease.dedup_key
    {
        return Err(RunSupervisionError::LeaseInconsistent {
            outbox_id: lease.id,
        });
    }
    if row.try_get::<String, _>("state")? != "pending"
        || row.try_get::<Option<Uuid>, _>("lease_token")? != Some(lease.lease_token)
        || row.try_get::<Option<bool>, _>("live")? != Some(true)
    {
        return Ok(DispatchAuthorization::StaleLease);
    }
    let Some(run) = lease.run_id else {
        return refused();
    };
    derive_on(connection, scope, run, lease.id, lease.lease_token, witness).await
}

pub(crate) async fn derive_on(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    run_id: Uuid,
    outbox_id: Uuid,
    lease_token: Uuid,
    witness: OfficeAuthority,
) -> Result<DispatchAuthorization> {
    let target = sqlx::query(
        "SELECT project_id,work_item_id FROM work_executions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(target) = target else {
        return refused();
    };
    let project: Uuid = target.try_get("project_id")?;
    let item: Uuid = target.try_get("work_item_id")?;
    sqlx::query("SELECT id FROM projects WHERE company_id=$1 AND id=$2 FOR SHARE")
        .bind(scope.company_id())
        .bind(project)
        .fetch_one(&mut *connection)
        .await?;
    sqlx::query("SELECT id FROM work_items WHERE company_id=$1 AND id=$2 FOR SHARE")
        .bind(scope.company_id())
        .bind(item)
        .fetch_one(&mut *connection)
        .await?;
    let reviewed_current: bool = sqlx::query_scalar("SELECT ortak_lock_run_reviewed_memory($1,$2)")
        .bind(scope.company_id())
        .bind(run_id)
        .fetch_one(&mut *connection)
        .await?;
    if !reviewed_current {
        return refused();
    }
    let workspace_current: bool =
        sqlx::query_scalar("SELECT ortak_lock_run_workspace($1,$2,false)")
            .bind(scope.company_id())
            .bind(run_id)
            .fetch_one(&mut *connection)
            .await?;
    if !workspace_current {
        return refused();
    }
    let row = sqlx::query(sql::SOURCE)
        .bind(scope.company_id())
        .bind(run_id)
        .fetch_optional(&mut *connection)
        .await?;
    let Some(row) = row else { return refused() };
    if row.try_get::<String, _>("company_status")? != "active"
        || row.try_get::<String, _>("project_status")? != "active"
        || row.try_get::<String, _>("work_state")? != "in_progress"
        || !row.try_get::<bool, _>("current_lifecycle")?
        || row.try_get::<i64, _>("work_version")? != row.try_get::<i64, _>("execution_version")?
        || !row.try_get::<bool, _>("can_contribute")?
        || !row.try_get::<bool, _>("human_member")?
        || !row.try_get::<bool, _>("assigned")?
        || !row.try_get::<bool, _>("dependencies_clear")?
        || !row.try_get::<bool, _>("no_review")?
        || row
            .try_get::<Option<Uuid>, _>("routing_decision_id")?
            .is_some()
        || row.try_get::<Option<Vec<u8>>, _>("message_id")?.is_some()
        || row
            .try_get::<Option<Vec<u8>>, _>("root_message_id")?
            .is_some()
        || row.try_get::<Option<Uuid>, _>("run_work_item_id")? != Some(item)
        || row.try_get::<String, _>("run_employee_id")?
            != row.try_get::<String, _>("employee_id")?
        || row.try_get::<Uuid, _>("run_revision_id")?
            != row.try_get::<Uuid, _>("employee_revision_id")?
    {
        return refused();
    }
    let employee = EmployeeId::parse(row.try_get::<String, _>("employee_id")?)
        .map_err(|_| invalid("invalid Work executor identity".into()))?;
    let community: Uuid = row.try_get("community_id")?;
    let channel: Uuid = row.try_get("channel_id")?;
    let eligible = ortak_office::normalizer::channel_eligible_employees(
        &mut *connection,
        scope.company_id(),
        community,
        channel,
    )
    .await?;
    if !eligible.contains(&employee) {
        return refused();
    }
    if let Some(source) = row.try_get::<Option<Vec<u8>>, _>("source_message_id")? {
        let visible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM office_inbox i
            JOIN events e ON e.community_id=$2 AND e.id=i.event_id AND e.created_at=i.event_created_at
            AND e.channel_id=i.channel_id AND e.kind=i.event_kind AND e.pubkey=i.author_pubkey
            WHERE i.company_id=$1 AND i.event_id=$4 AND i.channel_id=$3 AND i.state='decided'
            AND e.kind IN(9,40002) AND e.deleted_at IS NULL)")
            .bind(scope.company_id()).bind(community).bind(channel).bind(source).fetch_one(&mut *connection).await?;
        if !visible {
            return refused();
        }
    }
    let manifest: serde_json::Value = row.try_get("manifest")?;
    let configuration = match validate_pinned_revision(
        &employee,
        authority::parse_employee_status(row.try_get("employee_status")?)?,
        &manifest,
        authority::stored_binding(&row)?.as_ref(),
    ) {
        Ok(value) => value,
        Err(reason) => return Ok(DispatchAuthorization::Refused(reason)),
    };
    let pinned: Employee =
        serde_json::from_value(manifest).map_err(|_| invalid("invalid Work revision".into()))?;
    let active: Employee = serde_json::from_value(row.try_get("active_manifest")?)
        .map_err(|_| invalid("invalid active Work revision".into()))?;
    if pinned.office != active.office {
        return refused();
    }
    let configuration = match configuration.with_validated_memory(
        authority::stored_memory(&row, "memory")?.as_ref(),
        active.memory.as_ref(),
    ) {
        Ok(value) => value,
        Err(reason) => return Ok(DispatchAuthorization::Refused(reason)),
    };
    let configuration = match configuration.with_validated_memory(
        authority::stored_memory(&row, "active_memory")?.as_ref(),
        active.memory.as_ref(),
    ) {
        Ok(value) => value,
        Err(reason) => return Ok(DispatchAuthorization::Refused(reason)),
    };
    if row.try_get::<String, _>("runtime_adapter")? != configuration.binding().adapter {
        return refused();
    }
    let bytes: Vec<u8> = row.try_get("definition_bytes")?;
    let hash: Vec<u8> = row.try_get("definition_hash")?;
    if bytes.is_empty()
        || bytes.len() > 32768
        || Sha256::digest(&bytes).as_slice() != hash.as_slice()
    {
        return refused();
    }
    let body =
        String::from_utf8(bytes).map_err(|_| invalid("invalid Work definition bytes".into()))?;
    let work = WorkRunOrigin {
        run_id,
        work_item_id: item,
        project_id: project,
        execution_version: row.try_get("execution_version")?,
        definition_hash: hex::encode(hash),
    };
    Ok(DispatchAuthorization::Authorized(Box::new(
        DispatchAuthority::from_work(
            scope.company_id(),
            outbox_id,
            lease_token,
            employee,
            row.try_get("employee_revision_id")?,
            configuration,
            RunInput {
                body,
                truncated: false,
                channel_id: Some(channel),
                event_kind: 0,
            },
            work,
            row.try_get("generation")?,
        )
        .with_office_authority(witness),
    )))
}

pub(super) async fn prepare(
    control: &PgControlPlane,
    scope: &CompanyScope,
    original: &DispatchAuthority,
) -> Result<PrepareOutcome> {
    if original.company_id() != scope.company_id() {
        return Err(invalid("Work dispatch scope mismatch".into()));
    }
    let work = original
        .work_origin()
        .ok_or_else(|| invalid("Work origin missing".into()))?;
    let mut tx = control.pool().begin().await?;
    let witness = ortak_control::postgres::lock_office_authority_on(&mut tx, scope).await?;
    let fresh = match derive_on(
        &mut tx,
        scope,
        work.run_id,
        original.outbox_id(),
        original.lease_token(),
        witness,
    )
    .await?
    {
        DispatchAuthorization::Authorized(value) => value,
        DispatchAuthorization::Refused(reason) => return Ok(PrepareOutcome::Refused(reason)),
        DispatchAuthorization::StaleLease => return Ok(PrepareOutcome::StaleLease),
    };
    if fresh.work_origin() != original.work_origin()
        || fresh.run_spec(work.run_id)? != original.run_spec(work.run_id)?
    {
        return Ok(PrepareOutcome::Refused(
            DispatchRefusal::WorkAuthorityChanged,
        ));
    }
    let row = sqlx::query(
        "SELECT status,runtime_run_ref FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(scope.company_id())
    .bind(work.run_id)
    .fetch_one(&mut *tx)
    .await?;
    let cancelled: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2)
        OR EXISTS(SELECT 1 FROM run_cancel_requests WHERE company_id=$1 AND run_id=$2)",
    )
    .bind(scope.company_id())
    .bind(work.run_id)
    .fetch_one(&mut *tx)
    .await?;
    if cancelled {
        return Ok(PrepareOutcome::Refused(
            DispatchRefusal::CancellationRequested,
        ));
    }
    let lease_deadline: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar("UPDATE outbox SET updated_at=clock_timestamp()
        WHERE company_id=$1 AND id=$2 AND lease_token=$3 AND lease_expires_at>clock_timestamp() AND state='pending'
        AND kind='work_run_dispatch' AND run_id=$4 AND employee_id=$5 AND routing_decision_id IS NULL RETURNING lease_expires_at")
        .bind(scope.company_id()).bind(original.outbox_id()).bind(original.lease_token()).bind(work.run_id)
        .bind(original.employee_id().as_str()).fetch_optional(&mut *tx).await?;
    let Some(deadline) = lease_deadline else {
        return Ok(PrepareOutcome::StaleLease);
    };
    renew(&mut tx, scope, &fresh, work.run_id, Some(deadline)).await?;
    tx.commit().await?;
    Ok(PrepareOutcome::Prepared(PreparedRun {
        run_id: work.run_id,
        status: parse_status(row.try_get("status")?)?,
        runtime_run_ref: row
            .try_get::<Option<String>, _>("runtime_run_ref")?
            .map(RuntimeRunRef),
        created: false,
    }))
}

pub(super) async fn renew(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    run_id: Uuid,
    lease_deadline: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<()> {
    let witness = authority
        .office_authority()
        .ok_or_else(|| invalid("Work Office witness missing".into()))?;
    let mut deadline = match (witness.valid_before(), lease_deadline) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let reviewed_deadline:Option<chrono::DateTime<chrono::Utc>>=sqlx::query_scalar("SELECT min(least(u.expires_at,t.valid_until))
        FROM run_reviewed_memory_uses u JOIN reviewed_memory_targets t ON t.company_id=u.company_id AND t.id=u.target_id
        WHERE u.company_id=$1 AND u.run_id=$2")
        .bind(scope.company_id()).bind(run_id).fetch_one(&mut *connection).await?;
    if let Some(reviewed) = reviewed_deadline {
        deadline = Some(deadline.map_or(reviewed, |d| d.min(reviewed)));
    }
    let workspace_deadline: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT min(b.expires_at) FROM run_workspace_uses u JOIN workspace_bindings b ON b.company_id=u.company_id AND b.id=u.workspace_id WHERE u.company_id=$1 AND u.run_id=$2")
        .bind(scope.company_id()).bind(run_id).fetch_one(&mut *connection).await?;
    if let Some(workspace) = workspace_deadline {
        deadline = Some(deadline.map_or(workspace, |d| d.min(workspace)));
    }
    sqlx::query("UPDATE runs SET office_admission_generation=$3,office_admission_valid_before=$4,
        office_admission_token=$5,work_admission_generation=$6,work_admission_token=$7 WHERE company_id=$1 AND id=$2")
        .bind(scope.company_id()).bind(run_id).bind(witness.generation()).bind(deadline).bind(Uuid::new_v4())
        .bind(authority.work_generation()).bind(Uuid::new_v4()).execute(connection).await?;
    Ok(())
}

pub(super) async fn refresh(
    control: &PgControlPlane,
    scope: &CompanyScope,
    run_id: Uuid,
) -> Result<bool> {
    let mut tx = control.pool().begin().await?;
    let witness = ortak_control::postgres::lock_office_authority_on(&mut tx, scope).await?;
    let outbox: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM outbox WHERE company_id=$1 AND run_id=$2 AND kind='work_run_dispatch'",
    )
    .bind(scope.company_id())
    .bind(run_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(outbox) = outbox else {
        return Ok(false);
    };
    let fresh = match derive_on(&mut tx, scope, run_id, outbox, Uuid::nil(), witness).await? {
        DispatchAuthorization::Authorized(value) => value,
        _ => return Ok(false),
    };
    sqlx::query("SELECT id FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company_id())
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
    renew(&mut tx, scope, &fresh, run_id, None).await?;
    tx.commit().await?;
    Ok(true)
}
