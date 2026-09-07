use super::execution::{ConfidentialLease, Lane, PgConfidentialExecution};
use super::{ConfidentialAdmissionError as Error, Result};
use crate::hermes::{ConfidentialEvent, ConfidentialEventBatch};
use chrono::{DateTime, Utc};
use ortak_control::{
    confidential::{ConfidentialEnvelope, PayloadPurpose},
    postgres::lock_office_authority_on,
    CompanyScope,
};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

impl ConfidentialLease {
    pub(crate) fn cancelling(&self) -> bool {
        self.lane == Lane::Cancel
    }
}
impl PgConfidentialExecution {
    pub(crate) async fn claim_observation(
        &self,
        scope: &CompanyScope,
        stops_only: bool,
    ) -> Result<Option<ConfidentialLease>> {
        let mut tx = self.begin().await?;
        lock_office_authority_on(&mut tx, scope).await?;
        // Explicit cancellation can precede dispatch or follow local completion.
        // It imports metadata only, without opening a snapshot or renewing rights.
        sqlx::query("INSERT INTO confidential_execution_leases(company_id,community_id,run_id,state,error_code) SELECT c.company_id,c.community_id,c.run_id,'cancelling','cancelled' FROM confidential_runs c JOIN runtime_cancellations stop ON stop.company_id=c.company_id AND stop.run_id=c.run_id LEFT JOIN confidential_execution_leases x ON x.company_id=c.company_id AND x.run_id=c.run_id WHERE c.company_id=$1 AND c.community_id=$2 AND stop.state='pending' AND (x.run_id IS NULL OR (x.state IN('complete','observing','sealing') AND (x.lease_expires_at IS NULL OR x.lease_expires_at+interval '5 seconds'<=clock_timestamp()))) ORDER BY stop.requested_at,c.run_id LIMIT 1 ON CONFLICT(company_id,run_id) DO UPDATE SET state='cancelling',finished_at=NULL,lease_token=NULL,lease_expires_at=NULL,next_attempt_at=clock_timestamp(),error_code='cancelled' WHERE confidential_execution_leases.state IN('complete','observing','sealing') AND (confidential_execution_leases.lease_expires_at IS NULL OR confidential_execution_leases.lease_expires_at+interval '5 seconds'<=clock_timestamp())")
            .bind(scope.company_id()).bind(scope.community_id()).execute(&mut *tx).await?;
        let row=sqlx::query("SELECT run_id,state FROM confidential_execution_leases WHERE company_id=$1 AND community_id=$2 AND state IN('observing','cancelling') AND (NOT $3 OR state='cancelling') AND next_attempt_at<=clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at+interval '5 seconds'<=clock_timestamp()) ORDER BY next_attempt_at,run_id LIMIT 1")
            .bind(scope.company_id()).bind(scope.community_id()).bind(stops_only).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        let run: Uuid = row.try_get("run_id")?;
        let observing = row.try_get::<String, _>("state")? == "observing";
        let current = if observing {
            sqlx::query_scalar::<_, bool>("SELECT ortak_lock_confidential_dm($1,$2)")
                .bind(scope.company_id())
                .bind(run)
                .fetch_one(&mut *tx)
                .await?
        } else {
            false
        };
        Self::fence_metadata(&mut tx, scope, run).await?;
        let row=sqlx::query("SELECT state,generation,cancel_attempts FROM confidential_execution_leases WHERE company_id=$1 AND run_id=$2 AND state IN('observing','cancelling') AND (lease_expires_at IS NULL OR lease_expires_at+interval '5 seconds'<=clock_timestamp()) FOR UPDATE SKIP LOCKED")
            .bind(scope.company_id()).bind(run).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        if observing && !current {
            Self::stop_on(&mut tx, scope, run, "authority_changed").await?;
            tx.commit().await?;
            return Ok(None);
        }
        let cancel = row.try_get::<String, _>("state")? == "cancelling";
        let generation: i64 = row.try_get("generation")?;
        if (!cancel && generation >= 123)
            || (cancel && (generation >= 128 || row.try_get::<i32, _>("cancel_attempts")? >= 3))
        {
            if !cancel {
                Self::stop_on(&mut tx, scope, run, "deadline_exceeded").await?;
            } else {
                sqlx::query("UPDATE confidential_execution_leases SET state='unconfirmed',error_code='unavailable',finished_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL WHERE company_id=$1 AND run_id=$2")
                .bind(scope.company_id()).bind(run).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            return Ok(None);
        }
        let token = Uuid::new_v4();
        if cancel {
            let changed=sqlx::query("UPDATE runtime_cancellations SET attempt_count=attempt_count+1,lease_token=$3,lease_expires_at=clock_timestamp()+interval '30 seconds' WHERE company_id=$1 AND run_id=$2 AND state='pending' AND attempt_count<least(max_attempts,3) AND next_attempt_at<=clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())")
                .bind(scope.company_id()).bind(run).bind(token).execute(&mut *tx).await?.rows_affected();
            if changed != 1 {
                let live:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2 AND state='pending' AND attempt_count<least(max_attempts,3) AND (next_attempt_at>clock_timestamp() OR lease_expires_at>clock_timestamp()))")
                    .bind(scope.company_id()).bind(run).fetch_one(&mut *tx).await?;
                if live {
                    return Ok(None);
                }
                let stopped:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2 AND state='acknowledged')")
                    .bind(scope.company_id()).bind(run).fetch_one(&mut *tx).await?;
                sqlx::query("UPDATE confidential_execution_leases SET state=$3,finished_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL,error_code=$4 WHERE company_id=$1 AND run_id=$2")
                    .bind(scope.company_id()).bind(run).bind(if stopped{"stopped"}else{"unconfirmed"}).bind(if stopped{None}else{Some("unavailable")}).execute(&mut *tx).await?;
                if !stopped {
                    sqlx::query("UPDATE runtime_cancellations SET state='failed',lease_token=NULL,lease_expires_at=NULL,last_error_code='confidential_stop_exhausted' WHERE company_id=$1 AND run_id=$2 AND state='pending' AND (lease_expires_at IS NULL OR lease_expires_at<=clock_timestamp())")
                        .bind(scope.company_id()).bind(run).execute(&mut *tx).await?;
                }
                tx.commit().await?;
                return Ok(None);
            }
        }
        let row=sqlx::query("UPDATE confidential_execution_leases SET generation=generation+1,cancel_attempts=cancel_attempts+(CASE WHEN state='cancelling' THEN 1 ELSE 0 END),lease_token=$3,lease_expires_at=clock_timestamp()+interval '30 seconds' WHERE company_id=$1 AND run_id=$2 RETURNING community_id,generation,lease_expires_at")
            .bind(scope.company_id()).bind(run).bind(token).fetch_one(&mut *tx).await?;
        let lease = ConfidentialLease {
            company: scope.company_id(),
            community: row.try_get("community_id")?,
            run,
            token,
            generation: row.try_get("generation")?,
            expires: row.try_get("lease_expires_at")?,
            lane: if cancel { Lane::Cancel } else { Lane::Observe },
            copy: 0,
        };
        tx.commit().await?;
        Ok(Some(lease))
    }
    pub(crate) async fn last_event_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        lease: &ConfidentialLease,
    ) -> Result<u32> {
        let ordinal:i32=sqlx::query_scalar("SELECT coalesce(max(ordinal),0) FROM confidential_event_receipts WHERE company_id=$1 AND run_id=$2")
            .bind(lease.company).bind(lease.run).fetch_one(&mut **tx).await?;
        u32::try_from(ordinal).map_err(|_| Error::Payload)
    }
    pub(crate) async fn copy_events_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        lease: &ConfidentialLease,
        batch: &ConfidentialEventBatch,
    ) -> Result<()> {
        self.lease_on(tx, lease).await?;
        if batch.events.len() > 4 {
            return Err(Error::Payload);
        }
        for event in &batch.events {
            let old=sqlx::query("SELECT p.envelope_bytes,e.occurred_at FROM confidential_run_payloads p JOIN confidential_event_receipts e USING(company_id,run_id,purpose,ordinal) WHERE p.company_id=$1 AND p.run_id=$2 AND p.purpose='runtime_event' AND p.ordinal=$3")
                .bind(lease.company).bind(lease.run).bind(event.ordinal as i32).fetch_optional(&mut **tx).await?;
            if let Some(old) = old {
                if old.try_get::<Vec<u8>, _>("envelope_bytes")? != event.envelope.canonical_bytes()
                    || old.try_get::<DateTime<Utc>, _>("occurred_at")? != event.occurred_at
                {
                    return Err(Error::Payload);
                }
                continue;
            }
            sqlx::query("INSERT INTO confidential_run_payloads(company_id,community_id,run_id,purpose,ordinal,envelope_bytes,nonce) VALUES($1,$2,$3,'runtime_event',$4,$5,$6)")
                .bind(lease.company).bind(lease.community).bind(lease.run).bind(event.ordinal as i32).bind(event.envelope.canonical_bytes()).bind(event.envelope.nonce().as_slice()).execute(&mut **tx).await?;
            sqlx::query("INSERT INTO confidential_event_receipts(company_id,community_id,run_id,ordinal,occurred_at) VALUES($1,$2,$3,$4,$5)")
                .bind(lease.company).bind(lease.community).bind(lease.run).bind(event.ordinal as i32).bind(event.occurred_at).execute(&mut **tx).await?;
        }
        Ok(())
    }
    pub(crate) async fn retained_events_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        lease: &ConfidentialLease,
    ) -> Result<Vec<ConfidentialEvent>> {
        self.lease_on(tx, lease).await?;
        // The first execution grammar is <=4 events, even though the envelope
        // protocol reserves 512 ordinals for a future separately reviewed mode.
        let rows=sqlx::query("SELECT p.ordinal,p.envelope_bytes,e.occurred_at FROM confidential_run_payloads p JOIN confidential_event_receipts e USING(company_id,run_id,purpose,ordinal) WHERE p.company_id=$1 AND p.run_id=$2 AND p.purpose='runtime_event' ORDER BY p.ordinal LIMIT 5")
            .bind(lease.company).bind(lease.run).fetch_all(&mut **tx).await?;
        if rows.len() > 4 {
            return Err(Error::Payload);
        }
        let mut result = Vec::with_capacity(rows.len());
        for (index, row) in rows.into_iter().enumerate() {
            let ordinal: i32 = row.try_get("ordinal")?;
            if ordinal != index as i32 + 1 {
                return Err(Error::Payload);
            }
            let envelope =
                ConfidentialEnvelope::parse(&row.try_get::<Vec<u8>, _>("envelope_bytes")?)
                    .map_err(|_| Error::Payload)?;
            result.push(ConfidentialEvent {
                ordinal: ordinal as u32,
                occurred_at: row.try_get("occurred_at")?,
                envelope,
            });
        }
        Ok(result)
    }
    pub(crate) async fn completed_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        scope: &CompanyScope,
        lease: &ConfidentialLease,
        reply: Option<&ConfidentialEnvelope>,
    ) -> Result<()> {
        self.lease_on(tx, lease).await?;
        if let Some(reply) = reply {
            if reply.header().purpose() != PayloadPurpose::ReplyDraft {
                return Err(Error::Payload);
            }
            // One encryption is retained before any signing/publication. If an
            // uncertain commit retries, completion is already terminal and the
            // next reply step loads this exact retained draft.
            sqlx::query("INSERT INTO confidential_run_payloads(company_id,community_id,run_id,purpose,ordinal,envelope_bytes,nonce) VALUES($1,$2,$3,'reply_draft',0,$4,$5)")
                .bind(lease.company).bind(lease.community).bind(lease.run).bind(reply.canonical_bytes()).bind(reply.nonce().as_slice()).execute(&mut **tx).await?;
        }
        sqlx::query("UPDATE runs SET status='completed',delivery_intent=$3,finished_at=clock_timestamp(),updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND status IN('running','waiting')")
            .bind(scope.company_id()).bind(lease.run).bind(if reply.is_some(){"reply"}else{"silent"}).execute(&mut **tx).await?;
        sqlx::query("UPDATE confidential_execution_leases SET state=$3,finished_at=(CASE WHEN $3='complete' THEN clock_timestamp() ELSE NULL END),next_attempt_at=clock_timestamp()+interval '1 second',lease_token=NULL,lease_expires_at=NULL,error_code=NULL WHERE company_id=$1 AND run_id=$2")
            .bind(lease.company).bind(lease.run).bind(if reply.is_some(){"sealing"}else{"complete"}).execute(&mut **tx).await?;
        Ok(())
    }
    pub(crate) async fn settle_observation(
        &self,
        scope: &CompanyScope,
        lease: &ConfidentialLease,
        code: Option<&str>,
    ) -> Result<()> {
        let mut tx = self.begin().await?;
        Self::fence_metadata(&mut tx, scope, lease.run).await?;
        self.lease_on(&mut tx, lease).await?;
        if lease.lane == Lane::Cancel {
            let exhausted:bool=sqlx::query_scalar("SELECT cancel_attempts>=3 FROM confidential_execution_leases WHERE company_id=$1 AND run_id=$2").bind(lease.company).bind(lease.run).fetch_one(&mut *tx).await?;
            sqlx::query("UPDATE confidential_execution_leases SET state=$3,finished_at=(CASE WHEN $3='cancelling' THEN NULL ELSE clock_timestamp() END),lease_token=NULL,lease_expires_at=NULL,next_attempt_at=clock_timestamp()+interval '5 seconds',error_code=$4 WHERE company_id=$1 AND run_id=$2")
                .bind(lease.company).bind(lease.run).bind(if code.is_none(){"stopped"}else if exhausted{"unconfirmed"}else{"cancelling"}).bind(code).execute(&mut *tx).await?;
            let changed=sqlx::query("UPDATE runtime_cancellations SET state=$4,acknowledged_at=(CASE WHEN $4='acknowledged' THEN clock_timestamp() ELSE NULL END),lease_token=NULL,lease_expires_at=NULL,last_error_code=$5,next_attempt_at=clock_timestamp()+interval '5 seconds' WHERE company_id=$1 AND run_id=$2 AND state='pending' AND lease_token=$3 AND lease_expires_at>clock_timestamp()")
                .bind(lease.company).bind(lease.run).bind(lease.token).bind(if code.is_none(){"acknowledged"}else if exhausted{"failed"}else{"pending"})
                .bind(if code.is_none(){None}else{Some("confidential_stop_unconfirmed")}).execute(&mut *tx).await?.rows_affected();
            if changed != 1 {
                return Err(Error::Refused);
            }
            sqlx::query("UPDATE run_cancel_requests h SET status=c.state,attempts=greatest(h.attempts,c.attempt_count),next_attempt_at=c.next_attempt_at,lease_token=c.lease_token,lease_expires_at=c.lease_expires_at,last_error_code=c.last_error_code,acknowledged_at=c.acknowledged_at FROM runtime_cancellations c WHERE h.company_id=$1 AND h.run_id=$2 AND c.company_id=h.company_id AND c.run_id=h.run_id AND h.status='pending'")
                .bind(lease.company).bind(lease.run).execute(&mut *tx).await?;
        } else if code.is_some_and(|c| c != "unavailable") {
            Self::stop_on(&mut tx, scope, lease.run, code.ok_or(Error::Payload)?).await?;
        } else {
            let failures:i32=sqlx::query_scalar("SELECT failures FROM confidential_execution_leases WHERE company_id=$1 AND run_id=$2").bind(lease.company).bind(lease.run).fetch_one(&mut *tx).await?;
            if code.is_some() && failures >= 2 {
                Self::stop_on(&mut tx, scope, lease.run, "unavailable").await?;
            } else {
                sqlx::query("UPDATE confidential_execution_leases SET failures=$3,lease_token=NULL,lease_expires_at=NULL,next_attempt_at=clock_timestamp()+interval '5 seconds',error_code=$4 WHERE company_id=$1 AND run_id=$2")
                .bind(lease.company).bind(lease.run).bind(if code.is_some(){failures+1}else{0}).bind(code).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }
}
