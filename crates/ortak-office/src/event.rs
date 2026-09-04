//! Bounded unsigned Office events and the verified, frozen signed form.
//!
//! [`OfficePublishIntent`] is what a run wants to publish. Its
//! [`IntentFingerprint`] deliberately excludes the timestamp and public key:
//! a retry that signs again after a crash produces a different event but the
//! same intent, and the outbox row pins that intent so a different payload can
//! never be frozen under the same row.
//!
//! [`FrozenSignedEvent`] can only be constructed through
//! [`FrozenSignedEvent::seal`] (fresh signer output) or
//! [`FrozenSignedEvent::from_stored`] (outbox read-back). Both verify the
//! event id and Schnorr signature, the author public key, and that the signed
//! fields are exactly the unsigned intent, so no code path can hold an
//! unverified event. Verification reuses the same `nostr` primitives as
//! `buzz_core::verification::verify_event`.

use std::fmt;

use chrono::{DateTime, Utc};
use nostr::JsonUtil;
use ortak_control::office_identity::OfficePublicKey;
use ortak_control::MessageId;
use ortak_domain::EmployeeId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Maximum UTF-8 bytes of event content.
pub const MAX_CONTENT_BYTES: usize = 32 * 1024;
/// Maximum number of tags.
pub const MAX_TAGS: usize = 64;
/// Maximum values (including the tag name) in one tag.
pub const MAX_TAG_VALUES: usize = 16;
/// Maximum UTF-8 bytes of one tag value.
pub const MAX_TAG_VALUE_BYTES: usize = 1024;
/// Maximum UTF-8 bytes across every tag value.
pub const MAX_TOTAL_TAG_BYTES: usize = 32 * 1024;
/// Maximum serialized bytes of a signed event accepted for freezing. Matches
/// the relay's 256 KiB frame limit; the input bounds above keep the
/// worst-case escaped serialization (control characters double, non-ASCII
/// is emitted raw) far below it.
pub const MAX_SIGNED_EVENT_BYTES: usize = 256 * 1024;

/// Domain prefix hashed into every intent fingerprint.
const FINGERPRINT_DOMAIN: &str = "ortak-office-intent-v1";

/// NIP-29 stream chat message kind (`buzz_core::kind::KIND_STREAM_MESSAGE`).
pub const KIND_STREAM_MESSAGE: u16 = 9;
/// Stream chat message kind v2 (`buzz_core::kind::KIND_STREAM_MESSAGE_V2`).
pub const KIND_STREAM_MESSAGE_V2: u16 = 40002;

/// The only event kinds this slice may sign: the exact chat message kinds
/// the retained Buzz Office ingress accepts as routable input
/// (`buzz_relay::handlers::office_ingress::is_office_routable_kind`), minus
/// NIP-17 gift wraps, which are encrypted envelopes rather than
/// employee-authored plaintext messages. Profile metadata (kind 0), deletion
/// requests (kind 5), channel metadata, and every replaceable, addressable,
/// or ephemeral range are excluded by construction; an employee signer must
/// never be able to rewrite its profile, delete history, or replace state
/// through the delivery outbox.
pub const ALLOWED_PUBLISH_KINDS: [u16; 2] = [KIND_STREAM_MESSAGE, KIND_STREAM_MESSAGE_V2];

/// True when `kind` is in [`ALLOWED_PUBLISH_KINDS`].
pub fn is_allowed_publish_kind(kind: u16) -> bool {
    ALLOWED_PUBLISH_KINDS.contains(&kind)
}

