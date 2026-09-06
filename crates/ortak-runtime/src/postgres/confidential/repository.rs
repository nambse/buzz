use ortak_control::{CompanyScope,confidential::{ConfidentialEnvelope,PayloadPurpose,ValidatedIdentity},postgres::lock_office_authority_on};
use ortak_domain::CredentialRef;
use ortak_office::encrypted::{jobs::{DmDecryptClaim,PgDecryptJobs},key_provider::WrappedMasterKey};
use sqlx::{PgConnection,Row};
use uuid::Uuid;

use super::{ConfidentialAdmissionError as Error,ConfidentialAdmissionReceipt,CurrentConfidentialPayload,PgConfidentialRuns,ProtectedConfidentialRun,Result};

impl PgConfidentialRuns {
    /// One atomic metadata/ciphertext commit. An uncertain response retries this
    /// same protected object, never fresh encryption. Exact old receipts remain
    /// recoverable after revocation; duplicate wrappers never start another run.
    pub async fn commit(&self,scope:&CompanyScope,claim:&DmDecryptClaim,protected:&ProtectedConfidentialRun)->Result<ConfidentialAdmissionReceipt>{
        let prepared=&protected.prepared;
        if prepared.source_id!=claim.expected().outer_id().to_bytes()
            || scope.company_id()!=claim.identity().company_id
            || scope.community_id()!=Some(claim.identity().community_id) { return Err(Error::Refused); }
        let mut tx=self.pool.begin().await?;
        if !PgDecryptJobs::retained_claim_matches_on(&mut tx,scope,claim).await? { return Err(Error::Refused); }
        if let Some(row)=sqlx::query("SELECT r.run_id,r.duplicate_rumor,c.identity_bytes,c.wrapped_key,p.envelope_bytes FROM confidential_dm_receipts r JOIN confidential_runs c USING(company_id,run_id) JOIN confidential_run_payloads p ON p.company_id=c.company_id AND p.run_id=c.run_id AND p.purpose='snapshot' AND p.ordinal=0 WHERE r.company_id=$1 AND r.source_id=$2")
            .bind(scope.company_id()).bind(&prepared.source_id).fetch_optional(&mut *tx).await? {
            let duplicate:bool=row.try_get("duplicate_rumor")?;
            if !duplicate && (row.try_get::<Vec<u8>,_>("identity_bytes")?!=prepared.identity.canonical_bytes()
                || row.try_get::<Vec<u8>,_>("wrapped_key")?!=protected.wrapped.canonical_bytes()
                || row.try_get::<Vec<u8>,_>("envelope_bytes")?!=protected.snapshot.canonical_bytes()) { return Err(Error::Payload); }
            let receipt=ConfidentialAdmissionReceipt {run_id:row.try_get("run_id")?,duplicate_rumor:duplicate};
            tx.commit().await?;
            return Ok(receipt);
        }
        PgDecryptJobs::lock_verified_on(&mut tx,scope,claim).await?;
        let row=sqlx::query("SELECT committed_run_id,duplicate_rumor FROM ortak_commit_confidential_dm($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(scope.company_id()).bind(&prepared.source_id).bind(prepared.run_id).bind(prepared.key_id)
            .bind(prepared.identity.canonical_bytes()).bind(protected.wrapped.canonical_bytes())
            .bind(protected.snapshot.canonical_bytes()).bind(protected.snapshot.nonce().as_slice())
            .fetch_one(&mut *tx).await?;
        let receipt=ConfidentialAdmissionReceipt { run_id:row.try_get("committed_run_id")?,duplicate_rumor:row.try_get("duplicate_rumor")? };
        tx.commit().await?;
        Ok(receipt)
    }

    /// Acquires current Office/selection/source fences BEFORE selecting any
    /// protected bytes. Retain this caller-owned READ COMMITTED transaction for
    /// the local authorization interval. It does not renew the frozen epoch.
    /// A false/absent current observation returns no envelope or wrapped key.
    pub async fn load_current_on(connection:&mut PgConnection,scope:&CompanyScope,run:Uuid)->Result<Option<CurrentConfidentialPayload>>{
        lock_office_authority_on(connection,scope).await?;
        let row=sqlx::query("SELECT c.selection_id FROM confidential_runs c WHERE c.company_id=$1 AND c.community_id=$2 AND c.run_id=$3")
            .bind(scope.company_id()).bind(scope.community_id()).bind(run).fetch_optional(&mut *connection).await?;
        if row.is_none() { return Ok(None); }
        let current:bool=sqlx::query_scalar("SELECT ortak_lock_confidential_dm($1,$2)")
            .bind(scope.company_id()).bind(run).fetch_one(&mut *connection).await?;
        if !current { return Ok(None); }
        let row=sqlx::query("SELECT ortak_confidential_dm_identity(c.company_id,c.source_id,c.run_id,c.key_id) AS expected,c.identity_bytes,c.wrapped_key,p.envelope_bytes,s.decrypt_ref,least(c.execution_deadline,b.valid_until,ch.ttl_deadline,clock_timestamp()+interval '5 seconds') AS valid_before FROM confidential_runs c JOIN confidential_run_payloads p ON p.company_id=c.company_id AND p.run_id=c.run_id AND p.purpose='snapshot' AND p.ordinal=0 JOIN encrypted_dm_selections s ON s.company_id=c.company_id AND s.selection_id=c.selection_id JOIN employee_office_bindings b ON b.company_id=s.company_id AND b.id=s.office_binding_id JOIN channels ch ON ch.community_id=s.community_id AND ch.id=s.channel_id WHERE c.company_id=$1 AND c.run_id=$2")
            .bind(scope.company_id()).bind(run).fetch_one(connection).await?;
        let expected:Vec<u8>=row.try_get("expected")?;
        if expected!=row.try_get::<Vec<u8>,_>("identity_bytes")? { return Err(Error::Payload); }
        let identity=ValidatedIdentity::parse(&expected).map_err(|_|Error::Payload)?;
        let snapshot=ConfidentialEnvelope::parse(&row.try_get::<Vec<u8>,_>("envelope_bytes")?).map_err(|_|Error::Payload)?;
        snapshot.header().require_expected(&identity,PayloadPurpose::Snapshot,0).map_err(|_|Error::Payload)?;
        let wrapped=WrappedMasterKey::parse(&row.try_get::<Vec<u8>,_>("wrapped_key")?).map_err(|_|Error::Payload)?;
        let signer_ref=CredentialRef::parse(row.try_get::<String,_>("decrypt_ref")?).map_err(|_|Error::Payload)?;
        Ok(Some(CurrentConfidentialPayload { identity,snapshot,wrapped,signer_ref,valid_before:row.try_get("valid_before")? }))
    }

    /// Metadata-only cancellation and durable containment obligation, including
    /// after source/key revocation. No content/key read or ordinary event write.
    /// Existing cancellation workers still require their own receipt-only adapter
    /// integration before this unactivated mode may be enabled.
    pub async fn cancel(&self,scope:&CompanyScope,run:Uuid)->Result<bool>{
        let mut tx=self.pool.begin().await?;
        lock_office_authority_on(&mut tx,scope).await?;
        let row=sqlx::query("SELECT r.status FROM runs r JOIN confidential_runs c ON c.company_id=r.company_id AND c.run_id=r.id WHERE r.company_id=$1 AND c.community_id=$2 AND r.id=$3 FOR UPDATE OF r")
            .bind(scope.company_id()).bind(scope.community_id()).bind(run).fetch_optional(&mut *tx).await?;
        let Some(row)=row else { return Ok(false); };
        sqlx::query("INSERT INTO runtime_cancellations(company_id,run_id,reason) VALUES($1,$2,'human_requested') ON CONFLICT(company_id,run_id) DO NOTHING")
            .bind(scope.company_id()).bind(run).execute(&mut *tx).await?;
        if matches!(row.try_get::<String,_>("status")?.as_str(),"queued"|"running"|"waiting") {
            sqlx::query("UPDATE runs SET status='cancelled',cancel_reason='human_requested',error_code='confidential_cancelled',finished_at=clock_timestamp(),updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
                .bind(scope.company_id()).bind(run).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE confidential_run_dispatches SET state='cancelled',finished_at=clock_timestamp(),error_code='cancelled',lease_token=NULL,lease_expires_at=NULL WHERE company_id=$1 AND run_id=$2 AND state='pending'")
            .bind(scope.company_id()).bind(run).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }
}
