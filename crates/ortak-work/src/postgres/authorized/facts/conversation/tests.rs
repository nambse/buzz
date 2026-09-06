use super::*;
use chrono::Duration;
use ortak_control::memory::conversation::{
    ConversationAudienceV1, ConversationEventIdentity, ConversationMemoryDigest,
};
use serde_json::json;

fn at(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}

fn project() -> Uuid {
    Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap()
}

fn draft() -> ReviewedConversationFactDraft {
    ReviewedConversationFactDraft {
        employee_id: EmployeeId::parse("ada-private").unwrap(),
        source_message_id: "a".repeat(64),
        audience: ConversationMemoryAudience::Thread {},
        expected_audience_hash: "b".repeat(64),
        content: "Human reviewed thread fact.\nSecond line.".into(),
        expires_at: at("2026-09-07T01:02:03Z"),
        reviewed: true,
    }
}

#[test]
fn conversation_request_requires_explicit_choice_and_rejects_client_identity_fields() {
    let request = json!({"employee_id":"ada-private","source_message_id":"a".repeat(64),
        "audience":{"kind":"thread"}});
    let parsed: ReviewedConversationPreviewRequest =
        serde_json::from_value(request.clone()).unwrap();
    assert!(parsed.validate().is_ok());
    for key in [
        "source_hash",
        "thread_root",
        "community_id",
        "project_id",
        "content",
        "reviewed",
    ] {
        let mut injected = request.clone();
        injected[key] = json!("client-value");
        assert!(
            serde_json::from_value::<ReviewedConversationPreviewRequest>(injected).is_err(),
            "{key}"
        );
    }
    for kind in ["thread", "channel"] {
        let mut injected = request.clone();
        injected["audience"] = json!({"kind":kind,"thread_root_event_id":"c".repeat(64)});
        assert!(serde_json::from_value::<ReviewedConversationPreviewRequest>(injected).is_err());
    }
    for audience in [
        json!(null),
        json!({}),
        json!({"kind":"project"}),
        json!({"kind":"dm"}),
    ] {
        let mut invalid = request.clone();
        invalid["audience"] = audience;
        assert!(serde_json::from_value::<ReviewedConversationPreviewRequest>(invalid).is_err());
    }
    let mut missing = request.clone();
    missing.as_object_mut().unwrap().remove("audience");
    assert!(serde_json::from_value::<ReviewedConversationPreviewRequest>(missing).is_err());
    let duplicate = r#"{"employee_id":"ada-private","source_message_id":"aa","source_message_id":"bb","audience":{"kind":"thread"}}"#;
    assert!(serde_json::from_str::<ReviewedConversationPreviewRequest>(duplicate).is_err());
}

#[test]
fn conversation_draft_validation_rejects_unreviewed_secret_control_and_precision_loss() {
    assert!(draft().validate().is_ok());
    for content in [
        " ".into(),
        "é".repeat(2049),
        "hidden\0text".into(),
        "value\rhidden".into(),
        "api_key=must-not-be-stored".into(),
    ] {
        let mut invalid = draft();
        invalid.content = content;
        assert!(invalid.validate().is_err());
    }
    let mut invalid = draft();
    invalid.reviewed = false;
    assert!(invalid.submitted_fingerprint(project()).is_err());
    for value in [
        "A".repeat(64),
        "a".repeat(63),
        "g".repeat(64),
        "private-token".into(),
    ] {
        let mut invalid = draft();
        invalid.source_message_id = value.clone();
        let error = invalid.validate().unwrap_err().to_string();
        assert!(!error.contains(&value));
        invalid = draft();
        invalid.expected_audience_hash = value;
        assert!(invalid.validate().is_err());
    }
    for expires_at in [
        at("1969-12-31T23:59:59Z"),
        at("2026-09-07T01:02:03.000000001Z"),
        at("2026-12-31T23:59:60Z"),
    ] {
        let mut invalid = draft();
        invalid.expires_at = expires_at;
        assert!(invalid.validate().is_err());
    }
    let mut wire = serde_json::to_value(draft()).unwrap();
    wire["source_hash"] = json!("c".repeat(64));
    assert!(serde_json::from_value::<ReviewedConversationFactDraft>(wire).is_err());
}

#[test]
fn conversation_submitted_hash_binds_every_mutable_field_without_live_observation() {
    let original = draft();
    let expected = original.submitted_fingerprint(project()).unwrap();
    // Independently generated with Python stdlib JSON (lexical keys, UTF-8)
    // and hashlib; literal value is not computed by this Rust implementation.
    assert_eq!(
        hex::encode(expected),
        "fb65c40f7f442f4508414ccc126e0c22e6de72447262f198c3d34e3aadc4e7b2"
    );
    assert_ne!(
        expected,
        original.submitted_fingerprint(Uuid::new_v4()).unwrap()
    );
    assert!(original.submitted_fingerprint(Uuid::nil()).is_err());
    let mut variants = vec![original.clone(); 6];
    variants[0].employee_id = EmployeeId::parse("other").unwrap();
    variants[1].source_message_id = "c".repeat(64);
    variants[2].audience = ConversationMemoryAudience::Channel {};
    variants[3].expected_audience_hash = "d".repeat(64);
    variants[4].content.push(' ');
    variants[5].expires_at += Duration::microseconds(1);
    for changed in variants {
        assert_ne!(expected, changed.submitted_fingerprint(project()).unwrap());
    }
    // An ordinary expiry changes new-admission eligibility, not immutable replay identity.
    assert!(original.validate_expiry(original.expires_at, None).is_err());
    assert_eq!(expected, original.submitted_fingerprint(project()).unwrap());
}