/// Failures while validating, sealing, or re-verifying an Office event.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OfficeEventError {
    /// Content exceeds [`MAX_CONTENT_BYTES`].
    #[error("event content exceeds {MAX_CONTENT_BYTES} bytes")]
    ContentTooLarge,
    /// More than [`MAX_TAGS`] tags.
    #[error("event carries more than {MAX_TAGS} tags")]
    TooManyTags,
    /// A tag is empty or has more than [`MAX_TAG_VALUES`] values.
    #[error("tag {index} must have between 1 and {MAX_TAG_VALUES} values")]
    InvalidTagShape {
        /// Zero-based tag index.
        index: usize,
    },
    /// A tag value exceeds [`MAX_TAG_VALUE_BYTES`].
    #[error("tag {index} has a value larger than {MAX_TAG_VALUE_BYTES} bytes")]
    TagValueTooLarge {
        /// Zero-based tag index.
        index: usize,
    },
    /// Tag values together exceed [`MAX_TOTAL_TAG_BYTES`].
    #[error("tag values together exceed {MAX_TOTAL_TAG_BYTES} bytes")]
    TagsTooLarge,
    /// Content or a tag contains a control character other than newline,
    /// carriage return, or tab.
    #[error("event text contains a control character")]
    ControlCharacter,
    /// The kind is not one of [`ALLOWED_PUBLISH_KINDS`].
    #[error("event kind {kind} is not an allowed office chat message kind")]
    KindNotAllowed {
        /// Rejected kind.
        kind: u16,
    },
    /// The timestamp is before the Unix epoch or otherwise unusable.
    #[error("event timestamp is invalid")]
    InvalidTimestamp,
    /// The public key bytes are not a valid x-only key.
    #[error("public key is not a valid x-only key")]
    InvalidPublicKey,
    /// The serialized signed event exceeds [`MAX_SIGNED_EVENT_BYTES`].
    #[error("signed event exceeds {MAX_SIGNED_EVENT_BYTES} bytes")]
    SignedEventTooLarge,
    /// The signed bytes are not a Nostr event.
    #[error("signed event is malformed: {detail}")]
    Malformed {
        /// Bounded parser detail.
        detail: String,
    },
    /// The event id does not hash the event fields.
    #[error("signed event id does not match its fields")]
    InvalidEventId,
    /// The signature does not verify for the event id and public key.
    #[error("signed event signature is invalid")]
    InvalidSignature,
    /// The signer produced a different key than the one expected.
    #[error("signed event author {produced} does not match expected key {expected}")]
    PublicKeyMismatch {
        /// Expected hex key.
        expected: String,
        /// Hex key on the signed event.
        produced: String,
    },
    /// The signed fields are not the unsigned event that was requested.
    #[error("signed event does not match the unsigned event that was requested")]
    EventMismatch,
    /// Stored bytes do not hash to the stored event id.
    #[error("stored signed event bytes do not match the stored event id")]
    StoredIdMismatch,
    /// Stored bytes do not describe the intent the row pinned.
    #[error("stored signed event does not match the pinned intent fingerprint")]
    FingerprintMismatch,
}

/// SHA-256 over the canonical intent, excluding timestamp and public key.
#[derive(Clone, Copy, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct IntentFingerprint([u8; 32]);

impl IntentFingerprint {
    /// Raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parses 64 hex characters.
    pub fn parse_hex(value: &str) -> Result<Self, String> {
        let bytes = hex::decode(value).map_err(|_| "intent fingerprint is not hex".to_owned())?;
        <[u8; 32]>::try_from(bytes.as_slice())
            .map(Self)
            .map_err(|_| "intent fingerprint must be 32 bytes".to_owned())
    }
}

impl TryFrom<String> for IntentFingerprint {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse_hex(&value)
    }
}

impl From<IntentFingerprint> for String {
    fn from(value: IntentFingerprint) -> Self {
        value.to_hex()
    }
}

impl fmt::Debug for IntentFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "IntentFingerprint({})", self.to_hex())
    }
}

impl fmt::Display for IntentFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

/// What one run intends to publish to the Office, before any key or time is
/// attached. Bounded by [`OfficePublishIntent::validate`].
///
/// The employee and revision here are provenance the control plane derived
/// from the run row, never caller input: callers submit an
/// [`OfficePublishDraft`](crate::repository::OfficePublishDraft), and only
/// the repository seam turns it into an intent (see
/// [`AuthorizedOfficePublish`](crate::repository::AuthorizedOfficePublish)).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfficePublishIntent {
    /// Company boundary; must equal the resolved scope at delivery time.
    pub company_id: Uuid,
    /// Run that produced the content.
    pub run_id: Uuid,
    /// Authoring employee, from the run row.
    pub employee_id: EmployeeId,
    /// Employee revision pinned by the run row.
    pub employee_revision_id: Uuid,
    /// Nostr event kind; must satisfy [`is_allowed_publish_kind`].
    pub kind: u16,
    /// Tags as string lists (`[name, value, ...]`).
    pub tags: Vec<Vec<String>>,
    /// Event content.
    pub content: String,
}

