use super::{
    inner, remaining, EncryptedExecution, EncryptedExecutionError as Error, ExecutionProgress,
    Result,
};
use crate::{
    confidential::{self, ConfidentialMasterKey},
    postgres::confidential::ConfidentialLease,
};
use ortak_control::confidential::PayloadPurpose;
use ortak_office::encrypted::{key_provider::DmKeySelection, publish::EncryptedDmPublisher};

impl EncryptedExecution<'_> {
    /// Freezes at most one two-copy reply. No network publication occurs here.
    pub async fn seal_reply_once(&self) -> Result<ExecutionProgress> {
        let Some(lease) = self.repository.claim_seal(self.scope).await? else {
            return Ok(ExecutionProgress::Idle);
        };
        match self.seal_claim(&lease).await {
            Ok(()) => Ok(ExecutionProgress::Recorded),
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
    async fn seal_claim(&self, lease: &ConfidentialLease) -> Result<()> {
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
        let draft = self.repository.draft_on(&mut tx, lease).await?;
        let plaintext = confidential::open(
            &master,
            current.payload.identity(),
            PayloadPurpose::ReplyDraft,
            0,
            &draft,
        )
        .map_err(|_| Error::Protocol)?;
        let text = inner::open_reply(plaintext.as_bytes(), current.payload.identity())?;
        let copies = tokio::time::timeout(
            remaining(deadline)?,
            self.keys.seal_reply(&selection, &text),
        )
        .await
        .map_err(|_| Error::Unavailable)?
        .map_err(|_| Error::Unavailable)?;
        drop(text);
        drop(plaintext);
        drop(master);
        remaining(deadline)?;
        self.repository
            .freeze_reply_on(&mut tx, lease, &copies)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    /// Attempts only one immutable copy through the explicit NIP-42 transport.
    /// Every retry rechecks current authority and signs a fresh AUTH challenge,
    /// while the actual EVENT bytes remain exactly the frozen initial bytes.
    pub async fn publish_once(
        &self,
        publisher: &EncryptedDmPublisher,
    ) -> Result<ExecutionProgress> {
        let Some(lease) = self.repository.claim_publish(self.scope).await? else {
            return Ok(ExecutionProgress::Idle);
        };
        let outcome = async {
            let mut tx = self.repository.begin().await?;
            let current = self
                .repository
                .current_on(&mut tx, self.scope, &lease)
                .await
                .map_err(|e| match e {
                    crate::postgres::confidential::ConfidentialAdmissionError::Refused => {
                        Error::Refused
                    }
                    _ => Error::Unavailable,
                })?;
            let deadline = current.payload.valid_before().min(lease.expires);
            let copy = self.repository.frozen_copy_on(&mut tx, &lease).await?;
            let selection = DmKeySelection::from_expected_claims(
                current.payload.identity(),
                current.payload.signer_ref().clone(),
            );
            publisher
                .publish(
                    self.keys,
                    &selection,
                    copy.ordinal,
                    &copy.id,
                    &copy.bytes,
                    remaining(deadline)?,
                )
                .await
                .map_err(|_| Error::Unavailable)?;
            // A known matching ACK is receipt-only accounting, even if a TTL
            // elapsed during the already-authorized network attempt.
            self.repository
                .settle_publish_on(&mut tx, &lease, true, false)
                .await?;
            tx.commit().await?;
            Ok::<(), Error>(())
        }
        .await;
        match outcome {
            Ok(()) => Ok(ExecutionProgress::Recorded),
            Err(error) => {
                self.repository
                    .defer_publish(
                        self.scope,
                        &lease,
                        matches!(error, Error::Refused | Error::Protocol),
                    )
                    .await?;
                Ok(ExecutionProgress::Deferred)
            }
        }
    }
}
