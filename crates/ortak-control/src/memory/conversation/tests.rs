use super::*;
use serde_json::{json, Value};

fn uuid(byte: u8) -> Uuid {
    Uuid::from_bytes([byte; 16])
}

fn at(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn event(byte: u8, time: &str) -> ConversationEventIdentity {
    ConversationEventIdentity::new(MessageId::from_bytes([byte; 32]), at(time)).unwrap()
}

fn audience(kind: &str) -> ConversationAudienceV1 {
    let mut value = ConversationAudienceV1::channel(
        Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap(),
        Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
        Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
        EmployeeId::parse("ada-private").unwrap(),
        Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap(),
    )
    .unwrap();
    if kind == "thread" {
        value = ConversationAudienceV1::thread(
            value.company_id(),
            value.community_id(),
            value.project_id(),
            value.employee_id().clone(),
            value.channel_id(),
            event(0xbb, "2026-09-06T01:02:03.000000Z"),
        )
        .unwrap();
    }
    value
}

fn provenance(kind: &str) -> ConversationProvenanceV1 {
    ConversationProvenanceV1::new(
        audience(kind),
        event(0xaa, "2026-09-06T01:03:04.123456Z"),
        ConversationMemoryDigest::from_bytes([0xcc; 32]),
    )
    .unwrap()
}

fn vectors() -> Vec<Value> {
    serde_json::from_str(include_str!("test_vectors.json")).unwrap()
}

#[test]
fn literal_python_vectors_bind_audience_and_new_source_hash_bytes() {
    for vector in vectors() {
        let kind = vector["kind"].as_str().unwrap();
        let value = provenance(kind);
        let audience_bytes = vector["audience_utf8"].as_str().unwrap().as_bytes();
        let provenance_bytes = vector["provenance_utf8"].as_str().unwrap().as_bytes();
        assert_eq!(value.audience().canonical_bytes().unwrap(), audience_bytes);
        assert_eq!(value.canonical_bytes().unwrap(), provenance_bytes);
        assert_eq!(
            value.audience().audience_hash().unwrap().to_hex(),
            vector["audience_sha256"]
        );
        assert_eq!(
            value.source_hash().unwrap().to_hex(),
            vector["source_sha256"]
        );
        assert_eq!(
            wire::digest(vector["source_binding_utf8"].as_str().unwrap().as_bytes()).to_hex(),
            vector["source_sha256"]
        );
        assert_eq!(
            ConversationAudienceV1::from_canonical_bytes(audience_bytes).unwrap(),
            *value.audience()
        );
        assert_eq!(
            ConversationProvenanceV1::from_canonical_bytes(provenance_bytes).unwrap(),
            value
        );
    }
}

#[test]
fn every_audience_axis_changes_the_hash_and_no_source_is_an_audience_axis() {
    let original = audience("thread");
    let hash = original.audience_hash().unwrap();
    let mut changed = Vec::new();
    for field in 0..7 {
        let mut value = original.clone();
        match field {
            0 => value.company_id = uuid(1),
            1 => value.community_id = uuid(2),
            2 => value.project_id = uuid(3),
            3 => value.employee_id = EmployeeId::parse("another-employee").unwrap(),
            4 => value.channel_id = uuid(4),
            5 => value.thread_root = Some(event(0xbc, "2026-09-06T01:02:03.000000Z")),
            6 => value.thread_root = Some(event(0xbb, "2026-09-06T01:02:03.000001Z")),
            _ => unreachable!(),
        }
        changed.push(value);
    }
    changed.push(audience("channel"));
    for value in changed {
        assert_ne!(value.audience_hash().unwrap(), hash);
    }
    let first = provenance("thread");
    let second = ConversationProvenanceV1::new(
        original,
        event(0xab, "2026-09-06T01:03:05.000000Z"),
        ConversationMemoryDigest::from_bytes([0xcd; 32]),
    )
    .unwrap();
    assert_eq!(first.audience(), second.audience());
    assert_eq!(first.audience().audience_hash().unwrap(), hash);
    assert_ne!(first.source_hash().unwrap(), second.source_hash().unwrap());
    assert_ne!(
        first.canonical_bytes().unwrap(),
        second.canonical_bytes().unwrap()
    );
    assert_eq!(first.source_evidence_hash().as_bytes(), &[0xcc; 32]);
    assert_ne!(first.source_hash().unwrap(), first.source_evidence_hash());
}

#[test]
fn audience_parser_refuses_partial_broadened_and_noncanonical_identities() {
    let original: Value =
        serde_json::from_slice(&audience("thread").canonical_bytes().unwrap()).unwrap();
    let mutations = [
        ("format", json!("ortak-reviewed-conversation-audience/2")),
        ("format", json!("ortak-reviewed-conversation-provenance/1")),
        ("kind", json!("project")),
        ("kind", json!("dm")),
        ("kind", json!("channel")),
        ("company_id", json!(Uuid::nil())),
        ("community_id", json!(Uuid::nil())),
        ("project_id", json!(Uuid::nil())),
        ("channel_id", json!(Uuid::nil())),
        ("company_id", json!("11111111111141118111111111111111")),
        ("employee_id", json!("Ada-private")),
        ("thread_root_event_id", Value::Null),
        ("thread_root_event_created_at", Value::Null),
        ("thread_root_event_id", json!("BB".repeat(32))),
        ("thread_root_event_id", json!("b".repeat(63))),
        (
            "thread_root_event_created_at",
            json!("2026-09-06T01:02:03Z"),
        ),
        (
            "thread_root_event_created_at",
            json!("2026-09-06T02:02:03.000000+01:00"),
        ),
        (
            "thread_root_event_created_at",
            json!("2026-09-06T01:02:03.000000001Z"),
        ),
        (
            "thread_root_event_created_at",
            json!("2026-09-06T01:02:60.000000Z"),
        ),
        ("source_event_id", json!("aa".repeat(32))),
        ("routing_root_message_id", json!("aa".repeat(32))),
    ];
    for (field, replacement) in mutations {
        let mut value = original.clone();
        value[field] = replacement;
        assert!(
            ConversationAudienceV1::from_canonical_bytes(&serde_json::to_vec(&value).unwrap())
                .is_err(),
            "accepted mutation of {field}"
        );
    }
    let channel = audience("channel");
    let mut value: Value = serde_json::from_slice(&channel.canonical_bytes().unwrap()).unwrap();
    value["kind"] = json!("thread");
    assert!(
        ConversationAudienceV1::from_canonical_bytes(&serde_json::to_vec(&value).unwrap()).is_err()
    );
    value["kind"] = json!("channel");
    value
        .as_object_mut()
        .unwrap()
        .remove("thread_root_event_id");
    assert!(
        ConversationAudienceV1::from_canonical_bytes(&serde_json::to_vec(&value).unwrap()).is_err()
    );
}

#[test]
fn canonical_parser_refuses_duplicate_fields_order_whitespace_and_size_overflow() {
    let raw = audience("channel").canonical_bytes().unwrap();
    let text = String::from_utf8(raw.clone()).unwrap();
    let duplicate = text.replacen('{', "{\"kind\":\"thread\",", 1);
    let reordered = text.replacen(
        "\"channel_id\":\"44444444-4444-4444-8444-444444444444\",",
        "",
        1,
    );
    let reordered = format!(
        "{},\"channel_id\":\"44444444-4444-4444-8444-444444444444\"}}",
        reordered.strip_suffix('}').unwrap()
    );
    for bytes in [
        duplicate.into_bytes(),
        reordered.into_bytes(),
        [raw.as_slice(), b"\n"].concat(),
        vec![],
        vec![b' '; MAX_CONVERSATION_AUDIENCE_BYTES + 1],
    ] {
        assert!(ConversationAudienceV1::from_canonical_bytes(&bytes).is_err());
    }
    assert!(ConversationProvenanceV1::from_canonical_bytes(&vec![
        b' ';
        MAX_CONVERSATION_PROVENANCE_BYTES
            + 1
    ])
    .is_err());
}

#[test]
fn exact_partition_precision_is_lossless_and_root_source_agreement_is_required() {
    for (seconds, nanos) in [
        (-1, 0),
        (253402300800, 0),
        (1, 1),
        (1, 999),
        (1483228799, 1_000_000_000),
    ] {
        let timestamp = DateTime::from_timestamp(seconds, nanos).unwrap();
        assert_eq!(
            ConversationEventIdentity::new(MessageId::from_bytes([1; 32]), timestamp),
            Err(ConversationMemoryError::InvalidTimestamp)
        );
    }
    for time in ["1970-01-01T00:00:00.000000Z", "9999-12-31T23:59:59.999999Z"] {
        let source = event(0xbb, time);
        assert_eq!(wire::timestamp(source.created_at()).unwrap(), time);
    }
    let audience = audience("thread");
    assert!(ConversationProvenanceV1::new(
        audience.clone(),
        audience.thread_root().unwrap().clone(),
        ConversationMemoryDigest::from_bytes([0xcc; 32]),
    )
    .is_ok());
    assert_eq!(
        ConversationProvenanceV1::new(
            audience,
            event(0xbb, "2026-09-06T01:02:03.000001Z"),
            ConversationMemoryDigest::from_bytes([0xcc; 32]),
        ),
        Err(ConversationMemoryError::InconsistentProvenance)
    );
}

#[test]
fn retained_provenance_rejects_forged_hashes_and_mismatched_nested_audience() {
    let value: Value =
        serde_json::from_slice(&provenance("thread").canonical_bytes().unwrap()).unwrap();
    for field in ["audience_hash", "source_evidence_hash", "source_hash"] {
        let mut forged = value.clone();
        forged[field] = json!("dd".repeat(32));
        assert_eq!(
            ConversationProvenanceV1::from_canonical_bytes(&serde_json::to_vec(&forged).unwrap()),
            Err(ConversationMemoryError::InconsistentProvenance),
            "accepted forged {field}"
        );
    }
    let mut forged = value.clone();
    forged["audience"]["employee_id"] = json!("other-employee");
    assert_eq!(
        ConversationProvenanceV1::from_canonical_bytes(&serde_json::to_vec(&forged).unwrap()),
        Err(ConversationMemoryError::InconsistentProvenance)
    );
    for (field, replacement) in [
        ("format", json!("ortak-reviewed-conversation-provenance/2")),
        ("source_hash", json!("DD".repeat(32))),
        ("source_event_id", json!("bb".repeat(32))),
        (
            "source_event_created_at",
            json!("2026-09-06T01:03:04.123456789Z"),
        ),
        ("source_authority", json!(true)),
    ] {
        let mut forged = value.clone();
        forged[field] = replacement;
        assert!(ConversationProvenanceV1::from_canonical_bytes(
            &serde_json::to_vec(&forged).unwrap()
        )
        .is_err());
    }
    // A self-consistent new identity is parseable but is still not ACL authority.
    let changed = ConversationProvenanceV1::new(
        audience("channel"),
        event(0xaa, "2026-09-06T01:03:04.123456Z"),
        ConversationMemoryDigest::from_bytes([0xdd; 32]),
    )
    .unwrap();
    assert_eq!(
        ConversationProvenanceV1::from_canonical_bytes(&changed.canonical_bytes().unwrap())
            .unwrap(),
        changed
    );
}

#[test]
fn digest_parser_is_strict_and_error_text_does_not_echo_rejected_input() {
    let expected = ConversationMemoryDigest::from_bytes([0xab; 32]);
    assert_eq!(
        ConversationMemoryDigest::parse_hex(&expected.to_hex()).unwrap(),
        expected
    );
    for value in [
        "AB".repeat(32),
        "x".repeat(64),
        "a".repeat(63),
        "a".repeat(65),
        "sensitive-invalid-input".into(),
    ] {
        let error = ConversationMemoryDigest::parse_hex(&value).unwrap_err();
        assert_eq!(error, ConversationMemoryError::InvalidDigest);
        assert!(!error.to_string().contains(&value));
    }
}

#[test]
fn legacy_project_scope_and_record_serialization_are_unchanged() {
    use crate::memory::{MemoryProvenance, MemoryRecord, MemoryScope};
    let record = MemoryRecord {
        record_ref: "existing-project-record".into(),
        scope: MemoryScope::ProjectContext {
            project_id: uuid(0x33),
        },
        content: "Previously approved fact".into(),
        provenance: MemoryProvenance {
            employee_id: EmployeeId::parse("ada-private").unwrap(),
            run_id: None,
            source: "review".into(),
            recorded_at: at("2026-09-06T01:02:03Z"),
        },
    };
    assert_eq!(
        serde_json::to_string(&record).unwrap(),
        r#"{"record_ref":"existing-project-record","scope":{"scope":"project_context","project_id":"33333333-3333-3333-3333-333333333333"},"content":"Previously approved fact","provenance":{"employee_id":"ada-private","run_id":null,"source":"review","recorded_at":"2026-09-06T01:02:03Z"}}"#
    );
    assert!(serde_json::from_value::<MemoryScope>(
        json!({"scope":"conversation","channel_id":uuid(4)})
    )
    .is_err());
}