impl OfficePublishIntent {
    /// Checks the kind policy and every bound, and rejects control
    /// characters other than newline, carriage return, and tab.
    pub fn validate(&self) -> Result<(), OfficeEventError> {
        if !is_allowed_publish_kind(self.kind) {
            return Err(OfficeEventError::KindNotAllowed { kind: self.kind });
        }
        if self.content.len() > MAX_CONTENT_BYTES {
            return Err(OfficeEventError::ContentTooLarge);
        }
        check_text(&self.content)?;
        if self.tags.len() > MAX_TAGS {
            return Err(OfficeEventError::TooManyTags);
        }
        let mut total_tag_bytes = 0usize;
        for (index, tag) in self.tags.iter().enumerate() {
            if tag.is_empty() || tag.len() > MAX_TAG_VALUES {
                return Err(OfficeEventError::InvalidTagShape { index });
            }
            for value in tag {
                if value.len() > MAX_TAG_VALUE_BYTES {
                    return Err(OfficeEventError::TagValueTooLarge { index });
                }
                check_text(value)?;
                total_tag_bytes = total_tag_bytes.saturating_add(value.len());
            }
        }
        if total_tag_bytes > MAX_TOTAL_TAG_BYTES {
            return Err(OfficeEventError::TagsTooLarge);
        }
        Ok(())
    }

    /// Canonical fingerprint: SHA-256 of a compact JSON array of the domain
    /// prefix, company, run, employee, revision, kind, tags, and content.
    /// `serde_json` escaping is deterministic, so equal intents hash equally
    /// across processes.
    pub fn fingerprint(&self) -> IntentFingerprint {
        let canonical = serde_json::json!([
            FINGERPRINT_DOMAIN,
            self.company_id,
            self.run_id,
            self.employee_id.as_str(),
            self.employee_revision_id,
            self.kind,
            self.tags,
            self.content,
        ]);
        let digest = Sha256::digest(canonical.to_string().as_bytes());
        IntentFingerprint(digest.into())
    }
}

fn check_text(value: &str) -> Result<(), OfficeEventError> {
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(OfficeEventError::ControlCharacter);
    }
    Ok(())
}

/// A validated unsigned Office event: the intent plus the author key and
/// timestamp that the signer must sign exactly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsignedOfficeEvent {
    intent: OfficePublishIntent,
    public_key: OfficePublicKey,
    created_at: u64,
    fingerprint: IntentFingerprint,
}

impl UnsignedOfficeEvent {
    /// Validates the intent and attaches the author key and timestamp.
    /// Crate-private: the only producer is
    /// [`AuthorizedOfficePublish::signing_request`](crate::repository::AuthorizedOfficePublish::signing_request),
    /// so the key attached here is always the one the control plane derived.
    pub(crate) fn new(
        intent: OfficePublishIntent,
        public_key: OfficePublicKey,
        created_at: DateTime<Utc>,
    ) -> Result<Self, OfficeEventError> {
        intent.validate()?;
        let created_at = u64::try_from(created_at.timestamp())
            .map_err(|_| OfficeEventError::InvalidTimestamp)?;
        nostr::PublicKey::from_slice(public_key.as_bytes())
            .map_err(|_| OfficeEventError::InvalidPublicKey)?;
        let fingerprint = intent.fingerprint();
        Ok(Self {
            intent,
            public_key,
            created_at,
            fingerprint,
        })
    }

    /// The validated intent.
    pub fn intent(&self) -> &OfficePublishIntent {
        &self.intent
    }

    /// Author key the signature must verify under.
    pub fn public_key(&self) -> &OfficePublicKey {
        &self.public_key
    }

    /// Unix seconds.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Intent fingerprint.
    pub fn fingerprint(&self) -> IntentFingerprint {
        self.fingerprint
    }

