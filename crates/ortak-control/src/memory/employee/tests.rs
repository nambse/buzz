use super::*;
use crate::{memory::MemoryScope, MessageId};
use chrono::{DateTime, Utc};
use serde_json::Value;

fn id(value: &str) -> Uuid {
    Uuid::parse_str(value).unwrap()
}

fn at(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}

fn key(byte: u8) -> OfficePublicKey {
    OfficePublicKey::parse_hex(&hex::encode([byte; 32])).unwrap()
}

fn audience(relationship: bool) -> EmployeeMemoryAudienceV1 {
    let company = id("11111111-1111-4111-8111-111111111111");
    let community = id("22222222-2222-4222-8222-222222222222");
    let channel = id("33333333-3333-4333-8333-333333333333");
    let employee = EmployeeId::parse("ada-test").unwrap();
    if relationship {
        EmployeeMemoryAudienceV1::relationship(company, employee, community, channel, key(0xbb))
    } else {
        EmployeeMemoryAudienceV1::experience(company, employee, community, channel)
    }
    .unwrap()
}

fn source() -> EmployeeMemorySourceV1 {
    EmployeeMemorySourceV1::new(
        id("22222222-2222-4222-8222-222222222222"),
        id("44444444-4444-4444-8444-444444444444"),
        MessageId::from_bytes([0xaa; 32]),
        at("2026-09-06T00:01:02.123456Z"),
        key(0xcc),
        EmployeeMemoryDigest::from_bytes([0xdd; 32]),
    )
    .unwrap()
}

fn approval() -> EmployeeSharingApprovalV1 {
    EmployeeSharingApprovalV1::new(
        id("55555555-5555-4555-8555-555555555555"),
        key(0xbb),
        EmployeeMemoryDigest::from_bytes([0xee; 32]),
        at("2026-09-07T00:01:02.123456Z"),
    )
    .unwrap()
}

fn provenance(relationship: bool) -> EmployeeMemoryProvenanceV1 {
    EmployeeMemoryProvenanceV1::new(audience(relationship), source(), approval()).unwrap()
}

fn vectors() -> Vec<Value> {
    serde_json::from_str(include_str!("test_vectors.json")).unwrap()
}

#[test]
fn literal_bytes_and_sha256_bind_both_employee_kinds() {
    // Expected JSON strings were authored independently; only SHA-256 was
    // computed over those literal bytes. No parallel canonical serializer.
    for vector in vectors() {
        let value = provenance(vector["kind"] == "relationship");
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
            value.sharing_hash().unwrap().to_hex(),
            vector["sharing_sha256"]
        );
        assert_eq!(
            EmployeeMemoryAudienceV1::from_canonical_bytes(audience_bytes).unwrap(),
            *value.audience()
        );
        assert_eq!(
            EmployeeMemoryProvenanceV1::from_canonical_bytes(provenance_bytes).unwrap(),
            value
        );
    }
}

