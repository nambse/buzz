use super::*;
use ortak_control::fakes::{FakeCredentialResolver, FakeOfficeIdentityAdapter};
use ortak_control::provisioning::{
    OperationMode, ProvisioningRequest, ProvisioningSaga, SagaConfig, SagaOutcome,
};

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal71"]
async fn reviewed_runtime_active_model_update_preserves_approved_employee_memory_without_changing_epoch(
) {
    let (x, item) = prepared(Duration::from_secs(86400)).await;
    let (previous, epoch): (Uuid, i64) = sqlx::query_as(
        "SELECT active_revision_id,lifecycle_epoch FROM employees WHERE company_id=$1 AND id='cem'",
    )
    .bind(x.f.company)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    let mut employee = x.employee.clone();
    employee.status = ortak_domain::EmployeeStatus::Draft;
    employee.runtime.model = "synthetic-new-model".into();
    let request=ProvisioningRequest {employee_id:employee.id.clone(),mode:OperationMode::Update,dry_run:false,
        idempotency_key:Uuid::new_v4().to_string(),manifest:serde_json::from_value(json!({"schema_version":"ortak.employee/v0","provisioning":"adopt","employee":employee})).unwrap()};
    let actor = x.f.operator.public_key().to_hex();
    sqlx::query("INSERT INTO employee_management_policies(company_id,public_key,fingerprint,enabled,employee_ids,channel_ids) VALUES($1,$2,$3,true,ARRAY['cem'],$4)")
        .bind(x.f.company).bind(&actor).bind([9_u8;32].as_slice()).bind(vec![x.f.channel]).execute(&x.f.pool).await.unwrap();
    let command = Uuid::new_v4();
    let token = Uuid::new_v4();
    let config = json!({"operation_key":request.idempotency_key,"mode":"update","dry_run":false,"manifest":request.manifest});
    sqlx::query("INSERT INTO employee_management_commands(company_id,id,employee_id,actor,auth_event_id,policy_fingerprint,policy_snapshot,action,idempotency_key,request_fingerprint,expected_revision_id,configuration,channel_ids,employee_lifecycle_epoch,status,attempts,lease_token,lease_expires_at)
        VALUES($1,$2,'cem',$3,$4,$5,'{}','update',$6,$7,$8,$9,$10,$11,'running',1,$12,clock_timestamp()+interval '180 seconds')")
        .bind(x.f.company).bind(command).bind(&actor).bind([10_u8;32].as_slice()).bind([9_u8;32].as_slice())
        .bind(Uuid::new_v4().to_string()).bind([11_u8;32].as_slice()).bind(previous).bind(config).bind(vec![x.f.channel]).bind(epoch).bind(token)
        .execute(&x.f.pool).await.unwrap();
    let bound =
        x.f.control
            .for_provisioning_command(&x.scope, command, token)
            .await
            .unwrap();
    let runtime = FakeRuntimeAdapter::new().with_existing_profile("fake://work-profile", true);
    let memory = NamedMemory(
        FakeMemoryAdapter::new().with_existing_binding(employee.memory.as_ref().unwrap()),
    );
    let office = FakeOfficeIdentityAdapter::new()
        .with_signer(
            employee.office.signer_ref.as_str(),
            &employee.office.public_key,
        )
        .with_existing_member(&employee.office.public_key);
    let credentials = FakeCredentialResolver::new().with_references([employee
        .office
        .signer_ref
        .as_str()
        .to_owned()]);
    let saga = ProvisioningSaga::new(
        &bound,
        &runtime,
        &memory,
        &office,
        &credentials,
        SagaConfig::default(),
    );
    let operation = saga.begin(&x.scope, &request).await.unwrap();
    assert!(matches!(
        saga.resume(&x.scope, operation.id).await.unwrap(),
        SagaOutcome::Succeeded(_)
    ));
    let (current, current_epoch): (Uuid, i64) = sqlx::query_as(
        "SELECT active_revision_id,lifecycle_epoch FROM employees WHERE company_id=$1 AND id='cem'",
    )
    .bind(x.f.company)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_ne!(current, previous);
    assert_eq!(current_epoch, epoch);
    x.advertise().await;
    let (_, adapter, _, _) = start(&x, &item).await;
    assert_eq!(adapter.start_specs()[0].revision_id, current);
    assert_eq!(
        adapter.start_specs()[0].binding.model,
        "synthetic-new-model"
    );
    assert!(adapter.start_specs()[0].context.memory_context[0].contains(&x.fact.to_string()));
    assert_eq!(
        x.counts().await,
        (1, 2, 1),
        "model update must not republish or replace approval"
    );
}
