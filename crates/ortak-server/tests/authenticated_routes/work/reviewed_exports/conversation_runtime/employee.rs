//! Three production-composition cases. Only the remote and runtime transports
//! are controlled; signed approvals, leases, selection, snapshots and uses are real.
use super::*;
use crate::employee_memory::exports::remote::Remote;
use chrono::DateTime;
use ortak_server::employee_memory_exports as employee_exports;

#[path = "employee/adapter.rs"]
mod adapter;
#[path = "employee/fixture.rs"]
mod fixture;
#[path = "employee/projection.rs"]
mod projection;
#[path = "employee/read_state.rs"]
mod read_state;
use adapter::MixedMemory;
use fixture::*;

const PROJECT_FACT: &str = "Reviewed deployment fact";

#[tokio::test]
#[ignore = "requires disposable55432 immutable76 plus employee storage/source/protocol/runtime candidates; local HTTP only"]
async fn employee_runtime_v5_mixed_work_materializes_and_legacy_fallback_has_no_employee_io() {
    let x = EmployeeFixture::new("relationship", true).await;
    let c = &x.c;
    let memory = x.memory();
    let item = c.ready_work().await;
    let (run, _) = crate::work::execution::fixture::queue(&c.x.f, &c.x.app, &item).await;
    let reference = c.dispatch_with(run, &memory).await;
    let frozen = c.bytes(run).await;
    let wire: Value = serde_json::from_slice(&frozen).unwrap();
    assert_eq!(wire["version"], 5);
    assert!(wire.get("reviewed").is_none() && wire.get("conversation").is_none());
    let records = wire["employee"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 3);
    for (record, scope, fact, text) in [
        (&records[0], "employee", x.fact, EMPLOYEE_FACT),
        (&records[1], "conversation", c.fact, FACT),
        (&records[2], "project", c.x.fact, PROJECT_FACT),
    ] {
        assert_eq!(record["scope"], scope);
        assert_eq!(record["record"]["pin"]["fact_id"], json!(fact));
        assert_eq!(record["record"]["content"], text);
    }
    let provenance: Value =
        serde_json::from_str(records[0]["record"]["provenance"].as_str().unwrap()).unwrap();
    assert_eq!(provenance["audience"]["kind"], "relationship");
    assert_eq!(
        provenance["audience"]["human_public_key"],
        c.x.f.operator.public_key().to_hex()
    );
    let origin: Value = serde_json::from_str(wire["employee"]["origin"].as_str().unwrap()).unwrap();
    assert_eq!(
        origin["requester_public_key"],
        c.x.f.operator.public_key().to_hex()
    );
    assert_eq!(origin["source"]["event_id"], c.x.source);
    assert_eq!(x.uses(run).await, vec![(0, x.fact)]);
    let legacy: Vec<(i32, Uuid)> = sqlx::query_as("SELECT ordinal,fact_id FROM run_reviewed_memory_uses WHERE company_id=$1 AND run_id=$2 ORDER BY ordinal")
        .bind(c.x.f.company).bind(run).fetch_all(&c.x.f.pool).await.unwrap();
    assert_eq!(legacy, vec![(1, c.fact), (2, c.x.fact)]);
    assert!(c.current(run).await);
    let calls = x.selected();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["record_ids"], json!([x.fact]));
    assert_eq!(
        calls[0]["human_public_key"],
        c.x.f.operator.public_key().to_hex()
    );
    assert!(!calls[0].to_string().contains(EMPLOYEE_FACT));
    let specs = c.runtime.start_specs();
    let rendered: Vec<Value> = specs[0]
        .context
        .memory_context
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(
        rendered
            .iter()
            .map(|r| r["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "reviewed_employee_memory",
            "reviewed_conversation_memory",
            "reviewed_project_memory",
            "run_scratch_memory"
        ]
    );
    assert_eq!(rendered[0]["record"], records[0]["record"]);
    c.complete(run, &reference).await;
    assert_eq!(
        ortak_work::schedule_work_outputs(&c.x.f.control, &c.x.scope, 8)
            .await
            .unwrap()
            .materialized,
        1
    );
    let artifact: (Uuid, Vec<u8>) =
        sqlx::query_as("SELECT id,content_bytes FROM artifacts WHERE company_id=$1 AND run_id=$2")
            .bind(c.x.f.company)
            .bind(run)
            .fetch_one(&c.x.f.pool)
            .await
            .unwrap();
    assert_eq!(artifact.1, ANSWER.as_bytes());
    assert!(c.current(run).await);
    assert_eq!(c.bytes(run).await, frozen);

    // Unconfigured Office still uses the unchanged v4 path even though a valid
    // employee target and remote record now exist for the same destination.
    let (office, office_ref) = c.start_office().await;
    let old_wire = c.wire(office).await;
    assert_eq!(old_wire["version"], 4);
    assert!(old_wire.get("employee").is_none());
    assert!(x.uses(office).await.is_empty());
    assert_eq!(x.selected().len(), 1);
    c.complete(office, &office_ref).await;

    // Explicitly configured employee recall cannot infer a requester/source
    // for a manual Work item. Legacy project recall remains format3.
    let (status, manual) = post(
        &c.x.app,
        &c.x.f.operator,
        &format!("/api/v1/projects/{}/work-items", c.x.project),
        &item_body("Deployment manual task"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{manual}");
    let manual = &manual["work_item"];
    let (status, assigned) = post(&c.x.app, &c.x.f.operator, &format!("/api/v1/work-items/{}/assignments", id(manual)),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(manual),"employee_id":"cem","role":"owner"})).await;
    assert_eq!(status, StatusCode::OK, "{assigned}");
    let manual = transition(&c.x.f, &c.x.app, assigned["work_item"].clone(), "ready").await;
    let (manual_run, _) = crate::work::execution::fixture::queue(&c.x.f, &c.x.app, &manual).await;
    let manual_ref = c.dispatch_with(manual_run, &memory).await;
    let manual_wire = c.wire(manual_run).await;
    assert_eq!(manual_wire["version"], 3);
    assert!(manual_wire.get("employee").is_none() && manual_wire.get("conversation").is_none());
    assert_eq!(
        manual_wire["reviewed"]["records"][0]["pin"]["fact_id"],
        json!(c.x.fact)
    );
    assert!(x.uses(manual_run).await.is_empty());
    assert_eq!(x.selected().len(), 1);
    c.complete(manual_run, &manual_ref).await;
    x.prove_target_unchanged().await;
}

