use super::super::{DmOfficeKeyBinding, OfficeSignerBinding};
use super::*;
use nostr::{nips::nip59::UnwrappedGift, Keys, SecretKey};
use ortak_control::{confidential::ValidatedIdentity, office_identity::OfficePublicKey};
use ortak_domain::{CredentialRef, EmployeeId};
use serde_json::{json, Value};

fn fixture() -> (EnvDmKeyProvider, DmKeySelection, Keys, Keys) {
    let employee = Keys::new(SecretKey::from_slice(&[0x31; 32]).unwrap());
    let human = Keys::new(SecretKey::from_slice(&[0x32; 32]).unwrap());
    let vector: Value = serde_json::from_str(include_str!(
        "../../../../../ortak-control/src/confidential/vector.json"
    ))
    .unwrap();
    let mut id = vector["identity"].clone();
    id["employee_public_key"] = json!(employee.public_key().to_hex());
    id["human_public_key"] = json!(human.public_key().to_hex());
    let identity = ValidatedIdentity::parse(&serde_json::to_vec(&id).unwrap()).unwrap();
    let claims = identity.key_claims();
    let reference = CredentialRef::parse("secret://office/selected-operation").unwrap();
    let binding = DmOfficeKeyBinding {
        signer: OfficeSignerBinding {
            company_id: claims.company_id.parse().unwrap(),
            employee_id: EmployeeId::parse(claims.employee_id).unwrap(),
            signer_ref: reference.clone(),
            public_key: OfficePublicKey::parse_hex(claims.employee_public_key).unwrap(),
            secret_env: "ORTAK_SYNTHETIC_SELECTED_OPERATION".into(),
        },
        office_binding_id: claims.office_binding_id.parse().unwrap(),
        key_version: 2,
        purposes: vec![OfficeKeyPurpose::DmSeal],
    };
    (
        EnvDmKeyProvider::new(vec![binding]).unwrap(),
        DmKeySelection::from_expected_claims(&identity, reference),
        employee,
        human,
    )
}
#[tokio::test]
async fn selected_seal_produces_one_rumor_with_two_verified_recipient_copies() {
    let (provider, selection, employee, human) = fixture();
    let reply = provider
        .seal_with_reader(&selection, "exact protected reply\nİ 🧭", |name| {
            assert_eq!(name, "ORTAK_SYNTHETIC_SELECTED_OPERATION");
            Ok(employee.secret_key().to_secret_hex())
        })
        .await
        .unwrap();
    let a = nostr::Event::from_json(reply.copies()[0].bytes()).unwrap();
    let b = nostr::Event::from_json(reply.copies()[1].bytes()).unwrap();
    assert_ne!(a.id, b.id);
    a.verify().unwrap();
    b.verify().unwrap();
    let recipient = UnwrappedGift::from_gift_wrap(&human, &a).await.unwrap();
    let history = UnwrappedGift::from_gift_wrap(&employee, &b).await.unwrap();
    assert_eq!(recipient.rumor, history.rumor);
    assert_eq!(recipient.rumor.id.unwrap().to_bytes(), *reply.rumor_id());
    assert_eq!(recipient.rumor.pubkey, employee.public_key());
    assert_eq!(recipient.rumor.content, "exact protected reply\nİ 🧭");
    assert!(provider
        .validate_reply_copy(&selection, 0, reply.copies()[0].bytes())
        .is_ok());
    assert!(provider
        .validate_reply_copy(&selection, 1, reply.copies()[1].bytes())
        .is_ok());
    assert!(provider
        .validate_reply_copy(&selection, 1, reply.copies()[0].bytes())
        .is_err());
}
#[tokio::test]
async fn selected_seal_refuses_wrong_purpose_reference_and_key_without_fallback() {
    let (mut provider, selection, employee, _) = fixture();
    provider.bindings[0].purposes = vec![OfficeKeyPurpose::WrapMaster];
    let mut reads = 0;
    assert!(provider
        .seal_with_reader(&selection, "text", |_| {
            reads += 1;
            Ok(employee.secret_key().to_secret_hex())
        })
        .await
        .is_err());
    assert_eq!(reads, 0);
    provider.bindings[0].purposes = vec![OfficeKeyPurpose::DmSeal];
    let changed = DmKeySelection::from_expected_claims(
        &selection.identity,
        CredentialRef::parse("secret://office/different").unwrap(),
    );
    assert!(provider
        .seal_with_reader(&changed, "text", |_| {
            reads += 1;
            Ok(employee.secret_key().to_secret_hex())
        })
        .await
        .is_err());
    assert_eq!(reads, 0);
    assert!(provider
        .seal_with_reader(&selection, "text", |_| Ok("32".repeat(32)))
        .await
        .is_err());
}