    /// Builds the Nostr unsigned event for a signer adapter. The adapter must
    /// sign these fields unchanged; [`FrozenSignedEvent::seal`] rejects
    /// anything else.
    pub fn to_nostr(&self) -> Result<nostr::UnsignedEvent, OfficeEventError> {
        let public_key = nostr::PublicKey::from_slice(self.public_key.as_bytes())
            .map_err(|_| OfficeEventError::InvalidPublicKey)?;
        let tags = self
            .intent
            .tags
            .iter()
            .enumerate()
            .map(|(index, tag)| {
                nostr::Tag::parse(tag.iter().cloned())
                    .map_err(|_| OfficeEventError::InvalidTagShape { index })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(nostr::UnsignedEvent::new(
            public_key,
            nostr::Timestamp::from_secs(self.created_at),
            nostr::Kind::from_u16(self.intent.kind),
            tags,
            self.intent.content.clone(),
        ))
    }

    /// The event id a correct signature must cover.
    pub fn expected_event_id(&self) -> Result<MessageId, OfficeEventError> {
        let mut unsigned = self.to_nostr()?;
        Ok(MessageId::from_bytes(unsigned.id().to_bytes()))
    }
}

/// Fields a verified signed event exposes about itself.
struct VerifiedEvent {
    event: nostr::Event,
    event_id: MessageId,
}

/// Parses and verifies signed bytes against the expected author key.
fn verify_signed_bytes(
    bytes: &[u8],
    expected_public_key: &OfficePublicKey,
) -> Result<VerifiedEvent, OfficeEventError> {
    if bytes.len() > MAX_SIGNED_EVENT_BYTES {
        return Err(OfficeEventError::SignedEventTooLarge);
    }
    let event = nostr::Event::from_json(bytes).map_err(|error| OfficeEventError::Malformed {
        detail: ortak_control::adapter::Detail::new(error.to_string())
            .as_str()
            .to_owned(),
    })?;
    if !event.verify_id() {
        return Err(OfficeEventError::InvalidEventId);
    }
    if !event.verify_signature() {
        return Err(OfficeEventError::InvalidSignature);
    }
    if event.pubkey.as_bytes() != expected_public_key.as_bytes() {
        return Err(OfficeEventError::PublicKeyMismatch {
            expected: expected_public_key.to_hex(),
            produced: event.pubkey.to_hex(),
        });
    }
    let event_id = MessageId::from_bytes(event.id.to_bytes());
    Ok(VerifiedEvent { event, event_id })
}

/// Exact signed event bytes and the provenance pinned with them.
///
/// Instances are always verified. The bytes are the canonical `nostr`
/// serialization produced once at seal time; retries and publishers use them
/// verbatim and never re-serialize or re-sign.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenSignedEvent {
    company_id: Uuid,
    run_id: Uuid,
    employee_id: EmployeeId,
    employee_revision_id: Uuid,
    fingerprint: IntentFingerprint,
    public_key: OfficePublicKey,
    event_id: MessageId,
    kind: u16,
    created_at: u64,
    signed_bytes: Vec<u8>,
}

impl FrozenSignedEvent {
    /// Verifies fresh signer output against the unsigned event it was asked
    /// to sign and freezes the canonical bytes.
    ///
    /// Fails closed when the bytes do not parse, the id or signature does not
    /// verify, the author is not the expected key, or any signed field differs
    /// from the unsigned event (detected through the id, which hashes every
    /// field).
    pub fn seal(
        unsigned: &UnsignedOfficeEvent,
        signed_json: &[u8],
    ) -> Result<Self, OfficeEventError> {
        let verified = verify_signed_bytes(signed_json, unsigned.public_key())?;
        if verified.event_id != unsigned.expected_event_id()? {
            return Err(OfficeEventError::EventMismatch);
        }
        let canonical = verified.event.as_json().into_bytes();
        // The bytes that will be frozen must verify on their own.
        let reverified = verify_signed_bytes(&canonical, unsigned.public_key())?;
        if reverified.event_id != verified.event_id {
            return Err(OfficeEventError::EventMismatch);
        }
        Ok(Self {
            company_id: unsigned.intent().company_id,
            run_id: unsigned.intent().run_id,
            employee_id: unsigned.intent().employee_id.clone(),
            employee_revision_id: unsigned.intent().employee_revision_id,
            fingerprint: unsigned.fingerprint(),
            public_key: *unsigned.public_key(),
            event_id: verified.event_id,
            kind: unsigned.intent().kind,
            created_at: unsigned.created_at(),
            signed_bytes: canonical,
        })
    }

