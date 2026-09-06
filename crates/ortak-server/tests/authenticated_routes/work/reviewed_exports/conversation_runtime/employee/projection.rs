//! Signed Activity reads over real selected HTTP bytes and durable mixed uses.
use super::*;

fn assert_withheld(memory: &Value) {
    assert_eq!(memory["scope"], "run_scratch_and_reviewed_employee");
    assert_eq!(memory["recall"]["withheld"], true);
    assert_eq!(memory["recall"]["records"], json!([]));
    for record in memory["reviewed"].as_array().unwrap() {
        assert_eq!(record["current"], false);
        assert!(record["content"].is_null() && record["audience"].is_null());
        assert!(record["approval_id"].is_string());
    }
    for canary in [EMPLOYEE_FACT, FACT, PROJECT_FACT, SCRATCH, ANSWER] {
        assert!(!memory.to_string().contains(canary), "disclosed {canary}");
    }
}

#[tokio::test]
#[ignore = "requires disposable55432 with employee v5 candidates; local HTTP only"]
async fn employee_runtime_v5_projection_mixed_work_is_requester_only_and_retains_history() {
    let x = EmployeeFixture::new("relationship", true).await;
    let c = &x.c;
    // Another genuine current channel member and project viewer passes the run
    // audience gate. That grant alone must not expose the relationship context.
    grant(&c.x.f, c.x.project, &c.x.f.reader, "viewer").await;
    let reader_app = work_app(&c.x.f, false, Role::Reader, vec![c.x.f.channel]);
    let item = c.ready_work().await;
    let (run, _) = crate::work::execution::fixture::queue(&c.x.f, &c.x.app, &item).await;
    let reference = c.dispatch_with(run, &x.memory()).await;
    let frozen = c.bytes(run).await;
    let (status, body) = c.read(run).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let before = &body["memory"];
    assert_eq!(before["scope"], "run_scratch_and_reviewed_employee");
    assert_eq!(before["recall"]["records"][0]["content"]["text"], SCRATCH);
    let records = before["reviewed"].as_array().unwrap();
    assert_eq!(records.len(), 3);
    for (record, fact, scope, content) in [
        (&records[0], x.fact, "employee", EMPLOYEE_FACT),
        (&records[1], c.fact, "conversation", FACT),
        (&records[2], c.x.fact, "project", PROJECT_FACT),
    ] {
        assert_eq!(record["fact_id"], json!(fact));
        assert_eq!(record["audience_kind"], scope);
        assert_eq!(record["current"], true);
        assert_eq!(record["content"]["text"], content);
    }
    assert_eq!(records[0]["audience"]["kind"], "relationship");
    assert_eq!(
        records[0]["audience"]["human_public_key"],
        c.x.f.operator.public_key().to_hex()
    );
    assert_eq!(
        records[0]["audience"]["destination_channel_id"],
        json!(c.x.f.channel)
    );
    assert_eq!(records[1]["audience"]["kind"], "thread");
    assert!(records[2]["audience"].is_null());
    let (status, other) = get(&reader_app, &c.x.f.reader, &format!("/api/v1/runs/{run}")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the run itself remains readable: {other}"
    );
    assert_withheld(&other["memory"]);
    assert_eq!(other["memory"]["reviewed"][0]["fact_id"], json!(x.fact));
    assert!(
        c.current(run).await,
        "a different viewer cannot revoke the run"
    );
    c.complete(run, &reference).await;
    assert_eq!(
        ortak_work::schedule_work_outputs(&c.x.f.control, &c.x.scope, 8)
            .await
            .unwrap()
            .materialized,
        1
    );
    let (status, after) = c.read(run).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(after["memory"], *before);
    assert_eq!(after["work_output"]["status"], "materialized");
    assert_eq!(c.bytes(run).await, frozen);
    assert_eq!(x.uses(run).await, vec![(0, x.fact)]);
    x.prove_target_unchanged().await;
}

#[tokio::test]
#[ignore = "requires disposable55432 with employee v5 candidates; local HTTP only"]
async fn employee_runtime_v5_projection_stop_withholds_scratch_and_acknowledged_write() {
    let x = EmployeeFixture::new("experience", false).await;
    let c = &x.c;
    let (run, reference) = c.start_office_with(&x.memory()).await;
    let frozen = c.bytes(run).await;
    c.complete(run, &reference).await;
    assert_eq!(
        ortak_runtime::office_output::schedule_office_outputs(&c.x.f.control, &c.x.scope, 1)
            .await
            .unwrap()
            .enqueued,
        1
    );
    let publisher = FakeOfficePublisher::new();
    let delivery = OfficeDeliveryService::new(
        c.x.f.control.clone(),
        &c.signer,
        &publisher,
        DeliveryConfig::default(),
    );
    assert!(
        ortak_runtime::office_delivery::deliver_one_office_output(
            &c.x.f.control,
            &c.x.scope,
            "v5-projection",
            &delivery
        )
        .await
        .unwrap()
    );
    assert_eq!(
        ortak_runtime::memory_output::schedule_memory_output(&c.x.f.control, &c.memory, &c.x.scope)
            .await
            .unwrap()
            .acknowledged,
        1
    );
    let (status, before) = c.read(run).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    let memory = &before["memory"];
    assert_eq!(memory["reviewed"][0]["content"]["text"], EMPLOYEE_FACT);
    assert_eq!(memory["reviewed"][0]["audience"]["kind"], "experience");
    assert!(memory["reviewed"][0]["audience"]["human_public_key"].is_null());
    assert_eq!(memory["write"]["status"], "acknowledged");
    assert_eq!(memory["write"]["content"]["text"], ANSWER);
    stop(c, &x.app, x.fact).await;
    assert!(!c.current(run).await);
    let (status, after) = c.read(run).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_withheld(&after["memory"]);
    assert_eq!(after["memory"]["write"]["status"], "acknowledged");
    assert_eq!(after["memory"]["write"]["withheld"], true);
    assert_eq!(after["memory"]["write"]["content"]["text"], "");
    assert_eq!(
        after["memory"]["write"]["receipt"],
        memory["write"]["receipt"]
    );
    assert_eq!(
        after["memory"]["write"]["source"],
        memory["write"]["source"]
    );
    assert_eq!(
        after["memory"]["reviewed"][0]["approval_id"],
        memory["reviewed"][0]["approval_id"]
    );
    assert_eq!(c.bytes(run).await, frozen);
    assert_eq!(x.uses(run).await, vec![(0, x.fact)]);
    assert_eq!(x.selected().len(), 1);
}
