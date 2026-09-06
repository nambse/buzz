//! New approval and recovery are exercised through real signed requests.
use super::*;
use chrono::{Duration, SecondsFormat};

async fn draft(f: &Fixture, app: &Router, project: Uuid, source: &str) -> Value {
    let preview = post(
        app,
        &f.operator,
        &format!("/api/v1/projects/{project}/conversation-memory/preview"),
        &json!({"employee_id":"cem","source_message_id":source,"audience":{"kind":"thread"}}),
    )
    .await;
    assert_eq!(preview.0, StatusCode::OK, "{preview:?}");
    json!({"operation_id":Uuid::new_v4(),"fact":{
        "employee_id":"cem","source_message_id":source,"audience":{"kind":"thread"},
        "expected_audience_hash":preview.1["preview"]["audience_hash"],
        "content":"Human reviewed conversation deployment fact.",
        "expires_at":(Utc::now()+Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Micros,true),
        "reviewed":true}})
}

async fn counts(f: &Fixture) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT
        (SELECT count(*) FROM reviewed_memory_facts WHERE company_id=$1),
        (SELECT count(*) FROM reviewed_memory_conversation_audiences WHERE company_id=$1),
        (SELECT count(*) FROM reviewed_memory_operations WHERE company_id=$1)",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with conversation75"]
async fn conversation_approval_is_atomic_idempotent_and_excluded_from_project_facts() {
    let f = Fixture::new().await;
    execution::fixture::employee(&f).await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = boundaries::source_message(&f, f.channel).await;
    let path = format!("/api/v1/projects/{project}/conversation-memory");
    let body = draft(&f, &app, project, &source).await;
    let mut mismatched = body.clone();
    mismatched["fact"]["expected_audience_hash"] = json!("0".repeat(64));
    assert_eq!(
        post(&app, &f.operator, &path, &mismatched).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(counts(&f).await, (0, 0, 0));

    // Test the real receipt seam: failure must roll back both approved text and audience.
    // Every identifier below is generated locally, never supplied by a request.
    let name = format!("conversation_receipt_{}", Uuid::new_v4().simple());
    let operation = Uuid::parse_str(body["operation_id"].as_str().unwrap()).unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {name}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture receipt failure'; END $$;
        CREATE TRIGGER {name} BEFORE INSERT ON reviewed_memory_operations FOR EACH ROW WHEN(NEW.operation_id='{operation}'::uuid) EXECUTE FUNCTION {name}();")))
        .execute(&f.pool).await.unwrap();
    let failed = post(&app, &f.operator, &path, &body).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON reviewed_memory_operations; DROP FUNCTION {name}();"
    )))
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(failed.0, StatusCode::SERVICE_UNAVAILABLE, "{failed:?}");
    assert_eq!(counts(&f).await, (0, 0, 0));

    let (a, b) = tokio::join!(
        post(&app, &f.operator, &path, &body),
        post(&app, &f.operator, &path, &body)
    );
    assert_eq!(a.0, StatusCode::OK, "{a:?}");
    assert_eq!(b.0, StatusCode::OK, "{b:?}");
    assert_eq!(a.1["fact"]["fact"]["id"], b.1["fact"]["fact"]["id"]);
    assert_ne!(a.1["created"], b.1["created"]);
    assert_eq!(counts(&f).await, (1, 1, 1));
    let fact_id = id(&a.1["fact"]["fact"]);
    assert_eq!(
        a.1["fact"]["audience_hash"],
        body["fact"]["expected_audience_hash"]
    );
    assert_eq!(a.1["fact"]["audience"]["thread_root_event_id"], source);

    let mut changed = body.clone();
    changed["fact"]["content"] = json!("Different approved text");
    assert_eq!(
        post(&app, &f.operator, &path, &changed).await.0,
        StatusCode::CONFLICT
    );
    let listed = get(&app, &f.operator, &format!("{path}?employee_id=cem")).await;
    assert_eq!(listed.0, StatusCode::OK, "{listed:?}");
    assert_eq!(listed.1["facts"].as_array().unwrap().len(), 1);
    let legacy = format!("/api/v1/projects/{project}/reviewed-memory");
    assert_eq!(
        get(&app, &f.operator, &format!("{legacy}?employee_id=cem"))
            .await
            .1["facts"],
        json!([])
    );
    let recall = post(
        &app,
        &f.operator,
        &format!("{legacy}/recall"),
        &json!({"employee_id":"cem","query":"deployment"}),
    )
    .await;
    assert_eq!(recall.0, StatusCode::OK, "{recall:?}");
    assert_eq!(recall.1["facts"], json!([]));
    let stop = json!({"operation_id":Uuid::new_v4(),"expected_version":1,"reason":"Wrong endpoint fixture"});
    assert_eq!(
        post(
            &app,
            &f.operator,
            &format!("{legacy}/{fact_id}/stop"),
            &stop
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let writes:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM reviewed_memory_exports WHERE company_id=$1),(SELECT count(*) FROM run_reviewed_memory_uses WHERE company_id=$1)")
        .bind(f.company).fetch_one(&f.pool).await.unwrap();
    assert_eq!(writes, (0, 0));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with conversation75"]
async fn conversation_source_loss_withholds_text_and_keeps_receipt_and_stop_recovery() {
    let f = Fixture::new().await;
    execution::fixture::employee(&f).await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = boundaries::source_message(&f, f.channel).await;
    let path = format!("/api/v1/projects/{project}/conversation-memory");
    let body = draft(&f, &app, project, &source).await;
    let saved = post(&app, &f.operator, &path, &body).await;
    assert_eq!(saved.0, StatusCode::OK, "{saved:?}");
    let fact_id = id(&saved.1["fact"]["fact"]);
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(hex::decode(&source).unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    let replay = post(&app, &f.operator, &path, &body).await;
    assert_eq!(replay.0, StatusCode::OK, "{replay:?}");
    assert_eq!(replay.1["created"], false);
    assert_eq!(id(&replay.1["fact"]["fact"]), fact_id);
    for key in ["audience", "audience_hash", "provenance"] {
        assert!(replay.1["fact"][key].is_null(), "{key}");
    }
    assert!(replay.1["fact"]["fact"]["content"].is_null());
    assert!(replay.1["fact"]["fact"]["source"].is_null());
    let mut fresh = body.clone();
    fresh["operation_id"] = json!(Uuid::new_v4());
    assert_eq!(
        post(&app, &f.operator, &path, &fresh).await.0,
        StatusCode::FORBIDDEN
    );
    let stopped=post(&app,&f.operator,&format!("{path}/{fact_id}/stop"),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":1,"reason":"Stop after evidence removal"})).await;
    assert_eq!(stopped.0, StatusCode::OK, "{stopped:?}");
    assert_eq!(stopped.1["fact"]["fact"]["status"], "revoked");
    assert_eq!(stopped.1["fact"]["fact"]["version"], 2);
    assert_eq!(counts(&f).await, (1, 1, 2));
}