#[tokio::test]
#[ignore = "requires disposable55432 immutable76 plus employee storage/source/protocol/runtime candidates; local HTTP only"]
async fn employee_runtime_v5_office_stop_fences_frozen_output_before_remote_erasure() {
    use ortak_runtime::office_delivery::deliver_one_office_output;
    let x = EmployeeFixture::new("experience", false).await;
    let c = &x.c;
    let (run, reference) = c.start_office_with(&x.memory()).await;
    let frozen = c.bytes(run).await;
    assert_eq!(c.wire(run).await["version"], 5);
    assert_eq!(x.uses(run).await, vec![(0, x.fact)]);
    c.complete(run, &reference).await;
    assert_eq!(
        ortak_runtime::office_output::schedule_office_outputs(&c.x.f.control, &c.x.scope, 1)
            .await
            .unwrap()
            .enqueued,
        1
    );
    let publisher = FakeOfficePublisher::new();
    publisher.fail_next(1);
    let service = OfficeDeliveryService::new(
        c.x.f.control.clone(),
        &c.signer,
        &publisher,
        DeliveryConfig::default(),
    );
    assert!(
        deliver_one_office_output(&c.x.f.control, &c.x.scope, "employee-first", &service)
            .await
            .unwrap()
    );
    assert_eq!(publisher.publish_calls(), 1);
    assert_eq!(c.signer.sign_calls(), 1);
    let signed: Vec<u8> = sqlx::query_scalar("SELECT signed_event_bytes FROM outbox WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'")
        .bind(c.x.f.company).bind(run).fetch_one(&c.x.f.pool).await.unwrap();
    stop(c, &x.app, x.fact).await;
    assert!(
        !c.current(run).await,
        "local Stop fences use before the remote withdrawal ACK"
    );
    let acked: i64 = sqlx::query_scalar("SELECT count(*) FROM employee_reviewed_memory_export_receipts WHERE company_id=$1 AND fact_id=$2 AND action='withdraw'")
        .bind(c.x.f.company).bind(x.fact).fetch_one(&c.x.f.pool).await.unwrap();
    assert_eq!(acked, 0);
    sqlx::query("UPDATE outbox SET retry_after=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'")
        .bind(c.x.f.company).bind(run).execute(&c.x.f.pool).await.unwrap();
    assert!(
        deliver_one_office_output(&c.x.f.control, &c.x.scope, "employee-stopped", &service)
            .await
            .unwrap()
    );
    assert_eq!(
        publisher.publish_calls(),
        1,
        "retained signed bytes cannot authorize a second send"
    );
    assert_eq!(c.signer.sign_calls(), 1);
    let retained: (String, String, Vec<u8>) = sqlx::query_as("SELECT state,last_error,signed_event_bytes FROM outbox WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'")
        .bind(c.x.f.company).bind(run).fetch_one(&c.x.f.pool).await.unwrap();
    assert_eq!(
        retained,
        (
            "pending".into(),
            "office_delivery_authority_refused".into(),
            signed
        )
    );
    assert_eq!(c.bytes(run).await, frozen);
    assert_eq!(x.uses(run).await, vec![(0, x.fact)]);
    assert!(employee_exports::schedule_one(
        &c.x.f.control,
        &c.x.scope,
        &employee_exports::HonchoEmployeeExportAdapter::new(&x.remote.service)
    )
    .await
    .unwrap());
    assert!(!c.current(run).await);
    x.prove_target_unchanged().await;
}

