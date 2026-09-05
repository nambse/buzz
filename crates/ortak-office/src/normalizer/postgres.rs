//! Canonical-row reads behind [`PgChannelNormalizer`](super::PgChannelNormalizer).
//!
//! Every statement is scoped by the company and by the community that
//! `office_company_bindings` binds to it; nothing here accepts a community or
//! company identifier from the message. The reads are split so that an
//! encrypted wrap (kind 1059) is validated by [`canonical_facts`] alone,
//! which never selects `events.content`.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use ortak_control::office_identity::OfficePublicKey;
use ortak_control::{ControlError, MessageId, Result};
use ortak_domain::EmployeeId;
use sqlx::{PgExecutor, Row};
use uuid::Uuid;

/// The signed event as the relay stored it, minus its content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalFacts {
    /// Community the company is bound to and the event belongs to.
    pub community_id: Uuid,
    /// `events.kind`.
    pub kind: i32,
    /// `events.pubkey`.
    pub author: [u8; 32],
    /// Relay-derived channel, if channel-scoped.
    pub channel_id: Option<Uuid>,
    /// Set when the event was deleted after acceptance.
    pub deleted: bool,
}

/// Reads the canonical event facts for one inbox row without its content.
///
/// Returns `None` when the company has no community binding or the event
/// is not stored under that community at the inbox's partition key.
pub async fn canonical_facts<'e>(
    executor: impl PgExecutor<'e>,
    company_id: Uuid,
    message_id: MessageId,
    event_created_at: DateTime<Utc>,
) -> Result<Option<CanonicalFacts>> {
    let row = sqlx::query(
        "SELECT ocb.community_id, ev.kind, ev.pubkey, ev.channel_id,
                ev.deleted_at IS NOT NULL AS deleted
           FROM office_company_bindings ocb
           JOIN events ev
             ON ev.community_id = ocb.community_id
            AND ev.created_at = $2
            AND ev.id = $3
          WHERE ocb.company_id = $1",
    )
    .bind(company_id)
    .bind(event_created_at)
    .bind(message_id.as_bytes().as_slice())
    .fetch_optional(executor)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let author: Vec<u8> = row.try_get("pubkey")?;
    Ok(Some(CanonicalFacts {
        community_id: row.try_get("community_id")?,
        kind: row.try_get("kind")?,
        author: <[u8; 32]>::try_from(author.as_slice())
            .map_err(|_| ControlError::InvalidData("events.pubkey must be 32 bytes".to_owned()))?,
        channel_id: row.try_get("channel_id")?,
        deleted: row.try_get("deleted")?,
    }))
}

/// Channel message body, tags, thread provenance, and channel state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelMessageRow {
    /// `events.content` (plaintext for the supported channel kinds).
    pub content: String,
    /// Decoded `events.tags`.
    pub tags: Vec<Vec<String>>,
    /// Server-persisted reply parent from `thread_metadata`, if any.
    pub parent: Option<(MessageId, DateTime<Utc>)>,
    /// `channels.channel_type` when the channel row exists in the community.
    pub channel_type: Option<String>,
    /// True when the channel row is archived.
    pub channel_archived: bool,
    /// True when the channel row is soft-deleted.
    pub channel_deleted: bool,
}

