//! Production runner selection and durable retry boundaries, without a provider.

use nostr::Keys;
use ortak_control::{ports::CompanyDirectory, CompanyScope, PgControlPlane};
use ortak_domain::RoutingPolicy;
use ortak_server::provisioning::{provision_once, ProvisioningConfig};
use serde_json::{json, Value};
use sqlx::{postgres::PgConnectOptions, PgPool};
use uuid::Uuid;

#[path = "provisioning_selection/recovery.rs"]
mod recovery;

async fn fixture() -> (PgPool, CompanyScope, Value) {
    let url = std::env::var("ORTAK_TEST_DATABASE_URL").expect("explicit disposable URL");
    let options: PgConnectOptions = url.parse().unwrap();
    assert_eq!(options.get_port(), 55432);
    assert!(matches!(options.get_host(), "localhost" | "127.0.0.1"));
    let pool = ortak_server::connect_private_database(&url).await.unwrap();
    buzz_db::migration::run_migrations(&pool).await.unwrap();
    let company = Uuid::new_v4();
    let community = Uuid::new_v4();
    sqlx::query("INSERT INTO communities (id, host) VALUES ($1,$2)")
        .bind(community)
        .bind(format!("prov-{}.example", community.simple()))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO companies (id, slug, display_name, routing_policy) VALUES ($1,$2,'Prepared fixture',$3)")
        .bind(company).bind(format!("co-{}", company.simple())).bind(serde_json::to_value(RoutingPolicy::default()).unwrap())
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO office_company_bindings (company_id, community_id) VALUES ($1,$2)")
        .bind(company)
        .bind(community)
        .execute(&pool)
        .await
        .unwrap();
    let scope = PgControlPlane::new(pool.clone())
        .resolve_company_for_community(community)
        .await
        .unwrap();
    let config = config(&scope);
    (pool, scope, config)
}

