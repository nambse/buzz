//! Production channel [`MessageNormalizer`] over canonical Office rows.
//!
//! [`PgChannelNormalizer`] turns one claimed `office_inbox` row into the
//! typed [`Normalization`] the inbox routing service commits. Everything the
//! router treats as trusted is derived from server rows:
//!
//! - **Inbox facts are validated, never used.** The row's id, partition
//!   timestamp, kind, author, and channel must match the signed `events` row
//!   stored under the company's bound community; any disagreement is a
//!   typed [`ControlError::InboxFactMismatch`] and the claim is released for
//!   bounded retry.
//! - **Supported kinds** are the retained channel text kinds
//!   ([`KIND_STREAM_MESSAGE`] and [`KIND_STREAM_MESSAGE_V2`]), the same set
//!   the Office ingress accepts minus gift wraps. A gift wrap
//!   ([`KIND_GIFT_WRAP`]) is refused with `dm_normalization_pending` before
//!   its content column is ever selected; its outer signing key is recorded
//!   as the closed integration attribution `gift-wrap-transport:<hex>`, not
//!   as a verified human. Anything else is not Office input.
//! - **Channel state** must be live: the channel row exists, is neither
//!   archived nor deleted, and has the channel type the kind is defined for
//!   (`stream`). A DM-typed or otherwise unexpected channel is refused.
//! - **Origin** resolves the author key through every
//!   `employee_office_bindings` row of the company first (historical,
//!   retired, unverified, and disabled bindings included). A key that is not
//!   an employee is then checked against the relay's legacy automation
//!   markers (`bot` channel membership, `users.agent_type`,
//!   `users.agent_owner_pubkey`) and `users.deactivated_at`; automation is
//!   refused as `legacy_automation_origin` (attributed as
//!   `legacy-automation:<hex>`), a deactivated user as `origin_deactivated`,
//!   and only a live channel member or `relay_members` row is a known human.
//!   Every other key is refused as `unknown_origin`.
//! - **Source-channel access** is checked separately from identity, for
//!   employees and humans alike, with the relay's ingest rule: a live
//!   channel member may write anywhere; a non-member may write only when
//!   the canonical `channels.visibility` is `open`. A known author outside a
//!   private channel is refused as `origin_not_channel_member`. A missing
//!   channel row is an inbox fact mismatch, and any visibility value other
//!   than `open` is treated as private, so an unknown value never widens
//!   access.
//! - **Mentions** come from accepted `p` key tags resolved through the same
//!   binding table. Names are never remapped to keys. An oversized tag or
//!   mention set is refused, never truncated.
//! - **Replies** come from the relay-persisted `thread_metadata` parent,
//!   which must be a stored, non-deleted event of a supported kind in the
//!   same community and channel. A client `reply` marker without a
//!   persisted parent, a missing parent, or a cross-channel parent is
//!   refused.
//! - **Loop root** for an employee-authored event is the `runs.root_message_id`
//!   of the run whose `office_publish` outbox row froze this exact signed
//!   event. An employee event Ortak never published has no trustworthy
//!   root and is refused rather than started as a fresh human chain. A
//!   human message roots its own chain.
//! - **Conversation eligibility** is the set of employees whose active
//!   revision manifest names an Office key and signer reference that match a
//!   verified, currently valid binding owned by the same employee, where that
//!   key is a live member of the channel. The binding's `revision_id` is the
//!   revision that introduced the key, not necessarily the active one. The
//!   routing service intersects every routing path with it; a known employee
//!   outside it is a visible drop.
//! - System origin, structured dispatch targets, Work assignments, and
//!   chain counters are never taken from the message. They stay empty.
//!
//! The normalizer performs reads only. Routing carries the control plane's
//! Office authority witness from before these reads and validates it under
//! the coordinated mutation fence at commit. Runtime admission can use
//! `normalize_on` within its own short transaction holding the same fence.

mod postgres;
mod tags;

use ortak_control::inbox::{InboxEvent, InboxRow};
use ortak_control::office_identity::OfficePublicKey;
use ortak_control::ports::{
    MessageNormalizer, Normalization, NormalizationRefusal, NormalizedMessage,
};
use ortak_control::{CompanyScope, ControlError, MessageId, Result};
use ortak_domain::{
    ConversationContext, EmployeeId, MessageEnvelope, MessageKind, MessageOrigin, ReplyContext,
    RoutingReason,
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

pub use ortak_control::inbox::{
    is_supported_channel_kind, KIND_GIFT_WRAP, KIND_STREAM_MESSAGE, KIND_STREAM_MESSAGE_V2,
};
pub use tags::{MAX_MENTION_KEYS, MAX_TAGS_EXAMINED};

/// Closed origin attribution prefix for the outer signing key of a refused
/// gift wrap. The key is a transport artifact, not a verified member.
pub const GIFT_WRAP_TRANSPORT_ORIGIN_PREFIX: &str = "gift-wrap-transport";
/// Closed origin attribution prefix for a refused legacy automation key.
pub const LEGACY_AUTOMATION_ORIGIN_PREFIX: &str = "legacy-automation";

/// `channels.channel_type` in which a supported channel kind is defined.
pub fn expected_channel_type(kind: i32) -> Option<&'static str> {
    is_supported_channel_kind(kind).then_some("stream")
}

