use super::*;
use ortak_control::memory::employee::EmployeeMemoryProvenanceV1;

/// Claims one due operation. First-attempt withdrawal is accelerated by current
/// ineligibility, so source loss does not wait until the original fact expiry.
pub async fn claim(
    control: &PgControlPlane,
    scope: &CompanyScope,
) -> Result<Option<EmployeeExportLease>> {
    bounded(async {
        let mut tx=control.pool().begin().await?;bounds(&mut tx).await?;
        sqlx::query("WITH due AS (SELECT j.company_id,j.fact_id FROM employee_reviewed_memory_export_jobs j
            JOIN employee_reviewed_memory_exports x ON x.company_id=j.company_id AND x.fact_id=j.fact_id
            WHERE j.company_id=$1 AND j.action='withdraw' AND j.state='pending' AND j.attempt_count=0 AND j.lease_token IS NULL
            AND j.next_attempt_at>clock_timestamp() AND NOT ortak_employee_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id)
            ORDER BY j.fact_id LIMIT 16 FOR UPDATE OF j SKIP LOCKED)
            UPDATE employee_reviewed_memory_export_jobs j SET next_attempt_at=clock_timestamp() FROM due
            WHERE j.company_id=due.company_id AND j.fact_id=due.fact_id AND j.action='withdraw'")
            .bind(scope.company_id()).execute(&mut *tx).await?;
        sqlx::query("WITH due AS (SELECT company_id,fact_id,action FROM employee_reviewed_memory_export_jobs
            WHERE company_id=$1 AND state='pending' AND attempt_count=20 AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())
            ORDER BY next_attempt_at,fact_id,action LIMIT 16 FOR UPDATE SKIP LOCKED)
            UPDATE employee_reviewed_memory_export_jobs j SET state='failed',last_error_code='lease_exhausted',lease_token=NULL,lease_expires_at=NULL
            FROM due WHERE j.company_id=due.company_id AND j.fact_id=due.fact_id AND j.action=due.action")
            .bind(scope.company_id()).execute(&mut *tx).await?;
        let row=sqlx::query("WITH due AS (SELECT company_id,fact_id,action FROM employee_reviewed_memory_export_jobs
            WHERE company_id=$1 AND state='pending' AND attempt_count<20 AND next_attempt_at<=clock_timestamp()
                AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())
            ORDER BY (action='withdraw') DESC,next_attempt_at,fact_id LIMIT 1 FOR UPDATE SKIP LOCKED)
            UPDATE employee_reviewed_memory_export_jobs j SET attempt_count=j.attempt_count+1,total_attempts=j.total_attempts+1,
                lease_token=gen_random_uuid(),lease_expires_at=clock_timestamp()+interval '60 seconds'
            FROM due WHERE j.company_id=due.company_id AND j.fact_id=due.fact_id AND j.action=due.action
            RETURNING j.fact_id,j.action,j.lease_token,j.total_attempts")
            .bind(scope.company_id()).fetch_optional(&mut *tx).await?;
        let result=row.map(|r|->Result<_>{Ok(EmployeeExportLease{fact_id:r.try_get("fact_id")?,action:EmployeeExportAction::parse(r.try_get("action")?)?,
            token:r.try_get("lease_token")?,total_attempts:r.try_get("total_attempts")?})}).transpose()?;
        tx.commit().await?;Ok(result)
    }).await
}