#[tokio::test]
#[ignore = "requires disposable55432 immutable76 plus employee storage/source/protocol/runtime candidates; local HTTP only"]
async fn employee_runtime_v5_stop_during_selected_http_refuses_freeze_and_held_start() {
    let x = EmployeeFixture::new("relationship", true).await;
    let c = &x.c;
    let memory = x.memory();
    memory
        .stop_after_read
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let item = c.ready_work().await;
    let (run, _) = crate::work::execution::fixture::queue(&c.x.f, &c.x.app, &item).await;
    let leases =
        c.x.f
            .control
            .claim_runtime_dispatches(
                &c.x.scope,
                "fake-runtime",
                "employee-read-race",
                Duration::from_secs(60),
                8,
            )
            .await
            .unwrap();
    assert_eq!(leases.len(), 1);
    let outcome = RunSupervisor::new(
        c.x.f.control.clone(),
        &c.runtime,
        SupervisorConfig::default(),
    )
    .with_run_memory(ReviewedRunMemory::new(
        &memory,
        c.x.f.control.clone(),
        c.x.scope.clone(),
    ))
    .dispatch(&c.x.scope, &leases[0])
    .await
    .unwrap();
    assert!(
        matches!(
            outcome,
            DispatchOutcome::Refused {
                refusal: DispatchRefusal::MemoryContextRejected,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(
        x.selected().len(),
        1,
        "the actual selected HTTP response raced with an authenticated Stop"
    );
    assert!(c.runtime.start_specs().is_empty());
    let rows: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
        (SELECT count(*) FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2),
        (SELECT count(*) FROM run_employee_reviewed_memory_uses WHERE company_id=$1 AND run_id=$2),
        (SELECT count(*) FROM run_reviewed_memory_uses WHERE company_id=$1 AND run_id=$2),
        (SELECT count(*) FROM artifacts WHERE company_id=$1 AND run_id=$2)",
    )
    .bind(c.x.f.company)
    .bind(run)
    .fetch_one(&c.x.f.pool)
    .await
    .unwrap();
    assert_eq!(rows, (0, 0, 0, 0));
    x.prove_target_unchanged().await;
}