#[test]
fn audience_source_and_edited_review_have_separate_hash_boundaries() {
    let original = provenance(false);
    let s = source();
    let changed_source = EmployeeMemorySourceV1::new(
        s.community_id(),
        s.channel_id(),
        MessageId::from_bytes([0xab; 32]),
        s.event_created_at(),
        s.author_public_key(),
        s.evidence_hash(),
    )
    .unwrap();
    let from_other_source =
        EmployeeMemoryProvenanceV1::new(audience(false), changed_source, approval()).unwrap();
    assert_eq!(
        original.audience().audience_hash().unwrap(),
        from_other_source.audience().audience_hash().unwrap()
    );
    assert_ne!(
        original.source_hash().unwrap(),
        from_other_source.source_hash().unwrap()
    );
    let a = approval();
    let edited = EmployeeSharingApprovalV1::new(
        a.approval_id(),
        a.approved_by(),
        EmployeeMemoryDigest::from_bytes([0xef; 32]),
        a.expires_at(),
    )
    .unwrap();
    let edited = EmployeeMemoryProvenanceV1::new(audience(false), source(), edited).unwrap();
    assert_eq!(
        original.source_hash().unwrap(),
        edited.source_hash().unwrap()
    );
    assert_ne!(
        original.sharing_hash().unwrap(),
        edited.sharing_hash().unwrap()
    );
    let base = audience(false);
    for changed in [
        EmployeeMemoryAudienceV1::experience(
            Uuid::from_bytes([1; 16]),
            base.employee_id().clone(),
            base.destination_community_id(),
            base.destination_channel_id(),
        )
        .unwrap(),
        EmployeeMemoryAudienceV1::experience(
            base.company_id(),
            EmployeeId::parse("another").unwrap(),
            base.destination_community_id(),
            base.destination_channel_id(),
        )
        .unwrap(),
        EmployeeMemoryAudienceV1::experience(
            base.company_id(),
            base.employee_id().clone(),
            Uuid::from_bytes([2; 16]),
            base.destination_channel_id(),
        )
        .unwrap(),
        EmployeeMemoryAudienceV1::experience(
            base.company_id(),
            base.employee_id().clone(),
            base.destination_community_id(),
            Uuid::from_bytes([3; 16]),
        )
        .unwrap(),
        audience(true),
    ] {
        assert_ne!(
            base.audience_hash().unwrap(),
            changed.audience_hash().unwrap()
        );
    }
    let different_human = EmployeeMemoryAudienceV1::relationship(
        base.company_id(),
        base.employee_id().clone(),
        base.destination_community_id(),
        base.destination_channel_id(),
        key(0xbc),
    )
    .unwrap();
    assert_ne!(
        audience(true).audience_hash().unwrap(),
        different_human.audience_hash().unwrap()
    );
}

#[test]
fn constructors_refuse_inconsistent_human_community_identity_and_precision() {
    let a = approval();
    let other_human = EmployeeSharingApprovalV1::new(
        a.approval_id(),
        key(0xbc),
        a.content_hash(),
        a.expires_at(),
    )
    .unwrap();
    assert_eq!(
        EmployeeMemoryProvenanceV1::new(audience(true), source(), other_human),
        Err(EmployeeMemoryError::InconsistentProvenance)
    );
    let s = source();
    let other_community = EmployeeMemorySourceV1::new(
        Uuid::from_bytes([2; 16]),
        s.channel_id(),
        s.event_id(),
        s.event_created_at(),
        s.author_public_key(),
        s.evidence_hash(),
    )
    .unwrap();
    assert_eq!(
        EmployeeMemoryProvenanceV1::new(audience(false), other_community, approval()),
        Err(EmployeeMemoryError::InconsistentProvenance)
    );
    assert_eq!(
        EmployeeMemoryAudienceV1::experience(
            Uuid::nil(),
            EmployeeId::parse("ada").unwrap(),
            s.community_id(),
            s.channel_id()
        ),
        Err(EmployeeMemoryError::InvalidIdentity)
    );
    assert_eq!(
        EmployeeSharingApprovalV1::new(
            Uuid::nil(),
            a.approved_by(),
            a.content_hash(),
            a.expires_at()
        ),
        Err(EmployeeMemoryError::InvalidIdentity)
    );
    assert_eq!(
        EmployeeMemorySourceV1::new(
            s.community_id(),
            s.channel_id(),
            s.event_id(),
            at("2026-09-06T00:01:02.123456001Z"),
            s.author_public_key(),
            s.evidence_hash()
        ),
        Err(EmployeeMemoryError::InvalidTimestamp)
    );
}

