use base64::{engine::general_purpose::STANDARD, Engine as _};
use nostr::nips::nip44;
use ortak_control::office_identity::OfficePublicKey;
use ortak_domain::EmployeeId;
use serde_json::{json, Value};
use std::cell::Cell;

use super::*;

struct Fixture {
    keys: Keys,
    binding: DmOfficeKeyBinding,
    selection: DmKeySelection,
    master: Zeroizing<[u8; 32]>,
}

impl Fixture {
    fn new() -> Self {
        let keys = Keys::generate();
        let vector: Value = serde_json::from_str(include_str!(
            "../../../../ortak-control/src/confidential/vector.json"
        ))
        .unwrap();
        let mut identity = vector["identity"].clone();
        identity["employee_public_key"] = json!(keys.public_key().to_hex());
        let identity = ValidatedIdentity::parse(&serde_json::to_vec(&identity).unwrap()).unwrap();
        let signer_ref = CredentialRef::parse("secret://office/dm-key-fixture").unwrap();
        let claims = identity.key_claims();
        let binding = DmOfficeKeyBinding {
            signer: OfficeSignerBinding {
                company_id: Uuid::parse_str(claims.company_id).unwrap(),
                employee_id: EmployeeId::parse(claims.employee_id).unwrap(),
                signer_ref: signer_ref.clone(),
                public_key: OfficePublicKey::parse_hex(claims.employee_public_key).unwrap(),
                secret_env: "ORTAK_DM_KEY_FIXTURE_SELECTED".into(),
            },
            office_binding_id: Uuid::parse_str(claims.office_binding_id).unwrap(),
            key_version: 2,
            purposes: vec![OfficeKeyPurpose::WrapMaster, OfficeKeyPurpose::UnwrapMaster],
        };
        Self {
            keys,
            binding,
            selection: DmKeySelection::from_expected_claims(&identity, signer_ref),
            master: Zeroizing::new([0x53; 32]),
        }
    }

    fn provider(&self) -> EnvDmKeyProvider {
        EnvDmKeyProvider::new(vec![self.binding.clone()]).unwrap()
    }

    fn read(&self, name: &str) -> Result<String, DmKeyError> {
        assert_eq!(name, self.binding.signer.secret_env);
        Ok(self.keys.secret_key().to_secret_hex())
    }

    fn wrap(&self) -> WrappedMasterKey {
        self.provider()
            .wrap_with_reader(&self.selection, &self.master, |name| self.read(name))
            .unwrap()
    }

    fn changed(&self, field: &str, value: Value) -> DmKeySelection {
        let mut identity: Value =
            serde_json::from_slice(self.selection.identity.canonical_bytes()).unwrap();
        identity[field] = value;
        let identity = ValidatedIdentity::parse(&serde_json::to_vec(&identity).unwrap()).unwrap();
        DmKeySelection::from_expected_claims(&identity, self.selection.signer_ref.clone())
    }

    fn inner(&self, wrapped: &WrappedMasterKey) -> Value {
        let bytes = Zeroizing::new(
            nip44::decrypt_to_bytes(
                self.keys.secret_key(),
                &self.keys.public_key(),
                &wrapped.ciphertext,
            )
            .unwrap(),
        );
        serde_json::from_slice(&bytes).unwrap()
    }

    // Malformed test input is encrypted by pinned NIP-44, then handed to the
    // production parser/provider. No alternate key-envelope parser is used.
    fn with_inner(&self, wrapped: &WrappedMasterKey, bytes: &[u8]) -> WrappedMasterKey {
        let mut outer: Value = serde_json::from_slice(wrapped.canonical_bytes()).unwrap();
        outer["ciphertext"] = json!(nip44::encrypt(
            self.keys.secret_key(),
            &self.keys.public_key(),
            bytes,
            nip44::Version::V2,
        )
        .unwrap());
        WrappedMasterKey::parse(&serde_json::to_vec(&outer).unwrap()).unwrap()
    }
}

