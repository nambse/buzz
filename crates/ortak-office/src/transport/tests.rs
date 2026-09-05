use super::*;
use crate::event::{OfficePublishIntent, UnsignedOfficeEvent};

fn identity() -> (OfficeSignerBinding, nostr::Keys) {
    let keys = nostr::Keys::generate();
    (
        OfficeSignerBinding {
            company_id: Uuid::new_v4(),
            employee_id: EmployeeId::parse("transport-test").unwrap(),
            signer_ref: CredentialRef::parse("secret://office/transport-test").unwrap(),
            public_key: OfficePublicKey::parse_hex(&keys.public_key().to_hex()).unwrap(),
            secret_env: "ORTAK_OFFICE_TRANSPORT_TEST_KEY".to_owned(),
        },
        keys,
    )
}

fn signer(binding: &OfficeSignerBinding, keys: &nostr::Keys) -> EnvOfficeSigner {
    EnvOfficeSigner::load(vec![binding.clone()], |_| {
        Ok(keys.secret_key().to_secret_hex())
    })
    .unwrap()
}

fn request(binding: &OfficeSignerBinding) -> SigningRequest {
    let intent = OfficePublishIntent {
        company_id: binding.company_id,
        run_id: Uuid::new_v4(),
        employee_id: binding.employee_id.clone(),
        employee_revision_id: Uuid::new_v4(),
        kind: 9,
        tags: vec![vec!["h".to_owned(), Uuid::new_v4().to_string()]],
        content: "Transport bytes: café\nsecond line".to_owned(),
    };
    SigningRequest::new(
        UnsignedOfficeEvent::new(intent, binding.public_key, chrono::Utc::now()).unwrap(),
        binding.signer_ref.clone(),
    )
}

#[tokio::test]
async fn signer_binds_company_employee_reference_and_expected_public_key() {
    let (binding, keys) = identity();
    let signer = signer(&binding, &keys);
    let correct = request(&binding);
    let event = signer.sign(&correct).await.unwrap();
    assert_eq!(event.public_key(), &binding.public_key);
    let mut wrong = binding.clone();
    wrong.company_id = Uuid::new_v4();
    assert!(signer.sign(&request(&wrong)).await.is_err());
    wrong = binding.clone();
    wrong.employee_id = EmployeeId::parse("other").unwrap();
    assert!(signer.sign(&request(&wrong)).await.is_err());
    wrong = binding.clone();
    wrong.signer_ref = CredentialRef::parse("secret://office/other").unwrap();
    assert!(signer.sign(&request(&wrong)).await.is_err());
    wrong = binding.clone();
    wrong.public_key = identity().0.public_key;
    assert!(signer.sign(&request(&wrong)).await.is_err());
}

#[tokio::test]
async fn each_retry_authenticates_exact_frozen_body_with_a_fresh_nip98_event() {
    let (binding, keys) = identity();
    let signer = signer(&binding, &keys);
    let frozen = signer.sign(&request(&binding)).await.unwrap();
    let url = "https://office.example.test/events";
    let first = signer
        .authorization(&frozen, &binding.signer_ref, url)
        .unwrap();
    let second = signer
        .authorization(&frozen, &binding.signer_ref, url)
        .unwrap();
    assert_ne!(first, second);
    for header in [first, second] {
        let json = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(header.strip_prefix("Nostr ").unwrap())
                .unwrap(),
        )
        .unwrap();
        let verified =
            buzz_auth::verify_nip98_event(&json, url, "POST", Some(frozen.signed_bytes())).unwrap();
        assert_eq!(verified, keys.public_key());
        assert!(buzz_auth::verify_nip98_event(&json, url, "POST", Some(b"{}")).is_err());
    }
}

