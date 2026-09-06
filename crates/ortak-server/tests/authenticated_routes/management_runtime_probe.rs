//! Signed action admission plus the actual durable probe/lease repository.
use super::*;
use ortak_control::CompanyScope;

async fn prepared() -> (
    Fixture,
    ApiConfig,
    Router,
    CompanyScope,
    PgControlPlane,
    Uuid,
    Uuid,
) {
    let (f, config, app, catalog, _) = setup().await;
    let (command, _) = admitted(&f, &app, catalog).await;
    let (scope, bound, _, request) = leased(&f, command).await;
    let operation = bound.begin_operation(&scope, &request).await.unwrap().id;
    sqlx::query("INSERT INTO provisioning_runner_selections(company_id,operation_id,configuration_fingerprint) VALUES($1,$2,$3)")
        .bind(f.company).bind(operation).bind([0x68_u8;32].as_slice()).execute(&f.pool).await.unwrap();
    (f, config, app, scope, bound, operation, command)
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with proposal68"]
async fn runtime_probe_concurrent_admission_crash_and_cli_cannot_replace_a_leased_child() {
    let (f, _, _, scope, bound, operation, _) = prepared().await;
    let (left, right) = tokio::join!(
        bound.admit_provisioning_runtime_probe(
            &scope,
            operation,
            "http://127.0.0.1:1",
            "ORTAK_FIXTURE_BRIDGE",
            None
        ),
        bound.admit_provisioning_runtime_probe(
            &scope,
            operation,
            "http://127.0.0.1:1",
            "ORTAK_FIXTURE_BRIDGE",
            None
        ),
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let admitted = left.ok().or(right.ok()).unwrap();
    let recovered = bound
        .provisioning_runtime_probe(&scope, operation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(admitted.id(), recovered.id());
    assert_eq!(recovered.state(), "running");
    assert!(bound
        .admit_provisioning_runtime_probe(
            &scope,
            operation,
            "http://127.0.0.1:1",
            "ORTAK_FIXTURE_BRIDGE",
            Some(admitted.id())
        )
        .await
        .is_err());
    // An ordinary CLI repository cannot bypass a managed action's current lease.
    assert!(f
        .control
        .admit_provisioning_runtime_probe(
            &scope,
            operation,
            "http://127.0.0.1:1",
            "ORTAK_FIXTURE_BRIDGE",
            Some(admitted.id())
        )
        .await
        .is_err());
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM provisioning_runtime_probes WHERE company_id=$1")
            .bind(f.company)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    bound
        .settle_provisioning_runtime_probe(&scope, &admitted, Some("probe_interrupted"))
        .await
        .unwrap();
    assert!(
        f.control
            .admit_provisioning_runtime_probe(
                &scope,
                operation,
                "http://127.0.0.1:1",
                "ORTAK_FIXTURE_BRIDGE",
                Some(admitted.id())
            )
            .await
            .is_err(),
        "CLI cannot replace a managed probe even after cleanup frees the child slot"
    );
    let next = bound
        .admit_provisioning_runtime_probe(
            &scope,
            operation,
            "http://127.0.0.1:1",
            "ORTAK_FIXTURE_BRIDGE",
            Some(admitted.id()),
        )
        .await
        .unwrap();
    assert_ne!(next.id(), admitted.id());
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with proposal68"]
async fn runtime_probe_revocation_refuses_readiness_but_retains_exact_cleanup() {
    let (f, mut config, _, scope, bound, operation, _) = prepared().await;
    let selected = bound
        .admit_provisioning_runtime_probe(
            &scope,
            operation,
            "http://127.0.0.1:1",
            "ORTAK_FIXTURE_BRIDGE",
            None,
        )
        .await
        .unwrap();
    config.humans[0].can_execute_provisioning = false;
    synchronize_authorizations(&f.control, &config)
        .await
        .unwrap();
    assert!(bound
        .check_provisioning_runtime_probe_authority(&scope, operation)
        .await
        .is_err());
    assert!(bound
        .settle_provisioning_runtime_probe(&scope, &selected, None)
        .await
        .is_err());
    bound
        .settle_provisioning_runtime_probe(&scope, &selected, Some("probe_authority_changed"))
        .await
        .unwrap();
    let row:(String,bool)=sqlx::query_as("SELECT state,contained_at IS NOT NULL FROM provisioning_runtime_probes WHERE company_id=$1 AND probe_id=$2")
        .bind(f.company).bind(selected.id()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(row, ("failed".into(), true));
    assert!(bound
        .admit_provisioning_runtime_probe(
            &scope,
            operation,
            "http://127.0.0.1:1",
            "ORTAK_FIXTURE_BRIDGE",
            Some(selected.id())
        )
        .await
        .is_err());
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with proposal68"]
async fn runtime_probe_expired_lease_keeps_cleanup_and_pre_saga_retry_visible() {
    let (f, _, app, scope, bound, operation, command) = prepared().await;
    let selected = bound
        .admit_provisioning_runtime_probe(
            &scope,
            operation,
            "http://127.0.0.1:1",
            "ORTAK_FIXTURE_BRIDGE",
            None,
        )
        .await
        .unwrap();
    // Simulate process death: the durable action remains running until its lease
    // expires. Reconciliation must not need a succeeded saga step to recover it.
    sqlx::query("UPDATE employee_management_commands SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(command).execute(&f.pool).await.unwrap();
    assert!(bound
        .check_provisioning_runtime_probe_authority(&scope, operation)
        .await
        .is_err());
    bound
        .settle_provisioning_runtime_probe(&scope, &selected, Some("probe_interrupted"))
        .await
        .unwrap();
    sqlx::query("UPDATE employee_management_commands SET status='failed',lease_token=NULL,lease_expires_at=NULL,error_code='command_attempts_exhausted' WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(command).execute(&f.pool).await.unwrap();
    let path = format!("/api/v1/employees/{EMPLOYEE}/management-commands");
    let (status, page) = response(&app, signed(&f.operator, "GET", &path, "", false)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["commands"][0]["can_retry"], true);
    assert_eq!(
        page["commands"][0]["runtime_probe"],
        json!({"generation":1,"state":"failed"})
    );
    assert!(!page.to_string().contains("ORTAK_FIXTURE_BRIDGE"));
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM provisioning_operation_steps WHERE company_id=$1 AND operation_id=$2 AND state='succeeded'")
        .bind(f.company).bind(operation).fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with proposal68"]
async fn runtime_probe_admission_rechecks_management_lease_at_commit() {
    let (f, _, _, _, _, operation, command) = prepared().await;
    let token: Uuid = sqlx::query_scalar(
        "SELECT lease_token FROM employee_management_commands WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company)
    .bind(command)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE employee_management_commands SET lease_expires_at=clock_timestamp()+interval '1 second' WHERE company_id=$1 AND id=$2").bind(f.company).bind(command).execute(&f.pool).await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("SELECT ortak_management_guard($1,$2,$3,$4)")
        .bind(f.company)
        .bind(command)
        .bind(token)
        .bind(operation)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO provisioning_runtime_probes(company_id,operation_id,employee_id,generation,probe_id,bridge_origin,bridge_token_env,state,created_at,deadline) VALUES($1,$2,$3,1,$4,'http://127.0.0.1:1','ORTAK_FIXTURE_BRIDGE','running',clock_timestamp(),clock_timestamp()+interval '89 seconds')")
        .bind(f.company).bind(operation).bind(EMPLOYEE).bind(Uuid::new_v4()).execute(&mut *tx).await.unwrap();
    sqlx::query("SELECT pg_sleep(1.1)")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        tx.commit().await.is_err(),
        "an expired action cannot commit external admission"
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM provisioning_runtime_probes WHERE company_id=$1")
            .bind(f.company)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with proposal68"]
async fn runtime_probe_final_success_expiry_disables_and_refuses_an_impossible_retry() {
    let (f, _, app, scope, bound, operation, command) = prepared().await;
    let mut previous = None;
    for _ in 1..20 {
        let probe = bound
            .admit_provisioning_runtime_probe(
                &scope,
                operation,
                "http://127.0.0.1:1",
                "ORTAK_FIXTURE_BRIDGE",
                previous,
            )
            .await
            .unwrap();
        bound
            .settle_provisioning_runtime_probe(&scope, &probe, Some("probe_interrupted"))
            .await
            .unwrap();
        previous = Some(probe.id());
    }
    // A short real deadline keeps this test bounded without mutating immutable
    // journal timestamps or manufacturing a post-deadline success.
    let token: Uuid = sqlx::query_scalar(
        "SELECT lease_token FROM employee_management_commands WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company)
    .bind(command)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("SELECT ortak_management_guard($1,$2,$3,$4)")
        .bind(f.company)
        .bind(command)
        .bind(token)
        .bind(operation)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO provisioning_runtime_probes(company_id,operation_id,employee_id,generation,probe_id,bridge_origin,bridge_token_env,state,created_at,deadline) VALUES($1,$2,$3,20,$4,'http://127.0.0.1:1','ORTAK_FIXTURE_BRIDGE','running',clock_timestamp(),clock_timestamp()+interval '3 seconds')")
        .bind(f.company).bind(operation).bind(EMPLOYEE).bind(Uuid::new_v4())
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let final_probe = bound
        .provisioning_runtime_probe(&scope, operation)
        .await
        .unwrap()
        .unwrap();
    bound
        .settle_provisioning_runtime_probe(&scope, &final_probe, None)
        .await
        .unwrap();
    sqlx::query("UPDATE employee_management_commands SET status='failed',lease_token=NULL,lease_expires_at=NULL,error_code='command_attempts_exhausted' WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(command).execute(&f.pool).await.unwrap();
    let path = format!("/api/v1/employees/{EMPLOYEE}/management-commands");
    let (status, fresh) = response(&app, signed(&f.operator, "GET", &path, "", false)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        fresh["commands"][0]["can_retry"], true,
        "fresh final success is still reusable"
    );
    sqlx::query("SELECT pg_sleep(GREATEST(0,extract(epoch FROM ($1::timestamptz-clock_timestamp())))::double precision+0.05)")
        .bind(final_probe.deadline()).execute(&f.pool).await.unwrap();
    let (status, expired) = response(&app, signed(&f.operator, "GET", &path, "", false)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(expired["commands"][0]["can_retry"], false);
    assert_eq!(
        expired["commands"][0]["runtime_probe"],
        json!({"generation":20,"state":"succeeded"})
    );
    let denied = post(
        &f,
        &app,
        "management-commands",
        json!({
            "idempotency_key":Uuid::new_v4(), "action":"retry", "operation_id":operation,
            "draft_id":null,"expected_revision_id":null,"expected_lifecycle_epoch":0
        }),
    )
    .await;
    assert_eq!(denied.0, StatusCode::CONFLICT);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM employee_management_commands WHERE company_id=$1")
            .bind(f.company)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        count, 1,
        "unavailable retries must not enqueue another command"
    );
}
