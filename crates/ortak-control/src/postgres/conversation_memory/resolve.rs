use std::collections::HashSet;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgRow, Row};
use uuid::Uuid;

use super::{ConversationObservation, ConversationReadRequest, MAX_CONVERSATION_ANCESTRY};
use crate::memory::conversation::{
    ConversationAudienceKind, ConversationAudienceV1, ConversationEventIdentity,
    ConversationMemoryDigest, ConversationProvenanceV1,
};
use crate::{MessageId, Result};

// Exact v1 source evidence preimage: this declaration's lexicographic field
// order, compact serde_json UTF-8, no insignificant whitespace. Hex is lowercase
// and created_at is UTC with six fractional digits. Tags preserve every original
// string and their array order; content preserves all whitespace and UTF-8.
// The hash covers the canonical server row, NOT legacy69's message:<id> marker.
// Changing these fields/encoding requires a new format, never silent rehashing.
#[derive(Serialize)]
struct SourceEvidence<'a> {
    author_pubkey: String,
    channel_id: Uuid,
    community_id: Uuid,
    company_id: Uuid,
    content: &'a str,
    event_created_at: String,
    event_id: String,
    format: &'static str,
    kind: i32,
    sig: String,
    tags: &'a [Vec<String>],
}

fn identity(bytes: &[u8], time: DateTime<Utc>) -> Option<ConversationEventIdentity> {
    let bytes: [u8; 32] = bytes.try_into().ok()?;
    ConversationEventIdentity::new(MessageId::from_bytes(bytes), time).ok()
}

fn pair(row: &PgRow, id: &str, time: &str) -> Result<Option<Option<ConversationEventIdentity>>> {
    let bytes: Option<Vec<u8>> = row.try_get(id)?;
    let stamp: Option<DateTime<Utc>> = row.try_get(time)?;
    Ok(match (bytes, stamp) {
        (None, None) => Some(None),
        (Some(bytes), Some(stamp)) => identity(&bytes, stamp).map(Some),
        _ => None,
    })
}

// D4 deliberately refuses ambiguous/unresolved e references. It does not apply
// legacy positional guessing or the ingest parser's last-marker-wins fallback.
// A valid direct reply has only reply; nested replies have root and reply.
// Explicit mention references do not establish ancestry. Other e forms refuse.
fn references(tags: &[Vec<String>]) -> Option<(Option<MessageId>, Option<MessageId>)> {
    let (mut root, mut parent) = (None, None);
    for tag in tags
        .iter()
        .filter(|tag| tag.first().is_some_and(|s| s == "e"))
    {
        let id = tag.get(1)?;
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let mut bytes = [0; 32];
        hex::decode_to_slice(id, &mut bytes).ok()?;
        let value = MessageId::from_bytes(bytes);
        let target = match tag.get(3)?.as_str() {
            "root" => &mut root,
            "reply" => &mut parent,
            "mention" => continue,
            _ => return None,
        };
        if target.replace(value).is_some() {
            return None;
        }
    }
    // A root-only claim has no canonical parent and cannot establish a thread.
    if root.is_some() && parent.is_none() {
        return None;
    }
    Some((root.or(parent), parent))
}