/// Reads the content, tags, persisted thread parent, and channel state of a
/// supported channel message.
pub async fn channel_message<'e>(
    executor: impl PgExecutor<'e>,
    community_id: Uuid,
    message_id: MessageId,
    event_created_at: DateTime<Utc>,
) -> Result<Option<ChannelMessageRow>> {
    let row = sqlx::query(
        "SELECT ev.content, ev.tags,
                tm.parent_event_id, tm.parent_event_created_at,
                ch.channel_type::text AS channel_type,
                ch.archived_at IS NOT NULL AS channel_archived,
                ch.deleted_at IS NOT NULL AS channel_deleted
           FROM events ev
           LEFT JOIN thread_metadata tm
             ON tm.community_id = ev.community_id
            AND tm.event_created_at = ev.created_at
            AND tm.event_id = ev.id
           LEFT JOIN channels ch
             ON ch.community_id = ev.community_id
            AND ch.id = ev.channel_id
          WHERE ev.community_id = $1 AND ev.created_at = $2 AND ev.id = $3",
    )
    .bind(community_id)
    .bind(event_created_at)
    .bind(message_id.as_bytes().as_slice())
    .fetch_optional(executor)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let tags: serde_json::Value = row.try_get("tags")?;
    let tags: Vec<Vec<String>> = serde_json::from_value(tags).map_err(|_| {
        ControlError::InvalidData("events.tags is not a string tag array".to_owned())
    })?;
    let parent_id: Option<Vec<u8>> = row.try_get("parent_event_id")?;
    let parent_created_at: Option<DateTime<Utc>> = row.try_get("parent_event_created_at")?;
    let parent = match (parent_id, parent_created_at) {
        (Some(id), Some(created_at)) => Some((MessageId::try_from_slice(&id)?, created_at)),
        (None, None) => None,
        _ => {
            return Err(ControlError::InvalidData(
                "thread_metadata parent id and timestamp disagree".to_owned(),
            ))
        }
    };
    Ok(Some(ChannelMessageRow {
        content: row.try_get("content")?,
        tags,
        parent,
        channel_type: row.try_get("channel_type")?,
        channel_archived: row.try_get("channel_archived")?,
        channel_deleted: row.try_get("channel_deleted")?,
    }))
}

/// Author key, kind, channel, and deletion state of a persisted parent event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParentFacts {
    /// `events.pubkey` of the parent.
    pub author: [u8; 32],
    /// `events.kind` of the parent.
    pub kind: i32,
    /// Relay-derived channel of the parent.
    pub channel_id: Option<Uuid>,
    /// Set when the parent was deleted after acceptance.
    pub deleted: bool,
}

/// Reads the parent event named by `thread_metadata`, within the same community.
pub async fn parent_facts<'e>(
    executor: impl PgExecutor<'e>,
    community_id: Uuid,
    parent_id: MessageId,
    parent_created_at: DateTime<Utc>,
) -> Result<Option<ParentFacts>> {
    let row = sqlx::query(
        "SELECT pubkey, kind, channel_id, deleted_at IS NOT NULL AS deleted FROM events
          WHERE community_id = $1 AND created_at = $2 AND id = $3",
    )
    .bind(community_id)
    .bind(parent_created_at)
    .bind(parent_id.as_bytes().as_slice())
    .fetch_optional(executor)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let author: Vec<u8> = row.try_get("pubkey")?;
    Ok(Some(ParentFacts {
        author: <[u8; 32]>::try_from(author.as_slice())
            .map_err(|_| ControlError::InvalidData("events.pubkey must be 32 bytes".to_owned()))?,
        kind: row.try_get("kind")?,
        channel_id: row.try_get("channel_id")?,
        deleted: row.try_get("deleted")?,
    }))
}

/// Resolves an Office key to the employee that has ever owned it.
///
/// Every `employee_office_bindings` row counts, whatever its validity
/// window or verification state: a key that once belonged to an employee
/// can never be treated as a human's.
pub async fn employee_for_key<'e>(
    executor: impl PgExecutor<'e>,
    company_id: Uuid,
    key: &[u8; 32],
) -> Result<Option<EmployeeId>> {
    let row = sqlx::query(
        "SELECT employee_id FROM employee_office_bindings
          WHERE company_id = $1 AND public_key = $2",
    )
    .bind(company_id)
    .bind(key.as_slice())
    .fetch_optional(executor)
    .await?;
    row.map(|row| {
        let id: String = row.try_get("employee_id")?;
        Ok(EmployeeId::parse(id)?)
    })
    .transpose()
}

