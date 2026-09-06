use super::*;
use serde_json::Value;

fn vector() -> Value {
    serde_json::from_str(include_str!(
        "../../../ortak-control/src/confidential/vector.json"
    ))
    .unwrap()
}
fn expected<'a>(f: &'a Value, name: &str) -> &'a str {
    f["expected"][name].as_str().unwrap()
}
fn key(f: &Value) -> ConfidentialMasterKey {
    ConfidentialMasterKey::from_owned(Zeroizing::new(
        hex::decode(f["master_hex"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap(),
    ))
}

#[test]
fn confidential_hkdf_rfc5869_and_aes256_gcm_nist_anchors() {
    // RFC5869 SHA256 test case 1, independent of our identity/header contract.
    let mut okm = [0u8; 42];
    hkdf_into(
        &[0x0b; 22],
        &hex::decode("000102030405060708090a0b0c").unwrap(),
        &hex::decode("f0f1f2f3f4f5f6f7f8f9").unwrap(),
        &mut okm,
    )
    .unwrap();
    assert_eq!(
        hex::encode(okm),
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
    );
    // NIST CAVS gcmEncryptExtIV256.rsp, also retained in aes-gcm0.10.3 tests.
    let key: [u8; 32] =
        hex::decode("b52c505a37d78eda5dd34f20c22540ea1b58963cf8e5bf8ffa85f9f2492505b4")
            .unwrap()
            .try_into()
            .unwrap();
    let nonce: [u8; 12] = hex::decode("516c33929df5a3284ff463d7")
        .unwrap()
        .try_into()
        .unwrap();
    assert_eq!(
        hex::encode(encrypt(&key, &nonce, b"", b"").unwrap()),
        "bdc1ac884d332457a1d2664f168c76f0"
    );
}

#[test]
fn confidential_shared_literal_vector_binds_kdf_aad_ciphertext_and_open() {
    let f = vector();
    let master = key(&f);
    let identity = ValidatedIdentity::parse(expected(&f, "identity_utf8").as_bytes()).unwrap();
    let plaintext = hex::decode(f["plaintext_hex"].as_str().unwrap()).unwrap();
    let nonce: [u8; 12] = hex::decode(f["nonce_hex"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let header =
        PayloadHeader::new(&identity, PayloadPurpose::Snapshot, 0, plaintext.len()).unwrap();
    assert_eq!(header.aad(), expected(&f, "aad_utf8").as_bytes());
    let derived = derive(&master, &identity, PayloadPurpose::Snapshot).unwrap();
    assert_eq!(hex::encode(&derived[..]), expected(&f, "derived_key_hex"));
    let sealed = seal_inner(&master, header, &plaintext, nonce).unwrap();
    assert_eq!(
        sealed.canonical_bytes(),
        expected(&f, "envelope_utf8").as_bytes()
    );
    let parsed = ConfidentialEnvelope::parse(expected(&f, "envelope_utf8").as_bytes()).unwrap();
    assert_eq!(
        open(&master, &identity, PayloadPurpose::Snapshot, 0, &parsed)
            .unwrap()
            .as_bytes(),
        plaintext
    );
}

#[test]
fn confidential_open_refuses_identity_purpose_ordinal_key_nonce_and_tag_changes() {
    let f = vector();
    let master = key(&f);
    let identity = ValidatedIdentity::parse(expected(&f, "identity_utf8").as_bytes()).unwrap();
    let parsed = ConfidentialEnvelope::parse(expected(&f, "envelope_utf8").as_bytes()).unwrap();
    let altered = expected(&f, "identity_utf8")
        .replace("\"authority_epoch\":\"3\"", "\"authority_epoch\":\"4\"");
    let other = ValidatedIdentity::parse(altered.as_bytes()).unwrap();
    assert!(open(&master, &other, PayloadPurpose::Snapshot, 0, &parsed).is_err());
    assert!(open(&master, &identity, PayloadPurpose::ReplyDraft, 0, &parsed).is_err());
    assert!(open(&master, &identity, PayloadPurpose::Snapshot, 1, &parsed).is_err());
    let wrong = ConfidentialMasterKey::from_owned(Zeroizing::new([0; 32]));
    assert!(matches!(
        open(&wrong, &identity, PayloadPurpose::Snapshot, 0, &parsed),
        Err(ConfidentialCryptoError::Authentication)
    ));
    let mut ciphertext = parsed.ciphertext().to_vec();
    *ciphertext.last_mut().unwrap() ^= 1;
    let changed =
        ConfidentialEnvelope::from_parts(parsed.header().clone(), *parsed.nonce(), ciphertext)
            .unwrap();
    assert!(matches!(
        open(&master, &identity, PayloadPurpose::Snapshot, 0, &changed),
        Err(ConfidentialCryptoError::Authentication)
    ));
    let mut nonce = *parsed.nonce();
    nonce[0] ^= 1;
    let changed = ConfidentialEnvelope::from_parts(
        parsed.header().clone(),
        nonce,
        parsed.ciphertext().to_vec(),
    )
    .unwrap();
    assert!(matches!(
        open(&master, &identity, PayloadPurpose::Snapshot, 0, &changed),
        Err(ConfidentialCryptoError::Authentication)
    ));
    // Even when expected claims match a modified header, its unchanged tag fails.
    let header = PayloadHeader::new(&other, PayloadPurpose::Snapshot, 0, 20).unwrap();
    let changed =
        ConfidentialEnvelope::from_parts(header, *parsed.nonce(), parsed.ciphertext().to_vec())
            .unwrap();
    assert!(matches!(
        open(&master, &other, PayloadPurpose::Snapshot, 0, &changed),
        Err(ConfidentialCryptoError::Authentication)
    ));
}

#[test]
fn confidential_fresh_seals_are_bounded_and_do_not_accept_caller_nonces() {
    let f = vector();
    let master = key(&f);
    let identity = ValidatedIdentity::parse(expected(&f, "identity_utf8").as_bytes()).unwrap();
    let text = "  confidential\nİstanbul \\ \u{2028}\0  ".as_bytes();
    let first = seal(&master, &identity, PayloadPurpose::RuntimeEvent, 1, text).unwrap();
    let second = seal(&master, &identity, PayloadPurpose::RuntimeEvent, 1, text).unwrap();
    assert_ne!(first.nonce(), second.nonce());
    assert_eq!(
        open(&master, &identity, PayloadPurpose::RuntimeEvent, 1, &first)
            .unwrap()
            .as_bytes(),
        text
    );
    // The codec preserves arbitrary bounded bytes. DM inner text validation is
    // still a separate requirement; this cannot admit NUL into a run by itself.
    for purpose in [
        PayloadPurpose::Snapshot,
        PayloadPurpose::RuntimeEvent,
        PayloadPurpose::ReplyDraft,
    ] {
        let ordinal = u32::from(purpose == PayloadPurpose::RuntimeEvent);
        let full = vec![0xff; purpose.max_plaintext_bytes()];
        let sealed = seal(&master, &identity, purpose, ordinal, &full).unwrap();
        assert_eq!(
            open(&master, &identity, purpose, ordinal, &sealed)
                .unwrap()
                .as_bytes(),
            full
        );
        assert!(
            seal(
                &master,
                &identity,
                purpose,
                ordinal,
                &vec![0; full.len() + 1]
            )
            .is_err()
        );
    }
}
