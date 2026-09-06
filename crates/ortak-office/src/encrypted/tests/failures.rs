use super::*;

#[test]
fn outer_signature_id_kind_and_recipient_cannot_be_replaced() {
    let f = Fixture::new();
    let outer = f.wrap(f.rumor("outer canary").as_json().as_bytes());
    let key = DmDecryptKey::for_recipient(&f.employee, f.employee.public_key()).unwrap();
    for (field, value, error) in [
        ("sig", json!("0".repeat(128)), DecodeError::Signature),
        ("id", json!("ab".repeat(32)), DecodeError::Signature),
        ("kind", json!(1), DecodeError::Kind),
    ] {
        let mut forged = serde_json::to_value(&outer).unwrap();
        forged[field] = value;
        assert_eq!(
            decode(
                &key,
                &f.expected(&outer),
                &serde_json::to_vec(&forged).unwrap()
            )
            .err(),
            Some(error)
        );
    }
    let seal = f.seal(
        f.rumor("foreign recipient").as_json().as_bytes(),
        Kind::Seal,
        &f.human,
    );
    assert_eq!(
        f.decode(&f.wrap_seal(seal.as_json().as_bytes(), f.other.public_key()))
            .err(),
        Some(DecodeError::Recipient)
    );
}

#[test]
fn real_signed_seal_requires_exact_kind_signature_and_human() {
    let f = Fixture::new();
    let rumor = f.rumor("seal canary").as_json();
    let wrong_kind = f.seal(rumor.as_bytes(), Kind::TextNote, &f.human);
    assert_eq!(
        f.decode(&f.wrap_seal(wrong_kind.as_json().as_bytes(), f.employee.public_key()))
            .err(),
        Some(DecodeError::Kind)
    );
    let wrong_sender = f.seal(rumor.as_bytes(), Kind::Seal, &f.other);
    assert_eq!(
        f.decode(&f.wrap_seal(wrong_sender.as_json().as_bytes(), f.employee.public_key()))
            .err(),
        Some(DecodeError::Sender)
    );
    let seal = f.seal(rumor.as_bytes(), Kind::Seal, &f.human);
    let mut invalid = serde_json::to_value(&seal).unwrap();
    invalid["sig"] = json!("0".repeat(128));
    assert_eq!(
        f.decode(&f.wrap_seal(
            &serde_json::to_vec(&invalid).unwrap(),
            f.employee.public_key()
        ))
        .err(),
        Some(DecodeError::Signature)
    );
    let tagged = EventBuilder::new(Kind::Seal, seal.content)
        .tags([Tag::public_key(f.employee.public_key())])
        .sign_with_keys(&f.human)
        .unwrap();
    assert_eq!(
        f.decode(&f.wrap_seal(tagged.as_json().as_bytes(), f.employee.public_key()))
            .err(),
        Some(DecodeError::Tags)
    );
}

#[test]
fn rumor_id_sender_kind_and_single_canonical_recipient_are_verified() {
    let f = Fixture::new();
    let original = serde_json::to_value(f.rumor("inner canary")).unwrap();
    for (field, value, error) in [
        ("id", json!("cd".repeat(32)), DecodeError::RumorId),
        (
            "pubkey",
            json!(f.other.public_key().to_hex()),
            DecodeError::Sender,
        ),
        ("kind", json!(1), DecodeError::Kind),
        (
            "tags",
            json!([["p", f.other.public_key().to_hex()]]),
            DecodeError::Recipient,
        ),
        (
            "tags",
            json!([
                ["p", f.employee.public_key().to_hex()],
                ["p", f.employee.public_key().to_hex()]
            ]),
            DecodeError::Tags,
        ),
        (
            "tags",
            json!([["p", f.employee.public_key().to_hex().to_uppercase()]]),
            DecodeError::Encoding,
        ),
        ("tags", json!([]), DecodeError::Recipient),
    ] {
        let mut forged = original.clone();
        forged[field] = value;
        f.refused_rumor(&serde_json::to_vec(&forged).unwrap(), error);
    }
    let mut absent = original;
    absent.as_object_mut().unwrap().remove("id");
    f.refused_rumor(&serde_json::to_vec(&absent).unwrap(), DecodeError::Encoding);
}

