use super::*;
use serde_json::{json, Value};

fn vector() -> Value {
    serde_json::from_str(include_str!("vector.json")).unwrap()
}
fn expected(f: &Value, name: &str) -> Vec<u8> {
    f["expected"][name].as_str().unwrap().as_bytes().to_vec()
}

#[test]
fn confidential_literal_canonical_identity_header_and_envelope() {
    let f = vector();
    let identity_bytes = expected(&f, "identity_utf8");
    let identity = ValidatedIdentity::parse(&identity_bytes).unwrap();
    assert_eq!(identity.canonical_bytes(), identity_bytes);
    assert_eq!(
        hex::encode(identity.sha256()),
        f["expected"]["identity_sha256_hex"].as_str().unwrap()
    );
    let header = PayloadHeader::new(&identity, PayloadPurpose::Snapshot, 0, 20).unwrap();
    assert_eq!(header.aad(), expected(&f, "aad_utf8"));
    let bytes = expected(&f, "envelope_utf8");
    let envelope = ConfidentialEnvelope::parse(&bytes).unwrap();
    assert_eq!(envelope.canonical_bytes(), bytes);
    assert_eq!(
        hex::encode(envelope.ciphertext()),
        f["expected"]["ciphertext_hex"].as_str().unwrap()
    );
    assert_eq!(
        hex::encode(envelope.nonce()),
        f["nonce_hex"].as_str().unwrap()
    );
}

#[test]
fn confidential_identity_rejects_ambiguous_or_noncanonical_claims() {
    let f = vector();
    let bytes = expected(&f, "identity_utf8");
    for (field, value) in [
        ("authority_epoch", json!(3)),
        ("authority_epoch", json!("03")),
        ("key_version", json!("-1")),
        ("key_version", json!("9223372036854775808")),
        ("company_id", json!("11111111111141118111111111111111")),
        ("run_id", json!("00000000-0000-0000-0000-000000000000")),
        ("employee_id", json!("Ada")),
        ("employee_id", json!("a".repeat(65))),
        (
            "human_public_key",
            f["identity"]["employee_public_key"].clone(),
        ),
        ("source_outer_id", json!("D".repeat(64))),
        (
            "source_outer_created_at",
            json!("2026-09-06T03:00:00.000000+03:00"),
        ),
        (
            "source_outer_created_at",
            json!("2026-09-06T00:00:00.000001Z"),
        ),
        (
            "source_outer_created_at",
            json!("2016-12-31T23:59:60.000000Z"),
        ),
        (
            "source_outer_created_at",
            json!("1969-12-31T23:59:59.000000Z"),
        ),
        ("unknown", json!("ignored")),
    ] {
        let mut changed = f["identity"].clone();
        changed[field] = value;
        assert!(
            ValidatedIdentity::parse(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "{field}"
        );
    }
    let text = String::from_utf8(bytes.clone()).unwrap();
    let duplicate = text.replacen('{', "{\"authority_epoch\":\"3\",", 1);
    let escaped = text.replace("fixture-employee", "fixture\\u002demployee");
    let array = serde_json::to_vec(
        &f["identity"]
            .as_object()
            .unwrap()
            .values()
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for changed in [
        duplicate.into_bytes(),
        escaped.into_bytes(),
        array,
        [b" ".as_slice(), &bytes].concat(),
        [bytes.as_slice(), b"\n"].concat(),
        vec![b'x'; MAX_HEADER_BYTES + 1],
        vec![0xff],
    ] {
        assert!(ValidatedIdentity::parse(&changed).is_err());
    }
}

#[test]
fn confidential_envelope_rejects_shape_encoding_lengths_and_ordinals() {
    let f = vector();
    let bytes = expected(&f, "envelope_utf8");
    let base: Value = serde_json::from_slice(&bytes).unwrap();
    for (field, value) in [
        ("algorithm", json!("A128GCM")),
        ("format", json!("ortak-confidential-payload/2")),
        ("purpose", json!("other")),
        ("ordinal", json!(1)),
        ("ordinal", json!(true)),
        ("ordinal", json!(0.0)),
        ("plaintext_bytes", json!(49153)),
        ("unknown", json!(null)),
    ] {
        let mut changed = base.clone();
        changed["header"][field] = value;
        assert!(
            ConfidentialEnvelope::parse(&serde_json::to_vec(&changed).unwrap()).is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("nonce", json!("ICEiIyQlJicoKSor=")),
        ("ciphertext", json!("x".repeat(96 * 1024))),
        ("ciphertext", json!("_".repeat(48))),
        ("unknown", json!("ignored")),
    ] {
        let mut changed = base.clone();
        changed[field] = value;
        assert!(ConfidentialEnvelope::parse(&serde_json::to_vec(&changed).unwrap()).is_err());
    }
    for target in ["envelope", "header", "identity"] {
        let mut changed = base.clone();
        match target {
            "envelope" => changed = json!(base.as_object().unwrap().values().collect::<Vec<_>>()),
            "header" => {
                changed["header"] = json!(base["header"]
                    .as_object()
                    .unwrap()
                    .values()
                    .collect::<Vec<_>>())
            }
            _ => {
                changed["header"]["identity"] = json!(f["identity"]
                    .as_object()
                    .unwrap()
                    .values()
                    .collect::<Vec<_>>())
            }
        }
        assert!(ConfidentialEnvelope::parse(&serde_json::to_vec(&changed).unwrap()).is_err());
    }
    let duplicate = String::from_utf8(bytes.clone()).unwrap().replacen(
        "\"ordinal\":0",
        "\"ordinal\":0,\"ordinal\":0",
        1,
    );
    assert!(ConfidentialEnvelope::parse(duplicate.as_bytes()).is_err());
    assert!(ConfidentialEnvelope::parse(&vec![b'x'; MAX_ENVELOPE_BYTES + 1]).is_err());
    let identity = ValidatedIdentity::parse(&expected(&f, "identity_utf8")).unwrap();
    let empty = ConfidentialEnvelope::from_parts(
        PayloadHeader::new(&identity, PayloadPurpose::Snapshot, 0, 0).unwrap(),
        [0; 12],
        vec![0; 16],
    )
    .unwrap();
    let nonzero_pad_bits = String::from_utf8(empty.canonical_bytes().to_vec())
        .unwrap()
        .replace("AAAAAAAAAAAAAAAAAAAAAA==", "AAAAAAAAAAAAAAAAAAAAAB==");
    assert!(ConfidentialEnvelope::parse(nonzero_pad_bits.as_bytes()).is_err());
    for (purpose, ordinal) in [
        (PayloadPurpose::Snapshot, 0),
        (PayloadPurpose::ReplyDraft, 0),
        (PayloadPurpose::RuntimeEvent, 1),
        (PayloadPurpose::RuntimeEvent, 512),
    ] {
        assert!(
            PayloadHeader::new(&identity, purpose, ordinal, purpose.max_plaintext_bytes()).is_ok()
        );
        assert!(PayloadHeader::new(
            &identity,
            purpose,
            ordinal,
            purpose.max_plaintext_bytes() + 1
        )
        .is_err());
    }
    assert!(PayloadHeader::new(&identity, PayloadPurpose::RuntimeEvent, 0, 0).is_err());
    assert!(PayloadHeader::new(&identity, PayloadPurpose::RuntimeEvent, 513, 0).is_err());
}
