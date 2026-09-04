//! `OfficePublisher` port: sends frozen bytes to the Office verbatim.

use ortak_control::adapter::Detail;
use ortak_control::CompanyScope;

use crate::event::FrozenSignedEvent;

/// Office acknowledgement of a publish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishReceipt {
    /// The Office accepted and stored the event.
    Accepted,
    /// The Office already held this event id; the retry was a no-op.
    AlreadyPresent,
}

/// Publish failures. Both leave the outbox row retryable.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OfficePublishError {
    /// The Office could not be reached or did not acknowledge in time.
    #[error("office unavailable: {detail}")]
    Unavailable {
        /// Bounded detail.
        detail: Detail,
    },
    /// The Office refused the event.
    #[error("office rejected the event: {detail}")]
    Rejected {
        /// Bounded detail.
        detail: Detail,
    },
}

/// Publishes frozen signed events.
#[allow(async_fn_in_trait)]
pub trait OfficePublisher {
    /// Sends [`FrozenSignedEvent::signed_bytes`] exactly as frozen. An
    /// implementation must not rebuild, re-serialize, or re-sign the event.
    async fn publish(
        &self,
        scope: &CompanyScope,
        event: &FrozenSignedEvent,
    ) -> Result<PublishReceipt, OfficePublishError>;
}

impl<T: OfficePublisher + ?Sized> OfficePublisher for &T {
    async fn publish(
        &self,
        scope: &CompanyScope,
        event: &FrozenSignedEvent,
    ) -> Result<PublishReceipt, OfficePublishError> {
        (**self).publish(scope, event).await
    }
}