#[test]
fn self_wrapping_uses_only_exact_selected_key_and_preserves_owned_master() {
    let f = Fixture::new();
    let provider = f.provider();
    let reads = Cell::new(0);
    let mut read = |name: &str| {
        reads.set(reads.get() + 1);
        f.read(name)
    };
    let wrapped = provider
        .wrap_with_reader(&f.selection, &f.master, &mut read)
        .unwrap();
    let saved = wrapped.canonical_bytes().to_vec();
    let parsed = WrappedMasterKey::parse(&saved).unwrap();
    let opened = provider
        .unwrap_with_reader(&f.selection, &parsed, &mut read)
        .unwrap()
        .into_owned();
    assert_eq!(opened.as_slice(), f.master.as_slice());
    assert_eq!(parsed.canonical_bytes(), saved);
    assert_eq!(reads.get(), 2);
    assert!(!String::from_utf8(saved)
        .unwrap()
        .contains(&STANDARD.encode(f.master.as_slice())));
    assert_ne!(wrapped.canonical_bytes(), f.wrap().canonical_bytes());
    let inner = f.inner(&wrapped);
    assert_eq!(
        inner["identity"].as_str().unwrap().as_bytes(),
        f.selection.identity.canonical_bytes()
    );
    assert_eq!(
        inner["identity_hash"],
        hex::encode(f.selection.identity.sha256())
    );

    // Changing a runtime revision does not relabel/select another Office owner.
    // This pure operation still requires the future caller's current authority.
    let changed = f.changed("employee_revision_id", json!(Uuid::new_v4().to_string()));
    let new = provider
        .wrap_with_reader(&changed, &f.master, |name| f.read(name))
        .unwrap();
    assert_eq!(
        provider
            .unwrap_with_reader(&changed, &new, |name| f.read(name))
            .unwrap()
            .into_owned()
            .as_slice(),
        f.master.as_slice()
    );
}

#[test]
fn whole_allowlist_and_closed_purposes_are_validated_without_key_reads() {
    let f = Fixture::new();
    assert_eq!(
        EnvDmKeyProvider::new(vec![]).err(),
        Some(DmKeyError::Configuration)
    );
    assert_eq!(
        EnvDmKeyProvider::new(vec![f.binding.clone(); 65]).err(),
        Some(DmKeyError::Configuration)
    );
    let mut cases = vec![f.binding.clone(); 5];
    cases[0].signer.secret_env = "bad-name".into();
    cases[1].office_binding_id = Uuid::nil();
    cases[2].key_version = i64::MAX as u64 + 1;
    cases[3].purposes = vec![];
    cases[4].purposes = vec![OfficeKeyPurpose::WrapMaster; 2];
    for invalid in cases {
        assert_eq!(
            EnvDmKeyProvider::new(vec![invalid]).err(),
            Some(DmKeyError::Configuration)
        );
    }
    assert_eq!(
        EnvDmKeyProvider::new(vec![f.binding.clone(); 2]).err(),
        Some(DmKeyError::Configuration)
    );
    for unsupported in [
        "dm_decrypt",
        "dm_seal",
        "sign",
        "runtime_key",
        "confidential_master",
    ] {
        assert!(serde_json::from_value::<OfficeKeyPurpose>(json!(unsupported)).is_err());
    }
    let mut second = f.binding.clone();
    second.office_binding_id = Uuid::new_v4();
    second.signer.secret_env = "ORTAK_DM_KEY_OTHER".into();
    assert_eq!(
        EnvDmKeyProvider::new(vec![f.binding.clone(), second]).err(),
        Some(DmKeyError::Configuration)
    );
    let mut same_owner = f.binding.clone();
    same_owner.signer.public_key =
        OfficePublicKey::parse_hex(&Keys::generate().public_key().to_hex()).unwrap();
    same_owner.signer.secret_env = "ORTAK_DM_KEY_OTHER".into();
    assert_eq!(
        EnvDmKeyProvider::new(vec![f.binding.clone(), same_owner]).err(),
        Some(DmKeyError::Configuration)
    );
    let mut same_env = f.binding.clone();
    same_env.office_binding_id = Uuid::new_v4();
    same_env.signer.public_key =
        OfficePublicKey::parse_hex(&Keys::generate().public_key().to_hex()).unwrap();
    assert_eq!(
        EnvDmKeyProvider::new(vec![f.binding.clone(), same_env]).err(),
        Some(DmKeyError::Configuration)
    );
}

