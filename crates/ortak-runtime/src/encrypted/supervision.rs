use super::{
    inner, remaining, EncryptedExecution, EncryptedExecutionError as Error, ExecutionProgress,
    Result,
};
use crate::{
    confidential::{self, ConfidentialMasterKey},
    hermes::ConfidentialRunStatus,
    postgres::confidential::ConfidentialLease,
};
use ortak_control::confidential::PayloadPurpose;
use ortak_office::encrypted::key_provider::DmKeySelection;

impl EncryptedExecution<'_> {
    /// One finite event replay or keyless containment operation. The caller owns
    /// scheduling; this method never loops on a failed/empty provider response.
    pub async fn observe_once(&self) -> Result<ExecutionProgress> {
        self.observe_selected(false).await
    }
    /// Receipt-only containment after configuration/key/Office binding loss.
    /// Never claims a normal observation or opens/requests protected content.
    pub async fn recover_stop_once(&self) -> Result<ExecutionProgress> {
        self.observe_selected(true).await
    }
    async fn observe_selected(&self, stops_only: bool) -> Result<ExecutionProgress> {
        let Some(lease) = self
            .repository
            .claim_observation(self.scope, stops_only)
            .await?
        else {
            return Ok(ExecutionProgress::Idle);
        };
        if lease.cancelling() {
            let key = crate::run_idempotency_key(self.scope.company_id(), lease.run);
            let acknowledged = matches!(
                tokio::time::timeout(
                    remaining(lease.expires)?,
                    self.adapter.cancel_confidential(&key)
                )
                .await,
                Ok(Ok(_))
            );
            self.repository
                .settle_observation(
                    self.scope,
                    &lease,
                    if acknowledged {
                        None
                    } else {
                        Some("unavailable")
                    },
                )
                .await?;
            return Ok(if acknowledged {
                ExecutionProgress::Recorded
            } else {
                ExecutionProgress::Deferred
            });
        }
        match self.observe_claim(&lease).await {
            Ok(complete) => {
                if !complete {
                    self.repository
                        .settle_observation(self.scope, &lease, None)
                        .await?;
                }
                Ok(ExecutionProgress::Recorded)
            }
            Err(error) => {
                self.repository
                    .settle_observation(
                        self.scope,
                        &lease,
                        Some(match error {
                            Error::Protocol => "protocol",
                            Error::Refused => "authority_changed",
                            Error::Unavailable => "unavailable",
                        }),
                    )
                    .await?;
                Ok(ExecutionProgress::Deferred)
            }
        }
    }
    async fn observe_claim(&self, lease: &ConfidentialLease) -> Result<bool> {
        let mut tx = self.repository.begin().await?;
        let current = self
            .repository
            .current_on(&mut tx, self.scope, lease)
            .await
            .map_err(|e| match e {
                crate::postgres::confidential::ConfidentialAdmissionError::Refused => {
                    Error::Refused
                }
                _ => Error::Unavailable,
            })?;
        let after = self.repository.last_event_on(&mut tx, lease).await?;
        let deadline = current.payload.valid_before().min(lease.expires);
        let batch = tokio::time::timeout(
            remaining(deadline)?,
            self.adapter
                .confidential_events(current.payload.identity(), after),
        )
        .await
        .map_err(|_| Error::Unavailable)?
        .map_err(|_| Error::Unavailable)?;
        remaining(deadline)?;
        self.repository
            .copy_events_on(&mut tx, lease, &batch)
            .await?;
        // Exact ciphertext/time is durable before opening. A crash now resumes
        // from this cursor and the same retained ciphertext, not fresh AEAD.
        tx.commit().await?;
        match batch.status {
            ConfidentialRunStatus::Failed
            | ConfidentialRunStatus::Cancelled
            | ConfidentialRunStatus::Cancelling => return Err(Error::Unavailable),
            ConfidentialRunStatus::Completed if batch.terminal => {}
            _ => return Ok(false),
        }
        let mut tx = self.repository.begin().await?;
        let current = self
            .repository
            .current_on(&mut tx, self.scope, lease)
            .await
            .map_err(|e| match e {
                crate::postgres::confidential::ConfidentialAdmissionError::Refused => {
                    Error::Refused
                }
                _ => Error::Unavailable,
            })?;
        let deadline = current.payload.valid_before().min(lease.expires);
        remaining(deadline)?;
        let selection = DmKeySelection::from_expected_claims(
            current.payload.identity(),
            current.payload.signer_ref().clone(),
        );
        let master = ConfidentialMasterKey::from_owned(
            self.keys
                .unwrap_master(&selection, current.payload.wrapped_master())
                .map_err(|_| Error::Unavailable)?
                .into_owned(),
        );
        let events = self.repository.retained_events_on(&mut tx, lease).await?;
        let mut fold = inner::Fold::new();
        for event in &events {
            remaining(deadline)?;
            let opened = confidential::open(
                &master,
                current.payload.identity(),
                PayloadPurpose::RuntimeEvent,
                event.ordinal,
                &event.envelope,
            )
            .map_err(|_| Error::Protocol)?;
            fold.push(
                opened.as_bytes(),
                current.payload.identity(),
                event.ordinal,
                event.occurred_at,
            )?;
        }
        let text = fold.finish()?;
        let reply = if let Some(text) = text {
            let bytes = inner::reply_bytes(current.payload.identity(), &text)?;
            Some(
                confidential::seal(
                    &master,
                    current.payload.identity(),
                    PayloadPurpose::ReplyDraft,
                    0,
                    &bytes,
                )
                .map_err(|_| Error::Protocol)?,
            )
        } else {
            None
        };
        remaining(deadline)?;
        self.repository
            .completed_on(&mut tx, self.scope, lease, reply.as_ref())
            .await?;
        tx.commit().await?;
        Ok(true)
    }
}