pub(super) fn observation(
    request: &ConversationReadRequest<'_>,
    rows: &[PgRow],
) -> Result<Option<ConversationObservation>> {
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    if rows.len() > MAX_CONVERSATION_ANCESTRY + 1 {
        return Ok(None);
    }
    let channel: Uuid = first.try_get("channel_id")?;
    let mut seen = HashSet::new();
    let mut nodes = Vec::with_capacity(rows.len());
    let mut source_tags = None;
    for (hop, row) in rows.iter().enumerate() {
        let bytes: Vec<u8> = row.try_get("id")?;
        let Some(event) = identity(&bytes, row.try_get("created_at")?) else {
            return Ok(None);
        };
        if row.try_get::<i32, _>("hop")? != hop as i32 || !seen.insert(event.event_id()) {
            return Ok(None);
        }
        let Some(tags) = row.try_get::<Option<serde_json::Value>, _>("tags")? else {
            return Ok(None);
        };
        let Ok(tags) = serde_json::from_value::<Vec<Vec<String>>>(tags) else {
            return Ok(None);
        };
        let Some((claimed_root, claimed_parent)) = references(&tags) else {
            return Ok(None);
        };
        let Some(parent) = pair(row, "parent_event_id", "parent_event_created_at")? else {
            return Ok(None);
        };
        let Some(root) = pair(row, "root_event_id", "root_event_created_at")? else {
            return Ok(None);
        };
        let present: bool = row.try_get("metadata_present")?;
        let depth: Option<i32> = row.try_get("depth")?;
        if present {
            if row.try_get::<Option<Uuid>, _>("metadata_channel")? != Some(channel) {
                return Ok(None);
            }
            match (&parent, &root, depth) {
                (None, None, Some(0)) if claimed_parent.is_none() => {}
                (None, Some(root), Some(0)) if root == &event && claimed_parent.is_none() => {}
                (Some(parent), Some(root), Some(depth))
                    if (1..=32).contains(&depth)
                        && claimed_parent == Some(parent.event_id())
                        && claimed_root == Some(root.event_id()) => {}
                _ => return Ok(None),
            }
        } else if parent.is_some() || root.is_some() || depth.is_some() || claimed_parent.is_some()
        {
            return Ok(None);
        }
        if hop == 0 {
            source_tags = Some(tags);
        }
        nodes.push((event, parent, root, depth.unwrap_or(0)));
    }
    let Some((root, last_parent, last_root, last_depth)) = nodes.last() else {
        return Ok(None);
    };
    if last_parent.is_some()
        || last_root.as_ref().is_some_and(|recorded| recorded != root)
        || *last_depth != 0
    {
        return Ok(None);
    }
    for (index, (_, parent, recorded_root, depth)) in nodes.iter().enumerate().take(nodes.len() - 1)
    {
        if parent.as_ref() != Some(&nodes[index + 1].0)
            || recorded_root.as_ref() != Some(root)
            || *depth as usize != nodes.len() - index - 1
        {
            return Ok(None);
        }
    }
    let Some(community) = request.scope.community_id() else {
        return Ok(None);
    };
    let audience = match request.audience_kind {
        ConversationAudienceKind::Channel => ConversationAudienceV1::channel(
            request.scope.company_id(),
            community,
            request.project_id,
            request.employee_id.clone(),
            channel,
        ),
        ConversationAudienceKind::Thread => ConversationAudienceV1::thread(
            request.scope.company_id(),
            community,
            request.project_id,
            request.employee_id.clone(),
            channel,
            root.clone(),
        ),
    };
    let Ok(audience) = audience else {
        return Ok(None);
    };
    let Some(tags) = source_tags else {
        return Ok(None);
    };
    let content: String = first.try_get("source_content")?;
    let author: Vec<u8> = first.try_get("source_author")?;
    let signature: Vec<u8> = first.try_get("source_signature")?;
    let evidence = SourceEvidence {
        author_pubkey: hex::encode(author),
        channel_id: channel,
        community_id: community,
        company_id: request.scope.company_id(),
        content: &content,
        event_created_at: nodes[0]
            .0
            .created_at()
            .to_rfc3339_opts(SecondsFormat::Micros, true),
        event_id: nodes[0].0.event_id().to_hex(),
        format: "ortak-reviewed-conversation-evidence/1",
        kind: first.try_get("source_kind")?,
        sig: hex::encode(signature),
        tags: &tags,
    };
    let hash =
        ConversationMemoryDigest::from_bytes(Sha256::digest(serde_json::to_vec(&evidence)?).into());
    let Ok(provenance) = ConversationProvenanceV1::new(audience, nodes[0].0.clone(), hash) else {
        return Ok(None);
    };
    Ok(Some(ConversationObservation {
        provenance,
        observed_at: first.try_get("observed_at")?,
        valid_before: first.try_get("valid_before")?,
    }))
}