#[test]
fn purpose_and_full_expected_identity_are_checked_before_key_resolution() {
    let f = Fixture::new();
    let wrapped = f.wrap();
    let never = |_name: &str| -> Result<String, DmKeyError> { panic!("refusal resolved a key") };
    let mut binding = f.binding.clone();
    binding.purposes = vec![OfficeKeyPurpose::UnwrapMaster];
    assert_eq!(
        EnvDmKeyProvider::new(vec![binding])
            .unwrap()
            .wrap_with_reader(&f.selection, &f.master, never)
            .err(),
        Some(DmKeyError::Refused)
    );
    let mut binding = f.binding.clone();
    binding.purposes = vec![OfficeKeyPurpose::WrapMaster];
    assert_eq!(
        EnvDmKeyProvider::new(vec![binding])
            .unwrap()
            .unwrap_with_reader(&f.selection, &wrapped, never)
            .err(),
        Some(DmKeyError::Refused)
    );

    for (field, value) in [
        ("company_id", json!(Uuid::new_v4().to_string())),
        ("community_id", json!(Uuid::new_v4().to_string())),
        ("conversation_id", json!(Uuid::new_v4().to_string())),
        ("employee_id", json!("other-employee")),
        ("employee_revision_id", json!(Uuid::new_v4().to_string())),
        ("employee_lifecycle_epoch", json!("2")),
        ("office_binding_id", json!(Uuid::new_v4().to_string())),
        (
            "employee_public_key",
            json!(Keys::generate().public_key().to_hex()),
        ),
        ("key_version", json!("3")),
        ("key_id", json!(Uuid::new_v4().to_string())),
        ("run_id", json!(Uuid::new_v4().to_string())),
        ("authority_epoch", json!("4")),
        ("source_outer_id", json!("f".repeat(64))),
        (
            "source_outer_created_at",
            json!("2026-09-06T00:00:01.000000Z"),
        ),
        ("source_evidence_hash", json!("f".repeat(64))),
        ("rumor_id", json!("f".repeat(64))),
        (
            "human_public_key",
            json!(Keys::generate().public_key().to_hex()),
        ),
    ] {
        let changed = f.changed(field, value);
        assert_eq!(
            f.provider()
                .unwrap_with_reader(&changed, &wrapped, never)
                .err(),
            Some(DmKeyError::Refused),
            "{field}"
        );
    }
    let changed = DmKeySelection::from_expected_claims(
        &f.selection.identity,
        CredentialRef::parse("secret://office/other").unwrap(),
    );
    assert_eq!(
        f.provider()
            .unwrap_with_reader(&changed, &wrapped, never)
            .err(),
        Some(DmKeyError::Refused)
    );
    assert_eq!(
        f.provider()
            .wrap_with_reader(&changed, &f.master, never)
            .err(),
        Some(DmKeyError::Refused)
    );
}

#[test]
fn authenticated_inner_rejects_retagged_identity_reference_and_purpose() {
    let f = Fixture::new();
    let wrapped = f.wrap();
    let original = f.inner(&wrapped);
    let mut malformed = Vec::new();
    for (field, value) in [
        ("format", json!("ortak-confidential-key/2")),
        ("purpose", json!("snapshot")),
        ("identity", json!("{}")),
        ("identity_hash", json!("0".repeat(64))),
        ("key_id", json!(Uuid::new_v4().to_string())),
        ("signer_ref", json!("secret://office/other")),
        ("master_key", json!("!".repeat(44))),
        ("master_key", json!(STANDARD.encode([0u8; 31]))),
        ("unknown", json!(true)),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        malformed.push(serde_json::to_vec(&changed).unwrap());
    }
    let text = serde_json::to_string(&original).unwrap();
    malformed.push(format!(" {text}").into_bytes());
    malformed.push(
        text.replacen('{', "{\"purpose\":\"confidential_master\",", 1)
            .into_bytes(),
    );
    malformed.push(
        serde_json::to_vec(&original.as_object().unwrap().values().collect::<Vec<_>>()).unwrap(),
    );
    malformed.push(vec![b'x'; MAX_KEY_PLAINTEXT_BYTES + 1]);
    for bytes in malformed {
        let forged = f.with_inner(&wrapped, &bytes);
        assert_eq!(
            f.provider()
                .unwrap_with_reader(&f.selection, &forged, |name| f.read(name))
                .err(),
            Some(DmKeyError::Authentication)
        );
    }

    // Even with the same actual material available under a separately selected
    // new ref, retagging only outer metadata cannot authenticate that ref.
    let new_ref = CredentialRef::parse("secret://office/relabelled").unwrap();
    let mut outer: Value = serde_json::from_slice(wrapped.canonical_bytes()).unwrap();
    outer["signer_ref"] = json!(new_ref.as_str());
    let retagged = WrappedMasterKey::parse(&serde_json::to_vec(&outer).unwrap()).unwrap();
    let selection = DmKeySelection::from_expected_claims(&f.selection.identity, new_ref.clone());
    let mut binding = f.binding.clone();
    binding.signer.signer_ref = new_ref;
    assert_eq!(
        EnvDmKeyProvider::new(vec![binding])
            .unwrap()
            .unwrap_with_reader(&selection, &retagged, |name| f.read(name))
            .err(),
        Some(DmKeyError::Authentication)
    );

    let changed = f.changed("employee_revision_id", json!(Uuid::new_v4().to_string()));
    let mut outer: Value = serde_json::from_slice(wrapped.canonical_bytes()).unwrap();
    outer["identity"] = json!(std::str::from_utf8(changed.identity.canonical_bytes()).unwrap());
    let retagged = WrappedMasterKey::parse(&serde_json::to_vec(&outer).unwrap()).unwrap();
    assert_eq!(
        f.provider()
            .unwrap_with_reader(&changed, &retagged, |name| f.read(name))
            .err(),
        Some(DmKeyError::Authentication)
    );
}

