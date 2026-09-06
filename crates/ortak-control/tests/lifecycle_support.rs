//! Shared PostgreSQL fixture: actual sealed lifecycle commands and saga writes,
//! with deterministic health adapters. No direct disabled→active SQL bypass.
use ortak_control::{
    fakes::{
        FakeCredentialResolver, FakeMemoryAdapter, FakeOfficeIdentityAdapter, FakeRuntimeAdapter,
    },
    provisioning::{OperationMode, ProvisioningRequest, ProvisioningSaga, SagaConfig, SagaOutcome},
    CompanyScope, PgControlPlane,
};
use ortak_domain::{Employee, EmployeeManifest, EmployeeStatus};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

/// Applies a genuine disable barrier then a fresh leased Update saga. The caller
/// keeps its old runtime binding to check that equivalent new identity cannot
/// revive old dispatch, cancellation, publication or memory admission.
pub async fn cycle(
    pool: &PgPool,
    control: &PgControlPlane,
    scope: &CompanyScope,
    employee: &Employee,
) -> Uuid {
    let memory = FakeMemoryAdapter::new().with_existing_binding(employee.memory.as_ref().unwrap());
    cycle_with_memory(pool, control, scope, employee, &memory).await
}

/// Same sealed lifecycle fixture with an explicitly supplied memory test adapter.
pub async fn cycle_with_memory<M: ortak_control::memory::MemoryAdapter>(
    pool: &PgPool,
    control: &PgControlPlane,
    scope: &CompanyScope,
    employee: &Employee,
    memory: &M,
) -> Uuid {
    assert_eq!(employee.runtime.adapter, "fake-runtime");
    let company = scope.company_id();
    let community = scope.community_id().expect("Office fixture");
    let (previous,epoch):(Uuid,i64)=sqlx::query_as("SELECT active_revision_id,lifecycle_epoch FROM employees WHERE company_id=$1 AND id=$2 AND status='active'").bind(company).bind(employee.id.as_str()).fetch_one(pool).await.unwrap();
    let actor = hex::encode([6_u8; 32]);
    let channel: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE community_id=$1 ORDER BY id LIMIT 1")
            .bind(community)
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO relay_members(community_id,pubkey,role) VALUES($1,$2,'member') ON CONFLICT DO NOTHING").bind(community).bind(&actor).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO employee_management_policies(company_id,public_key,fingerprint,enabled,employee_ids,channel_ids) VALUES($1,$2,$3,true,$4,$5)").bind(company).bind(&actor).bind([9_u8;32].as_slice()).bind(vec![employee.id.as_str()]).bind(vec![channel]).execute(pool).await.unwrap();
    let (disable, token) = command(
        pool, scope, employee, &actor, "disable", previous, epoch, None,
    )
    .await;
    control
        .for_provisioning_command(scope, disable, token)
        .await
        .unwrap()
        .disable_employee_for_command(scope)
        .await
        .unwrap();
    let mut candidate = employee.clone();
    candidate.status = EmployeeStatus::Draft;
    let manifest: EmployeeManifest = serde_json::from_value(
        json!({"schema_version":"ortak.employee/v0","provisioning":"adopt","employee":candidate}),
    )
    .unwrap();
    let request = ProvisioningRequest {
        employee_id: employee.id.clone(),
        mode: OperationMode::Update,
        dry_run: false,
        idempotency_key: Uuid::new_v4().to_string(),
        manifest,
    };
    let configuration = json!({"operation_key":request.idempotency_key,"mode":"update","dry_run":false,"manifest":request.manifest});
    let (reenable, token) = command(
        pool,
        scope,
        employee,
        &actor,
        "reenable",
        previous,
        epoch + 1,
        Some(configuration),
    )
    .await;
    let bound = control
        .for_provisioning_command(scope, reenable, token)
        .await
        .unwrap();
    let runtime = FakeRuntimeAdapter::new()
        .with_existing_profile(employee.runtime.profile_ref.as_deref().unwrap(), true);
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
    let saga = ProvisioningSaga::new(
        &bound,
        &runtime,
        memory,
        &office,
        &credentials,
        SagaConfig::default(),
    );
    let op = saga.begin(scope, &request).await.unwrap();
    let result = saga.resume(scope, op.id).await.unwrap();
    let SagaOutcome::Succeeded(operation) = result else {
        panic!("sealed lifecycle fixture activation failed")
    };
    let revision = operation.result_revision_id.unwrap();
    assert_ne!(revision, previous);
    let actual:(String,Uuid,i64)=sqlx::query_as("SELECT status,active_revision_id,lifecycle_epoch FROM employees WHERE company_id=$1 AND id=$2").bind(company).bind(employee.id.as_str()).fetch_one(pool).await.unwrap();
    assert_eq!(actual, ("active".into(), revision, epoch + 1));
    // Finish accounting, exactly as the outer executor does after saga commit.
    sqlx::query("UPDATE employee_management_commands SET status='succeeded',lease_token=NULL,lease_expires_at=NULL WHERE company_id=$1 AND id=$2 AND lease_token=$3").bind(company).bind(reenable).bind(token).execute(pool).await.unwrap();
    revision
}

#[allow(clippy::too_many_arguments)]
async fn command(
    pool: &PgPool,
    scope: &CompanyScope,
    employee: &Employee,
    actor: &str,
    action: &str,
    previous: Uuid,
    epoch: i64,
    configuration: Option<Value>,
) -> (Uuid, Uuid) {
    let id = Uuid::new_v4();
    let token = Uuid::new_v4();
    sqlx::query("INSERT INTO employee_management_commands(company_id,id,employee_id,actor,auth_event_id,policy_fingerprint,policy_snapshot,action,idempotency_key,request_fingerprint,expected_revision_id,configuration,channel_ids,employee_lifecycle_epoch,status,attempts,lease_token,lease_expires_at) VALUES($1,$2,$3,$4,$5,$6,'{}',$7,$8,$9,$10,$11,'{}',$12,'running',1,$13,clock_timestamp()+interval '180 seconds')")
        .bind(scope.company_id()).bind(id).bind(employee.id.as_str()).bind(actor).bind([10_u8;32].as_slice()).bind([9_u8;32].as_slice()).bind(action).bind(Uuid::new_v4().to_string()).bind([11_u8;32].as_slice()).bind(previous).bind(configuration).bind(epoch).bind(token).execute(pool).await.unwrap();
    (id, token)
}
