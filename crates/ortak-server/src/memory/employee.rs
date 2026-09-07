//! V5 history projection. Frozen claims never authorize a new viewer or use.
use super::*;
use ortak_control::memory::employee::{EmployeeMemoryKind, EmployeeMemoryProvenanceV1};
use ortak_runtime::memory_context::{EmployeeContextRecord, ReviewedEmployeeContext};
use sqlx::{postgres::PgRow, PgConnection};

mod pins;

pub(super) struct Input<'a> {
    pub principal: &'a Principal,
    pub community: Uuid,
    pub run: Uuid,
    pub context: &'a ReviewedEmployeeContext,
    pub legacy_rows: &'a [PgRow],
    pub legacy_views: Vec<Value>,
    pub current: bool,
    pub run_row: &'a PgRow,
}

pub(super) async fn project(connection: &mut PgConnection, input: Input<'_>) -> Result<Vec<Value>> {
    let Input {
        principal,
        community,
        run,
        context,
        legacy_rows,
        legacy_views,
        current,
        run_row,
    } = input;
    let (scratch_count, scratch_bytes) = scratch_budget(run_row)?;
    let rows = sqlx::query(
        "SELECT u.*,f.employee_id,f.content,f.provenance_bytes,f.audience_bytes,
        $3 AND ortak_employee_reviewed_runtime_eligible(u.company_id,u.run_id,u.fact_id,u.target_id,
            u.source_authority_epoch,u.destination_authority_epoch,u.consumption_epoch) AS current
        FROM run_employee_reviewed_memory_uses u
        JOIN employee_reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
        WHERE u.company_id=$1 AND u.run_id=$2 ORDER BY u.ordinal LIMIT 9",
    )
    .bind(principal.scope.company_id())
    .bind(run)
    .bind(current)
    .fetch_all(&mut *connection)
    .await?;
    if rows.is_empty()
        || context.records.len() > 8
        || rows.len() + legacy_rows.len() != context.records.len()
        || context.records.len() + scratch_count > MAX_CONTEXT_RECORDS
    {
        return Err(ApiError::unavailable());
    }
    let origin: Value = serde_json::from_slice(context.origin.canonical_bytes())
        .map_err(|_| ApiError::unavailable())?;
    let employee: &str = run_row.try_get("employee_id")?;
    if origin["company_id"] != json!(principal.scope.company_id())
        || origin["employee_id"] != employee
        || origin["source"]["community_id"] != json!(community)
    {
        return Err(ApiError::unavailable());
    }
    let destination = context
        .origin
        .destination_channel_id()
        .map_err(|_| ApiError::unavailable())?;
    let requester = context
        .origin
        .requester_public_key()
        .map_err(|_| ApiError::unavailable())?;
    let mut legacy = legacy_rows
        .iter()
        .zip(legacy_views)
        .collect::<Vec<_>>()
        .into_iter();
    let mut employees = rows.iter();
    let mut records = Vec::with_capacity(context.records.len());
    let mut ids = BTreeSet::new();
    let mut content_bytes = 0usize;
    let mut legacy_started = false;
    let mut employee_order = None;
    let namespace = hex::encode(Sha256::digest(
        serde_json::to_vec(&json!({
        "company_id":principal.scope.company_id(),"employee_id":employee,
        "format":"ortak-reviewed-employee-namespace/1"}))
        .map_err(|_| ApiError::unavailable())?,
    ));
    for (ordinal, record) in context.records.iter().enumerate() {
        content_bytes += record.content().len();
        if !ids.insert(record.fact_id())
            || record.content().trim().is_empty()
            || record.content().len() > 4096
            || content_bytes > 8192
            || content_bytes + scratch_bytes > MAX_CONTEXT_BYTES
        {
            return Err(ApiError::unavailable());
        }
        match record {
            EmployeeContextRecord::Employee { record } => {
                let row = employees.next().ok_or_else(ApiError::unavailable)?;
                let pin = pins::employee(row)?;
                let bytes: Vec<u8> = row.try_get("provenance_bytes")?;
                let provenance = EmployeeMemoryProvenanceV1::from_canonical_bytes(&bytes)
                    .map_err(|_| ApiError::unavailable())?;
                let audience = provenance.audience();
                let approval = provenance.approval();
                let order = (
                    if audience.kind() == EmployeeMemoryKind::Relationship {
                        0
                    } else {
                        1
                    },
                    pin.fact_id,
                );
                if row.try_get::<i32, _>("ordinal")? != ordinal as i32
                    || legacy_started
                    || employee_order.is_some_and(|old| old >= order)
                    || pin != record.pin
                    || record.provenance.as_bytes() != bytes
                    || record.content != row.try_get::<String, _>("content")?
                    || hex::encode(Sha256::digest(record.content.as_bytes())) != pin.content_hash
                    || audience.company_id() != principal.scope.company_id()
                    || audience.employee_id().as_str() != employee
                    || audience.destination_community_id() != community
                    || audience.destination_channel_id() != destination
                    || pin.namespace_hash != namespace
                    || json!(pin.destination_authority_epoch)
                        != origin["destination_authority_epoch"]
                    || audience
                        .human_public_key()
                        .is_some_and(|key| key.to_hex() != requester)
                    || audience
                        .canonical_bytes()
                        .map_err(|_| ApiError::unavailable())?
                        != row.try_get::<Vec<u8>, _>("audience_bytes")?
                    || audience
                        .audience_hash()
                        .map_err(|_| ApiError::unavailable())?
                        .to_hex()
                        != pin.audience_hash
                    || provenance
                        .source_hash()
                        .map_err(|_| ApiError::unavailable())?
                        .to_hex()
                        != pin.source_hash
                    || provenance
                        .sharing_hash()
                        .map_err(|_| ApiError::unavailable())?
                        .to_hex()
                        != pin.sharing_hash
                    || approval.approval_id() != pin.approval_id
                    || approval.approved_by().to_hex() != pin.approved_by
                    || approval.content_hash().to_hex() != pin.content_hash
                    || approval.expires_at() != pin.expires_at
                    || provenance.source().author_public_key() != approval.approved_by()
                {
                    return Err(ApiError::unavailable());
                }
                employee_order = Some(order);
                let readable: bool = row.try_get("current")?;
                records.push(json!({"fact_id":pin.fact_id,"approval_id":pin.approval_id,
                    "approved_by":pin.approved_by,"expires_at":pin.expires_at,"current":readable,
                    "content":if readable {text(&record.content)} else {Value::Null},
                    "audience_kind":"employee","audience":if readable {
                        serde_json::from_slice::<Value>(&row.try_get::<Vec<u8>,_>("audience_bytes")?)
                            .map_err(|_| ApiError::unavailable())?
                    } else {Value::Null}}));
            }
            _ => {
                legacy_started = true;
                let (row, view) = legacy.next().ok_or_else(ApiError::unavailable)?;
                pins::legacy(record, row)?;
                if row.try_get::<i32, _>("ordinal")? != ordinal as i32 {
                    return Err(ApiError::unavailable());
                }
                records.push(view);
            }
        }
    }
    if employees.next().is_some() || legacy.next().is_some() {
        return Err(ApiError::unavailable());
    }
    Ok(records)
}

fn scratch_budget(row: &PgRow) -> Result<(usize, usize)> {
    // Already bounded and hash-checked by the caller; parse only the selective
    // DTO, never runtime configuration or credential-bearing fields.
    let snapshot: Snapshot = serde_json::from_slice(&row.try_get::<Vec<u8>, _>("spec_bytes")?)
        .map_err(|_| ApiError::unavailable())?;
    Ok((
        snapshot.recall.records.len(),
        snapshot
            .recall
            .records
            .iter()
            .map(|record| record.content.len())
            .sum(),
    ))
}