#[test]
fn wrapped_wire_is_bounded_canonical_and_object_only_before_key_read() {
    let f = Fixture::new();
    let wrapped = f.wrap();
    let value: Value = serde_json::from_slice(wrapped.canonical_bytes()).unwrap();
    let mut malformed = vec![vec![b'{'; MAX_WRAPPED_MASTER_BYTES + 1]];
    for (field, replacement) in [
        ("format", json!("ortak-confidential-key-envelope/2")),
        ("purpose", json!("runtime_event")),
        ("identity", json!("[]")),
        ("signer_ref", json!("not-a-ref")),
        (
            "ciphertext",
            json!("A".repeat(MAX_KEY_CIPHERTEXT_BYTES + 1)),
        ),
        ("ciphertext", json!("A".repeat(128))),
        ("ciphertext", json!("!".repeat(132))),
        ("unknown", json!(true)),
    ] {
        let mut changed = value.clone();
        changed[field] = replacement;
        malformed.push(serde_json::to_vec(&changed).unwrap());
    }
    let text = std::str::from_utf8(wrapped.canonical_bytes()).unwrap();
    malformed.push(format!(" {text}").into_bytes());
    malformed.push(
        text.replacen('{', "{\"purpose\":\"confidential_master\",", 1)
            .into_bytes(),
    );
    malformed.push(
        serde_json::to_vec(&value.as_object().unwrap().values().collect::<Vec<_>>()).unwrap(),
    );
    let mut raw = STANDARD.decode(&wrapped.ciphertext).unwrap();
    raw[0] = 1;
    let mut changed = value.clone();
    changed["ciphertext"] = json!(STANDARD.encode(raw));
    malformed.push(serde_json::to_vec(&changed).unwrap());
    for bytes in malformed {
        assert_eq!(
            WrappedMasterKey::parse(&bytes).err(),
            Some(DmKeyError::Envelope)
        );
    }
}

#[test]
fn selected_material_failures_are_closed_and_do_not_try_another_key() {
    let f = Fixture::new();
    let provider = f.provider();
    let wrong = Keys::generate();
    for (material, expected) in [
        (Err(DmKeyError::Authentication), DmKeyError::Unavailable),
        (
            Ok("malformed secret canary".into()),
            DmKeyError::KeyMismatch,
        ),
        (Ok("z".repeat(64)), DmKeyError::KeyMismatch),
        (
            Ok(wrong.secret_key().to_secret_hex()),
            DmKeyError::KeyMismatch,
        ),
    ] {
        let reads = Cell::new(0);
        let outcome = provider.wrap_with_reader(&f.selection, &f.master, |name| {
            assert_eq!(name, f.binding.signer.secret_env);
            reads.set(reads.get() + 1);
            material.clone()
        });
        let error = outcome.err().unwrap();
        assert_eq!(error, expected);
        assert_eq!(reads.get(), 1);
        assert!(!error.to_string().contains("canary"));
        assert!(!format!("{error:?}").contains(&f.keys.secret_key().to_secret_hex()));
    }
}
