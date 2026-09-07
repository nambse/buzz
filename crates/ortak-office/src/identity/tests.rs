use super::*;
use crate::transport::OfficeSignerBinding;
use nostr::JsonUtil;
use sqlx::postgres::PgPoolOptions;

#[path = "tests/fixture.rs"]
mod fixture;
#[path = "tests/postgres.rs"]
mod postgres_tests;

fn configuration() -> (OfficeIdentityConfig, EnvOfficeSigner) {
    let keys = nostr::Keys::generate();
    let company_id = Uuid::new_v4();
    let employee_id = EmployeeId::parse("identity-test").unwrap();
    let public_key = OfficePublicKey::parse_hex(&keys.public_key().to_hex()).unwrap();
    let signer_ref = CredentialRef::parse("credential://office/identity-test").unwrap();
    let channel = Uuid::new_v4();
    let signer = EnvOfficeSigner::load(
        vec![OfficeSignerBinding {
            company_id,
            employee_id: employee_id.clone(),
            public_key,
            signer_ref: signer_ref.clone(),
            secret_env: "ORTAK_IDENTITY_TEST_KEY".to_owned(),
        }],
        |_| Ok(keys.secret_key().to_secret_hex()),
    )
    .unwrap();
    (
        OfficeIdentityConfig {
            company_id,
            community_id: Uuid::new_v4(),
            origin: "http://127.0.0.1:39999".to_owned(),
            employees: vec![OfficeIdentityEmployee {
                employee_id,
                office: OfficeBinding {
                    public_key: public_key.to_hex(),
                    signer_ref,
                    home_channel_ref: Some(channel.to_string()),
                },
                channels: vec![channel],
            }],
        },
        signer,
    )
}

fn adapter(config: OfficeIdentityConfig, signer: EnvOfficeSigner) -> PgOfficeIdentityAdapter {
    // Lazy pool has no connection. Tests below prove fail-closed refusals and
    // cryptographic operations without consulting any ambient infrastructure.
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/unused")
        .unwrap();
    PgOfficeIdentityAdapter::new(
        PgControlPlane::new(pool),
        signer,
        config,
        Duration::from_secs(1),
    )
    .unwrap()
}

#[test]
fn complete_public_configuration_is_closed_and_canonical() {
    let (config, _) = configuration();
    config.validate().unwrap();
    let mut invalid = Vec::new();
    let mut copy = config.clone();
    copy.company_id = Uuid::nil();
    invalid.push(copy);
    let mut copy = config.clone();
    copy.community_id = Uuid::nil();
    invalid.push(copy);
    for origin in [
        "https://office.example/",
        "http://office.example",
        "https://user:pass@office.example",
        "https://office.example/path",
        "https://office.example?x=1",
    ] {
        let mut copy = config.clone();
        copy.origin = origin.to_owned();
        invalid.push(copy);
    }
    let mut copy = config.clone();
    copy.employees.clear();
    invalid.push(copy);
    let mut copy = config.clone();
    copy.employees.push(copy.employees[0].clone());
    invalid.push(copy);
    let mut copy = config.clone();
    copy.employees[0].channels.clear();
    invalid.push(copy);
    let mut copy = config.clone();
    copy.employees[0].channels.push(Uuid::nil());
    invalid.push(copy);
    let mut copy = config.clone();
    let duplicate = copy.employees[0].channels[0];
    copy.employees[0].channels.push(duplicate);
    invalid.push(copy);
    let mut copy = config.clone();
    copy.employees[0].channels = (0..65).map(|_| Uuid::new_v4()).collect();
    invalid.push(copy);
    let mut copy = config.clone();
    copy.employees[0].office.home_channel_ref = Some("unresolved-alias".to_owned());
    invalid.push(copy);
    let mut copy = config.clone();
    copy.employees[0].office.home_channel_ref = Some(Uuid::new_v4().to_string());
    invalid.push(copy);
    let mut copy = config.clone();
    copy.employees[0].office.public_key = "bad-key".to_owned();
    invalid.push(copy);
    for copy in invalid {
        assert!(copy.validate().is_err());
    }
    for field in ["employee_id", "public_key", "signer_ref"] {
        let mut copy = config.clone();
        let (other, _) = configuration();
        let mut second = other.employees[0].clone();
        second.employee_id = EmployeeId::parse("second-employee").unwrap();
        second.office.signer_ref = CredentialRef::parse("credential://office/second").unwrap();
        match field {
            "employee_id" => second.employee_id = copy.employees[0].employee_id.clone(),
            "public_key" => second.office.public_key = copy.employees[0].office.public_key.clone(),
            _ => second.office.signer_ref = copy.employees[0].office.signer_ref.clone(),
        }
        copy.employees.push(second);
        assert!(copy.validate().is_err(), "duplicate {field}");
    }
}

