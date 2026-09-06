use super::*;

pub(super) async fn read(
    c: &mut PgConnection,
    selected: &ReviewedEmployeeSelection,
    run: Uuid,
    ids: Option<&[Uuid]>,
) -> RuntimeResult<Vec<sqlx::postgres::PgRow>> {
    Ok(sqlx::query("SELECT f.*,t.id AS target_id,t.binding_hash,t.namespace_hash,t.consumption_epoch,
        source_scope.epoch AS source_epoch,destination_scope.epoch AS destination_epoch
        FROM employee_reviewed_memory_facts f
        JOIN employee_reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
        JOIN employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        JOIN employee_memory_channel_authorities source_scope ON source_scope.company_id=f.company_id
            AND source_scope.community_id=f.community_id AND source_scope.employee_id=f.employee_id AND source_scope.channel_id=f.source_channel_id
        JOIN employee_memory_channel_authorities destination_scope ON destination_scope.company_id=f.company_id
            AND destination_scope.community_id=f.community_id AND destination_scope.employee_id=f.employee_id AND destination_scope.channel_id=f.destination_channel_id
        WHERE f.company_id=$1 AND f.employee_id=$2 AND f.destination_channel_id=$3 AND t.id=$4
            AND t.binding=$5 AND ($7::uuid[] IS NULL OR f.id=ANY($7))
            AND ortak_employee_reviewed_runtime_eligible($1,$6,f.id,t.id,source_scope.epoch,destination_scope.epoch,t.consumption_epoch)
        ORDER BY CASE WHEN f.kind='relationship' THEN 0 ELSE 1 END,f.id LIMIT 32")
        .bind(selected.company_id).bind(selected.employee_id.as_str()).bind(selected.destination.destination_channel_id)
        .bind(selected.destination.target_id).bind(serde_json::to_value(&selected.binding).map_err(|_|invalid())?)
        .bind(run).bind(ids).fetch_all(c).await?)
}

pub(super) fn record(row: &sqlx::postgres::PgRow) -> RuntimeResult<ReviewedEmployeeRecord> {
    Ok(ReviewedEmployeeRecord {
        pin: ReviewedEmployeePin {
            fact_id: row.try_get("id")?,
            target_id: row.try_get("target_id")?,
            fact_version: i64::from(row.try_get::<i32, _>("version")?),
            content_hash: hex::encode(row.try_get::<Vec<u8>, _>("content_hash")?),
            source_hash: hex::encode(row.try_get::<Vec<u8>, _>("source_hash")?),
            sharing_hash: hex::encode(row.try_get::<Vec<u8>, _>("sharing_hash")?),
            audience_hash: hex::encode(row.try_get::<Vec<u8>, _>("audience_hash")?),
            binding_hash: hex::encode(row.try_get::<Vec<u8>, _>("binding_hash")?),
            namespace_hash: hex::encode(row.try_get::<Vec<u8>, _>("namespace_hash")?),
            approval_id: row.try_get("approval_id")?,
            approved_by: hex::encode(row.try_get::<Vec<u8>, _>("approved_by")?),
            expires_at: row.try_get("expires_at")?,
            source_authority_epoch: row.try_get("source_epoch")?,
            destination_authority_epoch: row.try_get("destination_epoch")?,
            consumption_epoch: row.try_get("consumption_epoch")?,
        },
        content: row.try_get("content")?,
        provenance: String::from_utf8(row.try_get("provenance_bytes")?).map_err(|_| invalid())?,
    })
}
