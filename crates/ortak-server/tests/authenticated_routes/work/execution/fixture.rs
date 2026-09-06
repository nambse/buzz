use super::*;
use ortak_domain::{CredentialRef, Employee, EmployeeManifest, EmployeeStatus};

pub(in crate::work) async fn employee(f: &Fixture) -> Employee {
    employee_with_memory_adapter(f, "fake-memory").await
}

pub(in crate::work) async fn employee_with_memory_adapter(
    f: &Fixture,
    memory_adapter: &str,
) -> Employee {
    employee_configured(f, memory_adapter, None, None, false).await
}

pub(in crate::work) async fn employee_with_memory_and_signer(
    f: &Fixture,
    memory_adapter: &str,
    public_key: &str,
    signer_ref: &str,
) -> Employee {
    employee_configured(
        f,
        memory_adapter,
        None,
        Some((public_key, signer_ref)),
        false,
    )
    .await
}

pub(in crate::work) async fn employee_with_owned_memory_and_signer(
    f: &Fixture,
    public_key: &str,
    signer_ref: &str,
) -> Employee {
    employee_configured(f, "honcho", None, Some((public_key, signer_ref)), true).await
}

pub(in crate::work) async fn employee_with_workspace(f: &Fixture, reference: &str) -> Employee {
    employee_configured(f, "fake-memory", Some(reference), None, false).await
}

async fn employee_configured(
    f: &Fixture,
    memory_adapter: &str,
    workspace: Option<&str>,
    signer: Option<(&str, &str)>,
    owned_memory: bool,
) -> Employee {
    let mut employee: Employee = serde_yaml::from_str::<EmployeeManifest>(include_str!(
        "../../../../../../config/employees/cem.yaml"
    ))
    .unwrap()
    .employee;
    employee.status = EmployeeStatus::Active;
    employee.runtime.adapter = "fake-runtime".into();
    employee.runtime.profile_ref = Some("fake://work-profile".into());
    employee.runtime.credential_refs.clear();
    if let Some(reference) = workspace {
        employee.runtime.workspace_ref = reference.into();
        employee.permissions = ortak_domain::PermissionPolicy {
            allowed_tools: vec![ortak_domain::ToolCapability::Files],
            allowed_workspaces: vec![reference.into()],
            ..Default::default()
        };
    }

    employee.office.public_key = Keys::generate().public_key().to_hex();
    employee.office.signer_ref = CredentialRef::parse("credential://fixture/work-signer").unwrap();
    if let Some((public_key, signer_ref)) = signer {
        employee.office.public_key = public_key.to_owned();
        employee.office.signer_ref = CredentialRef::parse(signer_ref).unwrap();
    }
    employee.memory.as_mut().unwrap().adapter = memory_adapter.into();
    if owned_memory {
        let memory = employee.memory.as_mut().unwrap();
        memory.options.clear();
        memory.workspace = format!("employee_runtime_{}", Uuid::new_v4().simple());
    }
    let revision = Uuid::new_v4();
    let manifest = serde_json::to_value(&employee).unwrap();
    sqlx::query("INSERT INTO employee_revisions(company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode) VALUES($1,$2,'cem',2,$3,$4,'adopt')")
        .bind(f.company).bind(revision).bind(&manifest).bind(Sha256::digest(manifest.to_string().as_bytes()).to_vec()).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO employee_runtime_bindings(company_id,revision_id,employee_id,adapter,provisioning_mode,profile_ref,model,workspace_ref,credential_refs,options,validated_at)
        VALUES($1,$2,'cem','fake-runtime','adopt',$3,$4,$5,$6,$7,now())")
        .bind(f.company).bind(revision).bind(&employee.runtime.profile_ref).bind(&employee.runtime.model).bind(&employee.runtime.workspace_ref)
        .bind(json!(employee.runtime.credential_refs)).bind(json!(employee.runtime.options)).execute(&f.pool).await.unwrap();
    let memory = employee.memory.as_ref().unwrap();
    sqlx::query("INSERT INTO employee_memory_bindings(company_id,revision_id,employee_id,adapter,provisioning_mode,endpoint_ref,workspace,user_peer,employee_peer,options,validated_at)
        VALUES($1,$2,'cem',$8,'adopt',$3,$4,$5,$6,$7,now())")
        .bind(f.company).bind(revision).bind(&memory.endpoint_ref).bind(&memory.workspace).bind(&memory.user_peer).bind(&memory.employee_peer)
        .bind(json!(memory.options)).bind(memory_adapter).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO employee_office_bindings(company_id,employee_id,revision_id,provisioning_mode,public_key,signer_ref,verified_at) VALUES($1,'cem',$2,'adopt',$3,$4,now())")
        .bind(f.company).bind(revision).bind(hex::decode(&employee.office.public_key).unwrap()).bind(employee.office.signer_ref.as_str()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE employees SET active_revision_id=$2 WHERE company_id=$1 AND id='cem'")
        .bind(f.company)
        .bind(revision)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO channel_members(community_id,channel_id,pubkey,role) VALUES($1,$2,$3,'bot')",
    )
    .bind(f.community)
    .bind(f.channel)
    .bind(hex::decode(&employee.office.public_key).unwrap())
    .execute(&f.pool)
    .await
    .unwrap();
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let capture = f
        .control
        .begin_routing_capture(&scope, &[f.channel], &[employee.id.clone()])
        .await
        .unwrap();
    let progress = f
        .control
        .start_inbox_reconciliation(&scope, capture.capture_id, f.channel)
        .await
        .unwrap();
    assert!(progress.completed, "empty pre-promotion fixture channel");
    f.control
        .enable_routing_cohort(&scope, capture.capture_id)
        .await
        .unwrap();
    employee
}

pub(in crate::work) async fn ready(f: &Fixture, app: &Router) -> (Uuid, Value) {
    let project = project(f, app, f.channel).await;
    let source = super::super::boundaries::source_message(f, f.channel).await;
    let mut body = item_body("Produce the actual deliverable");
    body["source_message_id"] = json!(source);
    let (status, result) = post(
        app,
        &f.operator,
        &format!("/api/v1/projects/{project}/promotions"),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{result}");
    let item = result["work_item"].clone();
    let (status,assigned)=post(app,&f.operator,&format!("/api/v1/work-items/{}/assignments",id(&item)),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&item),"employee_id":"cem","role":"owner"})).await;
    assert_eq!(status, StatusCode::OK, "{assigned}");
    (
        project,
        transition(f, app, assigned["work_item"].clone(), "ready").await,
    )
}

