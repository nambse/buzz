use super::*;

async fn employee(
    c: &mut PgConnection,
    scope: &CompanyScope,
    run: Uuid,
    ordinal: usize,
    record: &ReviewedEmployeeRecord,
) -> Result<()> {
    let p = &record.pin;
    sqlx::query("INSERT INTO run_employee_reviewed_memory_uses(company_id,community_id,run_id,ordinal,fact_id,target_id,
        fact_version,content_hash,source_hash,sharing_hash,audience_hash,binding_hash,namespace_hash,
        approval_id,approved_by,expires_at,source_authority_epoch,destination_authority_epoch,consumption_epoch)
        SELECT f.company_id,f.community_id,$2,$3,f.id,$5,$6,decode($7,'hex'),decode($8,'hex'),decode($9,'hex'),decode($10,'hex'),
            decode($11,'hex'),decode($12,'hex'),$13,$14,$15,$16,$17,$18
        FROM employee_reviewed_memory_facts f WHERE f.company_id=$1 AND f.id=$4
        ON CONFLICT(company_id,run_id,ordinal) DO NOTHING")
        .bind(scope.company_id()).bind(run).bind(ordinal as i32).bind(p.fact_id).bind(p.target_id).bind(p.fact_version)
        .bind(&p.content_hash).bind(&p.source_hash).bind(&p.sharing_hash).bind(&p.audience_hash).bind(&p.binding_hash).bind(&p.namespace_hash)
        .bind(p.approval_id).bind(&p.approved_by).bind(p.expires_at).bind(p.source_authority_epoch)
        .bind(p.destination_authority_epoch).bind(p.consumption_epoch).execute(c).await?;
    Ok(())
}

pub(crate) async fn persist(
    c: &mut PgConnection,
    scope: &CompanyScope,
    run: Uuid,
    snapshot: &FrozenRunSnapshot,
) -> Result<()> {
    let context = snapshot
        .employee()
        .ok_or_else(|| invalid("conversation snapshot absent".into()))?;
    for (ordinal, record) in context.records.iter().enumerate() {
        let (pin, audience, authority_epoch, consumption_epoch) = match record {
            EmployeeContextRecord::Employee { record } => {
                employee(c, scope, run, ordinal, record).await?;
                continue;
            }
            EmployeeContextRecord::Project { record } => (record.pin.clone(), None, None, None),
            EmployeeContextRecord::Conversation { record } => {
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