/// Resolves several Office keys to employees in one bounded statement,
/// returning `(key, employee)` pairs for the keys that resolved.
pub async fn employees_for_keys<'e>(
    executor: impl PgExecutor<'e>,
    company_id: Uuid,
    keys: &[OfficePublicKey],
) -> Result<Vec<(OfficePublicKey, EmployeeId)>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let raw = keys
        .iter()
        .map(|key| key.as_bytes().to_vec())
        .collect::<Vec<_>>();
    let rows = sqlx::query(
        "SELECT public_key, employee_id FROM employee_office_bindings
          WHERE company_id = $1 AND public_key = ANY($2)",
    )
    .bind(company_id)
    .bind(raw)
    .fetch_all(executor)
    .await?;
    rows.iter()
        .map(|row| {
            let key: Vec<u8> = row.try_get("public_key")?;
            let key = <[u8; 32]>::try_from(key.as_slice()).map_err(|_| {
                ControlError::InvalidData("employee_office_bindings.public_key".to_owned())
            })?;
            let id: String = row.try_get("employee_id")?;
            Ok((
                OfficePublicKey::parse_hex(&hex::encode(key))?,
                EmployeeId::parse(id)?,
            ))
        })
        .collect()
}

/// Whether the channel admits non-members, as the relay's ingest gate
/// reads `channels.visibility`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelVisibility {
    /// `open`: the relay accepts writes from any relay member.
    Open,
    /// `private`: the relay accepts writes only from live channel members.
    Private,
}

/// Reads the canonical visibility of `channel_id` in the community.
///
/// Returns `None` when the channel row does not exist. Any visibility
/// value other than the literal `open` is reported as [`ChannelVisibility::Private`],
/// mirroring the relay's `visibility == "open"` test: an unknown value
/// never widens access.
pub async fn channel_visibility<'e>(
    executor: impl PgExecutor<'e>,
    community_id: Uuid,
    channel_id: Uuid,
) -> Result<Option<ChannelVisibility>> {
    let row = sqlx::query(
        "SELECT visibility::text AS visibility FROM channels
          WHERE community_id = $1 AND id = $2",
    )
    .bind(community_id)
    .bind(channel_id)
    .fetch_optional(executor)
    .await?;
    row.map(|row| {
        let visibility: String = row.try_get("visibility")?;
        Ok(if visibility == "open" {
            ChannelVisibility::Open
        } else {
            ChannelVisibility::Private
        })
    })
    .transpose()
}

/// What the relay knows about an authoring key in the source channel.
///
/// Identity and access are reported separately: `relay_member` and
/// `channel_member` say whether the key is known to the community at all,
/// while only `channel_member` (together with the channel's visibility)
/// decides whether the key may write in this channel.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuthorFacts {
    /// Live (not removed) member of the source channel.
    pub channel_member: bool,
    /// The key has a `relay_members` row in the community.
    pub relay_member: bool,
    /// The key holds (or held) a `bot` channel membership anywhere in the
    /// community, or its `users` row carries a legacy agent registration
    /// (`agent_type` or `agent_owner_pubkey`).
    pub legacy_automation: bool,
    /// The `users` row is deactivated.
    pub deactivated: bool,
}

impl AuthorFacts {
    /// True when the relay's ingest gate would accept a channel write from
    /// this key: a live channel member, or any key in an open channel.
    pub fn may_write(&self, visibility: ChannelVisibility) -> bool {
        self.channel_member || visibility == ChannelVisibility::Open
    }
}

/// Reads channel membership, relay membership, legacy-automation, and
/// deactivation facts for an authoring key.
pub async fn author_facts<'e>(
    executor: impl PgExecutor<'e>,
    community_id: Uuid,
    channel_id: Uuid,
    key: &[u8; 32],
) -> Result<AuthorFacts> {
    let row = sqlx::query(
        "SELECT EXISTS (
                    SELECT 1 FROM channel_members cm
                     WHERE cm.community_id = $1 AND cm.channel_id = $2
                       AND cm.pubkey = $3 AND cm.removed_at IS NULL
                ) AS channel_member,
                EXISTS (
                    SELECT 1 FROM relay_members rm
                     WHERE rm.community_id = $1 AND rm.pubkey = $4
                ) AS relay_member,
                (EXISTS (
                    SELECT 1 FROM channel_members cm
                     WHERE cm.community_id = $1 AND cm.pubkey = $3 AND cm.role = 'bot'
                )
                OR COALESCE(u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL, false)
                ) AS legacy_automation,
                COALESCE(u.deactivated_at IS NOT NULL, false) AS deactivated
           FROM (SELECT 1) AS one
           LEFT JOIN users u ON u.community_id = $1 AND u.pubkey = $3",
    )
    .bind(community_id)
    .bind(channel_id)
    .bind(key.as_slice())
    .bind(hex::encode(key))
    .fetch_one(executor)
    .await?;
    Ok(AuthorFacts {
        channel_member: row.try_get("channel_member")?,
        relay_member: row.try_get("relay_member")?,
        legacy_automation: row.try_get("legacy_automation")?,
        deactivated: row.try_get("deactivated")?,
    })
}

