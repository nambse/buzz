use super::execution::{ConfidentialLease, Lane, PgConfidentialExecution};
use super::{ConfidentialAdmissionError as Error, Result};
use ortak_control::{
    confidential::ConfidentialEnvelope, postgres::lock_office_authority_on, CompanyScope,
};
use ortak_office::encrypted::key_provider::SealedDmReply;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

pub(crate) struct FrozenReplyCopy {
    pub(crate) id: [u8; 32],
    pub(crate) bytes: Vec<u8>,
    pub(crate) ordinal: u8,
}
impl PgConfidentialExecution {
    pub(crate) async fn claim_seal(
        &self,
        scope: &CompanyScope,
    ) -> Result<Option<ConfidentialLease>> {
        let mut tx = self.begin().await?;
        lock_office_authority_on(&mut tx, scope).await?;
        let run:Option<Uuid>=sqlx::query_scalar("SELECT run_id FROM confidential_execution_leases WHERE company_id=$1 AND community_id=$2 AND state='sealing' AND next_attempt_at<=clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at+interval '5 seconds'<=clock_timestamp()) ORDER BY next_attempt_at,run_id LIMIT 1")
            .bind(scope.company_id()).bind(scope.community_id()).fetch_optional(&mut *tx).await?;
        let Some(run) = run else { return Ok(None) };
        let current: bool = sqlx::query_scalar("SELECT ortak_lock_confidential_dm($1,$2)")
            .bind(scope.company_id())
            .bind(run)
            .fetch_one(&mut *tx)
            .await?;
        Self::fence_metadata(&mut tx, scope, run).await?;
        let row=sqlx::query("SELECT generation,failures FROM confidential_execution_leases WHERE company_id=$1 AND run_id=$2 AND state='sealing' AND (lease_expires_at IS NULL OR lease_expires_at+interval '5 seconds'<=clock_timestamp()) FOR UPDATE SKIP LOCKED")
            .bind(scope.company_id()).bind(run).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        if !current
            || row.try_get::<i64, _>("generation")? >= 123
            || row.try_get::<i32, _>("failures")? >= 3
        {
            Self::stop_on(&mut tx, scope, run, "authority_changed").await?;
            tx.commit().await?;
            return Ok(None);
        }
        let token = Uuid::new_v4();
        let row=sqlx::query("UPDATE confidential_execution_leases SET generation=generation+1,lease_token=$3,lease_expires_at=clock_timestamp()+interval '30 seconds' WHERE company_id=$1 AND run_id=$2 RETURNING community_id,generation,lease_expires_at")
            .bind(scope.company_id()).bind(run).bind(token).fetch_one(&mut *tx).await?;
        let lease = ConfidentialLease {
            company: scope.company_id(),
            community: row.try_get("community_id")?,
            run,
            token,
            generation: row.try_get("generation")?,
            expires: row.try_get("lease_expires_at")?,
            lane: Lane::Seal,
            copy: 0,
        };
        tx.commit().await?;
        Ok(Some(lease))
    }
    pub(crate) async fn draft_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        lease: &ConfidentialLease,
    ) -> Result<ConfidentialEnvelope> {
        self.lease_on(tx, lease).await?;
        let bytes:Vec<u8>=sqlx::query_scalar("SELECT envelope_bytes FROM confidential_run_payloads WHERE company_id=$1 AND run_id=$2 AND purpose='reply_draft' AND ordinal=0")
            .bind(lease.company).bind(lease.run).fetch_one(&mut **tx).await?;
        ConfidentialEnvelope::parse(&bytes).map_err(|_| Error::Payload)
    }
    pub(crate) async fn freeze_reply_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        lease: &ConfidentialLease,
        reply: &SealedDmReply,
    ) -> Result<()> {
        self.lease_on(tx, lease).await?;
        sqlx::query("INSERT INTO confidential_reply_bundles(company_id,community_id,run_id,rumor_id,rumor_hash,recipient_id,history_id,recipient_bytes,history_bytes) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(lease.company).bind(lease.community).bind(lease.run).bind(reply.rumor_id().as_slice()).bind(reply.rumor_hash().as_slice())
            .bind(reply.copies()[0].id().as_slice()).bind(reply.copies()[1].id().as_slice()).bind(reply.copies()[0].bytes()).bind(reply.copies()[1].bytes()).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO confidential_reply_outbox(company_id,community_id,run_id,copy) VALUES($1,$2,$3,0),($1,$2,$3,1)")
            .bind(lease.company).bind(lease.community).bind(lease.run).execute(&mut **tx).await?;
        sqlx::query("UPDATE confidential_execution_leases SET state='complete',finished_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL,error_code=NULL WHERE company_id=$1 AND run_id=$2")
            .bind(lease.company).bind(lease.run).execute(&mut **tx).await?;
        Ok(())
    }
    pub(crate) async fn claim_publish(
        &self,
        scope: &CompanyScope,
    ) -> Result<Option<ConfidentialLease>> {
        let mut tx = self.begin().await?;
        lock_office_authority_on(&mut tx, scope).await?;
        let row=sqlx::query("SELECT run_id,copy FROM confidential_reply_outbox WHERE company_id=$1 AND community_id=$2 AND state='pending' AND next_attempt_at<=clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at+interval '5 seconds'<=clock_timestamp()) ORDER BY next_attempt_at,run_id,copy LIMIT 1")
            .bind(scope.company_id()).bind(scope.community_id()).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        let run: Uuid = row.try_get("run_id")?;
        let copy: i32 = row.try_get("copy")?;
        let current: bool = sqlx::query_scalar("SELECT ortak_lock_confidential_dm($1,$2)")
            .bind(scope.company_id())
            .bind(run)
            .fetch_one(&mut *tx)
            .await?;
        Self::fence_metadata(&mut tx, scope, run).await?;
        let row=sqlx::query("SELECT attempts FROM confidential_reply_outbox WHERE company_id=$1 AND run_id=$2 AND copy=$3 AND state='pending' AND (lease_expires_at IS NULL OR lease_expires_at+interval '5 seconds'<=clock_timestamp()) FOR UPDATE SKIP LOCKED")
            .bind(scope.company_id()).bind(run).bind(copy).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        if !current || row.try_get::<i32, _>("attempts")? >= 3 {
            sqlx::query("UPDATE confidential_reply_outbox SET state=$4,error_code=$5,finished_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL WHERE company_id=$1 AND run_id=$2 AND copy=$3")
                .bind(scope.company_id()).bind(run).bind(copy).bind(if current{"failed"}else{"retired"}).bind(if current{"unavailable"}else{"authority_changed"}).execute(&mut *tx).await?;
            tx.commit().await?;
            return Ok(None);
        }
        let token = Uuid::new_v4();
        let row=sqlx::query("UPDATE confidential_reply_outbox o SET attempts=attempts+1,generation=generation+1,lease_token=$4,lease_expires_at=least(c.execution_deadline,clock_timestamp()+interval '30 seconds') FROM confidential_runs c WHERE o.company_id=$1 AND o.run_id=$2 AND o.copy=$3 AND c.company_id=o.company_id AND c.run_id=o.run_id RETURNING o.community_id,o.generation,o.lease_expires_at")
            .bind(scope.company_id()).bind(run).bind(copy).bind(token).fetch_one(&mut *tx).await?;
        let lease = ConfidentialLease {
            company: scope.company_id(),
            community: row.try_get("community_id")?,
            run,
            token,
            generation: row.try_get("generation")?,
            expires: row.try_get("lease_expires_at")?,
            lane: Lane::Publish,
            copy,
        };
        tx.commit().await?;
        Ok(Some(lease))
    }
    pub(crate) async fn frozen_copy_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        lease: &ConfidentialLease,
    ) -> Result<FrozenReplyCopy> {
        self.lease_on(tx, lease).await?;
        let row=sqlx::query("SELECT CASE $3 WHEN 0 THEN recipient_id ELSE history_id END AS id,CASE $3 WHEN 0 THEN recipient_bytes ELSE history_bytes END AS bytes FROM confidential_reply_bundles WHERE company_id=$1 AND run_id=$2")
            .bind(lease.company).bind(lease.run).bind(lease.copy).fetch_one(&mut **tx).await?;
        let id: Vec<u8> = row.try_get("id")?;
        Ok(FrozenReplyCopy {
            id: id.try_into().map_err(|_| Error::Payload)?,
            bytes: row.try_get("bytes")?,
            ordinal: lease.copy as u8,
        })
    }
    pub(crate) async fn settle_publish_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        lease: &ConfidentialLease,
        acked: bool,
        retire: bool,
    ) -> Result<()> {
        if acked {
            if retire || lease.lane != Lane::Publish {
                return Err(Error::Payload);
            }
            // The exact matching relay ACK has already arrived. UPDATE locks
            // and rechecks this unchanged owner; elapsed time may not discard
            // its receipt, and a takeover cannot be settled by the old token.
            let changed = sqlx::query(include_str!("reply/ack.sql"))
                .bind(lease.company)
                .bind(lease.community)
                .bind(lease.run)
                .bind(lease.copy)
                .bind(lease.generation)
                .bind(lease.token)
                .execute(&mut **tx)
                .await?
                .rows_affected();
            return if changed == 1 {
                Ok(())
            } else {
                Err(Error::Refused)
            };
        }
        self.lease_on(tx, lease).await?;
        let state = if retire {
            "retired"
        } else if lease.generation >= 3 {
            "failed"
        } else {
            "pending"
        };
        let changed=sqlx::query("UPDATE confidential_reply_outbox SET state=$5,finished_at=(CASE WHEN $5='pending' THEN NULL ELSE clock_timestamp() END),lease_token=NULL,lease_expires_at=NULL,next_attempt_at=clock_timestamp()+interval '5 seconds',error_code=$6 WHERE company_id=$1 AND run_id=$2 AND copy=$3 AND lease_token=$4 AND generation=$7 AND state='pending'")
            .bind(lease.company).bind(lease.run).bind(lease.copy).bind(lease.token).bind(state).bind(if retire{Some("authority_changed")}else{Some("unavailable")}).bind(lease.generation).execute(&mut **tx).await?.rows_affected();
        if changed != 1 {
            return Err(Error::Refused);
        }
        Ok(())
    }
    pub(crate) async fn defer_publish(
        &self,
        scope: &CompanyScope,
        lease: &ConfidentialLease,
        retire: bool,
    ) -> Result<()> {
        let mut tx = self.begin().await?;
        Self::fence_metadata(&mut tx, scope, lease.run).await?;
        self.settle_publish_on(&mut tx, lease, false, retire)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
