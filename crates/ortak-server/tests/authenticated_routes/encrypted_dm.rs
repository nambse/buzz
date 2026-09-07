//! Signed production HTTP authority over a canonical disposable pair. Requires
//! immutable76 plus encrypted jobs/admission candidates; no crypto/key resolver.
use super::*;
use ortak_control::{office_identity::OfficePublicKey, CompanyScope};
use ortak_domain::{CredentialRef, EmployeeManifest, EmployeeStatus, PermissionPolicy};
use ortak_office::encrypted::jobs::{ConfiguredDmPair, PgDecryptJobs};

#[path = "../../../ortak-control/tests/direct_channel_support.rs"]
mod direct_support;

struct EncryptedFixture {
    f: Fixture,
    scope: CompanyScope,
    channel: Uuid,
    pair: ConfiguredDmPair,
    jobs: PgDecryptJobs,
    app: Router,
}

impl EncryptedFixture {
    async fn new() -> Self {
        let f = Fixture::new().await;
        // Reuse the server's company/human fixture and the canonical employee
        // fixture manifest, with the same complete validated binding shape as
        // runtime Fixture::new_for_employee. No production guard is bypassed.
        let mut employee = serde_yaml::from_str::<EmployeeManifest>(include_str!(
            "../../../../config/employees/cem.yaml"
        ))
        .unwrap()
        .employee;
        employee.status = EmployeeStatus::Active;
        employee.permissions = PermissionPolicy::default();
        employee.runtime.adapter = "fake-runtime".into();
        employee.runtime.profile_ref = Some("fake://encrypted-authority/profile".into());
        employee.runtime.credential_refs.clear();
        employee.office.public_key = Keys::generate().public_key().to_hex();
        employee.office.signer_ref =
            CredentialRef::parse("credential://synthetic/encrypted-authority-office").unwrap();
        employee.memory.as_mut().unwrap().adapter = "fake-memory".into();
        let revision = Uuid::new_v4();
        let manifest = serde_json::to_value(&employee).unwrap();
        let mut tx = f.pool.begin().await.unwrap();
        sqlx::query("INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode) VALUES($1,$2,'cem',2,$3,$4,'adopt')")
            .bind(f.company).bind(revision).bind(&manifest).bind(Sha256::digest(manifest.to_string().as_bytes()).to_vec()).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO employee_runtime_bindings(company_id,revision_id,employee_id,adapter,provisioning_mode,profile_ref,model,workspace_ref,credential_refs,options,validated_at) VALUES($1,$2,'cem',$3,'adopt',$4,$5,$6,$7,$8,clock_timestamp())")
            .bind(f.company).bind(revision).bind(&employee.runtime.adapter).bind(&employee.runtime.profile_ref).bind(&employee.runtime.model).bind(&employee.runtime.workspace_ref)
            .bind(json!(employee.runtime.credential_refs)).bind(json!(employee.runtime.options)).execute(&mut *tx).await.unwrap();
        let memory = employee.memory.as_ref().unwrap();
        sqlx::query("INSERT INTO employee_memory_bindings(company_id,revision_id,employee_id,adapter,provisioning_mode,endpoint_ref,workspace,user_peer,employee_peer,options,validated_at) VALUES($1,$2,'cem',$3,'adopt',$4,$5,$6,$7,$8,clock_timestamp())")
            .bind(f.company).bind(revision).bind(&memory.adapter).bind(&memory.endpoint_ref).bind(&memory.workspace).bind(&memory.user_peer).bind(&memory.employee_peer).bind(json!(memory.options))
            .execute(&mut *tx).await.unwrap();
        let binding: Uuid = sqlx::query_scalar("INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,verified_at) VALUES($1,'cem',$2,'adopt',$3,$4,clock_timestamp()) RETURNING id")
            .bind(f.company).bind(revision).bind(hex::decode(&employee.office.public_key).unwrap()).bind(employee.office.signer_ref.as_str())
            .fetch_one(&mut *tx).await.unwrap();
        sqlx::query("UPDATE employees SET active_revision_id=$2 WHERE company_id=$1 AND id='cem'")
            .bind(f.company)
            .bind(revision)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let employee_key = OfficePublicKey::parse_hex(&employee.office.public_key).unwrap();
        let channel = direct_support::create(
            &f.pool,
            f.community,
            &[f.operator.public_key().to_bytes(), *employee_key.as_bytes()],
        )
        .await;
        let scope = f
            .control
            .resolve_company_for_community(f.community)
            .await
            .unwrap();
        direct_support::select(&f.control, &scope, channel, &employee.id).await;
        let pair = ConfiguredDmPair {
            selection_id: Uuid::new_v4(),
            channel_id: channel,
            employee_id: employee.id,
            human_public_key: OfficePublicKey::parse_hex(&f.operator.public_key().to_hex())
                .unwrap(),
            employee_public_key: employee_key,
            office_binding_id: binding,
            key_version: 0,
            decrypt_ref: employee.office.signer_ref,
        };
        let jobs = PgDecryptJobs::new(f.pool.clone());
        assert_eq!(jobs.register_pair(&scope, &pair).await.unwrap(), 1);
        let app = Self::router(&f, &f.operator, channel, "cem");
        Self {
            f,
            scope,
            channel,
            pair,
            jobs,
            app,
        }
    }

