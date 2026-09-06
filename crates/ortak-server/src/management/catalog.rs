use crate::provisioning::ProvisioningConfig;
use ortak_control::{ports::CompanyDirectory, PgControlPlane};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogImport {
    community_id: Uuid,
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: Uuid,
    label: String,
    configuration: Value,
}

/// Imports a complete, finite prepared-resource catalog without reading any
/// credentials or contacting adapters. Existing IDs are immutable. Omitted
/// choices are retired, while frozen drafts/commands remain recoverable.
pub async fn import_prepared_catalog(
    control: &PgControlPlane,
    json: &str,
) -> Result<usize, &'static str> {
    if json.len() > 4 * 1024 * 1024 {
        return Err("prepared catalog exceeds limit");
    }
    let catalog: CatalogImport =
        serde_json::from_str(json).map_err(|_| "invalid prepared catalog")?;
    if catalog.entries.len() > 64 {
        return Err("too many prepared choices");
    }
    let scope = control
        .resolve_company_for_community(catalog.community_id)
        .await
        .map_err(|_| "catalog company unavailable")?;
    let mut ids = BTreeSet::new();
    let mut prepared = Vec::new();
    for mut entry in catalog.entries {
        if entry.id.is_nil()
            || !ids.insert(entry.id)
            || entry.label.trim().is_empty()
            || entry.label.len() > 128
            || entry.label.chars().any(char::is_control)
        {
            return Err("invalid prepared choice");
        }
        if serde_json::to_vec(&entry.configuration)
            .map_err(|_| "invalid prepared configuration")?
            .len()
            > 65536
        {
            return Err("prepared configuration exceeds limit");
        }
        let config: ProvisioningConfig = serde_json::from_value(entry.configuration.clone())
            .map_err(|_| "invalid prepared configuration")?;
        config.validate(&scope)?;
        if config.dry_run {
            return Err("catalog requires actual prepared-resource configuration");
        }
        // Repository manifests include serde defaults. Freeze that same typed
        // representation so an omitted default cannot break exact admission.
        entry.configuration["manifest"] =
            serde_json::to_value(&config.manifest).map_err(|_| "invalid prepared manifest")?;
        let employee = config.manifest.employee.id;
        let fingerprint = super::fingerprint(&entry.configuration)?;
        prepared.push((entry, employee, fingerprint));
    }
    let mut tx = control
        .pool()
        .begin()
        .await
        .map_err(|_| "catalog database unavailable")?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("ortak-prepared-catalog:{}", scope.company_id()))
        .execute(&mut *tx)
        .await
        .map_err(|_| "catalog lock unavailable")?;
    sqlx::query(
        "UPDATE prepared_employee_catalog SET enabled=false WHERE company_id=$1 AND enabled",
    )
    .bind(scope.company_id())
    .execute(&mut *tx)
    .await
    .map_err(|_| "catalog retirement failed")?;
    for (entry, employee, fingerprint) in &prepared {
        sqlx::query("INSERT INTO prepared_employee_catalog(company_id,id,employee_id,label,configuration,fingerprint) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(company_id,id) DO NOTHING")
            .bind(scope.company_id()).bind(entry.id).bind(employee.as_str()).bind(&entry.label).bind(&entry.configuration).bind(fingerprint)
            .execute(&mut *tx).await.map_err(|_| "catalog persistence failed")?;
        let matches:bool=sqlx::query_scalar("SELECT fingerprint=$3 AND label=$4 AND employee_id=$5 FROM prepared_employee_catalog WHERE company_id=$1 AND id=$2")
            .bind(scope.company_id()).bind(entry.id).bind(fingerprint).bind(&entry.label).bind(employee.as_str()).fetch_one(&mut *tx).await.map_err(|_| "catalog verification failed")?;
        if !matches {
            return Err("prepared choice changed; import a new immutable ID");
        }
        sqlx::query(
            "UPDATE prepared_employee_catalog SET enabled=true WHERE company_id=$1 AND id=$2",
        )
        .bind(scope.company_id())
        .bind(entry.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| "catalog selection failed")?;
    }
    tx.commit().await.map_err(|_| "catalog commit failed")?;
    Ok(prepared.len())
}
