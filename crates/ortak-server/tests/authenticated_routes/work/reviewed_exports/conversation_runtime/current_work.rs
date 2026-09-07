//! Positive mixed-scope Work history through signed commands and real schedulers.
use super::*;

const PROJECT_FACT: &str = "Reviewed deployment fact";

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with conversation76"]
async fn conversation_runtime_current_promoted_work_materializes_mixed_memory_and_retains_completed_history(
) {
    let mut c = ConversationFixture::new().await;
    c.memory.project_enabled = true;
    c.x.target.runtime_consumption_enabled = true;
    super::super::conversation_publication::advertise(&c.x).await;
    // ExportFixture already approved this legacy project fact through the signed
    // API. Publication and acknowledgement still pass through the real outbox.
    c.x.publish().await;
    let remote = ObservedAdapter::default();
    assert!(schedule_one(&c.x.f.control, &c.x.scope, &remote)
        .await
        .unwrap());
    assert_eq!(remote.calls.lock().unwrap().len(), 1);
    c.memory
        .contents
        .lock()
        .unwrap()
        .insert(c.x.fact, PROJECT_FACT.into());

    let item = c.ready_work().await;
    let work_id = id(&item);
    let (run, _) = crate::work::execution::fixture::queue(&c.x.f, &c.x.app, &item).await;
    let reference = c.dispatch(run).await;
    let frozen = c.bytes(run).await;
    let wire: Value = serde_json::from_slice(&frozen).unwrap();
    assert_eq!(wire["version"], 4);
    assert!(wire.get("reviewed").is_none());
    let records = wire["conversation"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["scope"], "conversation");
    assert_eq!(records[0]["record"]["pin"]["fact_id"], json!(c.fact));
    assert_eq!(records[0]["record"]["content"], FACT);
    assert_eq!(records[1]["scope"], "project");
    assert_eq!(records[1]["record"]["pin"]["fact_id"], json!(c.x.fact));
    assert_eq!(records[1]["record"]["content"], PROJECT_FACT);
    assert_eq!(
        c.memory.selected.lock().unwrap().as_slice(),
        &[vec![c.fact, c.x.fact]]
    );
    let specs = c.runtime.start_specs();
    assert_eq!(specs.len(), 1);
    let rendered: Vec<Value> = specs[0]
        .context
        .memory_context
        .iter()
        .map(|value| serde_json::from_str(value).unwrap())
        .collect();
    assert_eq!(rendered.len(), 3);
    assert_eq!(rendered[0]["type"], "run_scratch_memory");
    assert_eq!(rendered[1]["type"], "reviewed_conversation_memory");
    assert_eq!(rendered[2]["type"], "reviewed_project_memory");
    let uses: Vec<(i32, Uuid)> = sqlx::query_as(
        "SELECT ordinal,fact_id FROM run_reviewed_memory_uses
        WHERE company_id=$1 AND run_id=$2 ORDER BY ordinal",
    )
    .bind(c.x.f.company)
    .bind(run)
    .fetch_all(&c.x.f.pool)
    .await
    .unwrap();
    assert_eq!(uses, vec![(0, c.fact), (1, c.x.fact)]);
    assert_current_memory(&c, run, &frozen).await;

    c.complete(run, &reference).await;
    assert!(c.current(run).await);
    let report = ortak_work::schedule_work_outputs(&c.x.f.control, &c.x.scope, 8)
        .await
        .unwrap();
    assert_eq!((report.attempted, report.materialized), (1, 1));
    assert_eq!(
        ortak_work::schedule_work_outputs(&c.x.f.control, &c.x.scope, 8)
            .await
            .unwrap()
            .attempted,
        0
    );
    let (status, result) = get(
        &c.x.app,
        &c.x.f.operator,
        &format!("/api/v1/work-items/{work_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let mut current = result["work_item"].clone();
    assert_eq!(current["state"], "review");
    assert_eq!(
        current["criteria"], item["criteria"],
        "runtime cannot satisfy human acceptance"
    );
    assert_eq!(
        current["approvals"], item["approvals"],
        "runtime cannot approve its own output"
    );
    let (_, run_body) = c.read(run).await;
    assert_eq!(run_body["work_output"]["status"], "materialized");
    let artifact = run_body["work_output"]["artifact_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_artifact(&c, work_id, &artifact).await;
    assert_current_memory(&c, run, &frozen).await;

    let criterion = current["criteria"][0]["id"].as_str().unwrap();
    let satisfied = post(
        &c.x.app,
        &c.x.f.operator,
        &format!("/api/v1/work-items/{work_id}/criteria/{criterion}/satisfy"),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&current)}),
    )
    .await;
    assert_eq!(satisfied.0, StatusCode::OK, "{satisfied:?}");
    current = satisfied.1["work_item"].clone();
    let approval = current["approvals"][0]["id"].as_str().unwrap();
    let approved = post(&c.x.app, &c.x.f.operator,
        &format!("/api/v1/work-items/{work_id}/approvals/{approval}/resolve"),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&current),"decision":"approve"})).await;
    assert_eq!(approved.0, StatusCode::OK, "{approved:?}");
    assert_current_memory(&c, run, &frozen).await;
    let completed = transition(
        &c.x.f,
        &c.x.app,
        approved.1["work_item"].clone(),
        "completed",
    )
    .await;
    assert_eq!(completed["state"], "completed");
    assert_current_memory(&c, run, &frozen).await;
    assert_artifact(&c, work_id, &artifact).await;
    assert_eq!(
        c.runtime.start_specs().len(),
        1,
        "human review never reruns the provider"
    );
}

async fn assert_current_memory(c: &ConversationFixture, run: Uuid, frozen: &[u8]) {
    assert!(c.current(run).await);
    let (status, body) = c.read(run).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let memory = &body["memory"];
    assert_eq!(memory["scope"], "run_scratch_and_reviewed_conversation");
    let records = memory["reviewed"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["fact_id"], json!(c.fact));
    assert_eq!(records[0]["current"], true);
    assert_eq!(records[0]["content"]["text"], FACT);
    assert_eq!(records[0]["audience_kind"], "conversation");
    assert_eq!(records[0]["audience"]["kind"], "thread");
    assert_eq!(records[0]["audience"]["channel_id"], json!(c.x.f.channel));
    assert_eq!(records[0]["audience"]["thread_root_event_id"], c.x.source);
    assert_eq!(records[1]["fact_id"], json!(c.x.fact));
    assert_eq!(records[1]["current"], true);
    assert_eq!(records[1]["content"]["text"], PROJECT_FACT);
    assert_eq!(records[1]["audience_kind"], "project");
    assert!(records[1]["audience"].is_null());
    assert_eq!(memory["recall"]["records"][0]["content"]["text"], SCRATCH);
    assert_eq!(c.bytes(run).await, frozen);
}

async fn assert_artifact(c: &ConversationFixture, work: Uuid, artifact: &str) {
    let path = format!("/api/v1/work-items/{work}/artifacts/{artifact}");
    let output =
        c.x.app
            .clone()
            .oneshot(signed(&c.x.f.operator, "GET", &path, "", false))
            .await
            .unwrap();
    assert_eq!(output.status(), StatusCode::OK);
    assert_eq!(
        output.headers()["content-type"],
        "text/plain; charset=utf-8"
    );
    assert_eq!(to_bytes(output.into_body(), 32768).await.unwrap(), ANSWER);
}