#[tokio::test]
async fn constructor_requires_the_exact_company_employee_signer_tuple() {
    let (config, signer) = configuration();
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://invalid@127.0.0.1:1/unused")
        .unwrap();
    for mutate in 0..4 {
        let mut wrong = config.clone();
        match mutate {
            0 => wrong.company_id = Uuid::new_v4(),
            1 => wrong.employees[0].employee_id = EmployeeId::parse("foreign").unwrap(),
            2 => {
                wrong.employees[0].office.signer_ref =
                    CredentialRef::parse("credential://foreign/key").unwrap()
            }
            _ => {
                wrong.employees[0].office.public_key = nostr::Keys::generate().public_key().to_hex()
            }
        }
        assert!(matches!(
            PgOfficeIdentityAdapter::new(
                PgControlPlane::new(pool.clone()),
                signer.clone(),
                wrong,
                Duration::from_secs(1)
            ),
            Err(OfficeTransportConfigError::SecretMismatch)
        ));
    }
    for timeout in [
        Duration::ZERO,
        Duration::from_nanos(1),
        Duration::from_secs(31),
    ] {
        assert!(PgOfficeIdentityAdapter::new(
            PgControlPlane::new(pool.clone()),
            signer.clone(),
            config.clone(),
            timeout
        )
        .is_err());
    }
}

#[tokio::test]
async fn signer_proof_and_unsupported_lifecycle_bind_the_production_port() {
    let (config, signer) = configuration();
    let entry = config.employees[0].clone();
    let expected = OfficePublicKey::parse_hex(&entry.office.public_key).unwrap();
    let adapter = adapter(config, signer);
    let proof = adapter
        .verify_signer(&entry.office.signer_ref, &expected)
        .await
        .unwrap();
    assert!(proof.matches_expected);
    assert_eq!(proof.produced_public_key, expected);
    assert!(adapter
        .verify_signer(
            &CredentialRef::parse("credential://foreign/key").unwrap(),
            &expected
        )
        .await
        .is_err());
    let wrong = OfficePublicKey::parse_hex(&nostr::Keys::generate().public_key().to_hex()).unwrap();
    assert!(adapter
        .verify_signer(&entry.office.signer_ref, &wrong)
        .await
        .is_err());
    let request = OfficeMembershipRequest {
        employee_id: entry.employee_id.clone(),
        binding: entry.office.clone(),
        mode: ProvisioningMode::Create,
        idempotency_key: "create-key".to_owned(),
    };
    assert!(matches!(adapter.ensure_membership(&request).await,
        Err(OfficeIdentityError::Rejected {detail}) if detail.as_str()=="office_membership_create_unsupported"));
    assert!(adapter
        .remove_created_membership("adopted", "delete-key")
        .await
        .is_err());
    let mut foreign = request.clone();
    foreign.employee_id = EmployeeId::parse("foreign").unwrap();
    assert!(adapter.ensure_membership(&foreign).await.is_err());
    assert!(adapter.membership_health(&wrong).await.is_err());
    assert!(adapter
        .publish_profile(&entry.employee_id, &entry.office, "secret\nname", "key")
        .await
        .is_err());
    assert!(adapter
        .publish_profile(&entry.employee_id, &entry.office, "Name", "bad/key")
        .await
        .is_err());
}

#[tokio::test]
async fn profile_verification_rejects_foreign_signed_fields_and_receipt_tampering() {
    let (config, signer) = configuration();
    let adapter = adapter(config, signer);
    let entry = &adapter.config.employees[0];
    let profile = adapter.sign_profile(entry, "Ada", 1_780_000_000).unwrap();
    adapter.validate_profile(entry, "Ada", &profile).unwrap();
    assert!(adapter
        .validate_profile(entry, "Different", &profile)
        .is_err());
    let event = nostr::Event::from_json(&profile.bytes).unwrap();
    assert_eq!(event.kind.as_u16(), 0);
    assert_eq!(event.pubkey.to_hex(), entry.office.public_key);
    for kind in [9, 27235] {
        let altered = nostr::UnsignedEvent::new(
            event.pubkey,
            event.created_at,
            nostr::Kind::from_u16(kind),
            [],
            event.content.clone(),
        )
        .sign_with_keys(adapter.keys(entry).unwrap())
        .unwrap();
        let invalid = profile::FrozenProfile {
            event_id: altered.id.to_hex(),
            bytes: altered.as_json().into_bytes(),
            acknowledged: false,
        };
        assert!(adapter.validate_profile(entry, "Ada", &invalid).is_err());
    }
    let mut invalid = profile;
    invalid.event_id = "00".repeat(32);
    assert!(adapter.validate_profile(entry, "Ada", &invalid).is_err());
    invalid.bytes = vec![b'x'; 16_385];
    assert!(adapter.validate_profile(entry, "Ada", &invalid).is_err());
}

#[tokio::test]
async fn channel_configuration_order_does_not_change_the_profile_request_hash() {
    let (mut config, signer) = configuration();
    config.employees[0].channels.push(Uuid::new_v4());
    let first = adapter(config.clone(), signer.clone());
    config.employees[0].channels.reverse();
    let second = adapter(config, signer);
    assert_eq!(
        first.profile_hash(&first.config.employees[0], "Ada"),
        second.profile_hash(&second.config.employees[0], "Ada")
    );
}
