//! Test adapters provide health only; all lease, saga and activation writes use
//! the production PgControlPlane. This is not real provider acceptance evidence.
use super::*;
use ortak_control::{
    fakes::{
        FakeCredentialResolver, FakeMemoryAdapter, FakeOfficeIdentityAdapter, FakeRuntimeAdapter,
    },
    provisioning::{ProvisioningSaga, SagaConfig, SagaOutcome},
};
use ortak_domain::{EmployeeManifest, EmployeeStatus};

pub(super) async fn active_employee(f: &Fixture, prepared: &Value) -> Uuid {
    let manifest: EmployeeManifest = serde_json::from_value(prepared["manifest"].clone()).unwrap();
    let mut employee = manifest.employee;
    employee.status = EmployeeStatus::Active;
    let revision = Uuid::new_v4();
    sqlx::query("INSERT INTO employees(company_id,id) VALUES($1,$2)")
        .bind(f.company)
        .bind(EMPLOYEE)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode) VALUES($1,$2,$3,1,$4,$5,'adopt')").bind(f.company).bind(revision).bind(EMPLOYEE).bind(serde_json::to_value(employee).unwrap()).bind([0_u8;32].as_slice()).execute(&f.pool).await.unwrap();
    sqlx::query(
        "UPDATE employees SET status='active',active_revision_id=$3 WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company)
    .bind(EMPLOYEE)
    .bind(revision)
    .execute(&f.pool)
    .await
    .unwrap();
    revision
}

pub(super) async fn employee_state(f: &Fixture) -> (String, Uuid, i64) {
    sqlx::query_as("SELECT status,active_revision_id,lifecycle_epoch FROM employees WHERE company_id=$1 AND id=$2").bind(f.company).bind(EMPLOYEE).fetch_one(&f.pool).await.unwrap()
}

pub(super) async fn reenable_with_test_adapters(
    f: &Fixture,
    config: &ApiConfig,
    prepared: &Value,
    previous: Option<Uuid>,
) -> Uuid {
    let mut selected: ProvisioningConfig = serde_json::from_value(prepared.clone()).unwrap();
    selected.mode = OperationMode::Update;
    selected.operation_key = Uuid::new_v4().to_string();
    selected.manifest.employee.runtime.adapter = "fake-runtime".into();
    selected.manifest.employee.memory.as_mut().unwrap().adapter = "fake-memory".into();
    let employee = &selected.manifest.employee;
    let runtime = FakeRuntimeAdapter::new()
        .with_existing_profile(employee.runtime.profile_ref.as_deref().unwrap(), true);
    let memory = FakeMemoryAdapter::new().with_existing_binding(employee.memory.as_ref().unwrap());
    let office = FakeOfficeIdentityAdapter::new()
        .with_signer(
            employee.office.signer_ref.as_str(),
            &employee.office.public_key,
        )
        .with_existing_member(&employee.office.public_key);
    let credentials = FakeCredentialResolver::new().with_references(
        employee
            .runtime
            .credential_refs
            .iter()
            .map(|v| v.as_str().to_owned())
            .chain(std::iter::once(
                employee.office.signer_ref.as_str().to_owned(),
            )),
    );
    let mut configuration = prepared.clone();
    configuration["manifest"] = serde_json::to_value(&selected.manifest).unwrap();
    configuration["mode"] = json!("update");
    configuration["operation_key"] = json!(selected.operation_key);
    let command = Uuid::new_v4();
    let actor = f.operator.public_key().to_hex();
    let policy:Vec<u8>=sqlx::query_scalar("SELECT fingerprint FROM employee_management_policies WHERE company_id=$1 AND public_key=$2").bind(f.company).bind(&actor).fetch_one(&f.pool).await.unwrap();
    // The signed HTTP negative-health test exercises the real Hermes catalog.
    // This seeded selection isolates successful production repository activation
    // from external runtimes while retaining the same immutable lease contract.
    sqlx::query("INSERT INTO employee_management_commands(company_id,id,employee_id,actor,auth_event_id,policy_fingerprint,policy_snapshot,action,idempotency_key,request_fingerprint,expected_revision_id,configuration,channel_ids,employee_lifecycle_epoch) VALUES($1,$2,$3,$4,$5,$6,$7,'reenable',$8,$9,$10,$11,$12,1)")
        .bind(f.company).bind(command).bind(EMPLOYEE).bind(actor).bind([1_u8;32].as_slice()).bind(policy).bind(serde_json::to_value(&config.humans[0]).unwrap()).bind(Uuid::new_v4().to_string()).bind([2_u8;32].as_slice()).bind(previous).bind(configuration).bind(vec![f.channel]).execute(&f.pool).await.unwrap();
    let (scope, bound, _, request) = leased(f, command).await;
    let saga = ProvisioningSaga::new(
        &bound,
        &runtime,
        &memory,
        &office,
        &credentials,
        SagaConfig::default(),
    );
    let operation = saga.begin(&scope, &request).await.unwrap();
    assert!(
        !f.control
            .allow_reenable_operation(&scope, operation.id)
            .await
            .unwrap(),
        "an ordinary CLI repository cannot issue reenable authority"
    );
    assert!(bound
        .allow_reenable_operation(&scope, operation.id)
        .await
        .unwrap());
    let outcome = saga.resume(&scope, operation.id).await.unwrap();
    assert!(
        matches!(outcome, SagaOutcome::Succeeded(_)),
        "fresh successful test-adapter health must reach real atomic activation"
    );
    let row:(Uuid,bool)=sqlx::query_as("SELECT r.id,rb.validated_at IS NOT NULL AND mb.validated_at IS NOT NULL AND ob.verified_at IS NOT NULL FROM employee_revisions r JOIN provisioning_operations o ON o.company_id=r.company_id AND o.result_revision_id=r.id JOIN employee_runtime_bindings rb ON rb.company_id=r.company_id AND rb.revision_id=r.id JOIN employee_memory_bindings mb ON mb.company_id=r.company_id AND mb.revision_id=r.id JOIN employee_office_bindings ob ON ob.company_id=r.company_id AND ob.revision_id=r.id WHERE o.company_id=$1 AND o.id=$2 AND o.status='succeeded'").bind(f.company).bind(operation.id).fetch_one(&f.pool).await.unwrap();
    assert!(row.1);
    row.0
}
