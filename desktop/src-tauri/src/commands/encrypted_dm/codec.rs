//! Narrow native NIP-59 codec. Pinned Nostr implements hashing/signatures/NIP44;
//! these checks add the selected two-person grammar and allocation ceilings.
//! No plaintext type implements Debug or durable serialization.

use base64::Engine;
use nostr::{
    nips::nip44, Event, EventBuilder, EventId, JsonUtil, Keys, Kind, PublicKey, Tag, Timestamp,
};
use serde::Deserialize;
use zeroize::Zeroizing;

use super::{authority::hex, Error, Result};

pub(super) const MAX_OUTER: usize = 65536;
pub(super) const MAX_TEXT: usize = 8192;

#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Frozen {
    pub rumor_id: String,
    pub outer_ids: [String; 2],
    pub outer_json: [String; 2],
}

pub(super) struct Opened {
    pub rumor_id: String,
    pub sender: String,
    pub created_at: u64,
    pub reply_to: Option<String>,
    pub text: Zeroizing<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Signed {
    id: String,
    pubkey: String,
    created_at: u64,
    kind: u16,
    tags: Vec<Vec<String>>,
    content: String,
    sig: String,
}

struct Secret(Zeroizing<String>);
impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(d).map(|s| Self(Zeroizing::new(s)))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rumor {
    id: String,
    pubkey: String,
    created_at: u64,
    kind: u16,
    tags: Vec<Vec<String>>,
    content: Secret,
}

pub(super) fn text(value: &str, empty: bool) -> Result<()> {
    if value.len() > MAX_TEXT
        || (!empty && value.trim().is_empty())
        || value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(Error::Bounds);
    }
    Ok(())
}

