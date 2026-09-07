//! Real prepared targets and deferred activation56 guards on explicit disposable PG.
use super::*;
use chrono::{DateTime, Utc};
use std::time::Duration;

async fn private_setup() -> (PgPool, PgControlPlane, CompanyScope) {
    let selected = std::env::var("ORTAK_TEST_DATABASE_URL")
        .expect("explicit disposable ORTAK_TEST_DATABASE_URL is required");
    let options: sqlx::postgres::PgConnectOptions = selected.parse().expect("test URL");
    assert_eq!(options.get_host(), "127.0.0.1", "literal loopback only");
    assert_eq!(options.get_port(), 55432, "never the live private database");
    assert!(
        !selected.contains('?') && !selected.contains('#'),
        "no endpoint overrides"
    );
    // The parent's first selection is this required, already-validated variable.
    // This module never reaches its legacy fallback with an absent selection.
    setup().await
}

async fn captured(
    control: &PgControlPlane,
    scope: &CompanyScope,
    lifetime: Duration,
) -> (EmployeeManifest, Uuid, RevisionActivation) {
    let manifest = disposable();
    let fakes = Fakes::creatable(&manifest);
    let capture = support::Capture::new(control);
    let saga = ProvisioningSaga::new(
        &capture,
        &fakes.runtime,
        &fakes.memory,
        &fakes.office,
        &fakes.credentials,
        SagaConfig {
            activation_lifetime: lifetime,
            ..SagaConfig::default()
        },
    );
    let operation = saga
        .begin(scope, &request(&manifest, OperationMode::Create, false))
        .await
        .expect("begin real saga");
    let _outcome = saga.resume(scope, operation.id).await;
    assert!(
        capture.has_candidate(),
        "real prepare and fresh probes must produce a candidate"
    );
    (manifest, operation.id, capture.take())
}

async fn snapshot(pool: &PgPool, scope: &CompanyScope, operation: Uuid) -> serde_json::Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object('operation',to_jsonb(o), 'steps',
          (SELECT jsonb_agg(to_jsonb(s) ORDER BY step_index) FROM provisioning_operation_steps s
           WHERE s.company_id=o.company_id AND s.operation_id=o.id), 'employee',
          (SELECT to_jsonb(e) FROM employees e WHERE e.company_id=o.company_id AND e.id=o.employee_id))
         FROM provisioning_operations o WHERE o.company_id=$1 AND o.id=$2",
    ).bind(scope.company_id()).bind(operation).fetch_one(pool).await.expect("durable snapshot")
}

async fn no_activation(pool: &PgPool, scope: &CompanyScope, employee: &str) {
    assert_eq!(revision_count(pool, scope, employee).await, 0);
    for query in [
        "SELECT count(*) FROM employee_runtime_bindings WHERE company_id=$1 AND employee_id=$2",
        "SELECT count(*) FROM employee_memory_bindings WHERE company_id=$1 AND employee_id=$2",
        "SELECT count(*) FROM employee_office_bindings WHERE company_id=$1 AND employee_id=$2",
        "SELECT count(*) FROM employee_aliases WHERE company_id=$1 AND employee_id=$2",
    ] {
        let count: i64 = sqlx::query_scalar(query)
            .bind(scope.company_id())
            .bind(employee)
            .fetch_one(pool)
            .await
            .expect("count activation effects");
        assert_eq!(count, 0, "no partial activation: {query}");
    }
}

fn sqlstate(error: &sqlx::Error) -> Option<String> {
    error
        .as_database_error()
        .and_then(|e| e.code())
        .map(|code| code.into_owned())
}

async fn blocked(pool: &PgPool, pid: i32) -> bool {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database()
                 AND $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(pool)
            .await
            .expect("observe exact blocker");
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok()
}

async fn expired(pool: &PgPool, deadline: DateTime<Utc>) -> bool {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>=$1")
                .bind(deadline)
                .fetch_one(pool)
                .await
                .expect("database clock");
            if expired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok()
}

