use super::*;

/// Claims at most one due job; expiry cleanup is durable from publication onward.
pub async fn claim(
    control: &PgControlPlane,
    scope: &CompanyScope,
) -> Result<Option<ReviewedExportLease>> {
    bounded(async {
        let mut tx=control.pool().begin().await?;bounds(&mut tx).await?;
        sqlx::query("WITH due AS (SELECT company_id,fact_id,action FROM reviewed_memory_export_jobs WHERE company_id=$1 AND state='pending' AND attempt_count=20
            AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp()) ORDER BY next_attempt_at,fact_id,action LIMIT 16 FOR UPDATE SKIP LOCKED)
            UPDATE reviewed_memory_export_jobs j SET state='failed',last_error_code='lease_exhausted',lease_token=NULL,lease_expires_at=NULL,updated_at=clock_timestamp()
            FROM due WHERE j.company_id=due.company_id AND j.fact_id=due.fact_id AND j.action=due.action")
            .bind(scope.company_id()).execute(&mut *tx).await?;
        let row=sqlx::query("WITH due AS (SELECT company_id,fact_id,action FROM reviewed_memory_export_jobs WHERE company_id=$1 AND state='pending' AND attempt_count<20
            AND next_attempt_at<=clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())
            ORDER BY (action='withdraw') DESC,next_attempt_at,fact_id LIMIT 1 FOR UPDATE SKIP LOCKED)
            UPDATE reviewed_memory_export_jobs j SET attempt_count=j.attempt_count+1,total_attempts=j.total_attempts+1,lease_token=gen_random_uuid(),
                lease_expires_at=clock_timestamp()+INTERVAL '60 seconds',updated_at=clock_timestamp()
            FROM due WHERE j.company_id=due.company_id AND j.fact_id=due.fact_id AND j.action=due.action
            RETURNING j.fact_id,j.action,j.lease_token,j.total_attempts")
            .bind(scope.company_id()).fetch_optional(&mut *tx).await?;
        let lease=row.map(|row| -> Result<ReviewedExportLease> {Ok(ReviewedExportLease{fact_id:row.try_get("fact_id")?,action:ReviewedExportAction::parse(row.try_get("action")?)?,
            token:row.try_get("lease_token")?,total_attempts:row.try_get("total_attempts")?})}).transpose()?;
        tx.commit().await?;Ok(lease)
    }).await
}