fn config(scope: &CompanyScope) -> Value {
    let public_key = Keys::generate().public_key().to_hex();
    let channel = Uuid::new_v4();
    let memory = json!({"adapter":"honcho","endpoint_ref":"service://fixture/honcho",
        "workspace":"fixture-workspace","user_peer":"fixture-human","employee_peer":"fixture-employee"});
    let office = json!({"public_key":public_key,"signer_ref":"secret://fixture/office","home_channel_ref":channel.to_string()});
    let missing = format!("ORTAK_MISSING_{}", Uuid::new_v4().simple());
    assert!(std::env::var_os(&missing).is_none());
    json!({
        "community_id":scope.community_id().unwrap(),"operation_key":Uuid::new_v4().to_string(),"mode":"adopt","dry_run":false,
        "manifest":{"schema_version":"ortak.employee/v0","provisioning":"adopt","employee":{
            "id":"prepared-fixture","name":"Prepared Fixture","title":"Assistant","biography":"Isolated test",
            "status":"draft","aliases":[],"responsibilities":[],"domains":[],
            "runtime":{"adapter":"hermes","profile_ref":"fixture-profile","model":"fixture-model","workspace_ref":"/fixture",
                "credential_refs":["secret://fixture/oauth"]},
            "memory":memory,"office":office,"permissions":{},"routing":{"enabled":false,"semantic_min_score":null}}},
        "bridge_origin":"http://127.0.0.1:1","bridge_token_env":missing,
        "runtime_credentials":{"source":"environment","bindings":[{"credential_ref":"secret://fixture/oauth","environment_variable":"ORTAK_FIXTURE_OAUTH"}]},
        "office_signer":{"company_id":scope.company_id(),"employee_id":"prepared-fixture","signer_ref":"secret://fixture/office",
            "public_key":public_key,"secret_env":"ORTAK_FIXTURE_OFFICE"},
        "office":{"company_id":scope.company_id(),"community_id":scope.community_id().unwrap(),"origin":"http://127.0.0.1:1",
            "employees":[{"employee_id":"prepared-fixture","office":office,"channels":[channel]}]},
        "memory":{"origin":"http://127.0.0.1:1","token_ref":"secret://fixture/honcho","token_env":"ORTAK_FIXTURE_HONCHO",
            "validate_memory_io":true,"validation_run_id":Uuid::new_v4(),"validation_recorded_at":"2026-09-05T12:00:00Z",
            "creation_receipt":{"company_id":scope.company_id(),"deployment_id":Uuid::new_v4(),"employee_id":"prepared-fixture",
                "binding":memory,"creation_key":"original-create-key","request_hash":"0".repeat(64),
                "native_ids":{"workspace":"native-workspace","peers":{"fixture-human":"native-human","fixture-employee":"native-employee"}},
                "resources":{"workspace":{"resource_ref":"fixture-workspace","ownership":"created"},
                    "user_peer":{"resource_ref":"fixture-human","ownership":"created"},
                    "employee_peer":{"resource_ref":"fixture-employee","ownership":"created"}}}}
    })
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432"]
async fn runner_refuses_cross_selection_before_operations_or_credential_reads() {
    let (pool, scope, original) = fixture().await;
    let baseline: ProvisioningConfig = serde_json::from_value(original.clone()).unwrap();
    baseline.validate(&scope).unwrap();
    let mut bridge_owned = original.clone();
    bridge_owned["runtime_credentials"] = json!({"source":"hermes_profile"});
    serde_json::from_value::<ProvisioningConfig>(bridge_owned.clone())
        .unwrap()
        .validate(&scope)
        .unwrap();
    bridge_owned["runtime_credentials"]["bindings"] = json!([]);
    assert!(serde_json::from_value::<ProvisioningConfig>(bridge_owned).is_err());
    for pointer in [
        "/office/company_id",
        "/office/community_id",
        "/office_signer/company_id",
        "/memory/creation_receipt/company_id",
        "/community_id",
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).unwrap() = json!(Uuid::new_v4());
        let config: ProvisioningConfig = serde_json::from_value(value.clone()).unwrap();
        assert!(config.validate(&scope).is_err(), "{pointer}");
        assert!(provision_once(pool.clone(), &value.to_string(), false)
            .await
            .is_err());
    }
    for (pointer, changed) in [
        ("/mode", json!("create")),
        (
            "/runtime_credentials/bindings/0/credential_ref",
            json!("secret://unselected/oauth"),
        ),
        (
            "/runtime_credentials/bindings/0/environment_variable",
            json!("INVALID-NAME"),
        ),
        ("/office/employees/0/channels", json!([])),
        ("/memory/validate_memory_io", json!(false)),
        ("/bridge_token_env", json!("ORTAK_FIXTURE_OFFICE")),
        ("/memory/token_env", json!("ORTAK_FIXTURE_OAUTH")),
        ("/memory/token_env", json!("INVALID-NAME")),
        ("/memory/token_ref", json!("secret://fixture/office")),
        ("/memory/origin", json!("http://unselected.example")),
        ("/bridge_origin", json!("http://unselected.example")),
        ("/office_signer/secret_env", json!("OTHER")),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).unwrap() = changed;
        assert!(
            provision_once(pool.clone(), &value.to_string(), false)
                .await
                .is_err(),
            "{pointer}"
        );
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM provisioning_operations WHERE company_id=$1")
            .bind(scope.company_id())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432"]
async fn runner_missing_credential_replays_one_operation_and_freezes_original_receipt() {
    let (pool, scope, value) = fixture().await;
    for _ in 0..2 {
        let error = provision_once(pool.clone(), &value.to_string(), false)
            .await
            .err();
        assert_eq!(error, Some("selected bridge credential unavailable"));
    }
    let mut changed = value.clone();
    changed["memory"]["creation_receipt"]["native_ids"]["workspace"] = json!("replacement");
    assert_eq!(provision_once(pool.clone(), &changed.to_string(), false).await.err(),
        Some("provisioning selection changed; retained operation requires its original configuration"));
    let states: Vec<(Uuid, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT id,status,result_revision_id FROM provisioning_operations WHERE company_id=$1",
    )
    .bind(scope.company_id())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].1, "pending");
    assert_eq!(states[0].2, None);
    let attempts: i64 = sqlx::query_scalar(
        "SELECT sum(attempt_count)::bigint FROM provisioning_operation_steps WHERE company_id=$1",
    )
    .bind(scope.company_id())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(attempts, 0);
    for query in [
        "UPDATE provisioning_runner_selections SET configuration_fingerprint=decode(repeat('01',32),'hex') WHERE company_id=$1",
        "DELETE FROM provisioning_runner_selections WHERE company_id=$1",
    ] {
        assert!(sqlx::query(query).bind(scope.company_id()).execute(&pool).await.is_err());
    }
    // Every failed invocation dropped its dedicated lock connection. It may not
    // strand another retry or leak the session lock into a pooled connection.
    let unlocked: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!(
                "ortak-provision-employee:{}:{}",
                scope.company_id(),
                value["manifest"]["employee"]["id"].as_str().unwrap()
            ))
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(unlocked);
    pool.close().await;
}

#[test]
fn command_is_disabled_without_explicit_enablement() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ortak-provision"))
        .env_clear()
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("ORTAK_PROVISIONING_ENABLED=true"));
}
