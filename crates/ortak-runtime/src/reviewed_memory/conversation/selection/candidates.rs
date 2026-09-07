use super::*;
use sqlx::PgConnection;
mod query;

pub(super) struct Candidate {
    pub pin: ReviewedSelectionPin,
    pub content: String,
}

pub(super) async fn read(
    connection: &mut PgConnection,
    selected: &mut ReviewedConversationSelection,
    run: Uuid,
    query: &str,
    include_project: bool,
) -> RuntimeResult<Vec<Candidate>> {
    // All scope/ACL/publication/current epoch checks precede LIMIT. The two
    // eligibility functions remain separate; project opt-in never admits Office.
    let initial = query::rows(connection, selected, run, query, include_project, None).await?;
    selected.truncated |= initial.len() == 32;
    let ids = initial
        .iter()
        .map(|row| row.try_get::<Uuid, _>("id"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let targets = initial
        .iter()
        .map(|row| row.try_get::<Uuid, _>("target_id"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    // Priority order is not lock order. Lock all exact facts, then targets,
    // each by UUID; re-read current eligibility after acquiring those locks.
    sqlx::query("SELECT id FROM reviewed_memory_facts WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE")
        .bind(selected.company_id).bind(&ids).fetch_all(&mut *connection).await?;
    sqlx::query("SELECT id FROM reviewed_memory_targets WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE")
        .bind(selected.company_id).bind(&targets).fetch_all(&mut *connection).await?;
    let rows = query::rows(
        connection,
        selected,
        run,
        query,
        include_project,
        Some(&ids),
    )
    .await?;
    rows.iter()
        .map(|row| {
            let common = ReviewedMemoryPin {
                fact_id: row.try_get("id")?,
                target_id: row.try_get("target_id")?,
                fact_version: row.try_get("version")?,
                consumption_epoch: row.try_get("consumption_epoch")?,
                content_hash: hex::encode(row.try_get::<Vec<u8>, _>("content_hash")?),
                source_hash: hex::encode(row.try_get::<Vec<u8>, _>("source_hash")?),
                binding_hash: hex::encode(row.try_get::<Vec<u8>, _>("binding_hash")?),
                approval_id: row.try_get("promotion_operation_id")?,
                approved_by: row.try_get("approved_by")?,
                expires_at: row.try_get("expires_at")?,
            };
            let pin = match row.try_get::<String, _>("audience_kind")?.as_str() {
                "project" => ReviewedSelectionPin::Project { pin: common },
                "conversation" => ReviewedSelectionPin::Conversation {
                    pin: ReviewedConversationPin {
                        fact_id: common.fact_id,
                        target_id: common.target_id,
                        fact_version: common.fact_version,
                        consumption_epoch: 0,
                        content_hash: common.content_hash,
                        source_hash: common.source_hash,
                        binding_hash: common.binding_hash,
                        approval_id: common.approval_id,
                        approved_by: common.approved_by,
                        expires_at: common.expires_at,
                        conversation_audience_hash: hex::encode(
                            row.try_get::<Vec<u8>, _>("audience_hash")?,
                        ),
                        conversation_authority_epoch: row.try_get("authority_epoch")?,
                        conversation_consumption_epoch: row
                            .try_get("conversation_consumption_epoch")?,
                    },
                    provenance: String::from_utf8(row.try_get("provenance_bytes")?)
                        .map_err(|_| invalid())?,
                },
                _ => return Err(invalid()),
            };
            Ok(Candidate {
                pin,
                content: row.try_get("content")?,
            })
        })
        .collect()
}

pub(super) fn choose(
    selected: &mut ReviewedConversationSelection,
    authority: &DispatchAuthority,
    candidates: Vec<Candidate>,
) -> RuntimeResult<()> {
    if candidates.len() > 32 || !selected.records.is_empty() {
        return Err(invalid());
    }
    // Exactly 32 observed candidates means the bounded scan may omit more.
    selected.truncated |= candidates.len() == 32;
    let mut ordered = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for candidate in candidates {
        if !ids.insert(candidate.pin.fact_id()) {
            return Err(invalid());
        }
        let record = candidate.pin.record(candidate.content);
        let priority = match &record {
            ReviewedContextRecord::Project { record } => {
                if authority.work_origin().is_none() {
                    return Err(invalid());
                }
                ReviewedMemoryContext {
                    records: vec![record.clone()],
                    truncated: false,
                }
                .validate()?;
                2
            }
            ReviewedContextRecord::Conversation { record } => {
                let p = record.validate()?;
                if p.audience().thread_root().is_some() {
                    0
                } else {
                    1
                }
            }
        };
        let kind = if priority == 2 {
            "reviewed_project_memory"
        } else {
            "reviewed_conversation_memory"
        };
        let raw = match &record {
            ReviewedContextRecord::Project { record } => {
                serde_json::json!({"type":kind,"trust":"untrusted_data","record":record})
            }
            ReviewedContextRecord::Conversation { record } => {
                serde_json::json!({"type":kind,"trust":"untrusted_data","record":record})
            }
        };
        if serde_json::to_vec(&raw).map_err(|_| invalid())?.len() > 8192 {
            selected.truncated = true;
            continue;
        }
        if priority != 2 {
            ReviewedConversationContext {
                origin: selected.origin.clone(),
                records: vec![record.clone()],
                truncated: false,
            }
            .validate_for(authority)?;
        }
        ordered.push((
            priority,
            candidate.pin.fact_id(),
            candidate.pin,
            record.content().len(),
        ));
    }
    ordered.sort_by_key(|(priority, id, _, _)| (*priority, *id));
    let mut bytes = 0usize;
    for (_, _, pin, size) in ordered {
        if selected.records.len() == 8 || bytes + size > 8192 {
            selected.truncated = true;
            continue;
        }
        bytes += size;
        selected.records.push(pin);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
