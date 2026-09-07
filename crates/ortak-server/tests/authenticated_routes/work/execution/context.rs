//! Real signed Work selection and current-source gates, through the supervisor.
use super::*;
#[path = "context_retry.rs"]
mod retry;

async fn reply(f: &Fixture, root: &str, content: &str) -> String {
    let event = EventBuilder::new(Kind::Custom(9), content)
        .tags([
            Tag::parse(["h", &f.channel.to_string()]).unwrap(),
            Tag::parse(["e", root, "", "reply"]).unwrap(),
            Tag::parse(["nonce", &Uuid::new_v4().to_string()]).unwrap(),
        ])
        .sign_with_keys(&f.operator)
        .unwrap();
    let at = chrono::DateTime::from_timestamp(event.created_at.as_secs() as i64, 0).unwrap();
    let root_at: chrono::DateTime<Utc> = sqlx::query_scalar(
        "SELECT created_at FROM events WHERE community_id=$1 AND id=decode($2,'hex')",
    )
    .bind(f.community)
    .bind(root)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id) VALUES($1,$2,$3,$4,9,$5,$6,$7,$8)")
        .bind(f.community).bind(event.id.to_bytes().as_slice()).bind(event.pubkey.to_bytes().as_slice()).bind(at)
        .bind(serde_json::to_value(&event.tags).unwrap()).bind(content).bind(event.sig.serialize().as_slice())
        .bind(f.channel).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO thread_metadata(community_id,event_id,event_created_at,channel_id,parent_event_id,parent_event_created_at,root_event_id,root_event_created_at,depth) VALUES($1,$2,$3,$4,decode($5,'hex'),$6,decode($5,'hex'),$6,1)")
        .bind(f.community).bind(event.id.to_bytes().as_slice()).bind(at).bind(f.channel)
        .bind(root).bind(root_at).execute(&f.pool).await.unwrap();
    event.id.to_hex()
}

