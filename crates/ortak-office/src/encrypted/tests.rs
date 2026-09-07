use chrono::{Duration, TimeZone};
use nostr::{nips::nip44, Event, EventBuilder, JsonUtil, Kind, Tag, Timestamp, UnsignedEvent};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::*;

struct Fixture {
    human: Keys,
    employee: Keys,
    other: Keys,
}

impl Fixture {
    fn new() -> Self {
        Self {
            human: Keys::generate(),
            employee: Keys::generate(),
            other: Keys::generate(),
        }
    }

    fn rumor(&self, text: &str) -> UnsignedEvent {
        let mut rumor = EventBuilder::private_msg_rumor(self.employee.public_key(), text)
            .custom_created_at(Timestamp::from(1_788_652_800))
            .build(self.human.public_key());
        rumor.ensure_id();
        rumor
    }

    fn seal(&self, rumor: &[u8], kind: Kind, sender: &Keys) -> Event {
        let content = nip44::encrypt(
            sender.secret_key(),
            &self.employee.public_key(),
            rumor,
            nip44::Version::V2,
        )
        .unwrap();
        EventBuilder::new(kind, content)
            .custom_created_at(Timestamp::from(1_788_652_701))
            .sign_with_keys(sender)
            .unwrap()
    }

    fn wrap_seal(&self, bytes: &[u8], recipient_tag: PublicKey) -> Event {
        let ephemeral = Keys::generate();
        let content = nip44::encrypt(
            ephemeral.secret_key(),
            &self.employee.public_key(),
            bytes,
            nip44::Version::V2,
        )
        .unwrap();
        EventBuilder::new(Kind::GiftWrap, content)
            .tags([Tag::public_key(recipient_tag)])
            .custom_created_at(Timestamp::from(1_788_652_601))
            .sign_with_keys(&ephemeral)
            .unwrap()
    }

    fn wrap(&self, rumor: &[u8]) -> Event {
        self.wrap_seal(
            self.seal(rumor, Kind::Seal, &self.human)
                .as_json()
                .as_bytes(),
            self.employee.public_key(),
        )
    }

    fn expected(&self, outer: &Event) -> ExpectedEnvelope {
        ExpectedEnvelope::new(
            outer.id,
            outer.pubkey,
            Utc.timestamp_opt(outer.created_at.as_secs() as i64, 0)
                .single()
                .unwrap(),
            self.human.public_key(),
            self.employee.public_key(),
        )
        .unwrap()
    }

    fn decode(&self, outer: &Event) -> Result<VerifiedDmRumor, DecodeError> {
        decode(
            &DmDecryptKey::for_recipient(&self.employee, self.employee.public_key()).unwrap(),
            &self.expected(outer),
            outer.as_json().as_bytes(),
        )
    }

    fn refused_rumor(&self, rumor: &[u8], error: DecodeError) {
        assert_eq!(self.decode(&self.wrap(rumor)).err(), Some(error));
    }
}

#[tokio::test]
async fn pinned_nip59_roundtrip_preserves_exact_rumor_and_two_wrapper_identity() {
    let f = Fixture::new();
    let text = "Private canary: İ, emoji 🧭, slash \\ and\nline\tend\u{2028}";
    let rumor = f.rumor(text);
    let raw = rumor.as_json();
    let first = EventBuilder::gift_wrap(&f.human, &f.employee.public_key(), rumor.clone(), [])
        .await
        .unwrap();
    let second = EventBuilder::gift_wrap(&f.human, &f.employee.public_key(), rumor.clone(), [])
        .await
        .unwrap();
    let a = f.decode(&first).unwrap();
    let b = f.decode(&second).unwrap();
    assert_eq!(a.text(), text);
    assert_eq!(a.rumor_bytes(), raw.as_bytes());
    assert_eq!(a.rumor_id(), rumor.id.unwrap());
    assert_eq!(
        a.rumor_hash(),
        &<[u8; 32]>::from(Sha256::digest(raw.as_bytes()))
    );
    assert_eq!(
        a.outer_hash(),
        &<[u8; 32]>::from(Sha256::digest(first.as_json().as_bytes()))
    );
    assert_eq!(a.rumor_id(), b.rumor_id());
    assert_eq!(a.rumor_hash(), b.rumor_hash());
    assert_ne!(a.source().outer_id(), b.source().outer_id());
    assert_eq!(a.source().human(), f.human.public_key());
    assert_eq!(a.source().recipient(), f.employee.public_key());
    assert_eq!(a.reply_to(), None);
}

#[test]
fn exact_source_partition_and_explicit_key_are_checked() {
    let f = Fixture::new();
    let outer = f.wrap(f.rumor("source identity").as_json().as_bytes());
    let key = DmDecryptKey::for_recipient(&f.employee, f.employee.public_key()).unwrap();
    let expected = f.expected(&outer);
    for changed in [
        ExpectedEnvelope {
            outer_id: EventId::from_hex(&"ab".repeat(32)).unwrap(),
            ..expected
        },
        ExpectedEnvelope {
            outer_author: f.other.public_key(),
            ..expected
        },
        ExpectedEnvelope {
            partition_at: expected.partition_at + Duration::seconds(1),
            ..expected
        },
    ] {
        assert_eq!(
            decode(&key, &changed, outer.as_json().as_bytes()).err(),
            Some(DecodeError::SourceMismatch)
        );
    }
    assert_eq!(
        ExpectedEnvelope::new(
            outer.id,
            outer.pubkey,
            expected.partition_at + Duration::microseconds(1),
            expected.human,
            expected.recipient
        )
        .err(),
        Some(DecodeError::Timestamp)
    );
    assert_eq!(
        DmDecryptKey::for_recipient(&f.other, f.employee.public_key()).err(),
        Some(DecodeError::KeyMismatch)
    );
    let wrong_key = DmDecryptKey::for_recipient(&f.other, f.other.public_key()).unwrap();
    assert_eq!(
        decode(&wrong_key, &expected, outer.as_json().as_bytes()).err(),
        Some(DecodeError::KeyMismatch)
    );
    assert_eq!(
        ExpectedEnvelope::new(
            outer.id,
            outer.pubkey,
            expected.partition_at,
            expected.human,
            expected.human
        )
        .err(),
        Some(DecodeError::Sender)
    );
}

mod failures;
mod limits;
