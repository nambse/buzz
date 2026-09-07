use super::*;
use chrono::{DateTime, Utc};

/// Publishes one actual adapter-verified selected revision. Only an explicit
/// operator configuration calls this; no user-provided verification ID exists.
pub(crate) async fn register(
    control: &PgControlPlane,
    scope: &CompanyScope,
    grant: &WorkspaceGrant,
    expires: DateTime<Utc>,
) -> Result<()> {
    grant.validate()?;
    if grant.company_id != scope.company_id() {
        return Err(invalid("workspace company differs".into()));
    }
    let value =
        serde_json::to_value(grant).map_err(|_| invalid("workspace encoding failed".into()))?;
    let bytes =
        serde_json::to_vec(&value).map_err(|_| invalid("workspace encoding failed".into()))?;
    let hash =
        hex::decode(&grant.manifest_hash).map_err(|_| invalid("workspace hash invalid".into()))?;
    let mut tx = control.pool().begin().await?;
    // Explicit publication mutates Office generation; acquire exclusive before
    // any row locks rather than upgrading a shared fence held elsewhere.
    sqlx::query("SELECT pg_advisory_xact_lock(ortak_office_company_lock_key($1))")
        .bind(scope.company_id())
        .execute(&mut *tx)
        .await?;
    let existing=sqlx::query("SELECT grant_bytes,expires_at,revoked_at FROM workspace_bindings WHERE company_id=$1 AND id=$2")
        .bind(scope.company_id()).bind(grant.revision).fetch_optional(&mut *tx).await?;
    if let Some(existing) = existing {
        if existing.try_get::<Vec<u8>, _>("grant_bytes")? != bytes
            || existing.try_get::<DateTime<Utc>, _>("expires_at")? != expires
            || existing
                .try_get::<Option<DateTime<Utc>>, _>("revoked_at")?
                .is_some()
        {
            return Err(invalid(
                "workspace revision already differs or is withdrawn".into(),
            ));
        }
        tx.commit().await?;
        return Ok(());
    }
    let community:Option<Uuid>=sqlx::query_scalar("SELECT pb.community_id FROM project_api_bindings pb
        JOIN office_company_bindings ob ON ob.company_id=pb.company_id AND ob.community_id=pb.community_id
        JOIN communities cm ON cm.id=pb.community_id JOIN projects p ON p.company_id=pb.company_id AND p.id=pb.project_id
        JOIN companies c ON c.id=pb.company_id JOIN employees e ON e.company_id=c.id AND e.id=$3
        WHERE pb.company_id=$1 AND pb.project_id=$2 AND c.status='active' AND p.status='active'
          AND cm.deletion_state='active' AND cm.deleted_at IS NULL")
        .bind(scope.company_id()).bind(grant.project_id).bind(grant.employee_id.as_str()).fetch_optional(&mut *tx).await?;
    let community =
        community.ok_or_else(|| invalid("workspace project scope unavailable".into()))?;
    sqlx::query("INSERT INTO workspace_bindings(company_id,community_id,project_id,employee_id,id,workspace_ref,
        grant_bytes,manifest_hash,verification_id,verified_at,expires_at)
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,clock_timestamp(),$10)")
        .bind(scope.company_id()).bind(community).bind(grant.project_id).bind(grant.employee_id.as_str()).bind(grant.revision)
        .bind(&grant.workspace_ref).bind(&bytes).bind(hash).bind(Uuid::new_v4()).bind(expires).execute(&mut *tx).await?;
    for (ordinal, file) in grant.files.iter().enumerate() {
        let hash =
            hex::decode(&file.sha256).map_err(|_| invalid("workspace file hash invalid".into()))?;
        sqlx::query("INSERT INTO workspace_files(company_id,community_id,workspace_id,id,ordinal,logical_name,media_type,byte_count,content_hash)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(scope.company_id()).bind(community).bind(grant.revision).bind(file.file_id).bind(ordinal as i32)
            .bind(&file.name).bind(&file.media_type).bind(file.bytes as i32).bind(hash).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Monotonic explicit withdrawal retains all input/run/action provenance.
/// New reads and output are fenced; cancellation reconciliation stops old runs.
pub async fn revoke(
    control: &PgControlPlane,
    scope: &CompanyScope,
    revision: Uuid,
) -> Result<bool> {
    let mut tx = control.pool().begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(ortak_office_company_lock_key($1))")
        .bind(scope.company_id())
        .execute(&mut *tx)
        .await?;
    let changed=sqlx::query("UPDATE workspace_bindings SET revoked_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND revoked_at IS NULL")
        .bind(scope.company_id()).bind(revision).execute(&mut *tx).await?.rows_affected()==1;
    tx.commit().await?;
    Ok(changed)
}
