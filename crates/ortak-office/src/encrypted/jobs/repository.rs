use chrono::{DateTime,Utc};
use nostr::EventId;
use ortak_control::{CompanyScope, postgres::lock_office_authority_on};
use ortak_domain::{CredentialRef,EmployeeId};
use sqlx::{PgConnection,PgPool,Row,postgres::PgRow};
use uuid::Uuid;

use super::{key, selection::lock_selection, DecryptFailure,DmClaimIdentity,DmDecryptClaim,DmJobError,DmOuterSource};
use crate::encrypted::{ExpectedEnvelope,VerifiedDmRumor};

/// Isolated PostgreSQL repository; construction starts no tasks or subscriptions.
/// The caller owns explicit configured-purpose authorization and finite scheduling.
pub struct PgDecryptJobs { pub(super) pool:PgPool }

impl PgDecryptJobs {
    /// Uses an explicitly supplied application pool; no environment discovery.
    pub fn new(pool:PgPool)->Self { Self { pool } }

    /// Queues only an untouched pending1059 accepted after this pair's opt-in.
    /// False means identical retained job; it never resets failures or the inbox.
    pub async fn enqueue(&self,scope:&CompanyScope,selection:Uuid,source:&DmOuterSource)->Result<bool,DmJobError>{
        let mut tx=self.pool.begin().await?;
        let office=lock_office_authority_on(&mut tx,scope).await?;
        lock_selection(&mut tx,scope,selection).await?;
        if let Some(old)=sqlx::query("SELECT selection_id,source_created_at FROM encrypted_dm_decrypt_jobs WHERE company_id=$1 AND source_id=$2 FOR UPDATE")
            .bind(scope.company_id()).bind(source.id.to_bytes().as_slice()).fetch_optional(&mut *tx).await? {
            if old.try_get::<Uuid,_>("selection_id")?!=selection || old.try_get::<DateTime<Utc>,_>("source_created_at")?!=source.created_at {
                return Err(DmJobError::Refused);
            }
            tx.commit().await?;
            return Ok(false);
        }
        let source_row:Option<Vec<u8>>=sqlx::query_scalar("SELECT event_id FROM office_inbox WHERE company_id=$1 AND event_id=$2 AND event_created_at=$3 AND state='pending' AND claim_generation=0 AND attempt_count=0 AND finalized_at IS NULL FOR SHARE")
            .bind(scope.company_id()).bind(source.id.to_bytes().as_slice()).bind(source.created_at).fetch_optional(&mut *tx).await?;
        if source_row.is_none() { return Err(DmJobError::Refused); }
        let inserted:Option<Vec<u8>>=sqlx::query_scalar(include_str!("enqueue.sql"))
            .bind(scope.company_id()).bind(selection).bind(source.id.to_bytes().as_slice()).bind(source.created_at)
            .bind(office.generation()).bind(office.valid_before()).fetch_optional(&mut *tx).await?;
        if inserted.is_none() {
            let same:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM encrypted_dm_decrypt_jobs WHERE company_id=$1 AND source_id=$2 AND source_created_at=$3 AND selection_id=$4)")
                .bind(scope.company_id()).bind(source.id.to_bytes().as_slice()).bind(source.created_at).bind(selection)
                .fetch_one(&mut *tx).await?;
            if !same { return Err(DmJobError::Refused); }
            tx.commit().await?;
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }

