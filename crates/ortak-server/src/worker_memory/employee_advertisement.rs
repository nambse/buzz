//! Default-off runtime selection, independent of namespace I/O witness freshness.
use super::*;
use ortak_control::PgControlPlane;
use sqlx::Row;

pub(super) async fn apply(
    worker: &WorkerMemory,
    control: &PgControlPlane,
    scope: &CompanyScope,
) -> ortak_work::Result<()> {
    tokio::time::timeout(Duration::from_secs(5), apply_on(worker, control, scope))
        .await
        .map_err(|_| ortak_work::WorkError::OperationTimedOut)?
}

async fn apply_on(
    worker: &WorkerMemory,
    control: &PgControlPlane,
    scope: &CompanyScope,
) -> ortak_work::Result<()> {
    let selections = {
        let values = worker
            .validations
            .lock()
            .map_err(|_| ortak_work::WorkError::OperationTimedOut)?;
        values
            .iter()
            .flat_map(|v| {
                v.reviewed_employee_destinations
                    .iter()
                    .map(|s| (*s, v.creation_receipt.clone()))
            })
            .collect::<Vec<_>>()
    };
    let mut tx = control.pool().begin().await?;
    sqlx::query("SELECT set_config('lock_timeout','500ms',true),set_config('statement_timeout','2s',true),set_config('idle_in_transaction_session_timeout','5s',true)")
        .execute(&mut *tx).await?;
    let installed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('public.employee_reviewed_memory_targets')
        AND attname='runtime_consumption_enabled' AND NOT attisdropped)").fetch_one(&mut *tx).await?;
    if !installed {
        if !selections.is_empty() {
            return Err(ortak_work::WorkError::OperationTimedOut);
        }
        tx.commit().await?;
        return Ok(());
    }
    ortak_control::postgres::lock_office_authority_on(&mut tx, scope).await?;
    // All target scope rows precede target writes. Missing config retires old
    // consumption epochs; re-enabling cannot revive a previously frozen use.
    sqlx::query(
        "SELECT channel_id FROM employee_memory_channel_authorities WHERE company_id=$1
        ORDER BY employee_id,channel_id FOR SHARE",
    )
    .bind(scope.company_id())
    .fetch_all(&mut *tx)
    .await?;
    let rows = sqlx::query(
        "SELECT id,employee_id,destination_channel_id,creation_receipt,runtime_consumption_enabled
        FROM employee_reviewed_memory_targets WHERE company_id=$1 ORDER BY id LIMIT 129 FOR UPDATE",
    )
    .bind(scope.company_id())
    .fetch_all(&mut *tx)
    .await?;
    if rows.len() > 128 {
        return Err(ortak_work::WorkError::OperationTimedOut);
    }
    let mut found = BTreeSet::new();
    for row in rows {
        let id: Uuid = row.try_get("id")?;
        let selected = selections.iter().find(|(s, _)| s.target_id == id);
        let enabled = selected.is_some();
        if let Some((selection, original)) = selected {
            let original = original
                .as_ref()
                .ok_or(ortak_work::WorkError::OperationTimedOut)?;
            let mut stored: serde_json::Value = row.try_get("creation_receipt")?;
            let object = stored
                .as_object_mut()
                .ok_or(ortak_work::WorkError::OperationTimedOut)?;
            let protocol = object.remove("protocol");
            object.remove("namespace_hash");
            if protocol != Some(serde_json::json!("reviewed-employee/1"))
                || serde_json::from_value::<HonchoCreatedResourcesReceipt>(stored)
                    .ok()
                    .as_ref()
                    != Some(original)
                || original.company_id != scope.company_id()
                || original.employee_id.as_str() != row.try_get::<String, _>("employee_id")?
                || selection.destination_channel_id
                    != row.try_get::<Uuid, _>("destination_channel_id")?
            {
                return Err(ortak_work::WorkError::OperationTimedOut);
            }
            found.insert(id);
        }
        if row.try_get::<bool, _>("runtime_consumption_enabled")? != enabled {
            sqlx::query("UPDATE employee_reviewed_memory_targets SET runtime_consumption_enabled=$3 WHERE company_id=$1 AND id=$2")
                .bind(scope.company_id()).bind(id).bind(enabled).execute(&mut *tx).await?;
        }
    }
    if found.len() != selections.len() {
        return Err(ortak_work::WorkError::OperationTimedOut);
    }
    tx.commit().await?;
    Ok(())
}
