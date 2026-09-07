#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Ortak Office delivery foundation (Architecture v0 §4.7, Milestone 2).
//!
//! This crate owns the employee-authored side of the Office boundary:
//!
//! - [`OfficeSigner`] signs a validated unsigned event through an opaque
//!   signer reference. Its only return type, [`FrozenSignedEvent`], is built
//!   by internal verification of the event id, Schnorr signature, author
//!   public key, and exact signed fields, so an inconsistent signer fails
//!   closed and no private key material crosses the port.
//! - [`OfficeDeliveryRepository`] authorizes a caller's [`OfficePublishDraft`]
//!   by deriving the employee and revision from the company-scoped run row
//!   and the signer reference and key from that revision's verified,
//!   in-window Office binding, returning the crate-sealed
//!   [`AuthorizedOfficePublish`] that is the only input to signing, freezing,
//!   and retry. It freezes the exact signed bytes and id into the
//!   `office_publish` outbox row under the current lease token before the
//!   first publish, never overwrites a frozen payload, and returns identical
//!   bytes on retry.
//! - Only the Office chat message kinds in [`ALLOWED_PUBLISH_KINDS`] can be
//!   signed; profile, deletion, and every replaceable kind are refused.
//! - [`OfficePublisher`] publishes frozen bytes verbatim, and
//!   [`OfficeDeliveryService`] prepares once, publishes idempotently, and
//!   settles the row through the existing outbox lease, bounded retry, and
//!   terminal-failure contract.
//!
//! The inbound side of the boundary is [`normalizer::PgChannelNormalizer`],
//! the production `MessageNormalizer` that turns a claimed inbox row into a
//! routable envelope or an explicit refusal from canonical Office rows.
//!
//! [`identity::PgOfficeIdentityAdapter`] additionally adopts prepared identities
//! inside a finite company/community/employee/channel allowlist and publishes
//! journaled profiles through the production HTTP transport. Callers explicitly
//! construct and invoke these adapters; this crate starts no background worker.

pub mod delivery;
#[cfg(feature = "encrypted-dm")]
pub mod encrypted;
mod error;
pub mod event;
pub mod fakes;
pub mod identity;
pub mod normalizer;
pub mod postgres;
pub mod publisher;
pub mod repository;
pub mod signer;
pub mod transport;

pub use delivery::{DeliveryConfig, DeliveryOutcome, OfficeDeliveryService};
pub use error::{BindingRejection, OfficeDeliveryError, Result};
pub use event::{
    is_allowed_publish_kind, FrozenSignedEvent, IntentFingerprint, OfficeEventError,
    OfficePublishIntent, OfficePublishPayload, UnsignedOfficeEvent, ALLOWED_PUBLISH_KINDS,
    KIND_STREAM_MESSAGE, KIND_STREAM_MESSAGE_V2,
};
pub use normalizer::{
    expected_channel_type, is_supported_channel_kind, PgChannelNormalizer, KIND_GIFT_WRAP,
};
pub use publisher::{OfficePublishError, OfficePublisher, PublishReceipt};
pub use repository::{
    AuthorizedOfficePublish, EnqueueOutcome, FreezeOutcome, FrozenLookup, OfficeDeliveryRepository,
    OfficePublishDraft,
};
pub use signer::{OfficeSigner, OfficeSignerError, SigningRequest};
