use super::*;
use ortak_runtime::memory_context::{ReviewedEmployeePin, ReviewedMemoryPin};

fn digest(row: &PgRow, name: &str) -> Result<String> {
    let bytes: Vec<u8> = row.try_get(name)?;
    if bytes.len() != 32 {
        return Err(ApiError::unavailable());
    }
    Ok(hex::encode(bytes))
}

fn common(row: &PgRow) -> Result<ReviewedMemoryPin> {
    Ok(ReviewedMemoryPin {
        fact_id: row.try_get("fact_id")?,
        target_id: row.try_get("target_id")?,
        fact_version: row.try_get("fact_version")?,
        consumption_epoch: row.try_get("consumption_epoch")?,
        content_hash: digest(row, "content_hash")?,
        source_hash: digest(row, "source_hash")?,
        binding_hash: digest(row, "binding_hash")?,
        approval_id: row.try_get("approval_id")?,
        approved_by: row.try_get("approved_by")?,
        expires_at: row.try_get("expires_at")?,
    })
}

pub(super) fn employee(row: &PgRow) -> Result<ReviewedEmployeePin> {
    let p = common(row)?;
    Ok(ReviewedEmployeePin {
        fact_id: p.fact_id,
        target_id: p.target_id,
        fact_version: p.fact_version,
        content_hash: p.content_hash,
        source_hash: p.source_hash,
        binding_hash: p.binding_hash,
        approval_id: p.approval_id,
        approved_by: p.approved_by,
        expires_at: p.expires_at,
        consumption_epoch: p.consumption_epoch,
        sharing_hash: digest(row, "sharing_hash")?,
        audience_hash: digest(row, "audience_hash")?,
        namespace_hash: digest(row, "namespace_hash")?,
        source_authority_epoch: row.try_get("source_authority_epoch")?,
        destination_authority_epoch: row.try_get("destination_authority_epoch")?,
    })
}

pub(super) fn legacy(record: &EmployeeContextRecord, row: &PgRow) -> Result<()> {
    let pin = common(row)?;
    let kind: &str = row.try_get("audience_kind")?;
    let matches = match record {
        EmployeeContextRecord::Project { record } => {
            kind == "project"
                && record.pin == pin
                && row
                    .try_get::<Option<Vec<u8>>, _>("conversation_audience_hash")?
                    .is_none()
        }
        EmployeeContextRecord::Conversation { record } => {
            let p = &record.pin;
            kind == "conversation"
                && p.fact_id == pin.fact_id
                && p.target_id == pin.target_id
                && p.fact_version == pin.fact_version
                && p.content_hash == pin.content_hash
                && p.source_hash == pin.source_hash
                && p.binding_hash == pin.binding_hash
                && p.approval_id == pin.approval_id
                && p.approved_by == pin.approved_by
                && p.expires_at == pin.expires_at
                && p.consumption_epoch == pin.consumption_epoch
                && p.conversation_audience_hash == digest(row, "conversation_audience_hash")?
                && Some(p.conversation_authority_epoch)
                    == row.try_get("conversation_authority_epoch")?
                && Some(p.conversation_consumption_epoch)
                    == row.try_get("conversation_consumption_epoch")?
                && Some(record.provenance.as_bytes())
                    == row
                        .try_get::<Option<Vec<u8>>, _>("provenance_bytes")?
                        .as_deref()
        }
        EmployeeContextRecord::Employee { .. } => false,
    };
    if !matches
        || record.content() != row.try_get::<String, _>("content")?
        || hex::encode(Sha256::digest(record.content().as_bytes())) != pin.content_hash
    {
        return Err(ApiError::unavailable());
    }
    Ok(())
}