/// Prepares one exact current publication, or retained-binding cleanup. No network
/// call happens while its project/fact/job locks are held. Cleanup does not require
/// active employee status, source visibility, current target advertisement or grants.
pub async fn prepare(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &ReviewedExportLease,
) -> Result<Option<PreparedReviewedExport>> {
    bounded(async {
        let mut tx=control.pool().begin().await?;bounds(&mut tx).await?;
        let witness=if lease.action==ReviewedExportAction::Publish {Some(ortak_control::postgres::lock_office_authority_on(&mut tx,scope).await?)}else{None};
        let project:Option<Uuid>=sqlx::query_scalar("SELECT project_id FROM reviewed_memory_exports WHERE company_id=$1 AND fact_id=$2")
            .bind(scope.company_id()).bind(lease.fact_id).fetch_optional(&mut *tx).await?;
        let Some(project)=project else{return Ok(None)};
        sqlx::query("SELECT id FROM projects WHERE company_id=$1 AND id=$2 FOR SHARE").bind(scope.company_id()).bind(project).fetch_one(&mut *tx).await?;
        if lease.action==ReviewedExportAction::Publish {
            sqlx::query("SELECT authority.channel_id FROM conversation_memory_authorities authority
                JOIN reviewed_memory_conversation_audiences audience ON audience.company_id=authority.company_id
                    AND audience.project_id=authority.project_id AND audience.channel_id=authority.channel_id
                WHERE audience.company_id=$1 AND audience.fact_id=$2 FOR SHARE OF authority NOWAIT")
                .bind(scope.company_id()).bind(lease.fact_id).fetch_optional(&mut *tx).await?;
        }
        sqlx::query("SELECT id FROM reviewed_memory_facts WHERE company_id=$1 AND id=$2 FOR SHARE").bind(scope.company_id()).bind(lease.fact_id).fetch_one(&mut *tx).await?;
        let row=sqlx::query("SELECT x.project_id,x.employee_id,x.content_hash,x.source_hash,f.content,f.approved_by,f.promotion_operation_id,f.expires_at,
            t.deployment_id,t.binding,t.creation_receipt,j.idempotency_key,j.request_hash,
            ((CASE WHEN f.audience_kind='conversation' THEN ortak_conversation_export_eligible(x.company_id,x.fact_id,x.target_id)
                ELSE ortak_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id) END) AND x.employee_revision_id=e.active_revision_id
                AND x.employee_lifecycle_epoch=e.lifecycle_epoch) AS eligible,
            (f.revoked_at IS NOT NULL OR f.expires_at<=clock_timestamp()) AS removal_due
            FROM reviewed_memory_exports x JOIN reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
            JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
            JOIN employees e ON e.company_id=x.company_id AND e.id=x.employee_id
            JOIN reviewed_memory_export_jobs j ON j.company_id=x.company_id AND j.fact_id=x.fact_id
            WHERE x.company_id=$1 AND x.fact_id=$2 AND j.action=$3 AND j.state='pending' AND j.lease_token=$4
                AND j.total_attempts=$5 AND j.lease_expires_at>clock_timestamp() FOR SHARE OF t FOR UPDATE OF j")
            .bind(scope.company_id()).bind(lease.fact_id).bind(lease.action.as_str()).bind(lease.token).bind(lease.total_attempts)
            .fetch_optional(&mut *tx).await?;
        let Some(row)=row else{return Ok(None)};
        if (lease.action==ReviewedExportAction::Publish&&!row.try_get::<bool,_>("eligible")?)
            ||(lease.action==ReviewedExportAction::Withdraw&&!row.try_get::<bool,_>("removal_due")?){return Err(WorkError::AccessDenied);}
        let content:String=row.try_get("content")?;
        let source:Vec<u8>=row.try_get("source_hash")?;
        let content_hash:Vec<u8>=row.try_get("content_hash")?;
        let employee=ortak_domain::EmployeeId::parse(row.try_get::<String,_>("employee_id")?)?;
        let binding=serde_json::from_value(row.try_get("binding")?).map_err(|_|invalid())?;
        let approved_by:String=row.try_get("approved_by")?;
        let approval:Uuid=row.try_get("promotion_operation_id")?;
        let expires:DateTime<Utc>=row.try_get("expires_at")?;
        let expected:Vec<u8>=row.try_get("request_hash")?;
        if Sha256::digest(content.as_bytes()).as_slice()!=content_hash||expected!=request_hash(scope.company_id(),project,lease.fact_id,&employee,&binding,
            lease.action,&content,&source,&approved_by,approval,expires)?{return Err(invalid());}
        if let Some(deadline)=witness.and_then(|w|w.valid_before()) {
            let live:bool=sqlx::query_scalar("SELECT clock_timestamp()<$1").bind(deadline).fetch_one(&mut *tx).await?;
            if !live{return Err(WorkError::OperationTimedOut);}
        }
        let result=PreparedReviewedExport{company_id:scope.company_id(),project_id:project,employee_id:employee,deployment_id:row.try_get("deployment_id")?,
            binding,creation_receipt:row.try_get("creation_receipt")?,lease:lease.clone(),idempotency_key:row.try_get("idempotency_key")?,
            request_hash:expected,content:if lease.action==ReviewedExportAction::Publish{Some(content)}else{None},source_hash:hex::encode(source),
            approved_by,approval_id:approval,expires_at:expires};
        tx.commit().await?;Ok(Some(result))
    }).await
}

/// Commits a validated hash-only acknowledgement with the exact live lease.
/// A late owner cannot replace a newer result; no response re-enables a fact.
pub async fn acknowledge(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &ReviewedExportLease,
    receipt: &ReviewedExportAcknowledgement,
) -> Result<bool> {
    bounded(async {
        if receipt.request_hash.len()!=32||receipt.binding_hash.len()!=32||receipt.content_hash.as_ref().is_some_and(|v|v.len()!=32)
            ||!matches!(receipt.remote_status.as_str(),"active"|"expired"|"withdrawn")
            ||receipt.erased_from_reviewed_store!=receipt.tombstone_at.is_some()
            ||(lease.action==ReviewedExportAction::Withdraw&&(!receipt.erased_from_reviewed_store||receipt.remote_status=="active")){return Err(invalid());}
        let mut tx=control.pool().begin().await?;bounds(&mut tx).await?;
        let community:Option<Uuid>=sqlx::query_scalar("UPDATE reviewed_memory_export_jobs SET state='acknowledged',last_error_code=NULL,updated_at=clock_timestamp()
            WHERE company_id=$1 AND fact_id=$2 AND action=$3 AND state='pending' AND lease_token=$4 AND total_attempts=$5
                AND lease_expires_at>clock_timestamp() AND request_hash=$6 RETURNING community_id")
            .bind(scope.company_id()).bind(lease.fact_id).bind(lease.action.as_str()).bind(lease.token).bind(lease.total_attempts).bind(&receipt.request_hash)
            .fetch_optional(&mut *tx).await?;
        let Some(community)=community else{return Ok(false)};
        sqlx::query("INSERT INTO reviewed_memory_export_receipts(company_id,community_id,fact_id,action,request_hash,binding_hash,content_hash,remote_status,
            erased_from_reviewed_store,tombstone_at,lease_token,total_attempts) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
            .bind(scope.company_id()).bind(community).bind(lease.fact_id).bind(lease.action.as_str()).bind(&receipt.request_hash).bind(&receipt.binding_hash)
            .bind(&receipt.content_hash).bind(&receipt.remote_status).bind(receipt.erased_from_reviewed_store).bind(receipt.tombstone_at)
            .bind(lease.token).bind(lease.total_attempts).execute(&mut *tx).await?;
        tx.commit().await?;Ok(true)
    }).await
}

/// Records one closed failure or leaves the newer lease untouched. A finite
/// retry budget and increasing backoff survive process restarts.
pub async fn fail(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &ReviewedExportLease,
    code: &str,
    permanent: bool,
) -> Result<bool> {
    if !matches!(
        code,
        "authority_refused"
            | "target_unavailable"
            | "service_retry"
            | "service_refused"
            | "database_retry"
            | "deadline"
    ) {
        return Err(invalid());
    }
    bounded(async {
        let result=sqlx::query("UPDATE reviewed_memory_export_jobs SET state=CASE WHEN $6 OR attempt_count>=20 THEN 'failed' ELSE 'pending' END,
            next_attempt_at=clock_timestamp()+make_interval(secs=>least(300,5*(1<<least(attempt_count,6)))),last_error_code=$7,
            lease_token=NULL,lease_expires_at=NULL,updated_at=clock_timestamp()
            WHERE company_id=$1 AND fact_id=$2 AND action=$3 AND state='pending' AND lease_token=$4 AND total_attempts=$5 AND lease_expires_at>clock_timestamp()")
            .bind(scope.company_id()).bind(lease.fact_id).bind(lease.action.as_str()).bind(lease.token).bind(lease.total_attempts).bind(permanent).bind(code)
            .execute(control.pool()).await?;
        Ok(result.rows_affected()==1)
    }).await
}
