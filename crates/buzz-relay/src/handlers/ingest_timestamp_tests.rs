//! In-process regression for the timestamp gate called by production ingest.
//! The real pinned builder chooses the outer time; controlling the server clock
//! relative to that exact signed time makes the boundary checks deterministic
//! without rewriting either retained wrapper or implementing another NIP-59 codec.

use super::{validate_event_timestamp, IngestError};
use nostr::{nips::nip59::UnwrappedGift, Event, EventBuilder, JsonUtil, Keys, Kind, Timestamp};

fn rejected(event: &Event, now: i64) {
    assert!(matches!(
        validate_event_timestamp(event, now),
        Err(IngestError::Rejected(message))
            if message == "invalid: event timestamp too far from server time"
    ));
}

#[tokio::test]
async fn nip59_timestamp_window_preserves_real_wrappers_and_ordinary_bounds() {
    let sender = Keys::generate();
    let recipient = Keys::generate();
    let mut rumor = EventBuilder::private_msg_rumor(
        recipient.public_key(),
        "synthetic encrypted timestamp regression",
    )
    .build(sender.public_key());
    rumor.ensure_id();
    let rumor_id = rumor.id;
    let recipient_copy =
        EventBuilder::gift_wrap(&sender, &recipient.public_key(), rumor.clone(), [])
            .await
            .unwrap();
    let history_copy = EventBuilder::gift_wrap(&sender, &sender.public_key(), rumor, [])
        .await
        .unwrap();

    for (event, reader) in [(&recipient_copy, &recipient), (&history_copy, &sender)] {
        event.verify().unwrap();
        let exact = event.as_json();
        let opened = UnwrappedGift::from_gift_wrap(reader, event).await.unwrap();
        assert_eq!(opened.rumor.id, rumor_id);
        let created = i64::try_from(event.created_at.as_secs()).unwrap();

        // Include the old rejected 901-second age, full NIP-59 backdate, and
        // both inclusive boundaries. One second outside each must still refuse.
        for age in [901, 172_800, 173_700, -900] {
            assert!(validate_event_timestamp(event, created + age).is_ok());
        }
        rejected(event, created + 173_701);
        rejected(event, created - 901);
        assert_eq!(event.as_json(), exact);
    }

    // No NIP-59 allowance for either ordinary plaintext or a seal submitted as
    // an ordinary event. These signed inputs reach the same production gate.
    for kind in [Kind::TextNote, Kind::Seal] {
        let event = EventBuilder::new(kind, "synthetic boundary")
            .custom_created_at(Timestamp::from(1_800_000_000))
            .sign_with_keys(&sender)
            .unwrap();
        for age in [-900, 0, 900] {
            assert!(validate_event_timestamp(&event, 1_800_000_000 + age).is_ok());
        }
        for age in [-901, 901, 172_800] {
            rejected(&event, 1_800_000_000 + age);
        }
    }

    // No signed cast/subtraction/abs overflow, and no pre-epoch clock grants.
    for seconds in [i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX] {
        let event = EventBuilder::new(Kind::TextNote, "synthetic overflow")
            .custom_created_at(Timestamp::from(seconds))
            .sign_with_keys(&sender)
            .unwrap();
        event.verify().unwrap();
        rejected(&event, 1_800_000_000);
    }
    let epoch = EventBuilder::new(Kind::TextNote, "synthetic epoch")
        .custom_created_at(Timestamp::from(0))
        .sign_with_keys(&sender)
        .unwrap();
    assert!(validate_event_timestamp(&epoch, 0).is_ok());
    assert!(validate_event_timestamp(&epoch, -1).is_err());
}
