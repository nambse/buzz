//! Current canonical source and independent epoch pins at the atomic freeze.
use super::*;
use crate::memory_context::{ConversationMemoryOrigin, ReviewedContextRecord, ReviewedMemoryPin};

pub(super) async fn validate(
    c: &mut PgConnection,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    snapshot: &FrozenRunSnapshot,
) -> Result<bool> {
    let Some(context) = snapshot.conversation() else {
        return Ok(false);
    };
    let parsed = context.origin.parsed_provenance()?;
    let project = parsed.audience().project_id();
    let channel = parsed.audience().channel_id();
    let run = snapshot.spec().run_id;
    // The caller already holds Office and, for Work, its project and Work row.
    // New Office candidates have no durable use rows to supply these lock keys.
    sqlx::query("SELECT id FROM projects WHERE company_id=$1 AND id=$2 FOR SHARE NOWAIT")
        .bind(scope.company_id())
        .bind(project)
        .fetch_one(&mut *c)
        .await?;
    let scoped: Option<i64> = sqlx::query_scalar(
        "SELECT epoch FROM conversation_memory_authorities
        WHERE company_id=$1 AND project_id=$2 AND channel_id=$3 FOR SHARE NOWAIT",
    )
    .bind(scope.company_id())
    .bind(project)
    .bind(channel)
    .fetch_optional(&mut *c)
    .await?;
    if scoped.is_none() {
        return Ok(false);
    }
    let ids: Vec<_> = context
        .records
        .iter()
        .map(ReviewedContextRecord::fact_id)
        .collect();
    let targets: Vec<_> = context
        .records
        .iter()
        .map(|record| match record {
            ReviewedContextRecord::Project { record } => record.pin.target_id,
            ReviewedContextRecord::Conversation { record } => record.pin.target_id,
        })
        .collect();
    sqlx::query("SELECT id FROM reviewed_memory_facts WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE NOWAIT")
        .bind(scope.company_id()).bind(&ids).fetch_all(&mut *c).await?;
    sqlx::query("SELECT id FROM reviewed_memory_targets WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE NOWAIT")
        .bind(scope.company_id()).bind(&targets).fetch_all(&mut *c).await?;
    let observed = sqlx::query(
        "SELECT requester_public_key,provenance_bytes FROM ortak_conversation_run_origin($1,$2,$3)",
    )
    .bind(scope.company_id())
    .bind(run)
    .bind(project)
    .fetch_optional(&mut *c)
    .await?;
    let Some(observed) = observed else {
        return Ok(false);
    };
    let requester: Vec<u8> = observed.try_get("requester_public_key")?;
    let provenance: Vec<u8> = observed.try_get("provenance_bytes")?;
    if ConversationMemoryOrigin::from_observation(&requester, &provenance)? != context.origin {
        return Ok(false);
    }
    for record in &context.records {
        let (pin, content) = match record {
            ReviewedContextRecord::Project { record } => (&record.pin, &record.content),
            ReviewedContextRecord::Conversation { record } => {
                let pin = &record.pin;
                let valid: Option<bool> = sqlx::query_scalar("SELECT a.provenance_bytes=$4
                    AND encode(a.audience_hash,'hex')=$5 AND ortak_conversation_runtime_eligible($1,$3,$2,$6,$7,$8)
                    FROM reviewed_memory_conversation_audiences a WHERE a.company_id=$1 AND a.fact_id=$2")
                    .bind(scope.company_id()).bind(pin.fact_id).bind(run).bind(record.provenance.as_bytes())
                    .bind(&pin.conversation_audience_hash).bind(pin.target_id)
                    .bind(pin.conversation_authority_epoch).bind(pin.conversation_consumption_epoch)
                    .fetch_optional(&mut *c).await?;
                if valid != Some(true) {
                    return Ok(false);
                }
                let common = ReviewedMemoryPin {
                    fact_id: pin.fact_id,
                    target_id: pin.target_id,
                    fact_version: pin.fact_version,
                    consumption_epoch: pin.consumption_epoch,
                    content_hash: pin.content_hash.clone(),
                    source_hash: pin.source_hash.clone(),
                    binding_hash: pin.binding_hash.clone(),
                    approval_id: pin.approval_id,
                    approved_by: pin.approved_by.clone(),
                    expires_at: pin.expires_at,
                };
                if !common_matches(c, scope, authority, project, &common, &record.content, true)
                    .await?
                {
                    return Ok(false);
                }
                continue;
            }
        };
        if authority.work_origin().is_none()
            || !common_matches(c, scope, authority, project, pin, content, false).await?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn common_matches(
    c: &mut PgConnection,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    project: Uuid,
    pin: &ReviewedMemoryPin,
    content: &str,
    conversation: bool,
) -> Result<bool> {
    let valid: Option<bool> = sqlx::query_scalar("SELECT f.version=$4 AND f.promotion_operation_id=$5 AND f.approved_by=$6
        AND f.expires_at=$7 AND f.content=$8 AND encode(x.content_hash,'hex')=$9 AND encode(x.source_hash,'hex')=$10
        AND encode(t.binding_hash,'hex')=$11 AND t.binding=$12 AND f.audience_kind=CASE WHEN $16 THEN 'conversation' ELSE 'project' END
        AND ($16 OR ortak_reviewed_runtime_eligible(f.company_id,f.id,t.id,$13))
        FROM reviewed_memory_facts f JOIN reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
        JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        WHERE f.company_id=$1 AND f.id=$2 AND t.id=$3 AND f.project_id=$14 AND f.employee_id=$15")
        .bind(scope.company_id()).bind(pin.fact_id).bind(pin.target_id).bind(pin.fact_version)
        .bind(pin.approval_id).bind(&pin.approved_by).bind(pin.expires_at).bind(content)
        .bind(&pin.content_hash).bind(&pin.source_hash).bind(&pin.binding_hash)
        .bind(serde_json::to_value(authority.memory_binding()).map_err(|_| invalid("reviewed binding invalid".into()))?)
        .bind(pin.consumption_epoch).bind(project).bind(authority.employee_id().as_str()).bind(conversation)
        .fetch_optional(c).await?;
    Ok(valid == Some(true))
}

pub(super) async fn persist(
    c: &mut PgConnection,
    scope: &CompanyScope,
    run: Uuid,
    snapshot: &FrozenRunSnapshot,
) -> Result<()> {
    let context = snapshot
        .conversation()
        .ok_or_else(|| invalid("conversation snapshot absent".into()))?;
    for (ordinal, record) in context.records.iter().enumerate() {
        let (pin, audience, authority_epoch, consumption_epoch) = match record {
            ReviewedContextRecord::Project { record } => (record.pin.clone(), None, None, None),
            ReviewedContextRecord::Conversation { record } => {
                let p = &record.pin;
                (
                    ReviewedMemoryPin {
                        fact_id: p.fact_id,
                        target_id: p.target_id,
                        fact_version: p.fact_version,
                        consumption_epoch: p.consumption_epoch,
                        content_hash: p.content_hash.clone(),
                        source_hash: p.source_hash.clone(),
                        binding_hash: p.binding_hash.clone(),
                        approval_id: p.approval_id,
                        approved_by: p.approved_by.clone(),
                        expires_at: p.expires_at,
                    },
                    Some(p.conversation_audience_hash.as_str()),
                    Some(p.conversation_authority_epoch),
                    Some(p.conversation_consumption_epoch),
                )
            }
        };
        sqlx::query("INSERT INTO run_reviewed_memory_uses(company_id,community_id,run_id,ordinal,fact_id,target_id,
            fact_version,consumption_epoch,content_hash,source_hash,binding_hash,approval_id,approved_by,expires_at,
            conversation_audience_hash,conversation_authority_epoch,conversation_consumption_epoch)
            SELECT f.company_id,f.community_id,$2,$3,f.id,$5,$6,$7,decode($8,'hex'),decode($9,'hex'),decode($10,'hex'),$11,$12,$13,
                decode($14,'hex'),$15,$16 FROM reviewed_memory_facts f WHERE f.company_id=$1 AND f.id=$4
            ON CONFLICT(company_id,run_id,ordinal) DO NOTHING")
            .bind(scope.company_id()).bind(run).bind(ordinal as i32).bind(pin.fact_id).bind(pin.target_id)
            .bind(pin.fact_version).bind(pin.consumption_epoch).bind(&pin.content_hash).bind(&pin.source_hash).bind(&pin.binding_hash)
            .bind(pin.approval_id).bind(&pin.approved_by).bind(pin.expires_at).bind(audience).bind(authority_epoch).bind(consumption_epoch)
            .execute(&mut *c).await?;
    }
    let current: bool = sqlx::query_scalar("SELECT ortak_run_reviewed_memory_current($1,$2)")
        .bind(scope.company_id())
        .bind(run)
        .fetch_one(c)
        .await?;
    if !current {
        return Err(invalid("conversation input authority changed".into()));
    }
    Ok(())
}
