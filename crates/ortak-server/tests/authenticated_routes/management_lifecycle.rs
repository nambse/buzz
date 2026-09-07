//! Signed lifecycle admission and real repository/runner behavior on PostgreSQL.
use super::*;

#[path = "management_lifecycle_fixture.rs"]
mod lifecycle_fixture;
use lifecycle_fixture::{active_employee, employee_state, reenable_with_test_adapters};

fn disable_body(revision: Uuid) -> Value {
    json!({"idempotency_key":Uuid::new_v4(),"action":"disable","draft_id":null,
        "operation_id":null,"expected_revision_id":revision,"expected_lifecycle_epoch":0})
}
async fn disable(f: &Fixture, app: &Router, revision: Uuid) -> Uuid {
    let (status, receipt) = post(f, app, "management-commands", disable_body(revision)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    execute_next(&f.control, f.community).await.unwrap();
    assert_eq!(employee_state(f).await, ("disabled".into(), revision, 1));
    Uuid::parse_str(receipt["command_id"].as_str().unwrap()).unwrap()
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with lifecycle schema"]
async fn lifecycle_signed_disable_is_atomic_idempotent_and_survives_catalog_retirement() {
    let (f, _, app, _, prepared) = setup().await;
    let revision = active_employee(&f, &prepared).await;
    import_prepared_catalog(
        &f.control,
        &json!({"community_id":f.community,"entries":[]}).to_string(),
    )
    .await
    .unwrap();
    let (status, catalog) = response(
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
    assert!(catalog["choices"].as_array().unwrap().is_empty());
    assert!(catalog["employees"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["employee_id"] == EMPLOYEE));
    let body = disable_body(revision);
    let (first, second) = tokio::join!(
        post(&f, &app, "management-commands", body.clone()),
        post(&f, &app, "management-commands", body.clone())
    );
    assert_eq!(first.0, StatusCode::ACCEPTED);
    assert_eq!(first, second);
    assert_eq!(
        employee_state(&f).await,
        ("active".into(), revision, 0),
        "HTTP admission must not run lifecycle mutation inline"
    );
    execute_next(&f.control, f.community).await.unwrap();
    assert_eq!(employee_state(&f).await, ("disabled".into(), revision, 1));
    assert_eq!(post(&f, &app, "management-commands", body).await, first);
    let saved:(i64,i64,i64,String,Option<Value>)=sqlx::query_as("SELECT (SELECT count(*) FROM employee_lifecycle_events WHERE company_id=$1),(SELECT count(*) FROM employee_revisions WHERE company_id=$1 AND employee_id=$2),(SELECT count(*) FROM provisioning_operations WHERE company_id=$1),status,configuration FROM employee_management_commands WHERE company_id=$1")
        .bind(f.company).bind(EMPLOYEE).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        saved,
        (1, 1, 0, "succeeded".into(), None),
        "disable must not construct adapters, create operations or delete retained resources"
    );
    assert_eq!(
        post(&f, &app, "management-commands", disable_body(revision))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert!(
        sqlx::query("UPDATE employees SET status='active' WHERE company_id=$1 AND id=$2")
            .bind(f.company)
            .bind(EMPLOYEE)
            .execute(&f.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE employees SET lifecycle_epoch=0 WHERE company_id=$1 AND id=$2")
            .bind(f.company)
            .bind(EMPLOYEE)
            .execute(&f.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM employee_lifecycle_events WHERE company_id=$1")
            .bind(f.company)
            .execute(&f.pool)
            .await
            .is_err()
    );
    let mut ordinary = prepared;
    ordinary["mode"] = json!("update");
    ordinary["operation_key"] = json!(Uuid::new_v4().to_string());
    let error = provision_once(f.pool.clone(), &ordinary.to_string(), false)
        .await
        .err()
        .expect("ordinary disabled CLI must fail");
    assert!(
        error.to_string().contains("lifecycle"),
        "ordinary CLI must refuse disabled identity before missing credential lookup"
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with lifecycle schema"]
async fn lifecycle_disable_rechecks_current_execution_authority_before_mutation() {
    let (f, mut config, app, _, prepared) = setup().await;
    let revision = active_employee(&f, &prepared).await;
    assert_eq!(
        post(&f, &app, "management-commands", disable_body(revision))
            .await
            .0,
        StatusCode::ACCEPTED
    );
    config.humans[0].can_execute_provisioning = false;
    synchronize_authorizations(&f.control, &config)
        .await
        .unwrap();
    execute_next(&f.control, f.community).await.unwrap();
    assert_eq!(employee_state(&f).await, ("active".into(), revision, 0));
    let command: (String, i32) = sqlx::query_as(
        "SELECT status,attempts FROM employee_management_commands WHERE company_id=$1",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(command, ("blocked".into(), 0));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM employee_lifecycle_events WHERE company_id=$1")
            .bind(f.company)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with lifecycle schema"]
async fn lifecycle_reenable_requires_fresh_epoch_draft_and_real_failed_health_stays_disabled() {
    let (f, _, app, catalog, prepared) = setup().await;
    let revision = active_employee(&f, &prepared).await;
    let old=post(&f,&app,"configuration-drafts",json!({"draft_id":Uuid::new_v4(),"catalog_id":catalog,"expected_revision_id":revision,"expected_lifecycle_epoch":0})).await;
    assert_eq!(old.0, StatusCode::CREATED);
    disable(&f, &app, revision).await;
    let stale = json!({"idempotency_key":Uuid::new_v4(),"action":"reenable","draft_id":old.1["draft_id"],"operation_id":null,"expected_revision_id":revision,"expected_lifecycle_epoch":1});
    assert_eq!(
        post(&f, &app, "management-commands", stale).await.0,
        StatusCode::CONFLICT
    );
    let draft=post(&f,&app,"configuration-drafts",json!({"draft_id":Uuid::new_v4(),"catalog_id":catalog,"expected_revision_id":revision,"expected_lifecycle_epoch":1})).await;
    assert_eq!(draft.0, StatusCode::CREATED);
    assert_eq!(draft.1["action"], "reenable");
    assert_eq!(draft.1["expected_lifecycle_epoch"], 1);
    let mut body = json!({"idempotency_key":Uuid::new_v4(),"action":"update","draft_id":draft.1["draft_id"],"operation_id":null,"expected_revision_id":revision,"expected_lifecycle_epoch":1});
    assert_eq!(
        post(&f, &app, "management-commands", body.clone()).await.0,
        StatusCode::CONFLICT
    );
    body["action"] = json!("reenable");
    assert_eq!(
        post(&f, &app, "management-commands", body).await.0,
        StatusCode::ACCEPTED
    );
    execute_next(&f.control, f.community).await.unwrap();
    assert_eq!(employee_state(&f).await, ("disabled".into(), revision, 1));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM employee_lifecycle_events WHERE company_id=$1 AND action='reenable'",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        count, 0,
        "missing real bridge credential cannot produce an activation receipt"
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with lifecycle schema"]
async fn lifecycle_disable_deadline_is_rechecked_at_commit_and_events_cannot_be_forged() {
    let (f, _, app, _, prepared) = setup().await;
    let revision = active_employee(&f, &prepared).await;
    let receipt = post(&f, &app, "management-commands", disable_body(revision)).await;
    assert_eq!(receipt.0, StatusCode::ACCEPTED);
    let id = Uuid::parse_str(receipt.1["command_id"].as_str().unwrap()).unwrap();
    let token = Uuid::new_v4();
    sqlx::query("UPDATE employee_management_commands SET status='running',attempts=1,lease_token=$3,lease_expires_at=clock_timestamp()+interval '180 seconds' WHERE company_id=$1 AND id=$2").bind(f.company).bind(id).bind(token).execute(&f.pool).await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(ortak_office_company_lock_key($1))")
        .bind(f.company)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT ortak_management_guard($1,$2,$3,NULL)")
        .bind(f.company)
        .bind(id)
        .bind(token)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE employee_management_commands SET lease_expires_at=clock_timestamp()+interval '250 milliseconds' WHERE company_id=$1 AND id=$2").bind(f.company).bind(id).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE employees SET status='disabled' WHERE company_id=$1 AND id=$2")
        .bind(f.company)
        .bind(EMPLOYEE)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE employee_management_commands SET status='succeeded',lease_token=NULL,lease_expires_at=NULL WHERE company_id=$1 AND id=$2").bind(f.company).bind(id).execute(&mut *tx).await.unwrap();
    // Commit's witness must outlive the row-level check, even after lease clearing.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    assert_eq!(employee_state(&f).await, ("active".into(), revision, 0));
    let forged=sqlx::query("INSERT INTO employee_lifecycle_events(company_id,employee_id,action,lifecycle_epoch,previous_revision_id,result_revision_id) VALUES($1,$2,'disable',1,$3,$3)").bind(f.company).bind(EMPLOYEE).bind(revision).execute(&f.pool).await.unwrap_err();
    assert_eq!(
        forged.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with lifecycle schema"]
async fn lifecycle_sealed_reenable_uses_fresh_saga_health_and_never_rewrites_old_run_pins() {
    let (f, config, app, _, prepared) = setup().await;
    let revision = active_employee(&f, &prepared).await;
    let old_run = Uuid::new_v4();
    sqlx::query("INSERT INTO runs(company_id,id,employee_id,employee_revision_id,runtime_adapter,office_admission_generation,office_admission_valid_before,office_admission_token) VALUES($1,$2,$3,$4,'hermes',ortak_lock_office_authority($1),clock_timestamp()+interval '1 hour',gen_random_uuid())").bind(f.company).bind(old_run).bind(EMPLOYEE).bind(revision).execute(&f.pool).await.unwrap();
    disable(&f, &app, revision).await;
    let next = reenable_with_test_adapters(&f, &config, &prepared, Some(revision)).await;
    assert_ne!(next, revision);
    assert_eq!(employee_state(&f).await, ("active".into(), next, 1));
    let pin: i64 = sqlx::query_scalar(
        "SELECT employee_lifecycle_epoch FROM runs WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company)
    .bind(old_run)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(pin, 0);
    assert!(sqlx::query(
        "UPDATE runs SET employee_lifecycle_epoch=1 WHERE company_id=$1 AND id=$2"
    )
    .bind(f.company)
    .bind(old_run)
    .execute(&f.pool)
    .await
    .is_err());
    let refresh = sqlx::query("UPDATE runs SET office_admission_generation=ortak_lock_office_authority($1) WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(old_run).execute(&f.pool).await.unwrap_err();
    assert_eq!(
        refresh.as_database_error().unwrap().code().as_deref(),
        Some("40001"),
        "token-preserving refresh must not revive an old epoch admission"
    );
    let fresh = Uuid::new_v4();
    sqlx::query("INSERT INTO runs(company_id,id,employee_id,employee_revision_id,runtime_adapter) VALUES($1,$2,$3,$4,'hermes')").bind(f.company).bind(fresh).bind(EMPLOYEE).bind(next).execute(&f.pool).await.unwrap();
    let pin: i64 = sqlx::query_scalar(
        "SELECT employee_lifecycle_epoch FROM runs WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company)
    .bind(fresh)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(pin, 1);
    let counts:(i64,i64)=sqlx::query_as("SELECT count(*) FILTER(WHERE action='disable'),count(*) FILTER(WHERE action='reenable') FROM employee_lifecycle_events WHERE company_id=$1").bind(f.company).fetch_one(&f.pool).await.unwrap();
    assert_eq!(counts, (1, 1));
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with lifecycle schema"]
async fn lifecycle_retry_before_the_first_saga_step_keeps_the_draft_identity_epoch() {
    let (f, _, app, catalog, _) = setup().await;
    let (id, _) = admitted(&f, &app, catalog).await;
    let (scope, bound, _, request) = leased(&f, id).await;
    let operation = bound.begin_operation(&scope, &request).await.unwrap();
    // begin_operation atomically reserves the draft identity before any saga
    // step or adapter work. Retry must retain this zero epoch and NULL revision.
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM employees WHERE company_id=$1 AND id=$2")
            .bind(f.company)
            .bind(EMPLOYEE)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    let baseline:(String,Option<Uuid>,i64)=sqlx::query_as("SELECT status,active_revision_id,lifecycle_epoch FROM employees WHERE company_id=$1 AND id=$2").bind(f.company).bind(EMPLOYEE).fetch_one(&f.pool).await.unwrap();
    assert_eq!(baseline, ("draft".into(), None, 0));
    sqlx::query("UPDATE employee_management_commands SET status='failed',lease_token=NULL,lease_expires_at=NULL WHERE company_id=$1 AND id=$2").bind(f.company).bind(id).execute(&f.pool).await.unwrap();
    let retry=post(&f,&app,"management-commands",json!({"idempotency_key":Uuid::new_v4(),"action":"retry","operation_id":operation.id,"draft_id":null,"expected_revision_id":null,"expected_lifecycle_epoch":0})).await;
    assert_eq!(retry.0, StatusCode::ACCEPTED);
    let saved:i64=sqlx::query_scalar("SELECT employee_lifecycle_epoch FROM employee_management_commands WHERE company_id=$1 AND id=$2").bind(f.company).bind(Uuid::parse_str(retry.1["command_id"].as_str().unwrap()).unwrap()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(saved, 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with lifecycle schema"]
async fn lifecycle_unactivated_disabled_identity_requires_update_and_fresh_health() {
    let (f, config, _, _, prepared) = setup().await;
    sqlx::query("INSERT INTO employees(company_id,id) VALUES($1,$2)")
        .bind(f.company)
        .bind(EMPLOYEE)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE employees SET status='disabled' WHERE company_id=$1 AND id=$2")
        .bind(f.company)
        .bind(EMPLOYEE)
        .execute(&f.pool)
        .await
        .unwrap();
    let revision = reenable_with_test_adapters(&f, &config, &prepared, None).await;
    assert_eq!(employee_state(&f).await, ("active".into(), revision, 1));
    let previous:Option<Uuid>=sqlx::query_scalar("SELECT previous_revision_id FROM employee_lifecycle_events WHERE company_id=$1 AND action='reenable'").bind(f.company).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        previous, None,
        "first activation retains a null prior revision"
    );

    let (f, _, app, catalog, _) = setup().await;
    sqlx::query("INSERT INTO employees(company_id,id) VALUES($1,$2)")
        .bind(f.company)
        .bind(EMPLOYEE)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE employees SET status='disabled' WHERE company_id=$1 AND id=$2")
        .bind(f.company)
        .bind(EMPLOYEE)
        .execute(&f.pool)
        .await
        .unwrap();
    let draft=post(&f,&app,"configuration-drafts",json!({"draft_id":Uuid::new_v4(),"catalog_id":catalog,"expected_revision_id":null,"expected_lifecycle_epoch":1})).await;
    assert_eq!(draft.0, StatusCode::CREATED);
    assert_eq!(draft.1["action"], "reenable");
    let mode: String = sqlx::query_scalar(
        "SELECT configuration->>'mode' FROM employee_configuration_drafts WHERE company_id=$1",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(mode, "update");
    let mut body = json!({"idempotency_key":Uuid::new_v4(),"action":"adopt","operation_id":null,"draft_id":draft.1["draft_id"],"expected_revision_id":null,"expected_lifecycle_epoch":1});
    assert_eq!(
        post(&f, &app, "management-commands", body.clone()).await.0,
        StatusCode::CONFLICT
    );
    body["action"] = json!("reenable");
    assert_eq!(
        post(&f, &app, "management-commands", body).await.0,
        StatusCode::ACCEPTED
    );
    execute_next(&f.control, f.community).await.unwrap();
    let baseline:(String,Option<Uuid>,i64)=sqlx::query_as("SELECT status,active_revision_id,lifecycle_epoch FROM employees WHERE company_id=$1 AND id=$2").bind(f.company).bind(EMPLOYEE).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        baseline,
        ("disabled".into(), None, 1),
        "real missing credentials cannot activate even a never-activated identity"
    );
}