/// Prepare current publication or original-target cleanup. A cleanup request
/// never reads retained edited text or reacquires current source authorization.
pub async fn prepare(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &EmployeeExportLease,
) -> Result<Option<PreparedEmployeeExport>> {
    bounded(async {
        let publish=lease.action==EmployeeExportAction::Publish;
        let mut tx=control.pool().begin().await?;bounds(&mut tx).await?;
        if publish {
            sqlx::query("SELECT ortak_lock_office_authority($1)").bind(scope.company_id()).execute(&mut *tx).await?;
            sqlx::query("SELECT a.channel_id FROM employee_memory_channel_authorities a JOIN employee_reviewed_memory_facts f
                ON f.company_id=a.company_id AND f.community_id=a.community_id AND f.employee_id=a.employee_id
                AND a.channel_id IN(f.source_channel_id,f.destination_channel_id)
                WHERE f.company_id=$1 AND f.id=$2 ORDER BY a.channel_id FOR SHARE OF a")
                .bind(scope.company_id()).bind(lease.fact_id).fetch_all(&mut *tx).await?;
        }
        sqlx::query("SELECT id FROM employee_reviewed_memory_facts WHERE company_id=$1 AND id=$2 FOR SHARE")
            .bind(scope.company_id()).bind(lease.fact_id).fetch_one(&mut *tx).await?;
        let row=sqlx::query("SELECT x.employee_id,x.target_id,x.destination_channel_id,x.content_hash,x.source_hash,x.sharing_hash,
            CASE WHEN $6 THEN f.content ELSE NULL END AS content,CASE WHEN $6 THEN f.provenance_bytes ELSE NULL END AS provenance_bytes,
            t.deployment_id,t.creation_receipt,t.namespace_hash,t.binding_hash,j.request_hash,
            (ortak_employee_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id)
                AND x.employee_revision_id=e.active_revision_id AND x.employee_lifecycle_epoch=e.lifecycle_epoch) AS eligible,
            (f.revoked_at IS NOT NULL OR f.expires_at<=clock_timestamp()
                OR NOT ortak_employee_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id)) AS removal_due
            FROM employee_reviewed_memory_exports x JOIN employee_reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
            JOIN employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
            JOIN employees e ON e.company_id=x.company_id AND e.id=x.employee_id
            JOIN employee_reviewed_memory_export_jobs j ON j.company_id=x.company_id AND j.fact_id=x.fact_id
            WHERE x.company_id=$1 AND x.fact_id=$2 AND j.action=$3 AND j.state='pending' AND j.lease_token=$4
                AND j.total_attempts=$5 AND j.lease_expires_at>clock_timestamp() FOR SHARE OF t FOR UPDATE OF j")
            .bind(scope.company_id()).bind(lease.fact_id).bind(lease.action.as_str()).bind(lease.token).bind(lease.total_attempts).bind(publish)
            .fetch_optional(&mut *tx).await?;
        let Some(row)=row else {return Ok(None)};
        if publish && !row.try_get::<bool,_>("eligible")? || !publish && !row.try_get::<bool,_>("removal_due")? {return Err(WorkError::AccessDenied);}
        let employee=ortak_domain::EmployeeId::parse(row.try_get::<String,_>("employee_id")?)?;
        let mut receipt:Value=row.try_get("creation_receipt")?;
        let object=receipt.as_object_mut().ok_or_else(invalid)?;
        object.remove("protocol");object.remove("namespace_hash");
        let original:HonchoCreatedResourcesReceipt=serde_json::from_value(receipt).map_err(|_|invalid())?;
        if original.company_id!=scope.company_id() || original.employee_id!=employee
            || original.deployment_id!=row.try_get::<Uuid,_>("deployment_id")? {return Err(invalid());}
        let commitment=ReviewedEmployeeCommitment{fact_id:lease.fact_id,target_id:row.try_get("target_id")?,destination_channel_id:row.try_get("destination_channel_id")?,
            content_hash:hex::encode(row.try_get::<Vec<u8>,_>("content_hash")?),source_hash:hex::encode(row.try_get::<Vec<u8>,_>("source_hash")?),
            sharing_hash:hex::encode(row.try_get::<Vec<u8>,_>("sharing_hash")?)};
        let namespace_hash=hex::encode(row.try_get::<Vec<u8>,_>("namespace_hash")?);
        let binding_hash=hex::encode(row.try_get::<Vec<u8>,_>("binding_hash")?);
        let request_hash=employee_reviewed_request_hash(&namespace_hash,&binding_hash,scope.company_id(),&employee,&commitment,!publish).map_err(memory_error)?;
        if bytes(&request_hash)?!=row.try_get::<Vec<u8>,_>("request_hash")? {return Err(invalid());}
        let provenance=row.try_get::<Option<Vec<u8>>,_>("provenance_bytes")?
            .map(|b|EmployeeMemoryProvenanceV1::from_canonical_bytes(&b).map_err(|_|invalid())).transpose()?;
        let result=PreparedEmployeeExport{company_id:scope.company_id(),employee_id:employee,original,lease:lease.clone(),commitment,
            namespace_hash,binding_hash,request_hash,content:row.try_get("content")?,provenance};
        // Current source time gates are checked again immediately before release.
        if publish && !sqlx::query_scalar::<_,bool>("SELECT ortak_employee_reviewed_export_eligible($1,$2,$3)")
            .bind(scope.company_id()).bind(lease.fact_id).bind(result.commitment.target_id).fetch_one(&mut *tx).await? {return Err(WorkError::AccessDenied);}
        tx.commit().await?;Ok(Some(result))
    }).await
}