    /// Re-verifies bytes read back from an outbox row.
    ///
    /// Besides id, signature, and author, this recomputes the intent
    /// fingerprint from the stored fields and requires it to equal the
    /// fingerprint the row pinned, so corrupted or swapped bytes are rejected.
    /// The recomputed intent must also pass [`OfficePublishIntent::validate`],
    /// so bytes of a kind outside the policy are never republished.
    pub fn from_stored(
        stored: &StoredSignedEvent<'_>,
        payload: &OfficePublishPayload,
    ) -> Result<Self, OfficeEventError> {
        let verified = verify_signed_bytes(stored.signed_bytes, &payload.public_key)?;
        if verified.event_id.as_bytes() != stored.event_id {
            return Err(OfficeEventError::StoredIdMismatch);
        }
        let intent = OfficePublishIntent {
            company_id: stored.company_id,
            run_id: stored.run_id,
            employee_id: payload.employee_id.clone(),
            employee_revision_id: payload.employee_revision_id,
            kind: verified.event.kind.as_u16(),
            tags: verified
                .event
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect(),
            content: verified.event.content.clone(),
        };
        intent.validate()?;
        if intent.fingerprint() != payload.intent_fingerprint {
            return Err(OfficeEventError::FingerprintMismatch);
        }
        Ok(Self {
            company_id: stored.company_id,
            run_id: stored.run_id,
            employee_id: payload.employee_id.clone(),
            employee_revision_id: payload.employee_revision_id,
            fingerprint: payload.intent_fingerprint,
            public_key: payload.public_key,
            event_id: verified.event_id,
            kind: verified.event.kind.as_u16(),
            created_at: verified.event.created_at.as_secs(),
            signed_bytes: stored.signed_bytes.to_vec(),
        })
    }

    /// Company boundary.
    pub fn company_id(&self) -> Uuid {
        self.company_id
    }

    /// Run the event belongs to.
    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    /// Authoring employee.
    pub fn employee_id(&self) -> &EmployeeId {
        &self.employee_id
    }

    /// Pinned employee revision.
    pub fn employee_revision_id(&self) -> Uuid {
        self.employee_revision_id
    }

    /// Intent fingerprint.
    pub fn fingerprint(&self) -> IntentFingerprint {
        self.fingerprint
    }

    /// Verified author key.
    pub fn public_key(&self) -> &OfficePublicKey {
        &self.public_key
    }

    /// Stable signed event id.
    pub fn event_id(&self) -> MessageId {
        self.event_id
    }

    /// Nostr kind.
    pub fn kind(&self) -> u16 {
        self.kind
    }

    /// Unix seconds.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Exact serialized bytes to publish.
    pub fn signed_bytes(&self) -> &[u8] {
        &self.signed_bytes
    }
}

/// Borrowed columns of a frozen outbox row.
#[derive(Clone, Copy, Debug)]
pub struct StoredSignedEvent<'a> {
    /// Row company.
    pub company_id: Uuid,
    /// Row run.
    pub run_id: Uuid,
    /// `outbox.signed_event_id`.
    pub event_id: &'a [u8],
    /// `outbox.signed_event_bytes`.
    pub signed_bytes: &'a [u8],
}

/// Current schema version of [`OfficePublishPayload`].
pub const OFFICE_PUBLISH_PAYLOAD_SCHEMA: u8 = 1;

/// Intent provenance stored in `outbox.payload` for an `office_publish` row
/// when it is enqueued, before any signing happens.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficePublishPayload {
    /// Payload schema version.
    pub schema: u8,
    /// Fingerprint of the intent the row may publish.
    pub intent_fingerprint: IntentFingerprint,
    /// Authoring employee.
    pub employee_id: EmployeeId,
    /// Pinned employee revision.
    pub employee_revision_id: Uuid,
    /// Public key the signed event must be authored by.
    pub public_key: OfficePublicKey,
}

impl OfficePublishPayload {
    /// Builds the payload for an intent and its expected author key.
    pub fn new(intent: &OfficePublishIntent, public_key: OfficePublicKey) -> Self {
        Self {
            schema: OFFICE_PUBLISH_PAYLOAD_SCHEMA,
            intent_fingerprint: intent.fingerprint(),
            employee_id: intent.employee_id.clone(),
            employee_revision_id: intent.employee_revision_id,
            public_key,
        }
    }