#[test]
fn conversation_expiry_applies_exact_ninety_days_and_tighter_current_deadline() {
    let now = at("2026-09-06T01:02:03Z");
    let mut value = draft();
    value.expires_at = now + Duration::days(90);
    assert!(value.validate_expiry(now, None).is_ok());
    value.expires_at += Duration::microseconds(1);
    assert!(value.validate_expiry(now, None).is_err());
    let deadline = now + Duration::hours(2);
    value.expires_at = deadline;
    assert!(value.validate_expiry(now, Some(deadline)).is_ok());
    value.expires_at += Duration::microseconds(1);
    assert!(value.validate_expiry(now, Some(deadline)).is_err());
    for expired in [now, now - Duration::microseconds(1)] {
        value.expires_at = expired;
        assert!(value.validate_expiry(now, None).is_err());
        assert!(types::expiry_limit(now, Some(expired)).is_err());
    }
    assert_eq!(earliest(Some(deadline), Some(now)), Some(now));
    assert_eq!(earliest(None, Some(deadline)), Some(deadline));
    assert_eq!(earliest(Some(deadline), None), Some(deadline));
    assert_eq!(earliest(None, None), None);
}

fn provenance(thread: bool) -> ConversationProvenanceV1 {
    let company = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    let community = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    let channel = Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap();
    let employee = draft().employee_id;
    let root = ConversationEventIdentity::new(
        MessageId::from_bytes([0xcc; 32]),
        at("2026-09-06T00:00:00.123456Z"),
    )
    .unwrap();
    let audience = if thread {
        ConversationAudienceV1::thread(company, community, project(), employee, channel, root)
    } else {
        ConversationAudienceV1::channel(company, community, project(), employee, channel)
    }
    .unwrap();
    ConversationProvenanceV1::new(
        audience,
        ConversationEventIdentity::new(
            MessageId::from_bytes([0xaa; 32]),
            at("2026-09-06T01:00:00Z"),
        )
        .unwrap(),
        ConversationMemoryDigest::from_bytes([0xdd; 32]),
    )
    .unwrap()
}

#[test]
fn conversation_preview_preserves_canonical_audience_and_provenance_without_text() {
    let now = at("2026-09-06T01:02:03Z");
    let deadline = now + Duration::hours(3);
    for thread in [false, true] {
        let source = provenance(thread);
        let view = preview_view(&source, now, Some(deadline)).unwrap();
        assert_eq!(view.max_expires_at, deadline);
        assert_eq!(view.observed_at, now);
        assert_eq!(view.valid_before, Some(deadline));
        assert_eq!(
            view.audience_hash,
            source.audience().audience_hash().unwrap().to_hex()
        );
        assert_eq!(
            view.audience,
            serde_json::from_slice::<serde_json::Value>(
                &source.audience().canonical_bytes().unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            view.provenance,
            serde_json::from_slice::<serde_json::Value>(&source.canonical_bytes().unwrap())
                .unwrap()
        );
        assert_eq!(
            view.audience["kind"],
            if thread { "thread" } else { "channel" }
        );
        assert_eq!(view.audience["thread_root_event_id"].is_null(), !thread);
        let wire = serde_json::to_value(view).unwrap();
        assert!(wire.get("content").is_none());
        assert!(wire.get("source_text").is_none());
        assert!(wire.get("approved").is_none());
        assert!(wire.get("epoch").is_none());
        assert!(preview_view(&source, now, Some(now)).is_err());
    }
    assert_ne!(
        preview_view(&provenance(true), now, None)
            .unwrap()
            .audience_hash,
        preview_view(&provenance(false), now, None)
            .unwrap()
            .audience_hash
    );
    assert_eq!(
        preview_view(&provenance(true), now, None)
            .unwrap()
            .max_expires_at,
        now + Duration::days(90)
    );
}

#[test]
fn conversation_current_projection_requires_full_source_provenance_not_only_audience() {
    let retained = provenance(true);
    assert!(records::same_source(&retained, &retained.clone()));
    let changed_evidence = ConversationProvenanceV1::new(
        retained.audience().clone(),
        retained.source().clone(),
        ConversationMemoryDigest::from_bytes([0xee; 32]),
    )
    .unwrap();
    assert_eq!(retained.audience(), changed_evidence.audience());
    assert!(!records::same_source(&retained, &changed_evidence));
    let sibling = ConversationProvenanceV1::new(
        retained.audience().clone(),
        ConversationEventIdentity::new(
            MessageId::from_bytes([0xbb; 32]),
            retained.source().created_at(),
        )
        .unwrap(),
        retained.source_evidence_hash(),
    )
    .unwrap();
    assert_eq!(retained.audience(), sibling.audience());
    assert!(!records::same_source(&retained, &sibling));
    let changed_partition = ConversationProvenanceV1::new(
        retained.audience().clone(),
        ConversationEventIdentity::new(
            retained.source().event_id(),
            retained.source().created_at() + Duration::microseconds(1),
        )
        .unwrap(),
        retained.source_evidence_hash(),
    )
    .unwrap();
    assert!(!records::same_source(&retained, &changed_partition));
    assert!(!records::same_source(&retained, &provenance(false)));
}