/// PostgreSQL-backed channel message normalizer.
#[derive(Clone, Debug)]
pub struct PgChannelNormalizer {
    pool: PgPool,
}

impl PgChannelNormalizer {
    /// Wraps the control-plane pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn mismatch(message_id: MessageId, field: &'static str) -> ControlError {
    ControlError::InboxFactMismatch {
        message_id: message_id.to_hex(),
        field,
    }
}

fn refused(reason: RoutingReason, origin: MessageOrigin) -> Normalization {
    Normalization::Refused(NormalizationRefusal { reason, origin })
}

fn human(key: &[u8; 32]) -> MessageOrigin {
    MessageOrigin::Human(hex::encode(key))
}

fn attributed(prefix: &str, key: &[u8; 32]) -> MessageOrigin {
    MessageOrigin::Integration(format!("{prefix}:{}", hex::encode(key)))
}

impl MessageNormalizer for PgChannelNormalizer {
    async fn normalize(&self, scope: &CompanyScope, inbox: &InboxRow) -> Result<Normalization> {
        let mut connection = self.pool.acquire().await?;
        Self::normalize_on(&mut connection, scope, &inbox.event).await
    }
}

impl PgChannelNormalizer {
    /// Normalizes canonical Office rows on the caller's connection.
    /// Runtime admission uses a READ COMMITTED transaction holding the shared
    /// Office authority fence; ordinary routing carries its snapshot witness.
    pub async fn normalize_on(
        connection: &mut PgConnection,
        scope: &CompanyScope,
        inbox: &InboxEvent,
    ) -> Result<Normalization> {
        let message_id = inbox.event_id;
        let company_id = scope.company_id();

        // 1. The canonical signed row, without content, must agree with the
        //    inbox copy on every fact the inbox carries.
        let facts = postgres::canonical_facts(
            &mut *connection,
            company_id,
            message_id,
            inbox.event_created_at,
        )
        .await?
        .ok_or_else(|| mismatch(message_id, "event"))?;
        if let Some(community_id) = scope.community_id() {
            if community_id != facts.community_id {
                return Err(mismatch(message_id, "community"));
            }
        }
        if facts.kind != inbox.event_kind {
            return Err(mismatch(message_id, "kind"));
        }
        if facts.author != inbox.author_pubkey {
            return Err(mismatch(message_id, "author"));
        }
        if facts.channel_id != inbox.channel_id {
            return Err(mismatch(message_id, "channel"));
        }

        // 2. Kind gate. The gift-wrap branch ends here: its content column
        //    was never selected and its outer key is a transport attribution.
        if facts.kind == KIND_GIFT_WRAP {
            return Ok(refused(
                RoutingReason::DmNormalizationPending,
                attributed(GIFT_WRAP_TRANSPORT_ORIGIN_PREFIX, &facts.author),
            ));
        }
        let Some(expected_type) = expected_channel_type(facts.kind) else {
            return Ok(Normalization::NotOfficeInput);
        };
        let Some(channel_id) = facts.channel_id else {
            return Err(mismatch(message_id, "channel_scope"));
        };

        // 3. Origin and source-channel access. Identity comes from employee
        //    bindings first, then from relay facts; access to this channel
        //    is the relay's ingest rule (live member, or any key in an open
        //    channel) and applies to employees and humans alike.
        let visibility =
            postgres::channel_visibility(&mut *connection, facts.community_id, channel_id)
                .await?
                .ok_or_else(|| mismatch(message_id, "channel_row"))?;
        let author_employee =
            postgres::employee_for_key(&mut *connection, company_id, &facts.author).await?;
        let author = postgres::author_facts(
            &mut *connection,
            facts.community_id,
            channel_id,
            &facts.author,
        )
        .await?;
        let origin = match &author_employee {
            Some(employee_id) => MessageOrigin::Employee(employee_id.clone()),
            None => {
                if author.legacy_automation {
                    return Ok(refused(
                        RoutingReason::LegacyAutomationOrigin,
                        attributed(LEGACY_AUTOMATION_ORIGIN_PREFIX, &facts.author),
                    ));
                }
                if author.deactivated {
                    return Ok(refused(
                        RoutingReason::OriginDeactivated,
                        human(&facts.author),
                    ));
                }
                if !(author.channel_member || author.relay_member) {
                    return Ok(refused(RoutingReason::UnknownOrigin, human(&facts.author)));
                }
                human(&facts.author)
            }
        };
        if !author.may_write(visibility) {
            return Ok(refused(RoutingReason::OriginNotChannelMember, origin));
        }
        if facts.deleted {
            return Ok(refused(RoutingReason::NonRoutableMessage, origin));
        }

        // 4. Content, tags, channel state, and persisted thread parent.
        let row = postgres::channel_message(
            &mut *connection,
            facts.community_id,
            message_id,
            inbox.event_created_at,
        )
        .await?
        .ok_or_else(|| mismatch(message_id, "event"))?;
        let Some(channel_type) = row.channel_type.as_deref() else {
            return Err(mismatch(message_id, "channel_row"));
        };
        if row.channel_deleted || row.channel_archived || channel_type != expected_type {
            return Ok(refused(RoutingReason::ChannelNotRoutable, origin));
        }

        // 5. Loop root: persisted publish provenance for employees, self for humans.
        let root_message_id = match &author_employee {
            None => message_id,
            Some(employee_id) => {
                match postgres::publish_provenance(&mut *connection, company_id, message_id).await?
                {
                    Some(provenance) if &provenance.employee_id != employee_id => {
                        return Err(ControlError::InvalidData(format!(
                            "office_publish provenance for {message_id} names employee {} but the signing key belongs to {employee_id}",
                            provenance.employee_id
                        )));
                    }
                    Some(provenance) => match provenance.root_message_id {
                        Some(root) => root,
                        None => return Ok(refused(RoutingReason::UnresolvedProvenance, origin)),
                    },
                    None => return Ok(refused(RoutingReason::UnresolvedProvenance, origin)),
                }
            }
        };

        // 6. Bounded tag scan; oversized sets are refused, never truncated.
        let tag_facts = match tags::scan_tags(&row.tags) {
            Ok(facts) => facts,
            Err(_) => return Ok(refused(RoutingReason::TagBoundsExceeded, origin)),
        };

        // 7. Reply parent from thread_metadata only.
        let reply_to = match row.parent {
            None if tag_facts.claims_reply => {
                return Ok(refused(RoutingReason::UnresolvedProvenance, origin));
            }
            None => None,
            Some((parent_id, parent_created_at)) => {
                let parent = postgres::parent_facts(
                    &mut *connection,
                    facts.community_id,
                    parent_id,
                    parent_created_at,
                )
                .await?;
                match parent {
                    Some(parent)
                        if parent.channel_id == Some(channel_id)
                            && !parent.deleted
                            && is_supported_channel_kind(parent.kind) =>
                    {
                        let parent_origin = match postgres::employee_for_key(
                            &mut *connection,
                            company_id,
                            &parent.author,
                        )
                        .await?
                        {
                            Some(employee_id) => MessageOrigin::Employee(employee_id),
                            None => human(&parent.author),
                        };
                        Some(ReplyContext {
                            message_id: parent_id.to_hex(),
                            origin: parent_origin,
                        })
                    }
                    _ => return Ok(refused(RoutingReason::UnresolvedProvenance, origin)),
                }
            }
        };

        // 8. Structured mentions from accepted key tags, and the employees
        //    that are live, verified members of this channel right now.
        let structured_mentions =
            resolve_mentions(&mut *connection, company_id, &tag_facts.mention_keys).await?;
        let eligible_employee_ids = postgres::channel_eligible_employees(
            &mut *connection,
            company_id,
            facts.community_id,
            channel_id,
        )
        .await?;

        let mut envelope = MessageEnvelope::root(
            message_id.to_hex(),
            MessageKind::Text,
            origin,
            ConversationContext::Channel {
                channel_id: channel_id.to_string(),
            },
            tags::strip_control_characters(&row.content),
        );
        envelope.structured_mentions = structured_mentions;
        envelope.reply_to = reply_to;

        Ok(Normalization::Message(Box::new(NormalizedMessage {
            envelope,
            root_message_id,
            eligible_employee_ids,
        })))
    }
}

