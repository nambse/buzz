//! Same-key Office reuse must preserve original identity provenance at activation.
use super::*;
use chrono::{DateTime, Utc};

async fn binding_snapshot(
    pool: &PgPool,
    scope: &CompanyScope,
    employee_id: &str,
) -> (serde_json::Value, DateTime<Utc>) {
    let rows = sqlx::query(
        "SELECT to_jsonb(b)-'verified_at' AS identity, b.verified_at
         FROM employee_office_bindings b WHERE company_id=$1 AND employee_id=$2",
    )
    .bind(scope.company_id())
    .bind(employee_id)
    .fetch_all(pool)
    .await
    .expect("read exact Office identity");
    assert_eq!(rows.len(), 1, "reuse must never add or rotate a binding");
    (
        rows[0].try_get("identity").unwrap(),
        rows[0].try_get("verified_at").unwrap(),
    )
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL at 127.0.0.1:55432"]
async fn same_office_key_reuses_original_binding_with_fresh_activation_admission() {
    let selected = std::env::var("ORTAK_TEST_DATABASE_URL")
        .expect("explicit disposable ORTAK_TEST_DATABASE_URL required");
    let options: sqlx::postgres::PgConnectOptions = selected.parse().expect("test URL");
    assert_eq!(options.get_host(), "127.0.0.1");
    assert_eq!(options.get_port(), 55432, "never the live private database");
    assert!(!selected.contains('?') && !selected.contains('#'));
    let (pool, control, scope) = setup().await;
    let mut manifest = disposable();
    let fakes = Fakes::creatable(&manifest);
    let saga = fakes.saga(&control);
    let first = saga
        .begin(&scope, &request(&manifest, OperationMode::Create, false))
        .await
        .expect("begin fresh create");
    let SagaOutcome::Succeeded(first) =
        saga.resume(&scope, first.id).await.expect("first activate")
    else {
        panic!("first activation must succeed through fresh production gates");
    };
    let first_revision = first.result_revision_id.expect("first revision");
    let employee_id = manifest.employee.id.to_string();
    let (identity, first_verified) = binding_snapshot(&pool, &scope, &employee_id).await;
    assert_eq!(identity["revision_id"], first_revision.to_string());
    assert_eq!(
        identity["signer_ref"],
        manifest.employee.office.signer_ref.as_str()
    );
    assert!(identity["valid_until"].is_null());
    let profiles = fakes.runtime.created_profiles();
    let memory_resources = fakes.memory.created_resources();

    // Use the resources the first real saga just created. The new operation
    // explicitly adopts that exact profile, memory tuple, key and signer.
    manifest.employee = first.effective_employee();
    manifest.employee.title = "Updated platform lead".into();
    manifest.provisioning = ProvisioningMode::Adopt;
    let second = saga
        .begin(&scope, &request(&manifest, OperationMode::Update, false))
        .await
        .expect("begin explicit same-key update");
    let SagaOutcome::Succeeded(second) = saga
        .resume(&scope, second.id)
        .await
        .expect("same-key activate")
    else {
        panic!("fresh same-key update must pass the final SQL admission guard");
    };
    let second_revision = second.result_revision_id.expect("second revision");
    assert_ne!(first_revision, second_revision);
    assert_eq!(revision_count(&pool, &scope, &employee_id).await, 2);
    assert_eq!(
        employee_row(&pool, &scope, &employee_id).await,
        ("active".into(), Some(second_revision))
    );
    let (reused_identity, second_verified) = binding_snapshot(&pool, &scope, &employee_id).await;
    assert_eq!(reused_identity, identity,
        "binding ID, original revision provenance, key, signer and validity window must all remain unchanged");
    assert!(
        second_verified > first_verified,
        "verification must come from the new admission"
    );
    assert_ne!(reused_identity["revision_id"], second_revision.to_string());
    assert_eq!(fakes.runtime.created_profiles(), profiles);
    assert_eq!(fakes.memory.created_resources(), memory_resources);

    let row = sqlx::query(
        "SELECT s.result,s.attempt_count,encode(r.manifest_fingerprint,'hex') AS fingerprint,
                rb.validated_at AS runtime_validated,mb.validated_at AS memory_validated
         FROM provisioning_operations o
         JOIN provisioning_operation_steps s ON s.company_id=o.company_id AND s.operation_id=o.id
           AND s.step_name='activate_revision' AND s.state='succeeded'
         JOIN employee_revisions r ON r.company_id=o.company_id AND r.id=o.result_revision_id
         JOIN employee_runtime_bindings rb ON rb.company_id=r.company_id AND rb.revision_id=r.id
         JOIN employee_memory_bindings mb ON mb.company_id=r.company_id AND mb.revision_id=r.id
         WHERE o.company_id=$1 AND o.id=$2 AND o.status='succeeded'",
    )
    .bind(scope.company_id())
    .bind(second.id)
    .fetch_one(&pool)
    .await
    .expect("read actual committed admission and bindings");
    let receipt: serde_json::Value = row.try_get("result").unwrap();
    let observed: DateTime<Utc> =
        serde_json::from_value(receipt["admission"]["observed_at"].clone()).unwrap();
    let deadline: DateTime<Utc> =
        serde_json::from_value(receipt["admission"]["valid_before"].clone()).unwrap();
    assert_eq!(receipt["admission"]["format"], "ortak.activation/v1");
    assert_eq!(receipt["admission"]["operation_id"], second.id.to_string());
    assert_eq!(receipt["admission"]["employee_id"], employee_id);
    assert_eq!(
        receipt["admission"]["attempt_count"],
        row.try_get::<i32, _>("attempt_count").unwrap()
    );
    assert_eq!(
        receipt["admission"]["manifest_fingerprint"],
        row.try_get::<String, _>("fingerprint").unwrap()
    );
    assert_eq!(receipt["result_revision_id"], second_revision.to_string());
    assert_eq!(observed, second_verified);
    assert_eq!(
        row.try_get::<DateTime<Utc>, _>("runtime_validated")
            .unwrap(),
        observed
    );
    assert_eq!(
        row.try_get::<DateTime<Utc>, _>("memory_validated").unwrap(),
        observed
    );
    assert!(deadline > observed && deadline - observed <= chrono::Duration::seconds(15));
    assert!(receipt["evidence"].is_object());
}
