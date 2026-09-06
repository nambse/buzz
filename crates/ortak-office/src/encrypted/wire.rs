use chrono::{DateTime, Datelike, TimeZone, Utc};
use nostr::{Event, EventId, JsonUtil, Kind, PublicKey, Tag, Timestamp};
use serde::{Deserialize, Deserializer, de};
use zeroize::Zeroizing;

use super::*;

/// A tag never retains more than four bounded values, and the list never
/// retains more than sixteen tags. Total serialized JSON is bounded separately.
struct BoundedTags(Vec<Vec<String>>);
struct BoundedTag(Vec<String>);
struct TagValue(String);

impl<'de> Deserialize<'de> for TagValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.len() > MAX_TAG_VALUE_BYTES || value.chars().any(char::is_control) {
            return Err(de::Error::custom("invalid DM tag value"));
        }
        Ok(Self(value))
    }
}

impl<'de> Deserialize<'de> for BoundedTag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = BoundedTag;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a bounded DM tag")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<TagValue>()? {
                    if values.len() == 4 {
                        return Err(de::Error::custom("too many DM tag values"));
                    }
                    values.push(value.0);
                }
                Ok(BoundedTag(values))
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}

impl<'de> Deserialize<'de> for BoundedTags {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = BoundedTags;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("bounded DM tags")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut tags = Vec::new();
                let mut bytes = 0;
                while let Some(tag) = seq.next_element::<BoundedTag>()? {
                    bytes += tag.0.iter().map(String::len).sum::<usize>();
                    if tags.len() == MAX_TAGS || bytes > MAX_TAG_BYTES {
                        return Err(de::Error::custom("too many DM tags"));
                    }
                    tags.push(tag.0);
                }
                Ok(BoundedTags(tags))
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}

struct SecretText(Zeroizing<String>);
impl<'de> Deserialize<'de> for SecretText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|value| Self(Zeroizing::new(value)))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedWire {
    id: String,
    pubkey: String,
    created_at: u64,
    kind: u16,
    tags: BoundedTags,
    content: String,
    sig: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RumorWire {
    id: String,
    pubkey: String,
    created_at: u64,
    kind: u16,
    tags: BoundedTags,
    content: SecretText,
}

pub(super) fn partition_seconds(at: DateTime<Utc>) -> Result<u64, DecodeError> {
    if !(1970..=9999).contains(&at.year()) || at.timestamp_subsec_nanos() != 0 {
        return Err(DecodeError::Timestamp);
    }
    u64::try_from(at.timestamp()).map_err(|_| DecodeError::Timestamp)
}

pub(super) fn timestamp(seconds: u64) -> Result<DateTime<Utc>, DecodeError> {
    let seconds = i64::try_from(seconds).map_err(|_| DecodeError::Timestamp)?;
    let at = Utc
        .timestamp_opt(seconds, 0)
        .single()
        .ok_or(DecodeError::Timestamp)?;
    partition_seconds(at)?;
    Ok(at)
}

fn hex(value: &str, bytes: usize) -> Result<(), DecodeError> {
    if value.len() != bytes * 2
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(DecodeError::Encoding);
    }
    Ok(())
}

fn event_id(value: &str) -> Result<EventId, DecodeError> {
    hex(value, 32)?;
    EventId::from_hex(value).map_err(|_| DecodeError::Encoding)
}

fn public_key(value: &str) -> Result<PublicKey, DecodeError> {
    hex(value, 32)?;
    PublicKey::from_hex(value).map_err(|_| DecodeError::Encoding)
}

fn bounded_object(bytes: &[u8], maximum: usize) -> Result<(), DecodeError> {
    if bytes.len() > maximum {
        return Err(DecodeError::Bounds);
    }
    // Serde's derived struct visitor also accepts positional arrays. Require
    // the object token explicitly; only the four JSON whitespace bytes precede
    // it. The strict deserializer still owns the rest of the JSON grammar.
    if bytes
        .iter()
        .copied()
        .find(|byte| !matches!(*byte, b' ' | b'\t' | b'\r' | b'\n'))
        != Some(b'{')
    {
        return Err(DecodeError::Encoding);
    }
    Ok(())
}