fn object(bytes: &[u8], cap: usize) -> Result<()> {
    if bytes.len() > cap {
        return Err(Error::Bounds);
    }
    if bytes
        .iter()
        .copied()
        .find(|b| !matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
        != Some(b'{')
    {
        return Err(Error::Encoding);
    }
    Ok(())
}

fn tags(value: &[Vec<String>]) -> Result<()> {
    if value.len() > 16
        || value
            .iter()
            .map(|t| t.iter().map(String::len).sum::<usize>())
            .sum::<usize>()
            > 2048
        || value.iter().any(|t| {
            t.len() > 4
                || t.iter()
                    .any(|s| s.len() > 256 || s.chars().any(char::is_control))
        })
    {
        return Err(Error::Bounds);
    }
    Ok(())
}

fn timestamp(seconds: u64) -> Result<()> {
    if seconds > 253_402_300_799 {
        return Err(Error::Encoding);
    }
    Ok(())
}

pub(super) fn outer(bytes: &[u8]) -> Result<Event> {
    signed(bytes, MAX_OUTER, 1059)
}

fn signed(bytes: &[u8], cap: usize, kind: u16) -> Result<Event> {
    object(bytes, cap)?;
    let wire: Signed = serde_json::from_slice(bytes).map_err(|_| Error::Encoding)?;
    hex(&wire.id)?;
    hex(&wire.pubkey)?;
    timestamp(wire.created_at)?;
    tags(&wire.tags)?;
    if wire.kind != kind
        || wire.sig.len() != 128
        || !wire
            .sig
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || wire.content.len() > if kind == 1059 { 49152 } else { 24576 }
        || (kind == 13 && !wire.tags.is_empty())
    {
        return Err(Error::Encoding);
    }
    let event = Event::from_json(bytes).map_err(|_| Error::Encoding)?;
    event.verify().map_err(|_| Error::Encoding)?;
    Ok(event)
}

fn decrypt(
    keys: &Keys,
    sender: &PublicKey,
    ciphertext: &str,
    cap: usize,
) -> Result<Zeroizing<Vec<u8>>> {
    let prefix = ciphertext.as_bytes().get(..4).ok_or(Error::Encoding)?;
    let version = base64::engine::general_purpose::STANDARD
        .decode(prefix)
        .map_err(|_| Error::Encoding)?;
    if version.first() != Some(&2) {
        return Err(Error::Encoding);
    }
    let bytes = Zeroizing::new(
        nip44::decrypt_to_bytes(keys.secret_key(), sender, ciphertext)
            .map_err(|_| Error::Encoding)?,
    );
    if bytes.len() > cap {
        return Err(Error::Bounds);
    }
    Ok(bytes)
}

/// Opens only a human-addressed outer, with either the selected employee sender
/// or a self-history seal whose inner p still names that same employee.
pub(super) fn open(keys: &Keys, employee: &str, bytes: &[u8]) -> Result<Opened> {
    hex(employee)?;
    let human = keys.public_key().to_hex();
    let event = outer(bytes)?;
    let outer_tags = event.tags.as_slice();
    if outer_tags.len() != 1 || outer_tags[0].as_slice() != ["p", human.as_str()] {
        return Err(Error::Encoding);
    }
    let seal_bytes = decrypt(keys, &event.pubkey, &event.content, 32768)?;
    let seal = signed(&seal_bytes, 32768, 13)?;
    let sender = seal.pubkey.to_hex();
    if sender != human && sender != employee {
        return Err(Error::Encoding);
    }
    let bytes = decrypt(keys, &seal.pubkey, &seal.content, 16384)?;
    object(&bytes, 16384)?;
    let rumor: Rumor = serde_json::from_slice(&bytes).map_err(|_| Error::Encoding)?;
    hex(&rumor.id)?;
    hex(&rumor.pubkey)?;
    timestamp(rumor.created_at)?;
    tags(&rumor.tags)?;
    if rumor.kind != 14 || rumor.pubkey != sender {
        return Err(Error::Encoding);
    }
    text(&rumor.content.0, false)?;
    let recipient = if sender == human {
        employee
    } else {
        human.as_str()
    };
    let mut p = false;
    let mut reply = None;
    let mut parsed = Vec::new();
    for tag in &rumor.tags {
        match tag.first().map(String::as_str) {
            Some("p") if !p && tag.len() == 2 && tag[1] == recipient => {
                p = true;
            }
            Some("e")
                if reply.is_none()
                    && (tag.len() == 2
                        || (tag.len() == 4 && tag[2].is_empty() && tag[3] == "reply")) =>
            {
                hex(&tag[1])?;
                reply = Some(tag[1].clone());
            }
            _ => return Err(Error::Encoding),
        }
        parsed.push(Tag::parse(tag.clone()).map_err(|_| Error::Encoding)?);
    }
    if !p {
        return Err(Error::Encoding);
    }
    let id = EventId::new(
        &seal.pubkey,
        &Timestamp::from(rumor.created_at),
        &Kind::PrivateDirectMessage,
        &parsed.into_iter().collect(),
        &rumor.content.0,
    );
    if id.to_hex() != rumor.id {
        return Err(Error::Encoding);
    }
    Ok(Opened {
        rumor_id: rumor.id,
        sender,
        created_at: rumor.created_at,
        reply_to: reply,
        text: rumor.content.0,
    })
}

pub(super) async fn freeze(keys: &Keys, employee: &str, plaintext: &str) -> Result<Frozen> {
    text(plaintext, false)?;
    hex(employee)?;
    let recipient = PublicKey::from_hex(employee).map_err(|_| Error::Encoding)?;
    if recipient == keys.public_key() {
        return Err(Error::Encoding);
    }
    // Build once: distinct wraps must not reconstruct rumor timestamps/IDs.
    let mut rumor = EventBuilder::private_msg_rumor(recipient, plaintext).build(keys.public_key());
    rumor.ensure_id();
    let rumor_id = rumor.id.ok_or(Error::Encoding)?.to_hex();
    let recipient_copy = EventBuilder::gift_wrap(keys, &recipient, rumor.clone(), [])
        .await
        .map_err(|_| Error::Encoding)?;
    let sender_copy = EventBuilder::gift_wrap(keys, &keys.public_key(), rumor, [])
        .await
        .map_err(|_| Error::Encoding)?;
    let value = Frozen {
        rumor_id,
        outer_ids: [recipient_copy.id.to_hex(), sender_copy.id.to_hex()],
        outer_json: [recipient_copy.as_json(), sender_copy.as_json()],
    };
    for bytes in &value.outer_json {
        outer(bytes.as_bytes())?;
    }
    let history = open(keys, employee, value.outer_json[1].as_bytes())?;
    if history.rumor_id != value.rumor_id || history.text.as_str() != plaintext {
        return Err(Error::Encoding);
    }
    Ok(value)
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