    /// Claims at most one due job, with at most two live claims per company.
    /// Expired crypto attempts wait 1s/5s before retry; three attempts or the
    /// original 120s receipt deadline leave retained failure. No lease renewal.
    /// One ineligible due row may be settled instead; callers must remain bounded.
    pub async fn claim_next(&self,scope:&CompanyScope,worker:Uuid)->Result<Option<DmDecryptClaim>,DmJobError>{
        if worker.is_nil() { return Err(DmJobError::Invalid); }
        let mut tx=self.pool.begin().await?;
        lock_office_authority_on(&mut tx,scope).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('ortak-encrypted-dm-claims:'||$1::text,0))")
            .bind(scope.company_id()).execute(&mut *tx).await?;
        let candidate=sqlx::query("SELECT source_id,selection_id FROM encrypted_dm_decrypt_jobs WHERE company_id=$1 AND community_id=$2 AND state IN('pending','claimed','verified') AND NOT ortak_encrypted_dm_job_consumed(company_id,source_id) AND next_attempt_at<=clock_timestamp() AND (state='pending' OR claim_expires_at+CASE WHEN attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END<=clock_timestamp()) ORDER BY source_received_at,source_id LIMIT 1")
            .bind(scope.company_id()).bind(scope.community_id()).fetch_optional(&mut *tx).await?;
        let Some(candidate)=candidate else { tx.commit().await?; return Ok(None); };
        let id:Vec<u8>=candidate.try_get("source_id")?;
        lock_selection(&mut tx,scope,candidate.try_get("selection_id")?).await?;
        let row=sqlx::query("SELECT j.*,valid_before<=clock_timestamp() AS expired,ortak_encrypted_dm_job_current(j) AS current FROM encrypted_dm_decrypt_jobs j WHERE company_id=$1 AND source_id=$2 FOR UPDATE")
            .bind(scope.company_id()).bind(&id).fetch_one(&mut *tx).await?;
        let code=if row.try_get::<bool,_>("expired")? { Some("deadline_exceeded") }
            else if row.try_get::<i32,_>("attempts")?>=3 { Some("attempts_exhausted") }
            else if !row.try_get::<bool,_>("current")? { Some("authority_changed") } else { None };
        if let Some(code)=code {
            sqlx::query("UPDATE encrypted_dm_decrypt_jobs SET state='failed',terminal_at=clock_timestamp(),error_code=$3,claim_token=NULL,worker_id=NULL,claimed_at=NULL,claim_expires_at=NULL,crypto_deadline=NULL WHERE company_id=$1 AND source_id=$2")
                .bind(scope.company_id()).bind(&id).bind(code).execute(&mut *tx).await?;
            tx.commit().await?;
            return Ok(None);
        }
        let live:i64=sqlx::query_scalar("SELECT count(*) FROM encrypted_dm_decrypt_jobs WHERE company_id=$1 AND state IN('claimed','verified') AND NOT ortak_encrypted_dm_job_consumed(company_id,source_id) AND claim_expires_at>clock_timestamp()")
            .bind(scope.company_id()).fetch_one(&mut *tx).await?;
        if live>=2 { tx.commit().await?; return Ok(None); }
        // The outer inbox remains canonical and untouched. Any ordinary routing
        // claim wins this race until the integrating lane replaces its consumer.
        let pending:Option<Vec<u8>>=sqlx::query_scalar("SELECT event_id FROM office_inbox WHERE company_id=$1 AND event_id=$2 AND state='pending' AND claim_generation=0 AND attempt_count=0 AND finalized_at IS NULL FOR SHARE")
            .bind(scope.company_id()).bind(&id).fetch_optional(&mut *tx).await?;
        if pending.is_none() { return Err(DmJobError::Refused); }
        let token=Uuid::new_v4();
        sqlx::query("WITH stamp AS MATERIALIZED(SELECT clock_timestamp() AS at) UPDATE encrypted_dm_decrypt_jobs SET state='claimed',attempts=attempts+1,claim_generation=claim_generation+1,claim_token=$3,worker_id=$4,claimed_at=stamp.at,claim_expires_at=least(stamp.at+interval '30 seconds',valid_before),crypto_deadline=least(stamp.at+interval '5 seconds',valid_before),next_attempt_at=stamp.at,error_code=NULL FROM stamp WHERE company_id=$1 AND source_id=$2")
            .bind(scope.company_id()).bind(&id).bind(token).bind(worker).execute(&mut *tx).await?;
        let row=sqlx::query("SELECT j.*,s.channel_id,s.human_public_key,s.employee_public_key,s.office_binding_id,s.key_version,s.decrypt_ref,ortak_encrypted_dm_outer(j.company_id,j.community_id,j.source_id,j.source_created_at,s.employee_public_key) AS outer_bytes FROM encrypted_dm_decrypt_jobs j JOIN encrypted_dm_selections s USING(company_id,selection_id) WHERE j.company_id=$1 AND j.source_id=$2")
            .bind(scope.company_id()).bind(&id).fetch_one(&mut *tx).await?;
        let claim=from_row(row)?;
        tx.commit().await?;
        Ok(Some(claim))
    }

    /// Rechecks the exact still-live claim immediately before key/crypto I/O.
    /// This does not extend either deadline or replace its authority generation.
    pub async fn claim_is_current(&self,scope:&CompanyScope,claim:&DmDecryptClaim)->Result<bool,DmJobError>{
        same_scope(scope,claim)?;
        let mut tx=self.pool.begin().await?;
        lock_office_authority_on(&mut tx,scope).await?;
        lock_selection(&mut tx,scope,claim.identity.selection_id).await?;
        let current:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM encrypted_dm_decrypt_jobs j WHERE company_id=$1 AND source_id=$2 AND state='claimed' AND claim_generation=$3 AND claim_token=$4 AND worker_id=$5 AND crypto_deadline>clock_timestamp() AND ortak_encrypted_dm_job_current(j))")
            .bind(scope.company_id()).bind(claim.expected.outer_id().to_bytes().as_slice()).bind(claim.generation).bind(claim.token).bind(claim.worker)
            .fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(current)
    }

    /// Stores only production-codec verification metadata on the exact claim.
    /// The result has no text and grants no run. A crash after this point requires
    /// fresh bounded decrypt work; an old metadata hash is never a plaintext cache.
    pub async fn record_verified(&self,scope:&CompanyScope,claim:&DmDecryptClaim,result:&VerifiedDmRumor)->Result<(),DmJobError>{
        same_scope(scope,claim)?;
        let source=result.source();
        if source.outer_id()!=claim.expected.outer_id() || source.partition_at()!=claim.expected.partition_at()
            || source.outer_author()!=claim.expected.outer_author() || source.human()!=claim.expected.human()
            || source.recipient()!=claim.expected.recipient() || result.outer_hash()!=&claim.outer_hash {
            return Err(DmJobError::Refused);
        }
        let mut tx=self.pool.begin().await?;
        lock_office_authority_on(&mut tx,scope).await?;
        lock_selection(&mut tx,scope,claim.identity.selection_id).await?;
        let updated=sqlx::query("UPDATE encrypted_dm_decrypt_jobs j SET state='verified',seal_id=$6,seal_created_at=$7,rumor_id=$8,rumor_created_at=$9,rumor_hash=$10,reply_to=$11,verified_at=coalesce(verified_at,clock_timestamp()) WHERE company_id=$1 AND source_id=$2 AND state IN('claimed','verified') AND claim_generation=$3 AND claim_token=$4 AND worker_id=$5 AND crypto_deadline>clock_timestamp() AND ortak_encrypted_dm_job_current(j)")
            .bind(scope.company_id()).bind(claim.expected.outer_id().to_bytes().as_slice()).bind(claim.generation).bind(claim.token).bind(claim.worker)
            .bind(result.seal_id().to_bytes().as_slice()).bind(result.seal_created_at()).bind(result.rumor_id().to_bytes().as_slice())
            .bind(result.rumor_created_at()).bind(result.rumor_hash().as_slice()).bind(result.reply_to().map(|id|id.to_bytes().to_vec()))
            .execute(&mut *tx).await?.rows_affected();
        if updated!=1 { return Err(DmJobError::Stale); }
        tx.commit().await?;
        Ok(())
    }

    /// Rechecks and locks one verified claim on the future confidential commit's
    /// caller-owned READ COMMITTED transaction. Locks are Office→selection→job→
    /// inbox; retain that same transaction. This writes nothing and does not
    /// replace the future deferred final-admission/expiry/dedupe checks.
    pub async fn lock_verified_on(connection:&mut PgConnection,scope:&CompanyScope,claim:&DmDecryptClaim)->Result<(),DmJobError>{
        same_scope(scope,claim)?;
        lock_office_authority_on(connection,scope).await?;
        lock_selection(connection,scope,claim.identity.selection_id).await?;
        let found:Option<Vec<u8>>=sqlx::query_scalar("SELECT source_id FROM encrypted_dm_decrypt_jobs WHERE company_id=$1 AND source_id=$2 AND state='verified' AND claim_generation=$3 AND claim_token=$4 AND worker_id=$5 AND claim_expires_at>clock_timestamp() FOR UPDATE")
            .bind(scope.company_id()).bind(claim.expected.outer_id().to_bytes().as_slice()).bind(claim.generation).bind(claim.token).bind(claim.worker)
            .fetch_optional(&mut *connection).await?;
        if found.is_none() { return Err(DmJobError::Stale); }
        sqlx::query("SELECT event_id FROM office_inbox WHERE company_id=$1 AND event_id=$2 FOR SHARE")
            .bind(scope.company_id()).bind(claim.expected.outer_id().to_bytes().as_slice()).execute(&mut *connection).await?;
        let current:bool=sqlx::query_scalar("SELECT ortak_encrypted_dm_job_current(j) AND j.claim_expires_at>clock_timestamp() FROM encrypted_dm_decrypt_jobs j WHERE company_id=$1 AND source_id=$2")
            .bind(scope.company_id()).bind(claim.expected.outer_id().to_bytes().as_slice()).fetch_one(connection).await?;
        if !current { return Err(DmJobError::Stale); }
        Ok(())
    }

    /// Exact private claim correlation for a retained commit receipt. This
    /// deliberately checks neither current authority nor expiry and grants no
    /// decrypt/start permission. Only the immutable receipt recovery lane uses it.
    pub async fn retained_claim_matches_on(connection:&mut PgConnection,scope:&CompanyScope,claim:&DmDecryptClaim)->Result<bool,DmJobError>{
        same_scope(scope,claim)?;
        Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM encrypted_dm_decrypt_jobs WHERE company_id=$1 AND source_id=$2 AND claim_generation=$3 AND claim_token=$4 AND worker_id=$5)")
            .bind(scope.company_id()).bind(claim.expected.outer_id().to_bytes().as_slice()).bind(claim.generation).bind(claim.token).bind(claim.worker)
            .fetch_one(connection).await?)
    }

    /// Exact receipt-only failure/cancellation accounting after authority loss.
    /// Only transient missing material retries (1s, then5s). A stale/expired token
    /// cannot change the job; claim_next later settles its retained expiry.
    pub async fn fail_claim(&self,scope:&CompanyScope,claim:&DmDecryptClaim,failure:DecryptFailure)->Result<(),DmJobError>{
        same_scope(scope,claim)?;
        let updated=sqlx::query("UPDATE encrypted_dm_decrypt_jobs SET state=CASE WHEN $6='material_unavailable' AND attempts<3 AND valid_before>clock_timestamp()+interval '5 seconds' THEN 'pending' WHEN $6='cancelled' THEN 'cancelled' ELSE 'failed' END,terminal_at=CASE WHEN $6='material_unavailable' AND attempts<3 AND valid_before>clock_timestamp()+interval '5 seconds' THEN NULL ELSE clock_timestamp() END,error_code=$6,next_attempt_at=clock_timestamp()+CASE WHEN attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END,claim_token=NULL,worker_id=NULL,claimed_at=NULL,claim_expires_at=NULL,crypto_deadline=NULL WHERE company_id=$1 AND source_id=$2 AND state IN('claimed','verified') AND claim_generation=$3 AND claim_token=$4 AND worker_id=$5 AND claim_expires_at>clock_timestamp()")
            .bind(scope.company_id()).bind(claim.expected.outer_id().to_bytes().as_slice()).bind(claim.generation).bind(claim.token).bind(claim.worker).bind(failure.code())
            .execute(&self.pool).await?.rows_affected();
        if updated!=1 { return Err(DmJobError::Stale); }
        Ok(())
    }
}