#[test]
fn configuration_rejects_wrong_secret_duplicate_identity_and_noncanonical_origins() {
    let (binding, _) = identity();
    let other = nostr::Keys::generate();
    let error = EnvOfficeSigner::load(vec![binding.clone()], |_| {
        Ok(other.secret_key().to_secret_hex())
    })
    .err()
    .unwrap();
    assert_eq!(
        error.to_string(),
        "configured Office signing secret does not match its public identity"
    );
    assert!(
        EnvOfficeSigner::load(vec![binding.clone(), binding], |_| Ok(other
            .secret_key()
            .to_secret_hex()))
        .is_err()
    );
    for origin in [
        "http://public.example.test",
        "https://user:password@office.example.test",
        "https://office.example.test/",
        "https://office.example.test?x=1",
        "https://office.example.test/path",
        "file:///tmp",
    ] {
        assert!(!valid_origin(origin));
    }
    for origin in [
        "https://office.example.test",
        "http://127.0.0.1:3000",
        "http://localhost:3000",
        "http://[::1]:3000",
    ] {
        assert!(valid_origin(origin));
    }
    assert!(!valid_env_name("OTHER_KEY"));
}

#[test]
fn relay_acknowledgement_requires_matching_id_and_explicit_acceptance() {
    assert_eq!(
        receipt(
            br#"{"event_id":"id","accepted":true,"message":"duplicate:"}"#,
            "id"
        )
        .unwrap(),
        PublishReceipt::AlreadyPresent
    );
    assert!(receipt(
        br#"{"event_id":"other","accepted":true,"message":""}"#,
        "id"
    )
    .is_err());
    assert!(receipt(
        br#"{"event_id":"id","accepted":false,"message":"duplicate:"}"#,
        "id"
    )
    .is_err());
    assert!(receipt(b"not-json", "id").is_err());
}

#[tokio::test]
async fn publisher_rejects_cross_company_before_database_or_http() {
    let (binding, keys) = identity();
    let signer = signer(&binding, &keys);
    let event = signer.sign(&request(&binding)).await.unwrap();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    let control = PgControlPlane::new(pool);
    let publisher = HttpOfficePublisher::new(
        control,
        signer,
        vec![OfficeRelayBinding {
            company_id: binding.company_id,
            community_id: Uuid::new_v4(),
            origin: "https://office.example.test".to_owned(),
        }],
        Duration::from_secs(1),
    )
    .unwrap();
    let foreign = ortak_control::fakes::InMemoryProvisioningRepository::new().scope();
    assert_eq!(
        publisher
            .publish(&foreign, &event)
            .await
            .unwrap_err()
            .to_string(),
        "office rejected the event: office_route_unconfigured"
    );
}

#[tokio::test]
#[ignore = "requires an explicitly permitted loopback socket; no live relay"]
async fn real_http_retry_sends_identical_frozen_bytes_with_new_auth() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (binding, keys) = identity();
    let signer = signer(&binding, &keys);
    let event = signer.sign(&request(&binding)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let expected = event.signed_bytes().to_vec();
    let id = event.event_id().to_hex();
    let url = format!("{origin}/events");
    let server = tokio::spawn(async move {
        let mut auths = Vec::new();
        for attempt in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let (header_end, length) = loop {
                let mut chunk = [0u8; 4096];
                let count = stream.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
                assert!(bytes.len() < 300_000);
                if let Some(end) = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .unwrap()
                        .parse()
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        break (end, length);
                    }
                }
            };
            let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
            assert!(headers.starts_with("POST /events HTTP/1.1"));
            assert_eq!(&bytes[header_end + 4..header_end + 4 + length], expected);
            let auth = headers
                .lines()
                .find_map(|line| line.strip_prefix("authorization: "))
                .unwrap();
            let json = String::from_utf8(
                base64::engine::general_purpose::STANDARD
                    .decode(auth.strip_prefix("Nostr ").unwrap())
                    .unwrap(),
            )
            .unwrap();
            buzz_auth::verify_nip98_event(&json, &url, "POST", Some(&expected)).unwrap();
            auths.push(json);
            let body = serde_json::json!({"event_id":id,"accepted":true,"message":if attempt == 0 { "" } else { "duplicate:" }}).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        assert_ne!(auths[0], auths[1]);
    });
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    let publisher = HttpOfficePublisher::new(
        PgControlPlane::new(pool),
        signer,
        vec![OfficeRelayBinding {
            company_id: binding.company_id,
            community_id: Uuid::new_v4(),
            origin: origin.clone(),
        }],
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        publisher
            .send(&event, &binding.signer_ref, &origin)
            .await
            .unwrap(),
        PublishReceipt::Accepted
    );
    assert_eq!(
        publisher
            .send(&event, &binding.signer_ref, &origin)
            .await
            .unwrap(),
        PublishReceipt::AlreadyPresent
    );
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
}
