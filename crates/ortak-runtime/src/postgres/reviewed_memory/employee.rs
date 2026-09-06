//! Atomic v5 validation and immutable use persistence; no remote I/O.
use super::*;
use crate::memory_context::{
    EmployeeContextRecord, EmployeeMemoryOrigin, ReviewedEmployeeRecord, ReviewedMemoryPin,
};

mod persist;
pub(super) use persist::persist;

pub(super) async fn validate(
    c: &mut PgConnection,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    snapshot: &FrozenRunSnapshot,
) -> Result<bool> {
    let context = snapshot
        .employee()
        .ok_or_else(|| invalid("employee context absent".into()))?;
    context.validate_for(authority)?;
    let origin = context.origin.parsed()?;
    let mut channels = vec![origin.source.channel_id, origin.destination_channel_id];
    let mut employee_ids = vec![];
    let mut employee_targets = vec![];
    let mut legacy_ids = vec![];
    let mut legacy_targets = vec![];
    for record in &context.records {
        match record {
            EmployeeContextRecord::Employee { record } => {
                channels.push(record.validate()?.source().channel_id());
                employee_ids.push(record.pin.fact_id);
                employee_targets.push(record.pin.target_id);
            }
            EmployeeContextRecord::Project { record } => {
                legacy_ids.push(record.pin.fact_id);
                legacy_targets.push(record.pin.target_id);
            }
            EmployeeContextRecord::Conversation { record } => {
                legacy_ids.push(record.pin.fact_id);
                legacy_targets.push(record.pin.target_id);
            }
        }
    }
    // Office is already held. Acquire every scope before any fact/target row.
    // Existing Work derivation has already held its project and Work item.
    if let Some(conversation) = &context.conversation_origin {
        let parsed = conversation.parsed_provenance()?;
        sqlx::query("SELECT id FROM projects WHERE company_id=$1 AND id=$2 FOR SHARE NOWAIT")
            .bind(scope.company_id())
            .bind(parsed.audience().project_id())
            .fetch_one(&mut *c)
            .await?;
        sqlx::query("SELECT epoch FROM conversation_memory_authorities WHERE company_id=$1 AND project_id=$2
            AND channel_id=$3 FOR SHARE NOWAIT")
            .bind(scope.company_id()).bind(parsed.audience().project_id()).bind(parsed.audience().channel_id())
            .fetch_one(&mut *c).await?;
    }
    channels.sort();
    channels.dedup();
    sqlx::query("SELECT channel_id FROM employee_memory_channel_authorities WHERE company_id=$1 AND employee_id=$2
        AND channel_id=ANY($3) ORDER BY channel_id FOR SHARE NOWAIT")
        .bind(scope.company_id()).bind(authority.employee_id().as_str()).bind(channels).fetch_all(&mut *c).await?;
    sqlx::query("SELECT id FROM reviewed_memory_facts WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE NOWAIT")
        .bind(scope.company_id()).bind(legacy_ids).fetch_all(&mut *c).await?;
    sqlx::query("SELECT id FROM employee_reviewed_memory_facts WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE NOWAIT")
        .bind(scope.company_id()).bind(employee_ids).fetch_all(&mut *c).await?;
    sqlx::query("SELECT id FROM reviewed_memory_targets WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE NOWAIT")
        .bind(scope.company_id()).bind(legacy_targets).fetch_all(&mut *c).await?;
    sqlx::query("SELECT id FROM employee_reviewed_memory_targets WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE NOWAIT")
        .bind(scope.company_id()).bind(employee_targets).fetch_all(&mut *c).await?;
    let observed: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT origin_bytes FROM ortak_employee_memory_run_origin($1,$2,$3)")
            .bind(scope.company_id())
            .bind(snapshot.spec().run_id)
            .bind(origin.destination_channel_id)
            .fetch_optional(&mut *c)
            .await?;
    if observed
        .as_deref()
        .map(EmployeeMemoryOrigin::from_observation)
        .transpose()?
        .as_ref()
        != Some(&context.origin)
    {
        return Ok(false);
    }
    let legacy = snapshot.employee_legacy_projection(authority)?;
    if !super::validate_legacy_candidate(c, scope, authority, &legacy).await? {
        return Ok(false);
    }
    for item in &context.records {
        if let EmployeeContextRecord::Employee { record } = item {
            if !current(c, scope, authority, snapshot.spec().run_id, record).await? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

async fn current(
    c: &mut PgConnection,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    run: Uuid,
    record: &ReviewedEmployeeRecord,
) -> Result<bool> {
    let p = &record.pin;
    let current:Option<bool>=sqlx::query_scalar("SELECT f.version::bigint=$5 AND f.approval_id=$6 AND encode(f.approved_by,'hex')=$7
        AND f.expires_at=$8 AND f.content=$9 AND f.provenance_bytes=$10 AND encode(f.content_hash,'hex')=$11
        AND encode(f.source_hash,'hex')=$12 AND encode(f.sharing_hash,'hex')=$13 AND encode(f.audience_hash,'hex')=$14
        AND encode(t.binding_hash,'hex')=$15 AND encode(t.namespace_hash,'hex')=$16 AND t.binding=$17
        AND f.employee_id=$18 AND ortak_employee_reviewed_runtime_eligible($1,$2,$3,$4,$19,$20,$21)
        FROM employee_reviewed_memory_facts f JOIN employee_reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
        JOIN employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        WHERE f.company_id=$1 AND f.id=$3 AND t.id=$4")
        .bind(scope.company_id()).bind(run).bind(p.fact_id).bind(p.target_id).bind(p.fact_version)
        .bind(p.approval_id).bind(&p.approved_by).bind(p.expires_at).bind(&record.content).bind(record.provenance.as_bytes())
        .bind(&p.content_hash).bind(&p.source_hash).bind(&p.sharing_hash).bind(&p.audience_hash).bind(&p.binding_hash).bind(&p.namespace_hash)
        .bind(serde_json::to_value(authority.memory_binding()).map_err(|_|invalid("employee binding invalid".into()))?)
        .bind(authority.employee_id().as_str()).bind(p.source_authority_epoch).bind(p.destination_authority_epoch).bind(p.consumption_epoch)
        .fetch_optional(c).await?;
    Ok(current == Some(true))
}