#[test]
fn every_layer_refuses_unknown_and_duplicate_json_without_error_content() {
    let f = Fixture::new();
    let rumor = f.rumor("must-not-appear-in-an-error").as_json();
    for changed in [
        rumor.replacen('{', "{\"unknown\":true,", 1),
        rumor.replacen('{', "{\"content\":\"must-not-appear-in-an-error\",", 1),
        rumor.replacen('{', "{\"sig\":null,", 1),
    ] {
        f.refused_rumor(changed.as_bytes(), DecodeError::Encoding);
    }
    let seal = f.seal(rumor.as_bytes(), Kind::Seal, &f.human).as_json();
    for changed in [
        seal.replacen('{', "{\"extra\":0,", 1),
        seal.replacen('{', "{\"kind\":13,", 1),
    ] {
        assert_eq!(
            f.decode(&f.wrap_seal(changed.as_bytes(), f.employee.public_key()))
                .err(),
            Some(DecodeError::Encoding)
        );
    }
    let outer = f.wrap(rumor.as_bytes());
    for changed in [
        outer.as_json().replacen('{', "{\"extra\":0,", 1),
        outer.as_json().replacen('{', "{\"kind\":1059,", 1),
    ] {
        let error = decode(
            &DmDecryptKey::for_recipient(&f.employee, f.employee.public_key()).unwrap(),
            &f.expected(&outer),
            changed.as_bytes(),
        )
        .err()
        .unwrap();
        assert_eq!(error, DecodeError::Encoding);
        assert!(!format!("{error:?} {error}").contains("must-not-appear-in-an-error"));
    }
}

#[test]
fn positional_arrays_are_rejected_at_all_three_layers_but_object_whitespace_is_valid() {
    let f = Fixture::new();
    let rumor = f.rumor("strict object canary");
    let rumor_array = json!([
        rumor.id,
        rumor.pubkey,
        rumor.created_at,
        rumor.kind,
        &rumor.tags,
        &rumor.content,
    ]);
    let seal = f.seal(rumor.as_json().as_bytes(), Kind::Seal, &f.human);
    let seal_array = json!([
        seal.id,
        seal.pubkey,
        seal.created_at,
        seal.kind,
        &seal.tags,
        &seal.content,
        seal.sig,
    ]);
    let outer = f.wrap_seal(seal.as_json().as_bytes(), f.employee.public_key());
    let outer_array = json!([
        outer.id,
        outer.pubkey,
        outer.created_at,
        outer.kind,
        &outer.tags,
        &outer.content,
        outer.sig,
    ]);
    let key = DmDecryptKey::for_recipient(&f.employee, f.employee.public_key()).unwrap();
    // All signed/hashed fields remain unchanged. Only the JSON container changes;
    // the inner arrays are encrypted and wrapped by the real pinned primitives.
    for prefix in ["", " \t\r\n"] {
        assert_eq!(
            decode(
                &key,
                &f.expected(&outer),
                format!("{prefix}{outer_array}").as_bytes()
            )
            .err(),
            Some(DecodeError::Encoding),
        );
        assert_eq!(
            f.decode(&f.wrap_seal(
                format!("{prefix}{seal_array}").as_bytes(),
                f.employee.public_key()
            ))
            .err(),
            Some(DecodeError::Encoding),
        );
        f.refused_rumor(
            format!("{prefix}{rumor_array}").as_bytes(),
            DecodeError::Encoding,
        );
    }

    let rumor_object = format!(" \t\r\n{}", rumor.as_json());
    let seal_object = format!(
        " \t\r\n{}",
        f.seal(rumor_object.as_bytes(), Kind::Seal, &f.human)
            .as_json()
    );
    let outer = f.wrap_seal(seal_object.as_bytes(), f.employee.public_key());
    let outer_object = format!(" \t\r\n{}", outer.as_json());
    let verified = decode(&key, &f.expected(&outer), outer_object.as_bytes()).unwrap();
    assert_eq!(verified.text(), rumor.content);
    assert_eq!(verified.rumor_bytes(), rumor_object.as_bytes());
}