    /// Company-unique idempotency key: one publish row per run.
    pub fn dedup_key(run_id: Uuid) -> String {
        format!("office_publish:{run_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> OfficePublicKey {
        OfficePublicKey::parse_hex(&"ab".repeat(32)).expect("key")
    }

    fn intent() -> OfficePublishIntent {
        OfficePublishIntent {
            company_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            employee_id: EmployeeId::parse("cem").expect("id"),
            employee_revision_id: Uuid::new_v4(),
            kind: KIND_STREAM_MESSAGE,
            tags: vec![vec!["h".to_owned(), "channel".to_owned()]],
            content: "hello".to_owned(),
        }
    }

    #[test]
    fn fingerprint_ignores_timestamp_and_key_but_not_content() {
        let intent = intent();
        let now = Utc::now();
        let first = UnsignedOfficeEvent::new(intent.clone(), key(), now).expect("unsigned");
        let later = UnsignedOfficeEvent::new(
            intent.clone(),
            OfficePublicKey::parse_hex(&"cd".repeat(32)).expect("key"),
            now + chrono::Duration::seconds(90),
        )
        .expect("unsigned");
        assert_eq!(first.fingerprint(), later.fingerprint());
        assert_ne!(
            first.expected_event_id().unwrap(),
            later.expected_event_id().unwrap()
        );

        let mut changed = intent;
        changed.content.push('!');
        assert_ne!(changed.fingerprint(), first.fingerprint());
    }

    #[test]
    fn bounds_are_enforced() {
        let mut too_large = intent();
        too_large.content = "x".repeat(MAX_CONTENT_BYTES + 1);
        assert_eq!(too_large.validate(), Err(OfficeEventError::ContentTooLarge));

        let mut nul = intent();
        nul.tags = vec![vec!["h".to_owned(), "a\0b".to_owned()]];
        assert_eq!(nul.validate(), Err(OfficeEventError::ControlCharacter));

        let mut newline_ok = intent();
        newline_ok.content = "line one\nline two\ttab".to_owned();
        assert_eq!(newline_ok.validate(), Ok(()));

        let mut tags_total = intent();
        tags_total.tags = vec![vec!["x".repeat(MAX_TAG_VALUE_BYTES)]; MAX_TAGS];
        assert_eq!(tags_total.validate(), Err(OfficeEventError::TagsTooLarge));

        let mut empty_tag = intent();
        empty_tag.tags = vec![vec![]];
        assert_eq!(
            empty_tag.validate(),
            Err(OfficeEventError::InvalidTagShape { index: 0 })
        );

        let mut too_many = intent();
        too_many.tags = vec![vec!["t".to_owned()]; MAX_TAGS + 1];
        assert_eq!(too_many.validate(), Err(OfficeEventError::TooManyTags));

        assert!(UnsignedOfficeEvent::new(
            intent(),
            key(),
            DateTime::<Utc>::from_timestamp(-1, 0).expect("timestamp")
        )
        .is_err());
    }

    #[test]
    fn only_office_chat_message_kinds_are_signable() {
        for kind in ALLOWED_PUBLISH_KINDS {
            let mut allowed = intent();
            allowed.kind = kind;
            assert_eq!(allowed.validate(), Ok(()), "kind {kind}");
        }
        // Profile, text note, contact list, deletion, channel metadata,
        // reaction, gift wrap, stream edit, and one kind from each of the
        // replaceable, ephemeral, and addressable ranges.
        for kind in [0, 1, 3, 5, 41, 7, 1059, 40003, 10000, 20002, 30023] {
            let mut refused = intent();
            refused.kind = kind;
            assert_eq!(
                refused.validate(),
                Err(OfficeEventError::KindNotAllowed { kind }),
                "kind {kind}"
            );
            assert!(UnsignedOfficeEvent::new(refused, key(), Utc::now()).is_err());
        }
    }

    #[test]
    fn payload_round_trips_through_json() {
        let payload = OfficePublishPayload::new(&intent(), key());
        let json = serde_json::to_value(&payload).expect("serialize");
        assert_eq!(json["schema"], OFFICE_PUBLISH_PAYLOAD_SCHEMA);
        assert_eq!(json["public_key"], key().to_hex());
        let parsed: OfficePublishPayload = serde_json::from_value(json).expect("parse");
        assert_eq!(parsed, payload);
    }
}
