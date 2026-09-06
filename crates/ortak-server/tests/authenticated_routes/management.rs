//! Production signed admission, repository leases and real runner composition.
use super::*;
use ortak_control::{
    ports::ProvisioningRepository,
    provisioning::{
        OperationMode, OperationStatus, OperationUpdate, ProvisioningRequest, ProvisioningStep,
        StepRecord, StepState,
    },
};
use ortak_server::{
    management::{execute_next, import_prepared_catalog, synchronize_authorizations},
    provisioning::{provision_once, ProvisioningConfig},
};

#[path = "management_fixture.rs"]
mod fixture;
const EMPLOYEE: &str = "prepared-fixture";

async fn setup() -> (Fixture, ApiConfig, Router, Uuid, Value) {
    let f = Fixture::new().await;
    let mut config = config(f.community, &f.operator, f.channel);
    config.humans[0].can_manage_employees = true;
    config.humans[0].can_execute_provisioning = true;
    config.humans[0]
        .employee_ids
        .push(EmployeeId::parse(EMPLOYEE).unwrap());
    synchronize_authorizations(&f.control, &config)
        .await
        .unwrap();
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let prepared = fixture::prepared(&scope, f.channel);
    let catalog = Uuid::new_v4();
    import_prepared_catalog(&f.control,&json!({"community_id":f.community,"entries":[{"id":catalog,"label":"Prepared fixture","configuration":prepared}]}).to_string()).await.unwrap();
    let app = product_router(
        f.control.clone(),
        config.clone(),
        Arc::new(Replay::default()),
    )
    .unwrap();
    (f, config, app, catalog, prepared)
}
async fn post(f: &Fixture, app: &Router, suffix: &str, body: Value) -> (StatusCode, Value) {
    response(
        app,
        signed(
            &f.operator,
            "POST",
            &format!("/api/v1/employees/{EMPLOYEE}/{suffix}"),
            &body.to_string(),
            true,
        ),
    )
    .await
}
async fn admitted(f: &Fixture, app: &Router, catalog: Uuid) -> (Uuid, Value) {
    let (status, draft) = post(
        f,
        app,
        "configuration-drafts",
        json!({"draft_id":Uuid::new_v4(),"catalog_id":catalog,"expected_revision_id":null}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "draft rejected");
    let body = json!({"idempotency_key":Uuid::new_v4(),"action":"adopt","draft_id":draft["draft_id"],"operation_id":null,"expected_revision_id":null});
    let (status, receipt) = post(f, app, "management-commands", body.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "command rejected");
    (
        Uuid::parse_str(receipt["command_id"].as_str().unwrap()).unwrap(),
        body,
    )
}
async fn leased(
    f: &Fixture,
    id: Uuid,
) -> (
    ortak_control::CompanyScope,
    PgControlPlane,
    Uuid,
    ProvisioningRequest,
) {
    let token = Uuid::new_v4();
    sqlx::query("UPDATE employee_management_commands SET status='running',attempts=attempts+1,lease_token=$3,lease_expires_at=clock_timestamp()+interval '180 seconds' WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(id).bind(token).execute(&f.pool).await.unwrap();
    let value: Value = sqlx::query_scalar(
        "SELECT configuration FROM employee_management_commands WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company)
    .bind(id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let config: ProvisioningConfig = serde_json::from_value(value).unwrap();
    let request = ProvisioningRequest {
        employee_id: config.manifest.employee.id.clone(),
        mode: config.mode,
        dry_run: config.dry_run,
        idempotency_key: config.operation_key,
        manifest: config.manifest,
    };
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let bound = f
        .control
        .for_provisioning_command(&scope, id, token)
        .await
        .unwrap();
    (scope, bound, token, request)
}

#[test]
fn execution_grant_is_separate_from_progress_read_authority() {
    let keys = Keys::generate();
    let mut config = config(Uuid::new_v4(), &keys, Uuid::new_v4());
    assert!(!config.humans[0].can_execute_provisioning);
    config.humans[0].can_manage_employees = true;
    assert!(config.clone().validate().is_ok());
    config.humans[0].can_manage_employees = false;
    config.humans[0].can_execute_provisioning = true;
    assert!(config.validate().is_err());
}

#[test]
fn management_process_is_default_off_without_configuration_or_database_lookup() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ortak-management"))
        .env_clear()
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.len() < 512);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with management schema"]
async fn management_update_uses_an_exact_prepared_variant_and_preserves_the_active_revision() {
    let (f, _, app, _, prepared) = setup().await;
    let mut prepared: Value =
        serde_json::from_str(&prepared.to_string().replace("prepared-fixture", "cem")).unwrap();
    prepared["manifest"]["employee"]["runtime"]["model"] = json!("new-fixture-model");
    let catalog = Uuid::new_v4();
    import_prepared_catalog(&f.control,&json!({"community_id":f.community,"entries":[{"id":catalog,"label":"Cem model variant","configuration":prepared}]}).to_string()).await.unwrap();
    let draft_id = Uuid::new_v4();
    let path = "/api/v1/employees/cem/configuration-drafts";
    let (status, draft) = response(
        &app,
        signed(
            &f.operator,
            "POST",
            path,
            &json!({"draft_id":draft_id,"catalog_id":catalog,"expected_revision_id":f.revision})
                .to_string(),
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(draft["model"], "new-fixture-model");
    assert_eq!(draft["action"], "update");
    let path = "/api/v1/employees/cem/management-commands";
    let mut request = json!({"idempotency_key":Uuid::new_v4(),"action":"update","draft_id":draft_id,"operation_id":null,"expected_revision_id":Uuid::new_v4()});
    assert_eq!(
        response(
            &app,
            signed(&f.operator, "POST", path, &request.to_string(), true)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    request["expected_revision_id"] = json!(f.revision);
    assert_eq!(
        response(
            &app,
            signed(&f.operator, "POST", path, &request.to_string(), true)
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    execute_next(&f.control, f.community).await.unwrap();
    let saved:(Uuid,String,i64)=sqlx::query_as("SELECT active_revision_id,status,(SELECT count(*) FROM employee_revisions WHERE company_id=$1 AND employee_id='cem') FROM employees WHERE company_id=$1 AND id='cem'").bind(f.company).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        saved,
        (f.revision, "active".into(), 1),
        "failed real health preparation cannot mutate the active revision"
    );
    let selected: Value = sqlx::query_scalar(
        "SELECT manifest FROM provisioning_operations WHERE company_id=$1 AND employee_id='cem'",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        selected["employee"]["runtime"]["model"],
        "new-fixture-model"
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with management schema"]
async fn management_concurrent_idempotency_and_current_channel_membership_are_enforced() {
    let (f, _, app, catalog, _) = setup().await;
    let (status, draft) = post(
        &f,
        &app,
        "configuration-drafts",
        json!({"draft_id":Uuid::new_v4(),"catalog_id":catalog,"expected_revision_id":null}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let body = json!({"idempotency_key":Uuid::new_v4(),"action":"adopt","draft_id":draft["draft_id"],"operation_id":null,"expected_revision_id":null});
    let (first, second) = tokio::join!(
        post(&f, &app, "management-commands", body.clone()),
        post(&f, &app, "management-commands", body.clone())
    );
    assert_eq!(first.0, StatusCode::ACCEPTED);
    assert_eq!(second.0, StatusCode::ACCEPTED);
    assert_eq!(first.1, second.1);
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(f.operator.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap();
    execute_next(&f.control, f.community).await.unwrap();
    let saved: (String, i32) = sqlx::query_as(
        "SELECT status,attempts FROM employee_management_commands WHERE company_id=$1",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        saved,
        ("blocked".into(), 0),
        "revocation must refuse before any adapter or operation exists"
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM provisioning_operations WHERE company_id=$1")
            .bind(f.company)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with management schema"]
async fn management_process_sigterm_retains_one_resumable_operation() {
    let (f, _, app, catalog, _) = setup().await;
    let (id, _) = admitted(&f, &app, catalog).await;
    struct OwnedChild(std::process::Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = OwnedChild(
        std::process::Command::new(env!("CARGO_BIN_EXE_ortak-management"))
            .env_clear()
            .env("ORTAK_MANAGEMENT_ENABLED", "true")
            .env("ORTAK_MANAGEMENT_ACTION", "work")
            .env("ORTAK_MANAGEMENT_COMMUNITY_ID", f.community.to_string())
            .env(
                "ORTAK_DATABASE_URL",
                std::env::var("ORTAK_TEST_DATABASE_URL").unwrap(),
            )
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let operation=tokio::time::timeout(std::time::Duration::from_secs(8),async{
        loop {
            assert!(child.0.try_wait().unwrap().is_none(),"owned worker exited before retaining its operation");
            let value:Option<Uuid>=sqlx::query_scalar("SELECT operation_id FROM employee_management_commands WHERE company_id=$1 AND id=$2").bind(f.company).bind(id).fetch_one(&f.pool).await.unwrap();
            if let Some(value)=value{break value;}
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.unwrap();
    assert!(std::process::Command::new("/bin/kill")
        .arg("-TERM")
        .arg(child.0.id().to_string())
        .status()
        .unwrap()
        .success());
    let status = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(status.success());
    // Advance only the disposable lease clock, preserving the operation,
    // attempts and immutable selection exactly as a restart would observe them.
    sqlx::query("UPDATE employee_management_commands SET next_attempt_at=clock_timestamp(),lease_expires_at=CASE WHEN lease_token IS NOT NULL THEN clock_timestamp()-interval '1 second' END WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(id).execute(&f.pool).await.unwrap();
    execute_next(&f.control, f.community).await.unwrap();
    let saved: Uuid = sqlx::query_scalar(
        "SELECT operation_id FROM employee_management_commands WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company)
    .bind(id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(saved, operation);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM provisioning_operations WHERE company_id=$1")
            .bind(f.company)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with management schema"]
async fn management_catalog_draft_and_admission_are_scoped_atomic_and_secret_free() {
    let (f, _, app, catalog, prepared) = setup().await;
    assert_eq!(
        response(
            &f.app,
            signed(
                &f.operator,
                "GET",
                "/api/v1/employee-preparations",
                "",
                false
            )
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, choices) = response(
        &app,
        signed(
            &f.operator,
            "GET",
            "/api/v1/employee-preparations",
            "",
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(choices["choices"].as_array().unwrap().len(), 1);
    for private in [
        "secret://",
        "ORTAK_FIXTURE",
        "creation_receipt",
        "configuration",
        "native-workspace",
    ] {
        assert!(!choices.to_string().contains(private));
    }
    let (id, body) = admitted(&f, &app, catalog).await;
    let (status, replayed) = post(&f, &app, "management-commands", body.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(replayed["command_id"], id.to_string());
    let mut different = body;
    different["expected_revision_id"] = json!(Uuid::new_v4());
    assert_eq!(
        post(&f, &app, "management-commands", different).await.0,
        StatusCode::CONFLICT
    );
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM employee_management_commands WHERE company_id=$1),(SELECT count(*) FROM provisioning_operations WHERE company_id=$1),(SELECT count(*) FROM employees WHERE company_id=$1 AND id='prepared-fixture')").bind(f.company).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        counts,
        (1, 0, 0),
        "HTTP must not reserve identities or start a runner"
    );
    let (status, page) = response(
        &app,
        signed(
            &f.operator,
            "GET",
            &format!("/api/v1/employees/{EMPLOYEE}/management-commands"),
            "",
            false,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["commands"][0]["status"], "pending");
    for private in [
        "secret://",
        "ORTAK_FIXTURE",
        "configuration",
        "policy_fingerprint",
        "lease_token",
        "creation_receipt",
    ] {
        assert!(!page.to_string().contains(private));
    }
    let mut changed = prepared;
    changed["memory"]["creation_receipt"]["native_ids"]["workspace"] = json!("replacement");
    assert!(import_prepared_catalog(&f.control,&json!({"community_id":f.community,"entries":[{"id":catalog,"label":"Prepared fixture","configuration":changed}]}).to_string()).await.is_err());
    for query in [
        "DELETE FROM employee_configuration_drafts WHERE company_id=$1",
        "DELETE FROM employee_management_commands WHERE company_id=$1",
        "DELETE FROM employee_management_audit WHERE company_id=$1",
    ] {
        assert!(sqlx::query(query)
            .bind(f.company)
            .execute(&f.pool)
            .await
            .is_err());
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with management schema"]
async fn management_executor_uses_the_real_runner_and_reuses_frozen_configuration_after_restart() {
    let (f, _, app, catalog, _) = setup().await;
    let (id, _) = admitted(&f, &app, catalog).await;
    execute_next(&f.control, f.community).await.unwrap();
    let first:(Uuid,String,i32,Value)=sqlx::query_as("SELECT operation_id,status,attempts,configuration FROM employee_management_commands WHERE company_id=$1 AND id=$2").bind(f.company).bind(id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(first.1, "pending");
    assert_eq!(first.2, 1);
    let original = first.3.to_string();
    assert_eq!(
        provision_once(f.pool.clone(), &original, false).await.err(),
        Some("provisioning begin failed; inspect the retained operation"),
        "refusal must happen before missing credential lookup"
    );
    // Retiring the current catalog cannot alter an already admitted selection.
    import_prepared_catalog(
        &f.control,
        &json!({"community_id":f.community,"entries":[]}).to_string(),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE employee_management_commands SET next_attempt_at=clock_timestamp() WHERE company_id=$1 AND id=$2").bind(f.company).bind(id).execute(&f.pool).await.unwrap();
    execute_next(&PgControlPlane::new(f.pool.clone()), f.community)
        .await
        .unwrap();
    let second:(Uuid,i32,Value)=sqlx::query_as("SELECT operation_id,attempts,configuration FROM employee_management_commands WHERE company_id=$1 AND id=$2").bind(f.company).bind(id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(second.0, first.0);
    assert_eq!(second.1, 2);
    assert_eq!(second.2, first.3);
    let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM provisioning_operations WHERE company_id=$1),(SELECT count(*) FROM office_identity_profiles WHERE company_id=$1)").bind(f.company).fetch_one(&f.pool).await.unwrap();
    assert_eq!(counts, (1, 0));
    let unlocked: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("ortak-provision-employee:{}:{EMPLOYEE}", f.company))
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert!(unlocked);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with management schema"]
async fn management_lease_scope_and_current_role_are_rechecked_by_production_repository() {
    let (f, mut config, app, catalog, _) = setup().await;
    let (id, _) = admitted(&f, &app, catalog).await;
    let (scope, bound, _, request) = leased(&f, id).await;
    let operation = bound.begin_operation(&scope, &request).await.unwrap();
    assert!(bound
        .reserve_employee_identity(&scope, &EmployeeId::parse("unselected").unwrap())
        .await
        .is_err());
    let mut step = StepRecord::pending(operation.id, ProvisioningStep::ValidateManifest);
    step.state = StepState::Running;
    step.attempt_count = 1;
    step.started_at = Some(Utc::now());
    let new_token = Uuid::new_v4();
    sqlx::query(
        "UPDATE employee_management_commands SET lease_token=$3 WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company)
    .bind(id)
    .bind(new_token)
    .execute(&f.pool)
    .await
    .unwrap();
    assert!(bound
        .record_step(&scope, operation.id, &step)
        .await
        .is_err());
    let current = f
        .control
        .for_provisioning_command(&scope, id, new_token)
        .await
        .unwrap();
    current
        .record_step(&scope, operation.id, &step)
        .await
        .unwrap();
    config.humans[0].can_execute_provisioning = false;
    synchronize_authorizations(&f.control, &config)
        .await
        .unwrap();
    step.attempt_count = 2;
    assert!(current
        .record_step(&scope, operation.id, &step)
        .await
        .is_err());
    let attempts:i32=sqlx::query_scalar("SELECT attempt_count FROM provisioning_operation_steps WHERE company_id=$1 AND operation_id=$2 AND step_index=0").bind(f.company).bind(operation.id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(attempts, 1);
    assert!(f
        .control
        .for_provisioning_command(&scope, id, Uuid::new_v4())
        .await
        .is_err());
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with management schema"]
async fn management_deferred_commit_rejects_expired_execution_leases() {
    let (f, _, app, catalog, _) = setup().await;
    let (id, _) = admitted(&f, &app, catalog).await;
    let (scope, bound, token, request) = leased(&f, id).await;
    let operation = bound.begin_operation(&scope, &request).await.unwrap();
    sqlx::query("UPDATE employee_management_commands SET lease_expires_at=clock_timestamp()+interval '300 milliseconds' WHERE company_id=$1 AND id=$2").bind(f.company).bind(id).execute(&f.pool).await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("SELECT ortak_management_guard($1,$2,$3,$4)")
        .bind(f.company)
        .bind(id)
        .bind(token)
        .bind(operation.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE provisioning_operation_steps SET attempt_count=1,state='running',started_at=clock_timestamp() WHERE company_id=$1 AND operation_id=$2 AND step_index=0").bind(f.company).bind(operation.id).execute(&mut *tx).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    assert!(tx.commit().await.is_err());
    let attempts:i32=sqlx::query_scalar("SELECT attempt_count FROM provisioning_operation_steps WHERE company_id=$1 AND operation_id=$2 AND step_index=0").bind(f.company).bind(operation.id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(attempts, 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with management schema"]
async fn management_adopted_compensation_needs_no_catalog_or_credential_lookup() {
    let (f, _, app, catalog, _) = setup().await;
    let (id, _) = admitted(&f, &app, catalog).await;
    let (scope, bound, _, request) = leased(&f, id).await;
    assert_eq!(request.mode, OperationMode::Adopt);
    let operation = bound.begin_operation(&scope, &request).await.unwrap();
    bound
        .update_operation(
            &scope,
            operation.id,
            &OperationUpdate {
                status: OperationStatus::Running,
                current_step: Some(ProvisioningStep::ValidateManifest),
                error_message: None,
            },
        )
        .await
        .unwrap();
    bound
        .update_operation(
            &scope,
            operation.id,
            &OperationUpdate {
                status: OperationStatus::Failed,
                current_step: Some(ProvisioningStep::ValidateManifest),
                error_message: Some("fixture failure before any adapter".into()),
            },
        )
        .await
        .unwrap();
    sqlx::query("UPDATE employee_management_commands SET status='failed',lease_token=NULL,lease_expires_at=NULL WHERE company_id=$1 AND id=$2").bind(f.company).bind(id).execute(&f.pool).await.unwrap();
    import_prepared_catalog(
        &f.control,
        &json!({"community_id":f.community,"entries":[]}).to_string(),
    )
    .await
    .unwrap();
    let (status,_)=post(&f,&app,"management-commands",json!({"idempotency_key":Uuid::new_v4(),"action":"compensate","operation_id":operation.id,"draft_id":null,"expected_revision_id":null})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    execute_next(&f.control, f.community).await.unwrap();
    let saved = f
        .control
        .load_operation(&scope, operation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.status, OperationStatus::Compensated);
    assert_eq!(saved.result_revision_id, None);
}

#[path = "management/claim.rs"]
mod claim;

#[path = "management_lifecycle.rs"]
mod lifecycle;

#[path = "management_runtime_probe.rs"]
mod runtime_probe;
