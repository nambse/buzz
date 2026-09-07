//! Signed fresh-child creation binds durable history, authority and human review.
use super::*;

#[path = "decomposition/boundaries.rs"]
mod boundaries;
#[path = "decomposition/runtime.rs"]
mod runtime;
#[path = "decomposition/storage.rs"]
mod storage;

fn path(parent: &Value) -> String {
    format!("/api/v1/work-items/{}/children", id(parent))
}
fn body(parent: &Value) -> Value {
    json!({"operation_id":Uuid::new_v4(),"expected_version":version(parent),"child":{
        "title":"Independent child","description":"Explicit child context","priority":"normal",
        "criteria":["Accept child independently"],"approvals":[{"gate":"child_review","required":true}]}})
}
async fn create(f: &Fixture, app: &Router, parent: &Value) -> Value {
    let created = post(app, &f.operator, &path(parent), &body(parent)).await;
    assert_eq!(created.0, StatusCode::CREATED, "{}", created.1);
    created.1
}
async fn read(f: &Fixture, app: &Router, item: &Value) -> Value {
    let result = get(
        app,
        &f.operator,
        &format!("/api/v1/work-items/{}/decomposition", id(item)),
    )
    .await;
    assert_eq!(result.0, StatusCode::OK);
    result.1
}
async fn snapshot(f: &Fixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object(
        'items',(SELECT jsonb_agg(to_jsonb(w) ORDER BY id) FROM work_items w WHERE company_id=$1),
        'history',(SELECT jsonb_agg(to_jsonb(h) ORDER BY work_item_id,sequence) FROM work_item_history h WHERE company_id=$1),
        'links',(SELECT jsonb_agg(to_jsonb(d) ORDER BY child_id) FROM work_decomposition d WHERE company_id=$1),
        'receipts',(SELECT jsonb_agg(to_jsonb(o) ORDER BY operation_id) FROM work_api_operations o WHERE company_id=$1))")
        .bind(f.company).fetch_one(&f.pool).await.unwrap()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with decomposition schema"]
async fn decomposition_atomic_concurrent_replay_preserves_independent_human_acceptance() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let parent = item(&f, &app, project).await;
    let request = body(&parent);
    let path = path(&parent);
    let (a, b) = tokio::join!(
        post(&app, &f.operator, &path, &request),
        post(&app, &f.operator, &path, &request)
    );
    let mut statuses = vec![a.0.as_u16(), b.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, vec![200, 201]);
    assert_eq!(a.1["child"], b.1["child"]);
    assert_eq!(a.1["work_item"], b.1["work_item"]);
    let current = a.1["work_item"].clone();
    assert_eq!(version(&current), version(&parent) + 1);
    assert_eq!(current["state"], parent["state"]);
    assert_eq!(current["criteria"], parent["criteria"]);
    assert_eq!(current["approvals"], parent["approvals"]);
    let child = a.1["child"].clone();
    assert_eq!(child["title"], request["child"]["title"]);
    assert_eq!(child["description"], request["child"]["description"]);
    assert_eq!(
        child["criteria"][0]["text"],
        request["child"]["criteria"][0]
    );
    assert_eq!(
        child["approvals"][0]["gate"],
        request["child"]["approvals"][0]["gate"]
    );
    assert_eq!(child["state"], "proposed");
    assert_eq!(version(&child), 1);
    assert_eq!(child["assignments"], json!([]));
    assert_eq!(child["source_message_id"], Value::Null);
    assert_eq!(
        read(&f, &app, &current).await["children"][0]["id"],
        child["id"]
    );
    assert_eq!(read(&f, &app, &child).await["parent"]["id"], parent["id"]);
    assert_eq!(current["history_omitted"], true);
    let mut child = transition(&f, &app, child, "ready").await;
    child = transition(&f, &app, child, "in_progress").await;
    child = transition(&f, &app, child, "review").await;
    let satisfy = post(
        &app,
        &f.operator,
        &format!(
            "/api/v1/work-items/{}/criteria/{}/satisfy",
            id(&child),
            id(&child["criteria"][0])
        ),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&child)}),
    )
    .await;
    assert_eq!(satisfy.0, StatusCode::OK);
    child = satisfy.1["work_item"].clone();
    let approve=post(&app,&f.operator,&format!("/api/v1/work-items/{}/approvals/{}/resolve",id(&child),id(&child["approvals"][0])),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&child),"decision":"approve","reason":"Human reviewed child"})).await;
    assert_eq!(approve.0, StatusCode::OK);
    child = approve.1["work_item"].clone();
    transition(&f, &app, child, "completed").await;
    let unchanged = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}", id(&parent)),
    )
    .await;
    assert_eq!(
        unchanged.1["work_item"], current,
        "child completion cannot satisfy or advance parent"
    );
    assert_eq!(
        post(&app, &f.operator, &path, &body(&parent)).await.0,
        StatusCode::CONFLICT
    );
}
