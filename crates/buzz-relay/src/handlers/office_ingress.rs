//! Buzz Office event → Ortak `office_inbox` adapter boundary.
//!
//! Ortak Architecture v0 invariant 8 and §4.7: an accepted Office input
//! cannot fall between stores. The signed event and its durable inbox
//! handoff row commit in one PostgreSQL transaction, and the sender is
//! acknowledged only after that commit. This module is the Milestone 2
//! production seam for that rule:
//!
//! - [`persist_with_office_inbox`] composes the inherited event-insert
//!   transaction ([`buzz_db::event::EventInsertTxHook`]) with
//!   [`ortak_control::postgres::insert_accepted_event_on`], so both rows
//!   commit or neither does.
//! - Company scope is derived inside that statement from the authenticated
//!   community through `office_company_bindings`. The signed event carries no
//!   company identifier and nothing client-supplied is consulted.
//! - An unbound community fails closed with
//!   [`OfficeIngressError::UnknownCompanyBinding`] and the transaction rolls
//!   back, so no orphan event is stored.
//! - The signed event bytes and id are persisted exactly as Buzz verified
//!   them; the inbox row copies only the id, partition key, kind, author,
//!   and server-derived channel.
//!
//! Selection is gated by [`central_routing_applies`]: the
//! `ORTAK_CENTRAL_ROUTING_ENABLED` flag defaults to off, and the disabled path
//! in `ingest.rs` is the untouched inherited call. Routing workers, the
//! outbox dispatcher, and the reconciliation scan for stored events that
//! predate this seam are not part of this module.
//!
//! Remaining wiring point: `ingest_event_inner` in `handlers/ingest.rs`
//! reaches this seam only for the non-replaceable persistent-event branch.
//! Reaction, replaceable, and parameterized-replaceable kinds keep their
//! inherited persistence paths and are never routable Office input.

use chrono::{DateTime, Utc};
use nostr::Event;
use uuid::Uuid;

use buzz_core::kind::{KIND_GIFT_WRAP, KIND_STREAM_MESSAGE, KIND_STREAM_MESSAGE_V2};
use buzz_core::{CommunityId, StoredEvent};
use buzz_db::event::{EventInsertTxHook, ThreadMetadataParams};
use buzz_db::{Db, DbError};
use ortak_control::inbox::{InboxEvent, InboxInsertOutcome};
use ortak_control::postgres::insert_accepted_event_on;
use ortak_control::{ControlError, MessageId};

use super::ingest::IngestError;
use crate::config::Config;

/// Office message kinds that become routing input.
///
/// Channel messages (both stream-message kinds) and NIP-17 DM wraps are the
/// only client-submitted persistent kinds a company router may act on.
/// Reactions, edits, pins, channel metadata, and every other kind never
/// enter the inbox; the router would drop them regardless. A reconciler that
/// backfills inbox rows for stored events must use this same predicate.
pub fn is_office_routable_kind(kind: u32) -> bool {
    matches!(
        kind,
        KIND_STREAM_MESSAGE | KIND_STREAM_MESSAGE_V2 | KIND_GIFT_WRAP
    )
}

/// True when this event must take the atomic event + inbox path.
///
/// Both conditions are required: the deployment flag is on and the kind is
/// routable Office input. Everything else stays on the inherited path.
pub fn central_routing_applies(config: &Config, kind: u32) -> bool {
    config.ortak_central_routing_enabled && is_office_routable_kind(kind)
}

/// Projects a verified signed event onto the inbox handoff facts.
///
/// `channel_id` is the relay-derived channel (never the raw `h` tag) and
/// `event_created_at` is the exact `events.created_at` value bound by the
/// insert, so consumers join back to the signed row without a partition
/// scan. The event id and author key are copied byte-for-byte.
pub fn office_inbox_event(
    event: &Event,
    channel_id: Option<Uuid>,
    event_created_at: DateTime<Utc>,
) -> InboxEvent {
    InboxEvent {
        event_id: MessageId::from_bytes(*event.id.as_bytes()),
        event_created_at,
        event_kind: buzz_core::kind::event_kind_i32(event),
        author_pubkey: event.pubkey.to_bytes(),
        channel_id,
    }
}

/// Typed failure of the atomic Office-ingress path. In every variant the
/// transaction was rolled back: no event row and no inbox row exist.
#[derive(Debug, thiserror::Error)]
pub enum OfficeIngressError {
    /// Central routing is enabled but the authenticated community has no
    /// server-owned company binding. The event was not persisted.
    #[error(
        "central routing is enabled but community {community_id} has no office_company_bindings row"
    )]
    UnknownCompanyBinding {
        /// Authenticated community that failed to resolve.
        community_id: Uuid,
    },
    /// The inbox handoff statement failed for another control-plane reason.
    #[error("office inbox handoff failed: {0}")]
    Handoff(#[source] ControlError),
    /// The event insert itself failed (same failures as the inherited path).
    #[error(transparent)]
    Persistence(DbError),
}