/// Employees that may be woken in `channel_id` right now.
///
/// The active revision's manifest names the Office public key and signer
/// reference the employee currently publishes with. The binding for that
/// key must be owned by the same employee, name the same signer reference,
/// be verified, and be inside its validity window, and the key must be a
/// live (not removed) channel member. These are the same checks
/// `OfficeDeliveryRepository` applies before signing. The binding's
/// `revision_id` is the revision that *introduced* the key and is not
/// compared with the active revision: provisioning reuses a key across
/// revisions without rewriting the binding.
///
/// Lifecycle status is deliberately not filtered here so the router can
/// still explain an inactive target as `employee_inactive` rather than as a
/// membership problem.
pub async fn channel_eligible_employees<'e>(
    executor: impl PgExecutor<'e>,
    company_id: Uuid,
    community_id: Uuid,
    channel_id: Uuid,
) -> Result<BTreeSet<EmployeeId>> {
    let rows = sqlx::query(
        "SELECT DISTINCT e.id AS employee_id
           FROM employees e
           JOIN employee_revisions rev
             ON rev.company_id = e.company_id
            AND rev.employee_id = e.id
            AND rev.id = e.active_revision_id
           JOIN employee_office_bindings b
             ON b.company_id = e.company_id
            AND b.employee_id = e.id
            AND encode(b.public_key, 'hex') = lower(rev.manifest #>> '{office,public_key}')
            AND b.signer_ref = rev.manifest #>> '{office,signer_ref}'
           JOIN channel_members cm
             ON cm.community_id = $2
            AND cm.channel_id = $3
            AND cm.pubkey = b.public_key
            AND cm.removed_at IS NULL
          WHERE e.company_id = $1
            AND e.active_revision_id IS NOT NULL
            AND b.verified_at IS NOT NULL
            AND b.valid_from <= now()
            AND (b.valid_until IS NULL OR b.valid_until > now())",
    )
    .bind(company_id)
    .bind(community_id)
    .bind(channel_id)
    .fetch_all(executor)
    .await?;
    rows.iter()
        .map(|row| {
            let id: String = row.try_get("employee_id")?;
            Ok(EmployeeId::parse(id)?)
        })
        .collect()
}

/// Persisted delivery provenance of an Ortak-published employee event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishProvenance {
    /// Employee whose run produced the event.
    pub employee_id: EmployeeId,
    /// Root of the delivery chain the run belonged to, if the run had one.
    pub root_message_id: Option<MessageId>,
}

/// Looks up the `office_publish` outbox row frozen with this exact signed
/// event id and the run it belongs to. Absent means Ortak never published
/// the event.
pub async fn publish_provenance<'e>(
    executor: impl PgExecutor<'e>,
    company_id: Uuid,
    message_id: MessageId,
) -> Result<Option<PublishProvenance>> {
    let row = sqlx::query(
        "SELECT r.employee_id, r.root_message_id
           FROM outbox o
           JOIN runs r ON r.company_id = o.company_id AND r.id = o.run_id
          WHERE o.company_id = $1 AND o.kind = 'office_publish' AND o.signed_event_id = $2",
    )
    .bind(company_id)
    .bind(message_id.as_bytes().as_slice())
    .fetch_optional(executor)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let employee_id: String = row.try_get("employee_id")?;
    let root: Option<Vec<u8>> = row.try_get("root_message_id")?;
    Ok(Some(PublishProvenance {
        employee_id: EmployeeId::parse(employee_id)?,
        root_message_id: root.as_deref().map(MessageId::try_from_slice).transpose()?,
    }))
}