/// Maps mention keys to employees, preserving first-appearance order and
/// dropping keys that belong to no employee (humans mention humans freely).
async fn resolve_mentions(
    connection: &mut PgConnection,
    company_id: Uuid,
    keys: &[OfficePublicKey],
) -> Result<Vec<EmployeeId>> {
    let resolved = postgres::employees_for_keys(connection, company_id, keys).await?;
    let mut mentions = Vec::with_capacity(resolved.len());
    for key in keys {
        if let Some((_, employee_id)) = resolved
            .iter()
            .find(|(resolved_key, _)| resolved_key == key)
        {
            if !mentions.contains(employee_id) {
                mentions.push(employee_id.clone());
            }
        }
    }
    Ok(mentions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_retained_channel_text_kinds_are_supported_in_stream_channels() {
        assert_eq!(expected_channel_type(9), Some("stream"));
        assert_eq!(expected_channel_type(40002), Some("stream"));
        assert_eq!(expected_channel_type(KIND_GIFT_WRAP), None);
        assert_eq!(expected_channel_type(1), None);
        assert_eq!(expected_channel_type(45001), None);
    }

    #[test]
    fn refused_transport_and_automation_keys_are_not_humans() {
        let key = [0xabu8; 32];
        let MessageOrigin::Integration(label) = attributed(GIFT_WRAP_TRANSPORT_ORIGIN_PREFIX, &key)
        else {
            panic!("transport attribution must be a closed integration origin");
        };
        assert_eq!(
            label,
            format!("{GIFT_WRAP_TRANSPORT_ORIGIN_PREFIX}:{}", "ab".repeat(32))
        );
        assert!(!attributed(LEGACY_AUTOMATION_ORIGIN_PREFIX, &key).allows_semantic_routing());
    }
}