    fn router(f: &Fixture, actor: &Keys, channel: Uuid, employee: &str) -> Router {
        let mut cfg = config(f.community, actor, channel);
        cfg.humans[0].role = Role::Reader;
        cfg.humans[0].employee_ids = vec![EmployeeId::parse(employee).unwrap()];
        product_router(f.control.clone(), cfg, Arc::new(Replay::default())).unwrap()
    }
    fn path(&self) -> String {
        format!("/api/v1/channels/{}/encrypted-dm/authority", self.channel)
    }
    async fn read(&self) -> (StatusCode, Value) {
        bounded(
            &self.app,
            signed(&self.f.operator, "GET", &self.path(), "", false),
        )
        .await
    }
    async fn member(&self, present: bool) {
        sqlx::query("UPDATE channel_members SET removed_at=CASE WHEN $4 THEN NULL ELSE clock_timestamp() END WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
            .bind(self.f.community).bind(self.channel).bind(self.f.operator.public_key().to_bytes().as_slice()).bind(present).execute(&self.f.pool).await.unwrap();
    }
    async fn no_crypto_effects(&self) {
        let counts: (i64,i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM encrypted_dm_decrypt_jobs WHERE company_id=$1),(SELECT count(*) FROM confidential_runs WHERE company_id=$1),(SELECT count(*) FROM runs WHERE company_id=$1),(SELECT count(*) FROM events WHERE community_id=$2 AND kind=1059)")
            .bind(self.f.company).bind(self.f.community).fetch_one(&self.f.pool).await.unwrap();
        assert_eq!(counts, (0, 0, 0, 0));
    }
}

async fn bounded(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    assert_eq!(response.headers()["cache-control"], "no-store");
    // The actual serialized native DTO, not an unbounded test projection.
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    (status, body)
}

fn observation(x: &EncryptedFixture, value: &Value) -> i64 {
    let mut expected = vec![
        "format",
        "company_id",
        "community_id",
        "channel_id",
        "employee_id",
        "human_public_key",
        "employee_public_key",
        "pair_hash",
        "selection_id",
        "selection_generation",
        "office_binding_id",
        "key_version",
        "office_generation",
        "authority_epoch",
        "observed_at",
        "valid_before",
    ];
    expected.sort();
    let mut actual = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    actual.sort();
    assert_eq!(actual, expected);
    assert_eq!(value["format"], "ortak-native-encrypted-dm-authority/1");
    assert_eq!(value["company_id"], x.f.company.to_string());
    assert_eq!(value["community_id"], x.f.community.to_string());
    assert_eq!(value["channel_id"], x.channel.to_string());
    assert_eq!(value["employee_id"], "cem");
    assert_eq!(
        value["human_public_key"],
        x.f.operator.public_key().to_hex()
    );
    assert_eq!(
        value["employee_public_key"],
        hex::encode(x.pair.employee_public_key.as_bytes())
    );
    assert_eq!(
        value["office_binding_id"],
        x.pair.office_binding_id.to_string()
    );
    assert_eq!(value["selection_id"], x.pair.selection_id.to_string());
    assert_eq!(value["key_version"], "0");
    let keys = [
        x.f.operator.public_key().to_bytes(),
        *x.pair.employee_public_key.as_bytes(),
    ];
    let refs = keys.iter().map(|key| key.as_slice()).collect::<Vec<_>>();
    assert_eq!(
        value["pair_hash"],
        hex::encode(buzz_db::dm::compute_participant_hash(&refs))
    );
    for key in [
        "selection_generation",
        "key_version",
        "office_generation",
        "authority_epoch",
    ] {
        let text = value[key].as_str().unwrap();
        assert_eq!(text.parse::<i64>().unwrap().to_string(), text);
    }
    assert_eq!(value["office_generation"], value["authority_epoch"]);
    let observed =
        chrono::DateTime::parse_from_rfc3339(value["observed_at"].as_str().unwrap()).unwrap();
    let until =
        chrono::DateTime::parse_from_rfc3339(value["valid_before"].as_str().unwrap()).unwrap();
    assert!(until > observed && until - observed <= chrono::Duration::seconds(5));
    assert!(!value.to_string().contains("credential://"));
    assert!(!value.to_string().contains("fake://"));
    value["authority_epoch"].as_str().unwrap().parse().unwrap()
}

#[tokio::test]
#[ignore = "requires disposable55432 schema76 plus encrypted jobs/admission candidates"]
async fn encrypted_dm_authority_signed_native_dto_requires_both_grants_and_exact_human_host() {
    let x = EncryptedFixture::new().await;
    let (status, value) = x.read().await;
    assert_eq!(status, StatusCode::OK, "{value}");
    observation(&x, &value);
    // Valid signed humans and nonempty configured audiences: each omitted
    // selected dimension must be denied by the actual endpoint, not config validation.
    for (actor, channel, employee) in [
        (&x.f.operator, x.f.channel, "cem"),
        (&x.f.operator, x.channel, "unselected-employee"),
        (&x.f.reader, x.channel, "cem"),
    ] {
        let app = EncryptedFixture::router(&x.f, actor, channel, employee);
        let (status, body) = bounded(&app, signed(actor, "GET", &x.path(), "", false)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body, json!({"error":{"code":"not_found"}}));
    }
    let mut wrong_host = signed(&x.f.operator, "GET", &x.path(), "", false);
    wrong_host
        .headers_mut()
        .insert("host", "foreign.example".parse().unwrap());
    assert_eq!(
        bounded(&x.app, wrong_host).await.0,
        StatusCode::UNAUTHORIZED
    );
    let foreign = Fixture::new().await;
    let mut cfg = config(foreign.community, &foreign.operator, x.channel);
    cfg.humans[0].role = Role::Reader;
    let app = product_router(foreign.control.clone(), cfg, Arc::new(Replay::default())).unwrap();
    assert_eq!(
        bounded(&app, signed(&foreign.operator, "GET", &x.path(), "", false))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    x.no_crypto_effects().await;
}

#[tokio::test]
#[ignore = "requires disposable55432 schema76 plus encrypted jobs/admission candidates"]
async fn encrypted_dm_authority_disable_and_member_restore_require_new_current_generations() {
    let x = EncryptedFixture::new().await;
    let (status, before) = x.read().await;
    assert_eq!(status, StatusCode::OK, "{before}");
    let epoch = observation(&x, &before);
    let disabled = x
        .jobs
        .set_enabled(&x.scope, x.pair.selection_id, 1, false)
        .await
        .unwrap();
    assert_eq!(disabled, 2);
    assert_eq!(
        x.read().await,
        (StatusCode::NOT_FOUND, json!({"error":{"code":"not_found"}}))
    );
    let enabled = x
        .jobs
        .set_enabled(&x.scope, x.pair.selection_id, disabled, true)
        .await
        .unwrap();
    assert_eq!(enabled, 3);
    let (status, reopened) = x.read().await;
    assert_eq!(status, StatusCode::OK, "{reopened}");
    let enabled_epoch = observation(&x, &reopened);
    assert!(enabled_epoch > epoch);
    assert_eq!(reopened["selection_generation"], "3");
    x.member(false).await;
    assert_eq!(
        x.read().await,
        (StatusCode::NOT_FOUND, json!({"error":{"code":"not_found"}}))
    );
    x.member(true).await;
    let (status, restored) = x.read().await;
    assert_eq!(status, StatusCode::OK, "{restored}");
    assert!(observation(&x, &restored) > enabled_epoch);
    assert_eq!(restored["selection_generation"], "3");
    for field in [
        "selection_id",
        "office_binding_id",
        "pair_hash",
        "key_version",
        "human_public_key",
        "employee_public_key",
    ] {
        assert_eq!(restored[field], before[field]);
    }
    x.no_crypto_effects().await;
}
