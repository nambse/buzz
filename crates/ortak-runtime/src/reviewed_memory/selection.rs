use super::*;
use crate::{DispatchAuthorization, Result};
use sqlx::Row;

pub(super) async fn select(
    control: &PgControlPlane,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    run: Uuid,
    query: &str,
) -> Result<ReviewedMemorySelection> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let work=authority.work_origin().ok_or_else(|| crate::postgres::invalid("reviewed Work origin required".into()))?;
        if authority.company_id()!=scope.company_id() || work.run_id!=run || query.trim().is_empty() {
            return Err(crate::postgres::invalid("reviewed selection identity differs".into()));
        }
        let binding=authority.memory_binding().cloned().ok_or_else(||crate::postgres::invalid("reviewed binding missing".into()))?;
        let mut tx=control.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout','500ms',true),set_config('statement_timeout','2s',true),set_config('idle_in_transaction_session_timeout','5s',true)").execute(&mut *tx).await?;
        let witness=ortak_control::postgres::lock_office_authority_on(&mut tx,scope).await?;
        let fresh=crate::postgres::work::derive_on(&mut tx,scope,run,authority.outbox_id(),authority.lease_token(),witness).await?;
        let DispatchAuthorization::Authorized(fresh)=fresh else { return Err(crate::postgres::invalid("reviewed Work authority changed".into())); };
        if fresh.run_spec(run)?!=authority.run_spec(run)? { return Err(crate::postgres::invalid("reviewed Work input changed".into())); }
        let rows=sqlx::query("SELECT f.id,f.version,f.promotion_operation_id,f.approved_by,f.expires_at,
            x.target_id,x.content_hash,x.source_hash,t.binding_hash,t.consumption_epoch
            FROM reviewed_memory_facts f JOIN reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
            JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
            WHERE f.company_id=$1 AND f.project_id=$2 AND f.employee_id=$3 AND t.binding=$4
            AND ortak_reviewed_runtime_eligible(f.company_id,f.id,t.id,t.consumption_epoch)
            AND to_tsvector('simple',f.content) @@ websearch_to_tsquery('simple',$5)
            ORDER BY f.id LIMIT 32 FOR SHARE OF f")
            .bind(scope.company_id()).bind(work.project_id).bind(authority.employee_id().as_str())
            .bind(serde_json::to_value(&binding).map_err(|_|crate::postgres::invalid("reviewed binding invalid".into()))?)
            .bind(query).fetch_all(&mut *tx).await?;
        let pins=rows.iter().map(|r| Ok(ReviewedMemoryPin {
            fact_id:r.try_get("id")?,target_id:r.try_get("target_id")?,fact_version:r.try_get("version")?,
            consumption_epoch:r.try_get("consumption_epoch")?,content_hash:hex::encode(r.try_get::<Vec<u8>,_>("content_hash")?),
            source_hash:hex::encode(r.try_get::<Vec<u8>,_>("source_hash")?),binding_hash:hex::encode(r.try_get::<Vec<u8>,_>("binding_hash")?),
            approval_id:r.try_get("promotion_operation_id")?,approved_by:r.try_get("approved_by")?,expires_at:r.try_get("expires_at")?
        })).collect::<std::result::Result<Vec<_>,sqlx::Error>>()?;
        tx.commit().await?;
        Ok(ReviewedMemorySelection {company_id:scope.company_id(),project_id:work.project_id,employee_id:authority.employee_id().clone(),binding,pins})
    }).await.map_err(|_|crate::postgres::invalid("reviewed selection timeout".into()))?
}