async fn joined(
    mut task: tokio::task::JoinHandle<ortak_control::Result<Uuid>>,
) -> Option<ortak_control::Result<Uuid>> {
    match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
        Ok(result) => Some(result.expect("activation task")),
        Err(_) => {
            task.abort();
            let _ = task.await;
            None
        }
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL at 127.0.0.1:55432"]
async fn real_fresh_activation_commits_one_immutable_operation_and_receipt() {
    let (pool, control, scope) = private_setup().await;
    let (manifest, operation, candidate) =
        captured(&control, &scope, Duration::from_secs(10)).await;
    let revision = control
        .activate_revision(&scope, operation, &candidate)
        .await
        .expect("fresh commit");
    assert_eq!(
        control
            .activate_revision(&scope, operation, &candidate)
            .await
            .expect("replay"),
        revision
    );
    assert_eq!(
        revision_count(&pool, &scope, manifest.employee.id.as_str()).await,
        1
    );
    assert_eq!(
        employee_row(&pool, &scope, manifest.employee.id.as_str()).await,
        ("active".to_owned(), Some(revision))
    );
    let before = snapshot(&pool, &scope, operation).await;
    assert_eq!(before["operation"]["status"], "succeeded");
    let receipt = &before["steps"][9]["result"];
    assert_eq!(receipt["admission"]["format"], "ortak.activation/v1");
    assert_eq!(receipt["admission"]["operation_id"], operation.to_string());
    assert_eq!(receipt["result_revision_id"], revision.to_string());
    assert!(receipt["evidence"].is_object());
    for query in [
        "UPDATE provisioning_operations SET status='failed' WHERE company_id=$1 AND id=$2",
        "DELETE FROM provisioning_operations WHERE company_id=$1 AND id=$2",
        "UPDATE provisioning_operation_steps SET result='{}'::jsonb WHERE company_id=$1 AND operation_id=$2 AND step_name='activate_revision'",
        "DELETE FROM provisioning_operation_steps WHERE company_id=$1 AND operation_id=$2 AND step_name='activate_revision'",
    ] {
        let error = sqlx::query(query)
            .bind(scope.company_id())
            .bind(operation)
            .execute(&pool)
            .await
            .expect_err("activation audit is immutable");
        assert_eq!(sqlstate(&error).as_deref(), Some("55000"));
    }
    assert_eq!(snapshot(&pool, &scope, operation).await, before);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL at 127.0.0.1:55432"]
async fn changed_office_generation_or_employee_baseline_refuses_real_candidate() {
    for pause_employee in [false, true] {
        let (pool, control, scope) = private_setup().await;
        let (manifest, operation, candidate) =
            captured(&control, &scope, Duration::from_secs(10)).await;
        if pause_employee {
            // This legitimate authority mutation also advances the Office fence;
            // no production trigger is disabled to manufacture an isolated baseline.
            sqlx::query("UPDATE employees SET status='paused' WHERE company_id=$1 AND id=$2")
                .bind(scope.company_id())
                .bind(manifest.employee.id.as_str())
                .execute(&pool)
                .await
                .expect("change employee baseline");
        } else {
            sqlx::query("SELECT ortak_advance_office_authority($1,'activation-freshness-fixture')")
                .bind(scope.company_id())
                .execute(&pool)
                .await
                .expect("advance actual Office fence");
        }
        let before = snapshot(&pool, &scope, operation).await;
        let result = control
            .activate_revision(&scope, operation, &candidate)
            .await;
        assert!(
            matches!(
                result,
                Err(ControlError::Provisioning(
                    ProvisioningError::Superseded { .. }
                ))
            ),
            "{result:?}"
        );
        assert_eq!(snapshot(&pool, &scope, operation).await, before);
        no_activation(&pool, &scope, manifest.employee.id.as_str()).await;
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL at 127.0.0.1:55432"]
async fn waiting_for_operation_row_cannot_consume_expired_activation_target() {
    let (pool, control, scope) = private_setup().await;
    let (manifest, operation, candidate) =
        captured(&control, &scope, Duration::from_millis(350)).await;
    let deadline = candidate
        .target
        .as_ref()
        .expect("real target")
        .valid_before();
    let before = snapshot(&pool, &scope, operation).await;
    let mut blocker = pool.begin().await.expect("operation blocker");
    sqlx::query("SELECT id FROM provisioning_operations WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company_id())
        .bind(operation)
        .fetch_one(&mut *blocker)
        .await
        .expect("hold operation");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .expect("blocker pid");
    let owned_control = control.clone();
    let owned_scope = scope.clone();
    let task = tokio::spawn(async move {
        owned_control
            .activate_revision(&owned_scope, operation, &candidate)
            .await
    });
    let reached = blocked(&pool, pid).await;
    let elapsed = expired(&pool, deadline).await;
    blocker
        .rollback()
        .await
        .expect("release owned operation lock");
    let result = joined(task).await.expect("bounded activation completion");
    assert!(
        reached && elapsed,
        "production lock wait and expiry must both be observed"
    );
    assert!(
        matches!(
            result,
            Err(ControlError::Provisioning(
                ProvisioningError::Superseded { .. }
            ))
        ),
        "{result:?}"
    );
    assert_eq!(snapshot(&pool, &scope, operation).await, before);
    no_activation(&pool, &scope, manifest.employee.id.as_str()).await;
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL at 127.0.0.1:55432"]
async fn expiry_at_final_success_write_rolls_back_every_activation_effect_at_commit() {
    let (pool, control, scope) = private_setup().await;
    let (manifest, operation, candidate) =
        captured(&control, &scope, Duration::from_millis(900)).await;
    let deadline = candidate
        .target
        .as_ref()
        .expect("real target")
        .valid_before();
    let before = snapshot(&pool, &scope, operation).await;
    let trigger = format!("test_activation_wait_{}", Uuid::new_v4().simple());
    let key = i64::from_be_bytes(
        Uuid::new_v4().as_bytes()[..8]
            .try_into()
            .expect("eight bytes"),
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION {trigger}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
          IF NEW.company_id='{}'::uuid AND NEW.id='{operation}'::uuid AND NEW.status='succeeded' THEN
            PERFORM set_config('lock_timeout','2s',true);
            PERFORM pg_advisory_xact_lock({key}::bigint);
          END IF; RETURN NEW; END $$;
         CREATE TRIGGER {trigger} AFTER UPDATE ON provisioning_operations
         FOR EACH ROW EXECUTE FUNCTION {trigger}();", scope.company_id()
    ))).execute(&pool).await.expect("install exact operation final-write barrier");
    let mut blocker = pool.begin().await.expect("barrier owner");
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(key)
        .execute(&mut *blocker)
        .await
        .expect("hold final-write barrier");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .expect("barrier pid");
    let owned_control = control.clone();
    let owned_scope = scope.clone();
    let task = tokio::spawn(async move {
        owned_control
            .activate_revision(&owned_scope, operation, &candidate)
            .await
    });
    let reached = blocked(&pool, pid).await;
    let elapsed = expired(&pool, deadline).await;
    blocker
        .rollback()
        .await
        .expect("release after target deadline");
    let result = joined(task).await;
    // Remove this request-scoped DDL before result assertions, including failure.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {trigger} ON provisioning_operations; DROP FUNCTION {trigger}();"
    )))
    .execute(&pool)
    .await
    .expect("remove only owned final-write barrier");
    assert!(
        reached && elapsed,
        "must reach final write while admitted, then outlive target"
    );
    let ControlError::Database(error) = result
        .expect("bounded activation completion")
        .expect_err("deferred activation deadline must reject commit")
    else {
        panic!("expected exact deferred database failure");
    };
    assert_eq!(sqlstate(&error).as_deref(), Some("40001"));
    assert_eq!(snapshot(&pool, &scope, operation).await, before);
    no_activation(&pool, &scope, manifest.employee.id.as_str()).await;
}