pub(super) fn signed(bytes: &[u8], maximum: usize, kind: u16) -> Result<Event, DecodeError> {
    bounded_object(bytes, maximum)?;
    let wire: SignedWire = serde_json::from_slice(bytes).map_err(|_| DecodeError::Encoding)?;
    if wire.kind != kind {
        return Err(DecodeError::Kind);
    }
    event_id(&wire.id)?;
    public_key(&wire.pubkey)?;
    timestamp(wire.created_at)?;
    hex(&wire.sig, 64)?;
    // Ciphertext bounds before base64 decoding in the next stage. These fields
    // are parsed strictly above; the library parser below only constructs its
    // verified crypto type from the same already-bounded bytes.
    let maximum_ciphertext = if kind == 1059 { 48 * 1024 } else { 24 * 1024 };
    if wire.content.len() > maximum_ciphertext {
        return Err(DecodeError::Bounds);
    }
    if kind == 13 && !wire.tags.0.is_empty() {
        return Err(DecodeError::Tags);
    }
    let event = Event::from_json(bytes).map_err(|_| DecodeError::Encoding)?;
    event.verify().map_err(|_| DecodeError::Signature)?;
    Ok(event)
}

pub(super) fn outer_recipient(event: &Event, expected: PublicKey) -> Result<(), DecodeError> {
    let tags = event.tags.as_slice();
    if tags.len() != 1 {
        return Err(DecodeError::Tags);
    }
    let tag = tags[0].as_slice();
    if tag.len() != 2 || tag[0] != "p" {
        return Err(DecodeError::Tags);
    }
    if public_key(&tag[1])? != expected {
        return Err(DecodeError::Recipient);
    }
    Ok(())
}

pub(super) struct DecodedRumor {
    pub id: EventId,
    pub created_at: DateTime<Utc>,
    pub reply_to: Option<EventId>,
    pub text: Zeroizing<String>,
}

pub(super) fn rumor(
    bytes: &[u8],
    expected: &ExpectedEnvelope,
) -> Result<DecodedRumor, DecodeError> {
    bounded_object(bytes, MAX_RUMOR_BYTES)?;
    let wire: RumorWire = serde_json::from_slice(bytes).map_err(|_| DecodeError::Encoding)?;
    if wire.kind != 14 {
        return Err(DecodeError::Kind);
    }
    let id = event_id(&wire.id)?;
    let sender = public_key(&wire.pubkey)?;
    if sender != expected.human {
        return Err(DecodeError::Sender);
    }
    let created_at = timestamp(wire.created_at)?;
    let text = &wire.content.0;
    if text.trim().is_empty()
        || text.len() > MAX_TEXT_BYTES
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(DecodeError::Text);
    }
    let mut recipient = false;
    let mut reply_to = None;
    let mut tags = Vec::new();
    for tag in wire.tags.0 {
        match tag.first().map(String::as_str) {
            Some("p") if tag.len() == 2 && !recipient => {
                if public_key(&tag[1])? != expected.recipient {
                    return Err(DecodeError::Recipient);
                }
                recipient = true;
            }
            Some("e")
                if reply_to.is_none()
                    && (tag.len() == 2
                        || (tag.len() == 4 && tag[2].is_empty() && tag[3] == "reply")) =>
            {
                reply_to = Some(event_id(&tag[1])?);
            }
            _ => return Err(DecodeError::Tags),
        }
        tags.push(Tag::parse(tag).map_err(|_| DecodeError::Tags)?);
    }
    if !recipient {
        return Err(DecodeError::Recipient);
    }
    // EventId::new is the pinned Nostr canonical [0,pubkey,time,kind,tags,content]
    // hashing implementation. An absent ID cannot reach this point.
    let tags = tags.into_iter().collect();
    let computed = EventId::new(
        &sender,
        &Timestamp::from(wire.created_at),
        &Kind::PrivateDirectMessage,
        &tags,
        text.as_str(),
    );
    if computed != id {
        return Err(DecodeError::RumorId);
    }
    Ok(DecodedRumor {
        id,
        created_at,
        reply_to,
        text: wire.content.0,
    })
}
