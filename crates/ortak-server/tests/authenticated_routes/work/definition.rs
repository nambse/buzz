use super::*;

fn definition_body(item: &Value) -> Value {
    json!({"operation_id":Uuid::new_v4(),"expected_version":version(item),
        "definition":{"title":"Revised title","description":"Revised description",
        "criteria":[{"id":item["criteria"][0]["id"],"text":"Revised criterion"}],
        "additional_criteria":["Second criterion"]}})
}
#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn definition_edit_is_one_authorized_idempotent_atomic_operation() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let initial = item(&f, &app, project).await;
    let path = format!("/api/v1/work-items/{}/definition", id(&initial));
    let body = definition_body(&initial);
    let before_runtime = runtime_counts(&f).await;
    let (status, result) = post(&app, &f.operator, &path, &body).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let saved = &result["work_item"];
    assert_eq!(saved["version"], 2);
    assert_eq!(saved["title"], "Revised title");
    assert_eq!(saved["description"], "Revised description");
    assert_eq!(saved["criteria"][0]["id"], initial["criteria"][0]["id"]);
    assert_eq!(saved["criteria"][0]["text"], "Revised criterion");
    assert_eq!(saved["criteria"][1]["text"], "Second criterion");
    assert_eq!(saved["criteria"][1]["position"], 1);
    assert_eq!(saved["history"].as_array().unwrap().len(), 2);
    assert_eq!(saved["history"][1]["event_type"], "work.definition_edited");
    assert_eq!(
        post(&app, &f.operator, &path, &body).await,
        (StatusCode::OK, result.clone())
    );
    let mut different = body.clone();
    different["definition"]["title"] = json!("Different payload");
    assert_eq!(
        post(&app, &f.operator, &path, &different).await.0,
        StatusCode::CONFLICT
    );
    different["operation_id"] = json!(Uuid::new_v4());
    assert_eq!(
        post(&app, &f.operator, &path, &different).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(runtime_counts(&f).await, before_runtime);
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT
        (SELECT version FROM work_items WHERE company_id=$1 AND id=$2),
        (SELECT count(*) FROM work_item_history WHERE company_id=$1 AND work_item_id=$2),
        (SELECT count(*) FROM work_api_operations WHERE company_id=$1 AND operation_id=$3)",
    )
    .bind(f.company)
    .bind(id(&initial))
    .bind(Uuid::parse_str(body["operation_id"].as_str().unwrap()).unwrap())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(counts, (2, 2, 1));
    grant(&f, project, &f.operator, "viewer").await;
    assert_eq!(
        post(&app, &f.operator, &path, &body).await.0,
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn definition_edit_refuses_ungranted_forged_or_reviewed_input_without_partial_writes() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let initial = item(&f, &app, project).await;
    let path = format!("/api/v1/work-items/{}/definition", id(&initial));
    let body = definition_body(&initial);
    let mut invalid = body.clone();
    invalid["definition"]["criteria"][0]["id"] = json!(Uuid::new_v4());
    assert_eq!(
        post(&app, &f.operator, &path, &invalid).await.0,
        StatusCode::BAD_REQUEST
    );
    invalid = body.clone();
    invalid["definition"]["actor"] = json!("forged");
    assert_eq!(
        post(&app, &f.operator, &path, &invalid).await.0,
        StatusCode::BAD_REQUEST
    );
    grant(&f, project, &f.reader, "owner").await;
    assert_eq!(
        post(&app, &f.reader, &path, &body).await.0,
        StatusCode::FORBIDDEN
    );
    let accept = format!(
        "/api/v1/work-items/{}/criteria/{}/satisfy",
        id(&initial),
        initial["criteria"][0]["id"].as_str().unwrap()
    );
    let (status, accepted) = post(
        &app,
        &f.operator,
        &accept,
        &json!({"operation_id":Uuid::new_v4(),"expected_version":1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut frozen = definition_body(&accepted["work_item"]);
    frozen["definition"]["criteria"][0]["text"] = json!("Cannot reinterpret accepted evidence");
    assert_eq!(
        post(&app, &f.operator, &path, &frozen).await.0,
        StatusCode::BAD_REQUEST
    );
    let (_, current) = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}", id(&initial)),
    )
    .await;
    assert_eq!(current["work_item"]["title"], initial["title"]);
    assert_eq!(current["work_item"]["version"], 2);
    assert_eq!(
        current["work_item"]["criteria"].as_array().unwrap().len(),
        1
    );
}
