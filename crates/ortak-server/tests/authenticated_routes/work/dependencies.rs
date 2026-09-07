//! Real signed retained dependency editing, visibility and graph serialization.
use super::execution::fixture;
use super::*;

fn path(item: &Value) -> String {
    format!("/api/v1/work-items/{}/dependencies", id(item))
}
async fn add(app: &Router, f: &Fixture, source: &Value, target: &Value) -> (StatusCode, Value) {
    post(app,&f.operator,&path(source),&json!({"operation_id":Uuid::new_v4(),"expected_version":version(source),"depends_on":id(target)})).await
}
async fn read(app: &Router, f: &Fixture, source: &Value) -> Value {
    let result = get(app, &f.operator, &path(source)).await;
    assert_eq!(result.0, StatusCode::OK);
    result.1
}
async fn remove(app: &Router, f: &Fixture, source: &Value, edge: Uuid) -> Value {
    let result=post(app,&f.operator,&format!("{}/{edge}/remove",path(source)),&json!({"operation_id":Uuid::new_v4(),"expected_version":version(source),"reason":"Dependency no longer required"})).await;
    assert_eq!(result.0, StatusCode::OK);
    result.1["work_item"].clone()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with dependency schema"]
async fn dependency_remove_readd_retains_identity_and_removes_the_real_start_blocker() {
    let f = Fixture::new().await;
    let employee = fixture::employee(&f).await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let (project, source) = fixture::ready(&f, &app).await;
    let target = item(&f, &app, project).await;
    let added = add(&app, &f, &source, &target).await;
    assert_eq!(added.0, StatusCode::OK);
    let source = added.1["work_item"].clone();
    let edges = read(&app, &f, &source).await;
    assert_eq!(edges["work_version"], source["version"]);
    assert_eq!(edges["dependencies"][0]["target"]["id"], target["id"]);
    let edge = id(&edges["dependencies"][0]);
    assert_eq!(post(&app,&f.operator,&format!("/api/v1/work-items/{}/executions",id(&source)),&json!({"operation_id":Uuid::new_v4(),"expected_version":version(&source),"employee_id":"cem"})).await.0,StatusCode::CONFLICT);
    let removed = remove(&app, &f, &source, edge).await;
    assert_eq!(version(&removed), version(&source) + 1);
    assert_eq!(removed["criteria"], source["criteria"]);
    assert_eq!(read(&app, &f, &removed).await["dependencies"], json!([]));
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let core = WorkService::new(f.control.clone())
        .work_item(&scope, id(&source))
        .await
        .unwrap();
    assert!(core.item.blocking_dependencies().is_empty());
    // The production start query must ignore released rows too, not only the UI.
    let (run, request) = fixture::queue(&f, &app, &removed).await;
    assert_eq!(request["expected_version"], removed["version"]);
    let (adapter, _, _) = fixture::start(&f, &employee, run).await;
    assert_eq!(adapter.start_specs().len(), 1);
    let current = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}", id(&source)),
    )
    .await
    .1["work_item"]
        .clone();
    let readded = add(&app, &f, &current, &target).await;
    assert_eq!(readded.0, StatusCode::OK);
    assert_eq!(
        read(&app, &f, &readded.1["work_item"]).await["dependencies"][0]["id"],
        edge.to_string()
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work_dependencies WHERE company_id=$1 AND work_item_id=$2",
    )
    .bind(f.company)
    .bind(id(&source))
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    let revoked = ortak_runtime::reconciliation::reconcile_runtime(
        &f.control,
        &adapter,
        &scope,
        &ortak_runtime::SupervisorConfig::default(),
        64,
    )
    .await
    .unwrap();
    assert_eq!((revoked.revocations, revoked.stop_attempts), (1, 1));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with dependency schema"]
async fn dependency_graph_limit_is_enforced_before_loading_unbounded_edges() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = item(&f, &app, project).await;
    let target = item(&f, &app, project).await;
    let nodes: Vec<Uuid> = (0..4097).map(|_| Uuid::new_v4()).collect();
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO work_items(company_id,id,project_id,title,created_by_type) SELECT $1,id,$2,'Bounded graph fixture','system' FROM unnest($3::uuid[]) AS id")
        .bind(f.company).bind(project).bind(&nodes).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO work_dependencies(company_id,project_id,work_item_id,depends_on_work_item_id,created_by_type)
        SELECT $1,$2,($3::uuid[])[i],($3::uuid[])[i+1],'system' FROM generate_series(1,4096) AS i")
        .bind(f.company).bind(project).bind(&nodes).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let refused = add(&app, &f, &source, &target).await;
    assert_eq!(refused.0, StatusCode::BAD_REQUEST);
    let after = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}", id(&source)),
    )
    .await;
    assert_eq!(after.1["work_item"], source);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with dependency schema"]
