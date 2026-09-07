//! Final sorted-row approval fence and atomic frozen-input use receipts.
use super::*;
use crate::memory_context::FrozenRunSnapshot;

mod conversation;
mod employee;

pub(super) async fn validate_candidate(
    c: &mut PgConnection,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    snapshot: &FrozenRunSnapshot,
) -> Result<bool> {
    if snapshot.employee().is_some() {
        return employee::validate(c, scope, authority, snapshot).await;
    }
    validate_legacy_candidate(c, scope, authority, snapshot).await
}

async fn validate_legacy_candidate(
    c: &mut PgConnection,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    snapshot: &FrozenRunSnapshot,
) -> Result<bool> {
    if snapshot.conversation().is_some() {
        return conversation::validate(c, scope, authority, snapshot).await;
    }
    let Some(context) = snapshot.reviewed() else {
        return Ok(true);
    };
    let Some(work) = authority.work_origin() else {
        return Ok(false);
    };
    let mut sorted: Vec<_> = context.records.iter().collect();
    sorted.sort_by_key(|r| r.pin.fact_id);
    for record in sorted {
        let pin = &record.pin;
        let valid:Option<bool>=sqlx::query_scalar("SELECT f.version=$4 AND f.promotion_operation_id=$5 AND f.approved_by=$6
            AND f.expires_at=$7 AND f.content=$8 AND encode(x.content_hash,'hex')=$9 AND encode(x.source_hash,'hex')=$10
            AND encode(t.binding_hash,'hex')=$11 AND t.binding=$12
            AND ortak_reviewed_runtime_eligible(f.company_id,f.id,t.id,$13)
            FROM reviewed_memory_facts f JOIN reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
            JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
            WHERE f.company_id=$1 AND f.id=$2 AND t.id=$3 AND f.project_id=$14 AND f.employee_id=$15
            FOR SHARE OF f,t")
            .bind(scope.company_id()).bind(pin.fact_id).bind(pin.target_id).bind(pin.fact_version)
            .bind(pin.approval_id).bind(&pin.approved_by).bind(pin.expires_at).bind(&record.content)
            .bind(&pin.content_hash).bind(&pin.source_hash).bind(&pin.binding_hash)
            .bind(serde_json::to_value(authority.memory_binding()).map_err(|_|invalid("reviewed binding invalid".into()))?)
            .bind(pin.consumption_epoch).bind(work.project_id).bind(authority.employee_id().as_str())
            .fetch_optional(&mut *c).await?;
        if valid != Some(true) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) async fn persist(
    c: &mut PgConnection,
    scope: &CompanyScope,
    run: Uuid,
    snapshot: &FrozenRunSnapshot,
) -> Result<()> {
    if snapshot.employee().is_some() {
        return employee::persist(c, scope, run, snapshot).await;
    }
    if snapshot.conversation().is_some() {
        return conversation::persist(c, scope, run, snapshot).await;
    }
    let Some(context) = snapshot.reviewed() else {
        return Ok(());
    };
    for (ordinal, record) in context.records.iter().enumerate() {
        let p = &record.pin;
        sqlx::query("INSERT INTO run_reviewed_memory_uses(company_id,community_id,run_id,ordinal,fact_id,target_id,
            fact_version,consumption_epoch,content_hash,source_hash,binding_hash,approval_id,approved_by,expires_at)
            SELECT f.company_id,f.community_id,$2,$3,f.id,$5,$6,$7,decode($8,'hex'),decode($9,'hex'),decode($10,'hex'),$11,$12,$13
            FROM reviewed_memory_facts f WHERE f.company_id=$1 AND f.id=$4
            ON CONFLICT(company_id,run_id,ordinal) DO NOTHING")
            .bind(scope.company_id()).bind(run).bind(ordinal as i32).bind(p.fact_id).bind(p.target_id)
            .bind(p.fact_version).bind(p.consumption_epoch).bind(&p.content_hash).bind(&p.source_hash).bind(&p.binding_hash)
            .bind(p.approval_id).bind(&p.approved_by).bind(p.expires_at).execute(&mut *c).await?;
    }
    let current: bool = sqlx::query_scalar("SELECT ortak_run_reviewed_memory_current($1,$2)")
        .bind(scope.company_id())
        .bind(run)
        .fetch_one(c)
        .await?;
    if !current {
        return Err(invalid("reviewed input authority changed".into()));
    }
    Ok(())
}