pub(super) fn request(item: &Value) -> Value {
    json!({"operation_id":Uuid::new_v4(),"expected_version":version(item),"employee_id":"cem"})
}
pub(in crate::work) async fn queue(f: &Fixture, app: &Router, item: &Value) -> (Uuid, Value) {
    let command = request(item);
    let (status, body) = post(
        app,
        &f.operator,
        &format!("/api/v1/work-items/{}/executions", id(item)),
        &command,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (
        Uuid::parse_str(body["execution"]["run_id"].as_str().unwrap()).unwrap(),
        command,
    )
}

pub(in crate::work) async fn start(
    f: &Fixture,
    employee: &Employee,
    run: Uuid,
) -> (FakeRuntimeAdapter, FakeMemoryAdapter, RuntimeRunRef) {
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let adapter = FakeRuntimeAdapter::new().with_existing_profile("fake://work-profile", true);
    let memory = FakeMemoryAdapter::new().with_existing_binding(employee.memory.as_ref().unwrap());
    let leases = f
        .control
        .claim_runtime_dispatches(
            &scope,
            "fake-runtime",
            "work-test",
            Duration::from_secs(60),
            8,
        )
        .await
        .unwrap();
    assert_eq!(leases.len(), 1);
    assert_eq!(
        leases[0].kind,
        ortak_control::outbox::OutboxKind::WorkRunDispatch
    );
    let supervisor = RunSupervisor::new(f.control.clone(), &adapter, SupervisorConfig::default())
        .with_memory(&memory);
    let outcome = supervisor.dispatch(&scope, &leases[0]).await.unwrap();
    let DispatchOutcome::Started {
        run_id,
        runtime_run_ref,
    } = outcome
    else {
        panic!("{outcome:?}")
    };
    assert_eq!(run_id, run);
    (adapter, memory, runtime_run_ref)
}

pub(in crate::work) async fn complete(
    f: &Fixture,
    adapter: &FakeRuntimeAdapter,
    memory: &FakeMemoryAdapter,
    run: Uuid,
    reference: &RuntimeRunRef,
    delta: BoundedText,
) {
    adapter.push_event(
        reference,
        RunEventPayload::AssistantDelta {
            turn: 0,
            delta: BoundedText::raw("intermediate"),
        },
    );
    adapter.push_event(
        reference,
        RunEventPayload::AssistantDelta { turn: 1, delta },
    );
    adapter.push_event(
        reference,
        RunEventPayload::DeliveryIntent {
            intent: DeliveryIntentKind::Silent,
            target_ref: None,
        },
    );
    adapter.push_event(
        reference,
        RunEventPayload::RunCompleted {
            delivery_intent: DeliveryIntentKind::Silent,
        },
    );
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    RunSupervisor::new(f.control.clone(), adapter, SupervisorConfig::default())
        .with_memory(memory)
        .pump(&scope, run)
        .await
        .unwrap();
    let status: String =
        sqlx::query_scalar("SELECT status FROM runs WHERE company_id=$1 AND id=$2")
            .bind(f.company)
            .bind(run)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(status, "completed");
}