impl OfficeIngressError {
    /// Maps the typed failure onto the ingest error taxonomy.
    ///
    /// `AuthEventRejected` keeps the inherited client-rejection text. Every
    /// other failure is a server fault: the WebSocket handler sanitizes
    /// `Internal` to `error: internal server error`, so the community id in
    /// the message reaches logs, not clients.
    pub fn into_ingest_error(self) -> IngestError {
        match self {
            Self::Persistence(DbError::AuthEventRejected) => {
                IngestError::Rejected("invalid: AUTH events cannot be stored".into())
            }
            Self::UnknownCompanyBinding { community_id } => IngestError::Internal(format!(
                "error: office company binding missing for community {community_id}"
            )),
            Self::Handoff(error) => {
                IngestError::Internal(format!("error: office inbox handoff failed: {error}"))
            }
            Self::Persistence(error) => {
                IngestError::Internal(format!("error: database error: {error}"))
            }
        }
    }
}

/// Result of the atomic Office-ingress path.
#[derive(Debug)]
pub struct OfficeIngressOutcome {
    /// The stored event, exactly as the inherited path returns it.
    pub stored_event: StoredEvent,
    /// `false` when the event id was already stored (sender replay).
    pub was_inserted: bool,
    /// Whether this transaction wrote the inbox row or found it present.
    pub inbox: InboxInsertOutcome,
}

/// Persists the verified signed event and its `office_inbox` handoff in one
/// transaction.
///
/// Atomicity: the inbox insert runs on the event-insert transaction's
/// connection before `COMMIT`; any error drops the transaction, so the
/// signed event, thread metadata, and inbox row are committed together or
/// not at all. Idempotency: a replayed event id skips the event insert
/// (`ON CONFLICT DO NOTHING`) and the inbox insert (`ON CONFLICT DO NOTHING`
/// on `(company_id, event_id)`), so a second inbox row is never created.
/// Fail closed: an unbound community writes nothing and returns
/// [`OfficeIngressError::UnknownCompanyBinding`].
pub async fn persist_with_office_inbox(
    db: &Db,
    community: CommunityId,
    event: &Event,
    channel_id: Option<Uuid>,
    thread_meta: Option<ThreadMetadataParams<'_>>,
) -> Result<OfficeIngressOutcome, OfficeIngressError> {
    let hook: EventInsertTxHook<'_, InboxInsertOutcome> = Box::new(move |mut tx, receipt| {
        Box::pin(async move {
            let inbox_event = office_inbox_event(event, channel_id, receipt.created_at);
            let outcome = insert_accepted_event_on(&mut tx, *community.as_uuid(), &inbox_event)
                .await
                .map_err(|error| DbError::TransactionHook(Box::new(error)))?;
            Ok((tx, outcome))
        })
    });

    match db
        .insert_event_with_thread_metadata_and_hook(community, event, channel_id, thread_meta, hook)
        .await
    {
        Ok((stored_event, was_inserted, inbox)) => Ok(OfficeIngressOutcome {
            stored_event,
            was_inserted,
            inbox,
        }),
        Err(DbError::TransactionHook(source)) => Err(match source.downcast::<ControlError>() {
            Ok(control) => match *control {
                ControlError::UnknownCompanyBinding { community_id } => {
                    OfficeIngressError::UnknownCompanyBinding { community_id }
                }
                other => OfficeIngressError::Handoff(other),
            },
            Err(other) => OfficeIngressError::Persistence(DbError::TransactionHook(other)),
        }),
        Err(other) => Err(OfficeIngressError::Persistence(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::kind::{KIND_REACTION, KIND_STREAM_MESSAGE_EDIT, KIND_TEXT_NOTE};

    #[test]
    fn only_office_message_kinds_are_routable() {
        assert!(is_office_routable_kind(KIND_STREAM_MESSAGE));
        assert!(is_office_routable_kind(KIND_STREAM_MESSAGE_V2));
        assert!(is_office_routable_kind(KIND_GIFT_WRAP));
        assert!(!is_office_routable_kind(KIND_REACTION));
        assert!(!is_office_routable_kind(KIND_STREAM_MESSAGE_EDIT));
        assert!(!is_office_routable_kind(KIND_TEXT_NOTE));
    }

    #[test]
    fn inbox_event_copies_exact_id_author_and_kind() {
        let keys = nostr::Keys::generate();
        let event = nostr::EventBuilder::new(nostr::Kind::Custom(KIND_STREAM_MESSAGE as u16), "hi")
            .sign_with_keys(&keys)
            .expect("sign");
        let channel_id = Some(Uuid::new_v4());
        let created_at = Utc::now();
        let inbox = office_inbox_event(&event, channel_id, created_at);
        assert_eq!(inbox.event_id.as_bytes(), event.id.as_bytes());
        assert_eq!(inbox.author_pubkey, event.pubkey.to_bytes());
        assert_eq!(inbox.event_kind, KIND_STREAM_MESSAGE as i32);
        assert_eq!(inbox.channel_id, channel_id);
        assert_eq!(inbox.event_created_at, created_at);
    }

    #[test]
    fn unknown_binding_maps_to_a_sanitized_internal_error() {
        let community_id = Uuid::new_v4();
        match (OfficeIngressError::UnknownCompanyBinding { community_id }).into_ingest_error() {
            IngestError::Internal(message) => {
                assert!(message.starts_with("error:"));
                assert!(message.contains(&community_id.to_string()));
            }
            other => panic!("expected an internal error, got {other:?}"),
        }
    }
}
