use super::*;
use chrono::SecondsFormat;

/// Admit one retained owned target from a private actual-I/O witness. This is an
/// explicit worker/deployment selection API, never an HTTP body or SQL grant.
/// `valid_until` is independently chosen by the operator and at most 90 days;
/// the 55-second witness only admits this initial registration.
pub async fn register_target(
    control: &PgControlPlane,
    scope: &CompanyScope,
    adapter: &HonchoMemoryAdapter,
    witness: &EmployeeNamespaceWitness,
    destination: Uuid,
    valid_until: DateTime<Utc>,
) -> Result<Uuid> {
    let namespace = witness.namespace();
    let r = namespace.original();
    if r.company_id != scope.company_id() || destination.is_nil() {
        return Err(invalid());
    }
    let mut original = serde_json::to_value(r).map_err(|_| invalid())?;
    let object = original.as_object_mut().ok_or_else(invalid)?;
    object.insert("protocol".into(), json!(REVIEWED_EMPLOYEE_PROTOCOL));
    object.insert("namespace_hash".into(), json!(namespace.namespace_hash()));
    let registration = json!({"format":"ortak-employee-namespace-registration/1",
        "diagnostic":witness.diagnostic(),"validated_at":witness.validated_at().to_rfc3339_opts(SecondsFormat::Micros,true)});
    bounded(async {
        let mut tx=control.pool().begin().await?;bounds(&mut tx).await?;
        sqlx::query("SELECT ortak_lock_office_authority($1)").bind(scope.company_id()).execute(&mut *tx).await?;
        let community:Uuid=sqlx::query_scalar("SELECT community_id FROM office_company_bindings WHERE company_id=$1")
            .bind(scope.company_id()).fetch_one(&mut *tx).await?;
        // Exact committed registration replay returns only the retained target;
        // it cannot refresh authority, enable it, or extend its original expiry.
        let existing=sqlx::query("SELECT id,creation_receipt,registration_receipt,valid_until FROM employee_reviewed_memory_targets
            WHERE company_id=$1 AND destination_channel_id=$2 AND employee_id=$3 AND deployment_id=$4 AND binding_hash=$5 FOR SHARE")
            .bind(scope.company_id()).bind(destination).bind(r.employee_id.as_str()).bind(r.deployment_id)
            .bind(bytes(namespace.binding_hash())?).fetch_optional(&mut *tx).await?;
        if let Some(row)=existing {
            if row.try_get::<Value,_>("creation_receipt")?!=original || row.try_get::<Value,_>("registration_receipt")?!=registration
                || row.try_get::<DateTime<Utc>,_>("valid_until")?!=valid_until {return Err(invalid());}
            let id=row.try_get("id")?;tx.commit().await?;return Ok(id);
        }
        adapter.employee_witness_current(witness).map_err(memory_error)?;
        sqlx::query("SELECT ortak_register_employee_memory_authorities($1,$2,$3,$4,$4)")
            .bind(scope.company_id()).bind(community).bind(r.employee_id.as_str()).bind(destination).execute(&mut *tx).await?;
        let id:Uuid=sqlx::query_scalar("INSERT INTO employee_reviewed_memory_targets(company_id,community_id,destination_channel_id,employee_id,deployment_id,
            namespace_bytes,namespace_hash,protocol,binding,creation_receipt,binding_hash,employee_revision_id,employee_lifecycle_epoch,enabled,valid_until,registration_receipt)
            VALUES($1,$2,$3,$4,$5,$6,$7,'reviewed-employee/1',$8,$9,$10,$11,$12,true,$13,$14) RETURNING id")
            .bind(scope.company_id()).bind(community).bind(destination).bind(r.employee_id.as_str()).bind(r.deployment_id)
            .bind(namespace.canonical_namespace().as_bytes()).bind(bytes(namespace.namespace_hash())?)
            .bind(serde_json::to_value(&r.binding).map_err(|_|invalid())?).bind(original).bind(bytes(namespace.binding_hash())?)
            .bind(witness.diagnostic().employee_revision_id).bind(witness.diagnostic().employee_lifecycle_epoch)
            .bind(valid_until).bind(registration).fetch_one(&mut *tx).await?;
        adapter.employee_witness_current(witness).map_err(memory_error)?;
        tx.commit().await?;Ok(id)
    }).await
}

/// Explicit current rebind/enable/disable on the original retained namespace.
/// Read-only inspection is sufficient after registration; no fresh diagnostic,
/// implicit lease renewal, source grant or replacement target is introduced.
pub async fn refresh_target(
    control: &PgControlPlane,
    scope: &CompanyScope,
    adapter: &HonchoMemoryAdapter,
    namespace: &ReviewedEmployeeNamespace,
    target: Uuid,
    enabled: bool,
    expected_valid_until: DateTime<Utc>,
) -> Result<bool> {
    if namespace.original().company_id != scope.company_id() || target.is_nil() {
        return Err(invalid());
    }
    // Disabling is a recovery action and must not require a reachable remote.
    if enabled {
        adapter
            .inspect_reviewed_employee_namespace(namespace.original())
            .await
            .map_err(memory_error)?;
    }
    bounded(async {
        let mut tx=control.pool().begin().await?;bounds(&mut tx).await?;
        sqlx::query("SELECT ortak_lock_office_authority($1)").bind(scope.company_id()).execute(&mut *tx).await?;
        sqlx::query("SELECT a.channel_id FROM employee_memory_channel_authorities a JOIN employee_reviewed_memory_targets t
            ON t.company_id=a.company_id AND t.community_id=a.community_id AND t.employee_id=a.employee_id AND t.destination_channel_id=a.channel_id
            WHERE t.company_id=$1 AND t.id=$2 FOR SHARE OF a")
            .bind(scope.company_id()).bind(target).fetch_optional(&mut *tx).await?;
        let changed=sqlx::query("UPDATE employee_reviewed_memory_targets t SET enabled=$3,
            employee_revision_id=CASE WHEN $3 THEN e.active_revision_id ELSE t.employee_revision_id END,
            employee_lifecycle_epoch=CASE WHEN $3 THEN e.lifecycle_epoch ELSE t.employee_lifecycle_epoch END
            FROM employees e WHERE t.company_id=$1 AND t.id=$2 AND e.company_id=t.company_id AND e.id=t.employee_id
            AND t.employee_id=$4 AND t.deployment_id=$5 AND t.namespace_hash=$6 AND t.binding_hash=$7 AND t.valid_until=$8")
            .bind(scope.company_id()).bind(target).bind(enabled).bind(namespace.original().employee_id.as_str())
            .bind(namespace.original().deployment_id).bind(bytes(namespace.namespace_hash())?).bind(bytes(namespace.binding_hash())?)
            .bind(expected_valid_until).execute(&mut *tx).await?.rows_affected()==1;
        tx.commit().await?;Ok(changed)
    }).await
}
