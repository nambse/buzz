use chrono::{DateTime, Utc};
use ortak_memory::{EmployeeNamespaceDiagnosticReceipt, REVIEWED_EMPLOYEE_PROTOCOL};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::{
    config::{self, Config, Target},
    Result,
};

/// Retained metadata only. This never grants current publication/recall or
/// recreates the adapter's private process-local registration witness.
pub async fn existing(pool: &PgPool, config: &Config, target: &Target) -> Result<Option<Value>> {
    let rows = sqlx::query(
        "SELECT id,community_id,binding,creation_receipt,registration_receipt,
        valid_until,namespace_hash,binding_hash,enabled,runtime_consumption_enabled
        FROM employee_reviewed_memory_targets WHERE company_id=$1 AND employee_id=$2
          AND destination_channel_id=$3 AND deployment_id=$4 LIMIT 2",
    )
    .bind(config.company_id)
    .bind(target.original.employee_id.as_str())
    .bind(target.destination_channel_id)
    .bind(config.deployment.deployment_id)
    .fetch_all(pool)
    .await
    .map_err(|_| "target_readback")?;
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() != 1 {
        return Err("target_ambiguous");
    }
    let row = &rows[0];
    let namespace_hash = hex::encode(
        row.try_get::<Vec<u8>, _>("namespace_hash")
            .map_err(|_| "target_metadata")?,
    );
    let mut original: Value = row
        .try_get("creation_receipt")
        .map_err(|_| "target_metadata")?;
    let object = original.as_object_mut().ok_or("target_metadata")?;
    if object.remove("protocol") != Some(json!(REVIEWED_EMPLOYEE_PROTOCOL))
        || object.remove("namespace_hash") != Some(json!(&namespace_hash))
        || original != serde_json::to_value(&target.original).map_err(|_| "target_metadata")?
        || row
            .try_get::<Uuid, _>("community_id")
            .map_err(|_| "target_metadata")?
            != config.community_id
        || row
            .try_get::<Value, _>("binding")
            .map_err(|_| "target_metadata")?
            != serde_json::to_value(&target.original.binding).map_err(|_| "target_metadata")?
        || row
            .try_get::<DateTime<Utc>, _>("valid_until")
            .map_err(|_| "target_metadata")?
            != target.valid_until
    {
        return Err("target_identity_conflict");
    }
    let registration: Value = row
        .try_get("registration_receipt")
        .map_err(|_| "target_metadata")?;
    let receipt: EmployeeNamespaceDiagnosticReceipt = serde_json::from_value(
        registration
            .get("diagnostic")
            .cloned()
            .ok_or("target_metadata")?,
    )
    .map_err(|_| "target_metadata")?;
    if registration.get("format") != Some(&json!("ortak-employee-namespace-registration/1"))
        || receipt.operation_id != target.diagnostic.operation_id
        || receipt.employee_revision_id != target.diagnostic.employee_revision_id
        || receipt.employee_lifecycle_epoch != target.diagnostic.employee_lifecycle_epoch
        || receipt.challenge_hash != config::hash(target.diagnostic.challenge.as_bytes())
        || !receipt.erased
        || receipt.write_request_hash.is_none()
    {
        return Err("target_diagnostic_conflict");
    }
    Ok(Some(
        json!({"status":"registered_retained","target_id":row.try_get::<Uuid,_>("id").map_err(|_| "target_metadata")?,
        "employee_id":target.original.employee_id,"destination_channel_id":target.destination_channel_id,
        "operation_id":target.diagnostic.operation_id,"namespace_hash":namespace_hash,
        "binding_hash":hex::encode(row.try_get::<Vec<u8>,_>("binding_hash").map_err(|_| "target_metadata")?),
        "valid_until":target.valid_until,"enabled":row.try_get::<bool,_>("enabled").map_err(|_| "target_metadata")?,
        "runtime_consumption_enabled":row.try_get::<bool,_>("runtime_consumption_enabled").map_err(|_| "target_metadata")?,
        "registration_receipt_sha256":config::hash(&serde_json::to_vec(&registration).map_err(|_| "target_metadata")?),
        "current_authority_claimed":false}),
    ))
}

/// A cheap explicit-selection check before diagnostic I/O, not a substitute
/// for register_target's final current destination/Office/ownership guards.
pub async fn current_employee(pool: &PgPool, config: &Config, target: &Target) -> Result<()> {
    let row = sqlx::query("SELECT e.active_revision_id,e.lifecycle_epoch,r.manifest->'memory' AS memory
        FROM employees e JOIN companies c ON c.id=e.company_id AND c.status='active'
        JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
        WHERE e.company_id=$1 AND e.id=$2 AND e.status='active'")
        .bind(config.company_id).bind(target.original.employee_id.as_str())
        .fetch_optional(pool).await.map_err(|_| "employee_read")?.ok_or("employee_unavailable")?;
    if row
        .try_get::<Uuid, _>("active_revision_id")
        .map_err(|_| "employee_read")?
        != target.diagnostic.employee_revision_id
        || row
            .try_get::<i64, _>("lifecycle_epoch")
            .map_err(|_| "employee_read")?
            != target.diagnostic.employee_lifecycle_epoch
        || row
            .try_get::<Value, _>("memory")
            .map_err(|_| "employee_read")?
            != serde_json::to_value(&target.original.binding).map_err(|_| "employee_read")?
    {
        return Err("employee_selection_changed");
    }
    Ok(())
}