#[test]
fn audience_parser_refuses_broadening_duplicates_noncanonical_and_oversize() {
    let vector = &vectors()[1];
    let original = vector["audience_utf8"].as_str().unwrap();
    for invalid in [
        original.replacen('{', "{\"project_id\":null,", 1),
        original.replacen('{', "{\"kind\":\"relationship\",", 1),
        format!(" {original}"),
        original.replace(&"bb".repeat(32), &"BB".repeat(32)),
        original.replace("\"relationship\"", "\"employee_private\""),
        original.replace(&format!("\"{}\"", "bb".repeat(32)), "null"),
        original.replace("\"human_public_key\":", "\"unqualified_human\":"),
    ] {
        assert!(EmployeeMemoryAudienceV1::from_canonical_bytes(invalid.as_bytes()).is_err());
    }
    let oversized = original.replace(
        &"bb".repeat(32),
        &"b".repeat(MAX_EMPLOYEE_MEMORY_AUDIENCE_BYTES + 1),
    );
    assert_eq!(
        EmployeeMemoryAudienceV1::from_canonical_bytes(oversized.as_bytes()),
        Err(EmployeeMemoryError::InvalidWire),
        "bound runs before the invalid digest parser"
    );
    let missing_null = vectors()[0]["audience_utf8"]
        .as_str()
        .unwrap()
        .replace("\"human_public_key\":null,", "");
    assert!(EmployeeMemoryAudienceV1::from_canonical_bytes(missing_null.as_bytes()).is_err());
}

#[test]
fn provenance_parser_checks_hashes_nested_shape_and_time_independent_history() {
    let vector = &vectors()[1];
    let original = vector["provenance_utf8"].as_str().unwrap();
    for field in ["audience_hash", "source_hash"] {
        let mut forged: Value = serde_json::from_str(original).unwrap();
        forged[field] = Value::String("00".repeat(32));
        assert_eq!(
            EmployeeMemoryProvenanceV1::from_canonical_bytes(&serde_json::to_vec(&forged).unwrap()),
            Err(EmployeeMemoryError::InconsistentProvenance)
        );
    }
    for invalid in [
        original.replacen("\"source\":{", "\"source\":{\"run_id\":null,", 1),
        original.replacen(
            "\"approval\":{",
            "\"approval\":{\"content_hash\":\"duplicate\",",
            1,
        ),
        original.replace("ortak-reviewed-employee-sharing/1", "private-retention/1"),
        original.replace(
            "2026-09-06T00:01:02.123456Z",
            "2026-09-06T03:01:02.123456+03:00",
        ),
        format!("{original}\n"),
    ] {
        assert!(EmployeeMemoryProvenanceV1::from_canonical_bytes(invalid.as_bytes()).is_err());
    }
    let oversized = original.replace(
        "2026-09-07T00:01:02.123456Z",
        &"x".repeat(MAX_EMPLOYEE_MEMORY_PROVENANCE_BYTES + 1),
    );
    assert_eq!(
        EmployeeMemoryProvenanceV1::from_canonical_bytes(oversized.as_bytes()),
        Err(EmployeeMemoryError::InvalidWire),
        "bound runs before the invalid timestamp parser"
    );
    let old = EmployeeSharingApprovalV1::new(
        approval().approval_id(),
        key(0xbb),
        approval().content_hash(),
        at("2000-01-01T00:00:00.000000Z"),
    )
    .unwrap();
    let history = EmployeeMemoryProvenanceV1::new(audience(true), source(), old).unwrap();
    assert_eq!(
        EmployeeMemoryProvenanceV1::from_canonical_bytes(&history.canonical_bytes().unwrap())
            .unwrap(),
        history
    );
}

#[test]
fn legacy_memory_scope_bytes_remain_unqualified_and_unchanged() {
    assert_eq!(
        [MemoryScope::EmployeeExperience, MemoryScope::Relationship]
            .iter()
            .map(|scope| serde_json::to_string(scope).unwrap())
            .collect::<Vec<_>>(),
        [
            r#"{"scope":"employee_experience"}"#,
            r#"{"scope":"relationship"}"#
        ]
        .map(str::to_owned),
    );
    assert!(
        EmployeeMemoryAudienceV1::from_canonical_bytes(br#"{"scope":"relationship"}"#).is_err()
    );
}
