use super::*;

use ortak_domain::{CredentialRef, EmployeeId};
use ortak_office::transport::{
    EnvOfficeSigner, HttpOfficePublisher, OfficeRelayBinding, OfficeSignerBinding,
};
use ortak_office::{OfficePublishError, OfficePublisher};

const EPHEMERAL_SECRET_ENV: &str = "ORTAK_OFFICE_HOST_TEST_SECRET";

#[tokio::test]
#[ignore = "requires Postgres"]
async fn configured_origin_must_match_the_live_community_before_signing_or_http() {
    // EnvOfficeSigner intentionally has no arbitrary-secret public constructor.
    // Supply an ephemeral secret only to this exact child test instead of
    // mutating the shared process environment after test threads have started.
    if std::env::var_os(EPHEMERAL_SECRET_ENV).is_none() {
        let keys = nostr::Keys::generate();
        let mut child = tokio::process::Command::new(std::env::current_exe().expect("test binary"));
        child
            .args([
                "--exact",
                "transport::configured_origin_must_match_the_live_community_before_signing_or_http",
                "--ignored",
                "--test-threads=1",
            ])
            .env(EPHEMERAL_SECRET_ENV, keys.secret_key().to_secret_hex())
            .kill_on_drop(true);
        let status = tokio::time::timeout(Duration::from_secs(60), child.status())
            .await
            .expect("bounded host-guard child")
            .expect("start host-guard child");
        assert!(status.success(), "host-guard child failed");
        return;
    }

    let fixture = Fixture::new().await;
    let authorized = fixture.enqueue().await;
    let lease = fixture.claim(Duration::from_secs(30)).await;
    let event = fixture
        .signer
        .sign(&authorized.signing_request(Utc::now()).expect("request"))
        .await
        .expect("sign canonical event");
    assert!(matches!(
        fixture
            .control
            .freeze_signed_event(&fixture.scope, &lease, &event)
            .await
            .expect("freeze event"),
        FreezeOutcome::Frozen(_)
    ));

    let secret = std::env::var(EPHEMERAL_SECRET_ENV).expect("child-only secret");
    let keys = nostr::Keys::new(nostr::SecretKey::from_hex(&secret).expect("ephemeral secret"));
    let public_key = OfficePublicKey::parse_hex(&keys.public_key().to_hex()).expect("public key");
    assert_ne!(public_key, fixture.public_key());
    let signer = EnvOfficeSigner::from_env(vec![OfficeSignerBinding {
        company_id: fixture.scope.company_id(),
        employee_id: EmployeeId::parse("cem").expect("employee"),
        signer_ref: CredentialRef::parse(SIGNER_REF).expect("signer reference"),
        public_key,
        secret_env: EPHEMERAL_SECRET_ENV.to_owned(),
    }])
    .expect("load isolated signer");
    let (community_id, host): (Uuid, String) = sqlx::query_as(
        "SELECT c.id,c.host FROM communities c JOIN office_company_bindings b ON b.community_id=c.id WHERE b.company_id=$1",
    )
    .bind(fixture.scope.company_id())
    .fetch_one(&fixture.pool)
    .await
    .expect("live community host");

    // The configured signer deliberately holds a different key. Correct host
    // authority reaches signing and stops there; neither branch can send HTTP.
    // Removing the host predicate makes the mismatched case return that same
    // signer error, so this test is bound to the production SQL guard.
    for (origin, expected_code) in [
        (format!("https://{host}"), "office_signer_unavailable"),
        (
            "https://misconfigured-office.example".to_owned(),
            "office_authority_unavailable",
        ),
    ] {
        let publisher = HttpOfficePublisher::new(
            fixture.control.clone(),
            signer.clone(),
            vec![OfficeRelayBinding {
                company_id: fixture.scope.company_id(),
                community_id,
                origin,
            }],
            Duration::from_secs(1),
        )
        .expect("publisher");
        assert!(matches!(
            publisher.publish(&fixture.scope, &event).await,
            Err(OfficePublishError::Rejected { detail }) if detail.as_str() == expected_code
        ));
    }
    let row = fixture.row(authorized.outbox_id()).await;
    assert_eq!(row.state, "pending");
    assert_eq!(
        row.signed_event_bytes.as_deref(),
        Some(event.signed_bytes())
    );
}