/// Atomically records an adapter-validated text-free response and the exact live
/// job. Late ACKs cannot replace a newer lease; they never re-enable memory.
pub async fn acknowledge(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &EmployeeExportLease,
    receipt: &ReviewedEmployeeAcknowledgement,
) -> Result<bool> {
    let r = &receipt.record;
    if r.company_id != scope.company_id()
        || r.record_id != lease.fact_id
        || r.content.is_some()
        || r.erased_from_reviewed_store != r.tombstone_at.is_some()
        || lease.action == EmployeeExportAction::Withdraw
            && (!r.erased_from_reviewed_store || r.status != ReviewedProjectStatus::Withdrawn)
    {
        return Err(invalid());
    }
    let status = match r.status {
        ReviewedProjectStatus::Active => "active",
        ReviewedProjectStatus::Expired => "expired",
        ReviewedProjectStatus::Withdrawn => "withdrawn",
    };
    bounded(async {
        let mut tx=control.pool().begin().await?;bounds(&mut tx).await?;
        let community:Option<Uuid>=sqlx::query_scalar("UPDATE employee_reviewed_memory_export_jobs j SET state='acknowledged',last_error_code=NULL
            FROM employee_reviewed_memory_exports x JOIN employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
            WHERE j.company_id=$1 AND j.fact_id=$2 AND j.action=$3 AND j.state='pending' AND j.lease_token=$4 AND j.total_attempts=$5
                AND j.lease_expires_at>clock_timestamp() AND j.request_hash=$6 AND x.company_id=j.company_id AND x.fact_id=j.fact_id
                AND x.target_id=$7 AND x.destination_channel_id=$8 AND x.employee_id=$9 AND t.deployment_id=$10
                AND t.namespace_hash=$11 AND t.binding_hash=$12 AND x.content_hash=$13 AND x.source_hash=$14 AND x.sharing_hash=$15
            RETURNING j.community_id")
            .bind(scope.company_id()).bind(lease.fact_id).bind(lease.action.as_str()).bind(lease.token).bind(lease.total_attempts)
            .bind(bytes(&receipt.request_hash)?).bind(r.target_id).bind(r.destination_channel_id).bind(r.employee_id.as_str()).bind(r.deployment_id)
            .bind(bytes(&r.namespace_hash)?).bind(bytes(&r.binding_hash)?).bind(bytes(&r.content_hash)?).bind(bytes(&r.source_hash)?).bind(bytes(&r.sharing_hash)?)
            .fetch_optional(&mut *tx).await?;
        let Some(community)=community else {return Ok(false)};
        sqlx::query("INSERT INTO employee_reviewed_memory_export_receipts(company_id,community_id,fact_id,action,request_hash,binding_hash,
            content_hash,remote_status,erased_from_reviewed_store,tombstone_at,lease_token,total_attempts) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
            .bind(scope.company_id()).bind(community).bind(lease.fact_id).bind(lease.action.as_str()).bind(bytes(&receipt.request_hash)?)
            .bind(bytes(&r.binding_hash)?).bind(bytes(&r.content_hash)?).bind(status).bind(r.erased_from_reviewed_store).bind(r.tombstone_at)
            .bind(lease.token).bind(lease.total_attempts).execute(&mut *tx).await?;
        tx.commit().await?;Ok(true)
    }).await
}

/// Bounded retry state survives a lost ACK. A failure on an expired lease leaves
/// its newer owner untouched and propagates database errors.
pub async fn fail(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lease: &EmployeeExportLease,
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
        Ok(sqlx::query("UPDATE employee_reviewed_memory_export_jobs SET state=CASE WHEN $6 OR attempt_count>=20 THEN 'failed' ELSE 'pending' END,
            next_attempt_at=clock_timestamp()+make_interval(secs=>least(300,5*(1<<least(attempt_count,6)))),last_error_code=$7,lease_token=NULL,lease_expires_at=NULL
            WHERE company_id=$1 AND fact_id=$2 AND action=$3 AND state='pending' AND lease_token=$4 AND total_attempts=$5 AND lease_expires_at>clock_timestamp()")
            .bind(scope.company_id()).bind(lease.fact_id).bind(lease.action.as_str()).bind(lease.token).bind(lease.total_attempts)
            .bind(permanent).bind(code).execute(control.pool()).await?.rows_affected()==1)
    }).await
}
