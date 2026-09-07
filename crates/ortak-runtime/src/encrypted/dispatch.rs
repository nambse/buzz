use super::{
    inner, remaining, EncryptedExecution, EncryptedExecutionError as Error, ExecutionProgress,
    Result,
};
use crate::{
    confidential::{self, ConfidentialMasterKey},
    postgres::confidential::ConfidentialLease,
};
use ortak_control::confidential::PayloadPurpose;
use ortak_office::encrypted::key_provider::DmKeySelection;

impl EncryptedExecution<'_> {
    /// Processes at most one due protected dispatch. Lookup precedes any key
    /// access or start; uncertain responses retain the exact stable start key.
    pub async fn dispatch_once(&self) -> Result<ExecutionProgress> {
        let Some(lease) = self.repository.claim_dispatch(self.scope).await? else {
            return Ok(ExecutionProgress::Idle);
        };
        match self.dispatch_claim(&lease).await {
            Ok(()) => Ok(ExecutionProgress::Recorded),
            Err(error) => {
                self.repository
                    .defer_dispatch(
                        self.scope,
                        &lease,
                        matches!(error, Error::Refused | Error::Protocol),
                    )
                    .await?;
                Ok(ExecutionProgress::Deferred)
            }
        }
    }
    async fn dispatch_claim(&self, lease: &ConfidentialLease) -> Result<()> {
        let key = crate::run_idempotency_key(self.scope.company_id(), lease.run);
        let known = tokio::time::timeout(
            remaining(lease.expires)?,
            self.adapter.lookup_confidential(&key),
        )
        .await
        .map_err(|_| Error::Unavailable)?
        .map_err(|_| Error::Unavailable)?;
        let mut tx = self.repository.begin().await?;
        let current = match self.repository.current_on(&mut tx, self.scope, lease).await {
            Ok(current) => current,
            Err(error) => {
                // The exact known reference can be accounted for after source
                // loss. record_start_on queues containment instead of granting
                // new execution when its fresh current predicate is false.
                if error == crate::postgres::confidential::ConfidentialAdmissionError::Refused {
                    if let Some(receipt) = known {
                        self.repository
                            .record_start_on(&mut tx, self.scope, lease, &receipt)
                            .await?;
                        tx.commit().await?;
                        return Ok(());
                    }
                }
                return Err(match error {
                    crate::postgres::confidential::ConfidentialAdmissionError::Refused => {
                        Error::Refused
                    }
                    _ => Error::Unavailable,
                });
            }
        };
        let deadline = current.payload.valid_before().min(lease.expires);
        if known.is_some() {
            let receipt = tokio::time::timeout(
                remaining(deadline)?,
                self.adapter
                    .replay_confidential(current.payload.identity(), current.payload.snapshot()),
            )
            .await
            .map_err(|_| Error::Unavailable)?
            .map_err(|_| Error::Unavailable)?
            .ok_or(Error::Protocol)?;
            self.repository
                .record_start_on(&mut tx, self.scope, lease, &receipt)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
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
        remaining(deadline)?;
        let opened = confidential::open(
            &master,
            current.payload.identity(),
            PayloadPurpose::Snapshot,
            0,
            current.payload.snapshot(),
        )
        .map_err(|_| Error::Protocol)?;
        inner::snapshot(
            opened.as_bytes(),
            current.payload.identity(),
            &current.binding,
            current.reply_to.as_deref(),
        )?;
        let body = confidential::prepare_start_body(
            &master,
            current.payload.identity(),
            current.payload.snapshot(),
        )
        .map_err(|_| Error::Protocol)?;
        // Office master/plaintext never enter the adapter. It receives only the
        // snapshot and the two purpose-derived transient keys.
        drop(opened);
        drop(master);
        let receipt =
            tokio::time::timeout(remaining(deadline)?, self.adapter.start_confidential(body))
                .await
                .map_err(|_| Error::Unavailable)?
                .map_err(|_| Error::Unavailable)?;
        self.repository
            .record_start_on(&mut tx, self.scope, lease, &receipt)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
