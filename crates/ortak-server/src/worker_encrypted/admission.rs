use super::WorkerEncrypted;
use chrono::{DateTime, Utc};
use nostr::EventId;
use ortak_control::CompanyScope;
use ortak_office::encrypted::{
    jobs::{DecryptFailure, DmDecryptClaim, DmOuterSource},
    key_provider::DmKeyError,
};
use ortak_runtime::postgres::confidential::{ConfidentialAdmissionError, ProtectedConfidentialRun};
use sqlx::Row;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Owns only ciphertext and original opaque claim evidence between commit
/// attempts. Never serialize this object into an ordinary outbox or log it.
pub(super) struct PendingAdmission {
    scope: CompanyScope,
    claim: DmDecryptClaim,
    protected: ProtectedConfidentialRun,
    attempts: u8,
    retry_at: Instant,
}

pub(super) struct SourceCursor {
    community: Option<Uuid>,
    received: DateTime<Utc>,
    id: Vec<u8>,
}

impl WorkerEncrypted {
    pub(super) async fn replay_admission(&mut self) -> Result<(), &'static str> {
        let Some(pending) = self.pending.as_mut() else {
            return Ok(());
        };
        if Instant::now() < pending.retry_at {
            return Ok(());
        }
        pending.attempts += 1;
        // The production repository checks an exact retained receipt first,
        // including after revocation/expiry. Never recompute protected bytes.
        match self
            .runs
            .commit(&pending.scope, &pending.claim, &pending.protected)
            .await
        {
            Ok(_) => {
                self.pending = None;
                Ok(())
            }
            Err(_) if pending.attempts < 3 => {
                pending.retry_at =
                    Instant::now() + Duration::from_secs(if pending.attempts == 1 { 1 } else { 5 });
                Ok(())
            }
            // A possibly committed claim MUST NOT be reset/failed here. On
            // restart its consumed receipt or original bounded job survives.
            Err(_) => Err("encrypted admission receipt remains unresolved"),
        }
    }

    pub(super) async fn admit_one(
        &mut self,
        scope: &CompanyScope,
        allowed: &[Uuid],
    ) -> Result<(), &'static str> {
        if !allowed.is_empty() {
            let cursor = self
                .source_cursor
                .as_ref()
                .filter(|c| c.community == scope.community_id());
            // The preselection bounds even unsuccessful outer reconstruction:
            // <=32 metadata rows x <=16 explicitly configured pairs. Keyset
            // advancement also passes unrelated wraps when ordinary work is off.
            let rows = sqlx::query("WITH candidates AS MATERIALIZED (
                SELECT i.company_id,i.event_id,i.event_created_at,i.received_at FROM office_inbox i
                WHERE i.company_id=$1 AND i.event_kind=1059 AND i.state='pending'
                  AND i.claim_generation=0 AND i.attempt_count=0 AND i.finalized_at IS NULL
                  AND i.received_at+interval '120 seconds'>clock_timestamp()
                  AND ($4::timestamptz IS NULL OR (i.received_at,i.event_id)>($4,$5::bytea))
                  AND NOT EXISTS(SELECT 1 FROM encrypted_dm_decrypt_jobs j WHERE j.company_id=i.company_id AND j.source_id=i.event_id)
                ORDER BY i.received_at,i.event_id LIMIT 32)
                SELECT i.event_id,i.event_created_at,i.received_at,selected.selection_id FROM candidates i
                LEFT JOIN LATERAL (SELECT s.selection_id FROM encrypted_dm_selections s
                  WHERE s.company_id=i.company_id AND s.community_id=$2 AND s.selection_id=ANY($3)
                    AND s.enabled AND i.received_at>=s.enabled_at AND ortak_encrypted_dm_pair_current(s)
                    AND ortak_encrypted_dm_outer(s.company_id,s.community_id,i.event_id,i.event_created_at,s.employee_public_key) IS NOT NULL
                  ORDER BY s.selection_id LIMIT 1) selected ON true ORDER BY i.received_at,i.event_id")
                .bind(scope.company_id()).bind(scope.community_id()).bind(allowed)
                .bind(cursor.map(|c| c.received)).bind(cursor.map(|c| c.id.as_slice()))
                .fetch_all(self.control.pool()).await.map_err(|_| "encrypted source discovery failed")?;
            if rows.is_empty() {
                self.source_cursor = None;
            }
            for row in rows {
                let id: Vec<u8> = row
                    .try_get("event_id")
                    .map_err(|_| "encrypted source metadata invalid")?;
                self.source_cursor = Some(SourceCursor {
                    community: scope.community_id(),
                    received: row
                        .try_get("received_at")
                        .map_err(|_| "encrypted source metadata invalid")?,
                    id: id.clone(),
                });
                let selection: Option<Uuid> = row
                    .try_get("selection_id")
                    .map_err(|_| "encrypted selection invalid")?;
                let Some(selection) = selection else {
                    continue;
                };
                let at: DateTime<Utc> = row
                    .try_get("event_created_at")
                    .map_err(|_| "encrypted source metadata invalid")?;
                let source = DmOuterSource::new(
                    EventId::from_slice(&id).map_err(|_| "encrypted source ID invalid")?,
                    at,
                )
                .map_err(|_| "encrypted source partition invalid")?;
                self.jobs
                    .enqueue(scope, selection, &source)
                    .await
                    .map_err(|_| "encrypted source enqueue unresolved")?;
                break;
            }
        }
        // Claim processing also runs with no allowlist so old jobs get bounded
        // cancellation/expiry metadata. There is no crypto before both checks.
        let Some(claim) = self
            .jobs
            .claim_next(scope, self.worker)
            .await
            .map_err(|_| "encrypted decrypt claim failed")?
        else {
            return Ok(());
        };
        if !allowed.contains(&claim.identity().selection_id) {
            return self.fail(scope, &claim, DecryptFailure::Cancelled).await;
        }
        if !self
            .jobs
            .claim_is_current(scope, &claim)
            .await
            .map_err(|_| "encrypted claim authority unavailable")?
        {
            return self
                .fail(scope, &claim, DecryptFailure::AuthorityChanged)
                .await;
        }
        let Some(selected) = &self.selected else {
            return self.fail(scope, &claim, DecryptFailure::Cancelled).await;
        };
        let verified = match selected.keys.decrypt_claim(&claim) {
            Ok(verified) => verified,
            Err(error) => {
                return self
                    .fail(
                        scope,
                        &claim,
                        match error {
                            DmKeyError::Unavailable => DecryptFailure::MaterialUnavailable,
                            DmKeyError::Refused if Utc::now() >= claim.crypto_deadline() => {
                                DecryptFailure::DeadlineExceeded
                            }
                            DmKeyError::Refused => DecryptFailure::AuthorityChanged,
                            _ => DecryptFailure::CryptoInvalid,
                        },
                    )
                    .await
            }
        };
        self.jobs
            .record_verified(scope, &claim, &verified)
            .await
            .map_err(|_| "encrypted verified result persist unresolved")?;
        let prepared = match self.runs.prepare(scope, &claim, &verified).await {
            Ok(prepared) => prepared,
            Err(ConfidentialAdmissionError::Unavailable) => {
                return Err("encrypted preparation storage unavailable")
            }
            Err(_) => {
                return self
                    .fail(scope, &claim, DecryptFailure::AuthorityChanged)
                    .await
            }
        };
        let protected = match prepared.protect(&selected.keys) {
            Ok(protected) => protected,
            Err(ConfidentialAdmissionError::Unavailable) => {
                return self
                    .fail(scope, &claim, DecryptFailure::MaterialUnavailable)
                    .await
            }
            Err(_) => {
                return self
                    .fail(scope, &claim, DecryptFailure::AuthorityChanged)
                    .await
            }
        };
        drop(verified);
        self.pending = Some(PendingAdmission {
            scope: scope.clone(),
            claim,
            protected,
            attempts: 0,
            retry_at: Instant::now(),
        });
        self.replay_admission().await
    }

    async fn fail(
        &self,
        scope: &CompanyScope,
        claim: &DmDecryptClaim,
        failure: DecryptFailure,
    ) -> Result<(), &'static str> {
        self.jobs
            .fail_claim(scope, claim, failure)
            .await
            .map_err(|_| "encrypted decrypt failure persist unresolved")
    }
}