fn same_scope(scope:&CompanyScope,claim:&DmDecryptClaim)->Result<(),DmJobError>{
    if scope.company_id()!=claim.identity.company_id || scope.community_id()!=Some(claim.identity.community_id) { return Err(DmJobError::Refused); }
    Ok(())
}

fn from_row(row:PgRow)->Result<DmDecryptClaim,DmJobError>{
    let source:Vec<u8>=row.try_get("source_id")?;
    let author:Vec<u8>=row.try_get("source_author")?;
    let human:Vec<u8>=row.try_get("human_public_key")?;
    let employee:Vec<u8>=row.try_get("employee_public_key")?;
    let outer:Vec<u8>=row.try_get("outer_bytes")?;
    if outer.len()>super::super::MAX_OUTER_BYTES { return Err(DmJobError::Unavailable); }
    let hash:Vec<u8>=row.try_get("source_hash")?;
    Ok(DmDecryptClaim {
        identity:DmClaimIdentity {
            company_id:row.try_get("company_id")?, community_id:row.try_get("community_id")?, channel_id:row.try_get("channel_id")?,
            selection_id:row.try_get("selection_id")?,selection_generation:row.try_get("selection_generation")?,
            employee_id:EmployeeId::parse(row.try_get::<String,_>("employee_id")?).map_err(|_|DmJobError::Unavailable)?,
            employee_revision_id:row.try_get("employee_revision_id")?,employee_lifecycle_epoch:row.try_get("employee_lifecycle_epoch")?,
            office_generation:row.try_get("office_generation")?,office_binding_id:row.try_get("office_binding_id")?,key_version:row.try_get("key_version")?,
            decrypt_ref:CredentialRef::parse(row.try_get::<String,_>("decrypt_ref")?).map_err(|_|DmJobError::Unavailable)?,
        },
        expected:ExpectedEnvelope::new(EventId::from_slice(&source).map_err(|_|DmJobError::Unavailable)?,key(&author)?,row.try_get("source_created_at")?,key(&human)?,key(&employee)?)
            .map_err(|_|DmJobError::Unavailable)?,
        outer,outer_hash:hash.try_into().map_err(|_|DmJobError::Unavailable)?,
        generation:row.try_get("claim_generation")?,token:row.try_get("claim_token")?,worker:row.try_get("worker_id")?,
        crypto_deadline:row.try_get("crypto_deadline")?,expires_at:row.try_get("claim_expires_at")?,
    })
}
