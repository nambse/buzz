use super::*;
use crate::{DispatchAuthority, RunDispatchRepository};
use ortak_control::outbox::OutboxLease;

pub(crate) async fn preflight(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &OutboxLease,
    original: &DispatchAuthority,
    grant: &WorkspaceGrant,
) -> Result<()> {
    let fresh = match control.authorize_dispatch(scope, lease).await? {
        DispatchAuthorization::Authorized(a) => a,
        _ => return Err(invalid("workspace dispatch is no longer authorized".into())),
    };
    if fresh.work_origin() != original.work_origin()
        || fresh.employee_id() != &grant.employee_id
        || fresh.work_origin().map(|w| w.project_id) != Some(grant.project_id)
    {
        return Err(invalid("workspace dispatch scope changed".into()));
    }
    let mut connection = control.pool().acquire().await?;
    selected_binding(&mut connection, scope, grant).await?;
    Ok(())
}

async fn selected_binding(
    connection: &mut PgConnection,
    scope: &CompanyScope,
    grant: &WorkspaceGrant,
) -> Result<Uuid> {
    let row = sqlx::query(
        "SELECT community_id,grant_bytes FROM workspace_bindings WHERE company_id=$1 AND id=$2
        AND revoked_at IS NULL AND expires_at>clock_timestamp() FOR SHARE",
    )
    .bind(scope.company_id())
    .bind(grant.revision)
    .fetch_optional(connection)
    .await?;
    let row = row.ok_or_else(|| invalid("selected workspace is not current".into()))?;
    if decode_grant(row.try_get("grant_bytes")?)? != *grant {
        return Err(invalid("selected workspace manifest differs".into()));
    }
    Ok(row.try_get("community_id")?)
}

pub(crate) async fn freeze(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &OutboxLease,
    original: &DispatchAuthority,
    grant: &WorkspaceGrant,
    prepared: &PreparedWorkspace,
) -> Result<()> {
    let work_origin = original
        .work_origin()
        .ok_or_else(|| invalid("workspace needs Work origin".into()))?;
    if prepared.run_id != work_origin.run_id
        || prepared.manifest_hash != grant.manifest_hash
        || prepared.store_ref
            != format!(
                "workspace-run:{}:{}",
                scope.company_id(),
                work_origin.run_id
            )
    {
        return Err(invalid(
            "workspace adapter preparation witness differs".into(),
        ));
    }
    let mut tx = control.pool().begin().await?;
    let witness = ortak_control::postgres::lock_office_authority_on(&mut tx, scope).await?;
    let fresh = match work::derive_on(
        &mut tx,
        scope,
        prepared.run_id,
        lease.id,
        lease.lease_token,
        witness,
    )
    .await?
    {
        DispatchAuthorization::Authorized(a) => a,
        _ => return Err(invalid("workspace admission authority changed".into())),
    };
    if fresh.work_origin() != original.work_origin()
        || fresh.run_spec(prepared.run_id)? != original.run_spec(prepared.run_id)?
    {
        return Err(invalid("workspace admission snapshot changed".into()));
    }
    let community = selected_binding(&mut tx, scope, grant).await?;
    let run=sqlx::query("SELECT employee_revision_id,employee_lifecycle_epoch FROM runs WHERE company_id=$1 AND id=$2
        AND status='queued' AND runtime_run_ref IS NULL FOR UPDATE")
        .bind(scope.company_id()).bind(prepared.run_id).fetch_optional(&mut *tx).await?;
    let run = run.ok_or_else(|| invalid("workspace run no longer accepts preparation".into()))?;
    let lease_live:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM outbox WHERE company_id=$1 AND id=$2
        AND run_id=$3 AND kind='work_run_dispatch' AND state='pending' AND lease_token=$4 AND lease_expires_at>clock_timestamp())
        AND NOT EXISTS(SELECT 1 FROM run_cancel_requests WHERE company_id=$1 AND run_id=$3)
        AND NOT EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=$1 AND run_id=$3)")
        .bind(scope.company_id()).bind(lease.id).bind(prepared.run_id).bind(lease.lease_token).fetch_one(&mut *tx).await?;
    if !lease_live {
        return Err(invalid(
            "workspace dispatch lease or cancellation changed".into(),
        ));
    }
    let existing=sqlx::query("SELECT workspace_id,manifest_hash,store_ref FROM run_workspace_uses WHERE company_id=$1 AND run_id=$2")
        .bind(scope.company_id()).bind(prepared.run_id).fetch_optional(&mut *tx).await?;
    if let Some(existing) = existing {
        if existing.try_get::<Uuid, _>("workspace_id")? != grant.revision
            || hex::encode(existing.try_get::<Vec<u8>, _>("manifest_hash")?) != grant.manifest_hash
            || existing.try_get::<String, _>("store_ref")? != prepared.store_ref
        {
            return Err(invalid(
                "run workspace use is already frozen differently".into(),
            ));
        }
    } else {
        sqlx::query("INSERT INTO run_workspace_uses(company_id,community_id,run_id,workspace_id,manifest_hash,store_ref,
            employee_revision_id,employee_lifecycle_epoch,outbox_id,admission_lease) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(scope.company_id()).bind(community).bind(prepared.run_id).bind(grant.revision)
            .bind(hex::decode(&grant.manifest_hash).map_err(|_|invalid("workspace hash invalid".into()))?).bind(&prepared.store_ref)
            .bind(run.try_get::<Uuid,_>("employee_revision_id")?).bind(run.try_get::<i64,_>("employee_lifecycle_epoch")?)
            .bind(lease.id).bind(lease.lease_token).execute(&mut *tx).await?;
    }
    let current: bool =
        sqlx::query_scalar("SELECT coalesce(ortak_run_workspace_current($1,$2),false)")
            .bind(scope.company_id())
            .bind(prepared.run_id)
            .fetch_one(&mut *tx)
            .await?;
    if !current {
        return Err(invalid("workspace use is not currently permitted".into()));
    }
    work::renew(&mut tx, scope, &fresh, prepared.run_id, None).await?;
    tx.commit().await?;
    Ok(())
}
