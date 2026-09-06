//! Signed reviewed-memory commands bind current project/source authority and PG receipts.
use super::execution::fixture;
use super::*;
use chrono::Duration as ChronoDuration;

#[path = "facts/retention.rs"]
mod retention;

fn approval(source: &str) -> Value {
    json!({"operation_id":Uuid::new_v4(),"fact":{"employee_id":"cem",
        "source":{"kind":"conversation","message_id":source},"content":"Reviewed deployment fact",
        "expires_at":Utc::now()+ChronoDuration::days(1),"reviewed":true}})
}
fn memory_path(project: Uuid) -> String {
    format!("/api/v1/projects/{project}/reviewed-memory")
}
async fn count(f: &Fixture) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM reviewed_memory_facts WHERE company_id=$1),
        (SELECT count(*) FROM reviewed_memory_operations WHERE company_id=$1)",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn reviewed_fact_signed_approval_is_atomic_idempotent_and_rejects_wrong_audience() {
    let f = Fixture::new().await;
    fixture::employee(&f).await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = boundaries::source_message(&f, f.channel).await;
    let path = memory_path(project);
    let body = approval(&source);
    for (field, value) in [
        ("reviewed", json!(false)),
        ("employee_id", json!("unconfigured")),
        ("company_id", json!(Uuid::new_v4())),
    ] {
        let mut invalid = body.clone();
        invalid["fact"][field] = value;
        assert_ne!(
            post(&app, &f.operator, &path, &invalid).await.0,
            StatusCode::OK
        );
    }
    grant(&f, project, &f.reader, "contributor").await;
    assert_eq!(
        post(&app, &f.reader, &path, &body).await.0,
        StatusCode::FORBIDDEN
    );
    let hidden = boundaries::source_message(&f, f.hidden).await;
    assert_eq!(
        post(&app, &f.operator, &path, &approval(&hidden)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(count(&f).await, (0, 0));
    let name = format!("reviewed_fact_storage_{}", Uuid::new_v4().simple());
    let operation = body["operation_id"].as_str().unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {name}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture receipt failure'; END $$;
        CREATE TRIGGER {name} BEFORE INSERT ON reviewed_memory_operations FOR EACH ROW WHEN(NEW.operation_id='{operation}'::uuid) EXECUTE FUNCTION {name}();"))).execute(&f.pool).await.unwrap();
    let failed = post(&app, &f.operator, &path, &body).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON reviewed_memory_operations; DROP FUNCTION {name}();"
    )))
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(failed.0, StatusCode::SERVICE_UNAVAILABLE, "{failed:?}");
    assert_eq!(count(&f).await, (0, 0));
    let (a, b) = tokio::join!(
        post(&app, &f.operator, &path, &body),
        post(&app, &f.operator, &path, &body)
    );
    assert_eq!(a.0, StatusCode::OK, "{a:?}");
    assert_eq!(b.0, StatusCode::OK, "{b:?}");
    assert_eq!(a.1["fact"]["id"], b.1["fact"]["id"]);
    assert_ne!(a.1["created"], b.1["created"]);
    assert_eq!(count(&f).await, (1, 1));
    let mut changed = body.clone();
    changed["fact"]["content"] = json!("Changed fact");
    assert_eq!(
        post(&app, &f.operator, &path, &changed).await.0,
        StatusCode::CONFLICT
    );
    let preview = post(
        &app,
        &f.operator,
        &format!("{path}/recall"),
        &json!({"employee_id":"cem","query":"deployment"}),
    )
    .await;
    assert_eq!(preview.0, StatusCode::OK, "{preview:?}");
    assert_eq!(preview.1["facts"].as_array().unwrap().len(), 1);
    let other = super::project(&f, &app, f.channel).await;
    assert_eq!(
        get(
            &app,
            &f.operator,
            &format!("{}?employee_id=cem", memory_path(other))
        )
        .await
        .1["facts"],
        json!([])
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn reviewed_fact_source_removal_withholds_text_but_retains_stop_use_after_archive() {
    let f = Fixture::new().await;
    fixture::employee(&f).await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = boundaries::source_message(&f, f.channel).await;
    let body = approval(&source);
    let path = memory_path(project);
    let saved = post(&app, &f.operator, &path, &body).await;
    assert_eq!(saved.0, StatusCode::OK, "{saved:?}");
    let id = id(&saved.1["fact"]);
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(hex::decode(&source).unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    let page = get(&app, &f.operator, &format!("{path}?employee_id=cem")).await;
    assert_eq!(page.0, StatusCode::OK, "{page:?}");
    let hidden = &page.1["facts"][0];
    assert_eq!(hidden["source_visible"], false);
    assert!(hidden["content"].is_null() && hidden["source"].is_null());
    assert_eq!(hidden["id"], id.to_string());
    assert_eq!(
        post(
            &app,
            &f.operator,
            &format!("{path}/recall"),
            &json!({"employee_id":"cem","query":"deployment"})
        )
        .await
        .1["facts"],
        json!([])
    );
    let replay = post(&app, &f.operator, &path, &body).await;
    assert_eq!(replay.0, StatusCode::OK, "{replay:?}");
    assert!(replay.1["fact"]["content"].is_null());
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    WorkService::new(f.control.clone())
        .archive_project(
            &scope,
            ortak_work::ArchiveProject {
                project_id: project,
                expected_version: 1,
                reason: Some("Fixture archived".into()),
                actor: WorkActor::Human(f.operator.public_key().to_hex()),
            },
        )
        .await
        .unwrap();
    // Paused employees are ineligible for recall; this is not a lifecycle disable bypass.
    sqlx::query("UPDATE employees SET status='paused' WHERE company_id=$1 AND id='cem'")
        .bind(f.company)
        .execute(&f.pool)
        .await
        .unwrap();
    let revoke = json!({"operation_id":Uuid::new_v4(),"expected_version":1,"reason":"Human selected Stop using"});
    let stop = format!("{path}/{id}/stop");
    let result = post(&app, &f.operator, &stop, &revoke).await;
    assert_eq!(result.0, StatusCode::OK, "{result:?}");
    assert_eq!(result.1["fact"]["status"], "revoked");
    assert_eq!(result.1["fact"]["version"], 2);
    assert_eq!(
        post(&app, &f.operator, &stop, &revoke).await.1["created"],
        false
    );
    assert_eq!(count(&f).await, (1, 2));
    sqlx::query("UPDATE project_access_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND project_id=$2")
        .bind(f.company).bind(project).execute(&f.pool).await.unwrap();
    assert_eq!(
        get(&app, &f.operator, &format!("{path}?employee_id=cem"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        post(&app, &f.operator, &stop, &revoke).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn reviewed_fact_expiry_needs_no_sweeper_and_database_refuses_rewrite_or_orphan() {
    let f = Fixture::new().await;
    fixture::employee(&f).await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = boundaries::source_message(&f, f.channel).await;
    let mut body = approval(&source);
    body["fact"]["expires_at"] = json!(Utc::now() + ChronoDuration::seconds(2));
    let path = memory_path(project);
    let saved = post(&app, &f.operator, &path, &body).await;
    assert_eq!(saved.0, StatusCode::OK, "{saved:?}");
    let id = id(&saved.1["fact"]);
    for statement in [
        "UPDATE reviewed_memory_facts SET content='rewritten' WHERE company_id=$1 AND id=$2",
        "UPDATE reviewed_memory_facts SET expires_at=expires_at+interval '1 day' WHERE company_id=$1 AND id=$2",
        "DELETE FROM reviewed_memory_facts WHERE company_id=$1 AND id=$2",
    ] { assert!(sqlx::query(statement).bind(f.company).bind(id).execute(&f.pool).await.is_err()); }
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("UPDATE reviewed_memory_facts SET version=2,revoked_at=clock_timestamp(),revoked_by=approved_by,revoke_reason='No receipt',revocation_operation_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(id).bind(Uuid::new_v4()).execute(&mut *tx).await.unwrap();
    assert!(
        tx.commit().await.is_err(),
        "transition without atomic receipt must fail"
    );
    assert_eq!(count(&f).await, (1, 1));
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    let replay = post(&app, &f.operator, &path, &body).await;
    assert_eq!(replay.0, StatusCode::OK, "{replay:?}");
    assert_eq!(replay.1["created"], false);
    assert_eq!(replay.1["fact"]["status"], "expired");
    assert_eq!(
        post(
            &app,
            &f.operator,
            &format!("{path}/recall"),
            &json!({"employee_id":"cem","query":"deployment"})
        )
        .await
        .1["facts"],
        json!([])
    );
    assert_eq!(count(&f).await, (1, 1));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn reviewed_artifact_promotion_is_explicit_and_pins_real_completed_employee_source() {
    let f = Fixture::new().await;
    let employee = fixture::employee(&f).await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let (project, item) = fixture::ready(&f, &app).await;
    let (run, _) = fixture::queue(&f, &app, &item).await;
    let (adapter, memory, reference) = fixture::start(&f, &employee, run).await;
    fixture::complete(
        &f,
        &adapter,
        &memory,
        run,
        &reference,
        ortak_control::run_event::BoundedText::raw("Raw run output is not an approved fact"),
    )
    .await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    assert_eq!(
        ortak_work::schedule_work_outputs(&f.control, &scope, 8)
            .await
            .unwrap()
            .materialized,
        1
    );
    assert_eq!(
        count(&f).await,
        (0, 0),
        "completed output must not auto-promote"
    );
    let artifact: Uuid =
        sqlx::query_scalar("SELECT id FROM artifacts WHERE company_id=$1 AND run_id=$2")
            .bind(f.company)
            .bind(run)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let mut body = approval(&"ab".repeat(32));
    body["fact"]["source"] = json!({"kind":"artifact","artifact_id":artifact});
    let result = post(&app, &f.operator, &memory_path(project), &body).await;
    assert_eq!(result.0, StatusCode::OK, "{result:?}");
    assert_eq!(result.1["fact"]["content"], "Reviewed deployment fact");
    assert_eq!(
        result.1["fact"]["source"]["artifact_id"],
        artifact.to_string()
    );
    let other = super::project(&f, &app, f.channel).await;
    body["operation_id"] = json!(Uuid::new_v4());
    assert_eq!(
        post(&app, &f.operator, &memory_path(other), &body).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn reviewed_memory_inspection_and_recall_keep_finite_separate_budgets() {
    let f = Fixture::new().await;
    fixture::employee(&f).await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = boundaries::source_message(&f, f.channel).await;
    let path = memory_path(project);
    for _ in 0..26 {
        let mut body = approval(&source);
        body["fact"]["content"] = json!("deployment ".repeat(150));
        let result = post(&app, &f.operator, &path, &body).await;
        assert_eq!(result.0, StatusCode::OK, "{result:?}");
    }
    let first = get(&app, &f.operator, &format!("{path}?employee_id=cem")).await;
    assert_eq!(first.0, StatusCode::OK, "{first:?}");
    assert_eq!(first.1["facts"].as_array().unwrap().len(), 25);
    let cursor = first.1["next_after"].as_str().unwrap();
    let next = get(
        &app,
        &f.operator,
        &format!("{path}?employee_id=cem&after={cursor}"),
    )
    .await;
    assert_eq!(next.0, StatusCode::OK, "{next:?}");
    assert_eq!(next.1["facts"].as_array().unwrap().len(), 1);
    assert!(next.1["next_after"].is_null());
    assert!(!first.1["facts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["id"] == next.1["facts"][0]["id"]));
    let recalled = post(
        &app,
        &f.operator,
        &format!("{path}/recall"),
        &json!({"employee_id":"cem","query":"deployment"}),
    )
    .await;
    assert_eq!(recalled.0, StatusCode::OK, "{recalled:?}");
    assert_eq!(recalled.1["truncated"], true);
    let facts = recalled.1["facts"].as_array().unwrap();
    assert_eq!(
        facts.len(),
        4,
        "1650-byte facts reach the aggregate byte limit first"
    );
    assert!(
        facts
            .iter()
            .map(|fact| fact["content"].as_str().unwrap().len())
            .sum::<usize>()
            <= 8192
    );
}