async fn dependency_hidden_target_keeps_opaque_removal_and_current_replay_authority() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = item(&f, &app, project).await;
    let message = boundaries::source_message(&f, f.channel).await;
    let mut draft = item_body("Private target canary");
    draft["source_message_id"] = json!(message);
    let target = post(
        &app,
        &f.operator,
        &format!("/api/v1/projects/{project}/promotions"),
        &draft,
    )
    .await
    .1["work_item"]
        .clone();
    let source = add(&app, &f, &source, &target).await.1["work_item"].clone();
    let edge = id(&read(&app, &f, &source).await["dependencies"][0]);
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(hex::decode(&message).unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    let hidden = read(&app, &f, &source).await;
    assert_eq!(hidden["dependencies"], json!([{"id":edge,"target":null}]));
    for canary in [
        "Private target canary",
        target["id"].as_str().unwrap(),
        &message,
    ] {
        assert!(!hidden.to_string().contains(canary));
    }
    let command = json!({"operation_id":Uuid::new_v4(),"expected_version":version(&source),"reason":"Remove unavailable target"});
    let remove_path = format!("{}/{edge}/remove", path(&source));
    grant(&f, project, &f.reader, "viewer").await;
    assert_eq!(
        post(&app, &f.reader, &remove_path, &command).await.0,
        StatusCode::FORBIDDEN
    );
    let removed = post(&app, &f.operator, &remove_path, &command).await;
    assert_eq!(removed.0, StatusCode::OK);
    assert_eq!(
        post(&app, &f.operator, &remove_path, &command).await.1,
        removed.1
    );
    assert_eq!(
        add(&app, &f, &removed.1["work_item"], &target).await.0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE project_access_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND project_id=$2").bind(f.company).bind(project).execute(&f.pool).await.unwrap();
    assert_eq!(
        post(&app, &f.operator, &remove_path, &command).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app, &f.operator, &path(&source)).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with dependency schema"]
async fn dependency_opposite_cycle_requests_serialize_without_project_lock_upgrade() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let a = item(&f, &app, project).await;
    let b = item(&f, &app, project).await;
    let (left, right) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::join!(add(&app, &f, &a, &b), add(&app, &f, &b, &a))
    })
    .await
    .expect("graph commands must settle without lock-upgrade deadlock");
    let mut statuses = vec![left.0.as_u16(), right.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, vec![200, 409]);
    let edges: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work_dependencies WHERE company_id=$1 AND released_at IS NULL",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(edges, 1);
    let another = super::project(&f, &app, f.channel).await;
    let foreign = item(&f, &app, another).await;
    let current = get(&app, &f.operator, &format!("/api/v1/work-items/{}", id(&a)))
        .await
        .1["work_item"]
        .clone();
    assert_eq!(
        add(&app, &f, &current, &foreign).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with dependency schema"]
async fn dependency_storage_failure_rolls_back_release_and_exact_retry_advances_once() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = item(&f, &app, project).await;
    let target = item(&f, &app, project).await;
    let source = add(&app, &f, &source, &target).await.1["work_item"].clone();
    let edge = id(&read(&app, &f, &source).await["dependencies"][0]);
    let body = json!({"operation_id":Uuid::new_v4(),"expected_version":version(&source),"reason":"Release exact edge"});
    let path = format!("{}/{edge}/remove", path(&source));
    let name = format!("dependency_failure_{}", Uuid::new_v4().simple());
    let op = body["operation_id"].as_str().unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {name}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture receipt failure'; END $$;
        CREATE TRIGGER {name} BEFORE INSERT ON work_api_operations FOR EACH ROW WHEN(NEW.operation_id='{op}'::uuid) EXECUTE FUNCTION {name}();"))).execute(&f.pool).await.unwrap();
    let failed = post(&app, &f.operator, &path, &body).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON work_api_operations; DROP FUNCTION {name}();"
    )))
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(failed.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        read(&app, &f, &source).await["dependencies"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let (a, b) = tokio::join!(
        post(&app, &f.operator, &path, &body),
        post(&app, &f.operator, &path, &body)
    );
    assert_eq!(a.0, StatusCode::OK);
    assert_eq!(b.0, StatusCode::OK);
    assert_eq!(a.1, b.1);
    assert_eq!(version(&a.1["work_item"]), version(&source) + 1);
    let terminal = transition(&f, &app, a.1["work_item"].clone(), "cancelled").await;
    assert_eq!(
        add(&app, &f, &terminal, &target).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        post(&app, &f.operator, &path, &body).await.0,
        StatusCode::OK,
        "current-scope replay is not a new terminal edit"
    );
}
