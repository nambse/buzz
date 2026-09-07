use super::*;

#[tokio::test]
async fn one_rumor_two_frozen_copies_and_selected_sender_history() {
    let human = Keys::generate();
    let employee = Keys::generate();
    let stranger = Keys::generate();
    let frozen = freeze(
        &human,
        &employee.public_key().to_hex(),
        "private native sentence",
    )
    .await
    .unwrap();
    let received = nostr::nips::nip59::extract_rumor(
        &employee,
        &outer(frozen.outer_json[0].as_bytes()).unwrap(),
    )
    .await
    .unwrap();
    let history = open(
        &human,
        &employee.public_key().to_hex(),
        frozen.outer_json[1].as_bytes(),
    )
    .unwrap();
    assert_eq!(received.rumor.id.unwrap().to_hex(), frozen.rumor_id);
    assert_eq!(history.rumor_id, frozen.rumor_id);
    assert_eq!(history.text.as_str(), "private native sentence");
    assert_ne!(frozen.outer_ids[0], frozen.outer_ids[1]);
    assert!(open(
        &human,
        &stranger.public_key().to_hex(),
        frozen.outer_json[1].as_bytes()
    )
    .is_err());
    assert!(open(
        &stranger,
        &employee.public_key().to_hex(),
        frozen.outer_json[1].as_bytes()
    )
    .is_err());
    assert!(!serde_json::to_string(&frozen)
        .unwrap()
        .contains("private native sentence"));
}

#[tokio::test]
async fn strict_object_signature_kind_and_pair_are_not_library_defaults() {
    let human = Keys::generate();
    let employee = Keys::generate();
    let frozen = freeze(&employee, &human.public_key().to_hex(), "incoming")
        .await
        .unwrap();
    assert_eq!(
        open(
            &human,
            &employee.public_key().to_hex(),
            frozen.outer_json[0].as_bytes()
        )
        .unwrap()
        .text
        .as_str(),
        "incoming"
    );
    let mut event: serde_json::Value = serde_json::from_str(&frozen.outer_json[0]).unwrap();
    event["id"] = serde_json::Value::String("0".repeat(64));
    assert!(outer(&serde_json::to_vec(&event).unwrap()).is_err());
    let array = [
        "id",
        "pubkey",
        "created_at",
        "kind",
        "tags",
        "content",
        "sig",
    ]
    .map(|key| event[key].clone());
    assert!(outer(&serde_json::to_vec(&array).unwrap()).is_err());
    assert!(outer(&vec![b' '; MAX_OUTER + 1]).is_err());
    assert!(freeze(
        &human,
        &employee.public_key().to_hex(),
        &"x".repeat(MAX_TEXT + 1)
    )
    .await
    .is_err());
}