async fn saved(f: &Fixture, app: &Router, item: &Value) -> Value {
    let (status, body) = get(
        app,
        &f.operator,
        &format!("/api/v1/work-items/{}", id(item)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["work_item"].clone()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn work_reference_pins_same_item_artifact_and_thread_before_request_and_revision_enters_review(
) {
    let f = Fixture::new().await;
    let employee = employee(&f).await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let (project, item) = ready(&f, &app).await;
    let source = item["source_message_id"].as_str().unwrap();
    let selected = reply(&f, source, "Keep the cafe name Mavi Fincan").await;
    let other = super::super::boundaries::source_message(&f, f.channel).await;
    reply(&f, &other, "Unrelated team translation").await;
    let (first, _) = queue(&f, &app, &item).await;
    let late = reply(&f, source, "Arrived after the first execution request").await;
    let (adapter, memory, reference) = start(&f, &employee, first).await;
    let specs = adapter.start_specs();
    let context = specs[0].context.work_context.as_ref().unwrap();
    assert_eq!(context.project_id, project);
    assert_eq!(context.messages.len(), 2);
    assert!(context.messages.iter().any(|m| m.message_id == selected));
    assert!(!context
        .messages
        .iter()
        .any(|m| m.message_id == late || m.message_id == other));
    complete(
        &f,
        &adapter,
        &memory,
        first,
        &reference,
        BoundedText::raw("Mavi Fincan: a cozy neighborhood break."),
    )
    .await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    assert_eq!(
        schedule_work_outputs(&f.control, &scope, 8)
            .await
            .unwrap()
            .materialized,
        1
    );
    let reviewed = saved(&f, &app, &item).await;
    assert_eq!(reviewed["state"], "review");
    let editable = transition(&f, &app, reviewed, "in_progress").await;
    let (status, edited) = post(&app, &f.operator, &format!("/api/v1/work-items/{}/definition", id(&item)),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&editable),"definition":{
            "title":"Shorten the previous deliverable","description":"Keep the cafe name; use four words.",
            "criteria":editable["criteria"].as_array().unwrap().iter().map(|c| json!({"id":c["id"],"text":c["text"]})).collect::<Vec<_>>(),
            "additional_criteria":[]}})).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    let (second, command) = queue(&f, &app, &edited["work_item"]).await;
    let after = reply(
        &f,
        source,
        "Later revision must not replace frozen references",
    )
    .await;
    let (runtime2, memory2, reference2) = start(&f, &employee, second).await;
    let specs2 = runtime2.start_specs();
    let spec = &specs2[0];
    assert!(spec.input.contains("Shorten the previous deliverable"));
    let context2 = spec.context.work_context.as_ref().unwrap();
    let artifact = context2.prior_artifact.as_ref().unwrap();
    assert_eq!(artifact.run_id, first);
    assert_eq!(artifact.execution_version, version(&item) + 1);
    assert_eq!(artifact.content, "Mavi Fincan: a cozy neighborhood break.");
    assert_eq!(
        artifact.sha256,
        hex::encode(Sha256::digest(artifact.content.as_bytes()))
    );
    assert_eq!(context2.messages.len(), 3);
    let (status, listed) = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}/executions", id(&item)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let projected = &listed["executions"][0]["reference_context"];
    assert_eq!(projected["status"], "ready");
    assert_eq!(projected["artifact_id"], artifact.artifact_id.to_string());
    assert_eq!(
        projected["artifact_execution_version"],
        artifact.execution_version
    );
    assert_eq!(projected["message_ids"].as_array().unwrap().len(), 3);
    assert!(!projected.to_string().contains(&artifact.content));
    assert!(!context2
        .messages
        .iter()
        .any(|m| m.message_id == after || m.message_id == other));
    let bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT spec_bytes FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.company)
    .bind(second)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let (status, replay) = post(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}/executions", id(&item)),
        &command,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["execution"]["run_id"], second.to_string());
    let replay_bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT spec_bytes FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.company)
    .bind(second)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(bytes, replay_bytes);
    complete(
        &f,
        &runtime2,
        &memory2,
        second,
        &reference2,
        BoundedText::raw("Mavi Fincan, cozy breaks."),
    )
    .await;
    assert_eq!(
        schedule_work_outputs(&f.control, &scope, 8)
            .await
            .unwrap()
            .materialized,
        1
    );
    let final_item = saved(&f, &app, &item).await;
    assert_eq!(final_item["state"], "review");
    assert_eq!(final_item["criteria"], item["criteria"]);
    assert_eq!(final_item["approvals"], item["approvals"]);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn deleting_selected_thread_reference_after_completion_blocks_artifact_materialization() {
    let f = Fixture::new().await;
    let employee = employee(&f).await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let (_, item) = ready(&f, &app).await;
    let selected = reply(
        &f,
        item["source_message_id"].as_str().unwrap(),
        "Selected reference",
    )
    .await;
    let (run, _) = queue(&f, &app, &item).await;
    let (adapter, memory, reference) = start(&f, &employee, run).await;
    assert!(adapter.start_specs()[0]
        .context
        .work_context
        .as_ref()
        .unwrap()
        .messages
        .iter()
        .any(|m| m.message_id == selected));
    complete(
        &f,
        &adapter,
        &memory,
        run,
        &reference,
        BoundedText::raw("Output derived from deleted reference"),
    )
    .await;
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=decode($2,'hex')")
        .bind(f.community).bind(selected).execute(&f.pool).await.unwrap();
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let report = schedule_work_outputs(&f.control, &scope, 8).await.unwrap();
    assert_eq!((report.attempted, report.materialized), (1, 0));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM artifacts WHERE company_id=$1 AND run_id=$2")
            .bind(f.company)
            .bind(run)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    assert_eq!(saved(&f, &app, &item).await["state"], "in_progress");
    let (status, listed) = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}/executions", id(&item)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(
        listed["executions"][0]["reference_context"]["status"],
        "unavailable"
    );
    assert_eq!(
        listed["executions"][0]["reference_context"]["message_ids"],
        json!([])
    );
}
