//! Office delivery service: prepare once, publish idempotently, settle the
//! leased outbox row through the existing lease/backoff contract.
//!
//! Order of operations for one leased `office_publish` row:
//!
//! 1. In-memory preconditions (company, outbox kind, run) fail closed with an
//!    error and touch nothing, because the lease may not even be ours to
//!    mutate.
//! 2. The authorized publish is re-derived from the run and binding rows and
//!    the row is read under its lease token. If it already holds a frozen
//!    event, that event is published as-is and the signer is never called.
//! 3. Otherwise the event is signed, verified, and frozen into the row before
//!    the first publish attempt.
//! 4. The frozen bytes are published. Success completes the lease; a signer
//!    or publisher failure records a bounded retry (or terminal failure) on
//!    the row through [`OutboxRepository::fail`].

use std::time::Duration;

use chrono::Utc;
use ortak_control::outbox::{OutboxFailOutcome, OutboxKind, OutboxLease};
use ortak_control::ports::OutboxRepository;
use ortak_control::{CompanyScope, MessageId};

use crate::error::{OfficeDeliveryError, Result};
use crate::event::FrozenSignedEvent;
use crate::publisher::{OfficePublisher, PublishReceipt};
use crate::repository::{
    AuthorizedOfficePublish, FreezeOutcome, FrozenLookup, OfficeDeliveryRepository,
};
use crate::signer::OfficeSigner;

/// Delivery tuning, mirroring the routing worker's fixed retry backoff.
#[derive(Clone, Debug)]
pub struct DeliveryConfig {
    /// Delay before a failed row becomes due again.
    pub retry_backoff: Duration,
}

impl Default for DeliveryConfig {
    fn default() -> Self {
        Self {
            retry_backoff: Duration::from_secs(30),
        }
    }
}

/// Outcome of delivering one leased row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryOutcome {
    /// The frozen event was published and the row completed.
    Delivered {
        /// Signed event id.
        event_id: MessageId,
        /// True when this attempt invoked the signer; false when it reused
        /// the frozen event of an earlier attempt.
        signed_now: bool,
        /// Office acknowledgement.
        receipt: PublishReceipt,
    },
    /// A signer or publish failure was recorded; the row is pending again.
    Retrying {
        /// Frozen event id, if one was frozen before the failure.
        event_id: Option<MessageId>,
        /// Durable error text.
        error: String,
    },
    /// A signer or publish failure exhausted the attempts; the row is terminal.
    Failed {
        /// Frozen event id, if any.
        event_id: Option<MessageId>,
        /// Durable error text.
        error: String,
    },
    /// The lease token was stale at some step; nothing was published under it.
    StaleLease,
}

/// Prepares, freezes, and publishes Office events for leased outbox rows.
#[derive(Clone, Debug)]
pub struct OfficeDeliveryService<R, S, P> {
    repository: R,
    signer: S,
    publisher: P,
    config: DeliveryConfig,
}

impl<R, S, P> OfficeDeliveryService<R, S, P>
where
    R: OfficeDeliveryRepository + OutboxRepository,
    S: OfficeSigner,
    P: OfficePublisher,
{
    /// Builds the service.
    pub fn new(repository: R, signer: S, publisher: P, config: DeliveryConfig) -> Self {
        Self {
            repository,
            signer,
            publisher,
            config,
        }
    }

    /// Delivers one leased `office_publish` row for `authorized`, the object
    /// returned by
    /// [`OfficeDeliveryRepository::enqueue_office_publish`] (a replayed
    /// enqueue returns the same object for a retry in a fresh process).
    pub async fn deliver(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        authorized: &AuthorizedOfficePublish,
    ) -> Result<DeliveryOutcome> {
        authorized.intent().validate()?;
        if authorized.company_id() != scope.company_id() {
            return Err(OfficeDeliveryError::CompanyMismatch {
                expected: scope.company_id(),
                found: authorized.company_id(),
            });
        }
        if lease.kind != OutboxKind::OfficePublish {
            return Err(OfficeDeliveryError::WrongKind { found: lease.kind });
        }
        if lease.run_id != Some(authorized.run_id()) {
            return Err(OfficeDeliveryError::WrongRun {
                expected: authorized.run_id(),
                found: lease.run_id,
            });
        }

        let (frozen, signed_now) = match self
            .repository
            .frozen_event(scope, lease, authorized)
            .await?
        {
            FrozenLookup::Frozen(event) => (*event, false),
            FrozenLookup::StaleLease => return Ok(DeliveryOutcome::StaleLease),
            FrozenLookup::Unfrozen => {
                let signing = authorized.signing_request(Utc::now())?;
                let signed = match self.signer.sign(&signing).await {
                    Ok(signed) => signed,
                    Err(error) => {
                        return self.record_failure(scope, lease, None, error.into()).await
                    }
                };
                match self
                    .repository
                    .freeze_signed_event(scope, lease, &signed)
                    .await?
                {
                    FreezeOutcome::Frozen(event) => (*event, true),
                    FreezeOutcome::StaleLease => return Ok(DeliveryOutcome::StaleLease),
                }
            }
        };

        self.publish_frozen(scope, lease, &frozen, signed_now).await
    }

    async fn publish_frozen(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        frozen: &FrozenSignedEvent,
        signed_now: bool,
    ) -> Result<DeliveryOutcome> {
        match self.publisher.publish(scope, frozen).await {
            Ok(receipt) => {
                if self.repository.complete(scope, lease).await? {
                    Ok(DeliveryOutcome::Delivered {
                        event_id: frozen.event_id(),
                        signed_now,
                        receipt,
                    })
                } else {
                    // Published under a lease that was reclaimed meanwhile. The
                    // new holder republishes the identical frozen bytes, which
                    // the Office deduplicates by event id.
                    tracing::warn!(
                        outbox_id = %lease.id,
                        event_id = %frozen.event_id(),
                        "office publish succeeded but the lease was stale at completion"
                    );
                    Ok(DeliveryOutcome::StaleLease)
                }
            }
            Err(error) => {
                self.record_failure(scope, lease, Some(frozen.event_id()), error.into())
                    .await
            }
        }
    }

    async fn record_failure(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        event_id: Option<MessageId>,
        error: OfficeDeliveryError,
    ) -> Result<DeliveryOutcome> {
        let retry_after = Utc::now()
            + chrono::Duration::from_std(self.config.retry_backoff)
                .unwrap_or_else(|_| chrono::Duration::seconds(30));
        let error = error.to_string();
        tracing::warn!(
            outbox_id = %lease.id,
            attempt = lease.attempt_count,
            error = %error,
            "office delivery attempt failed"
        );
        Ok(
            match self
                .repository
                .fail(scope, lease, &error, retry_after)
                .await?
            {
                OutboxFailOutcome::Retrying => DeliveryOutcome::Retrying { event_id, error },
                OutboxFailOutcome::Terminal => DeliveryOutcome::Failed { event_id, error },
                OutboxFailOutcome::Stale => DeliveryOutcome::StaleLease,
            },
        )
    }
}
