use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 plus employee-memory candidate assembly"]
async fn employee_memory_signed_concurrent_replay_and_exact_employee_stop() {
    let x = MemoryFixture::new(false).await;
    let f = &x.f;
    let command = x.command(&x.preview(&f.operator, "experience").await);
    let (one, two) = tokio::join!(
        post(&x.app, &f.operator, PATH, &command),
        post(&x.app, &f.operator, PATH, &command)
    );
    assert_eq!(one.0, StatusCode::OK, "{}", one.1);
    assert_eq!(two.0, StatusCode::OK, "{}", two.1);
    assert_ne!(one.1["created"], two.1["created"]);
    assert_eq!(one.1["effect"], two.1["effect"]);
    assert_eq!(x.counts().await, (1, 1, 0));
    let id = one.1["fact"]["id"].as_str().unwrap();
    let original:Vec<u8>=sqlx::query_scalar("SELECT auth_event_id FROM employee_reviewed_memory_operations WHERE company_id=$1 AND operation_id=$2")
        .bind(f.company).bind(Uuid::parse_str(command["operation_id"].as_str().unwrap()).unwrap()).fetch_one(&f.pool).await.unwrap();
    let req = signed(&f.operator, "POST", PATH, &command.to_string(), true);
    let auth = req.headers()["authorization"].clone();
    assert_eq!(response(&x.app, req).await.0, StatusCode::OK);
    let same = Request::builder()
        .method("POST")
        .uri(PATH)
        .header("host", "localhost:8787")
        .header("authorization", auth)
        .header("content-type", "application/json")
        .body(Body::from(command.to_string()))
        .unwrap();
    let (status, value) = response(&x.app, same).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(value["error"]["code"], "authentication_replayed");
    let retained:Vec<u8>=sqlx::query_scalar("SELECT auth_event_id FROM employee_reviewed_memory_operations WHERE company_id=$1 AND operation_id=$2")
        .bind(f.company).bind(Uuid::parse_str(command["operation_id"].as_str().unwrap()).unwrap()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        retained, original,
        "fresh signed retries must not overwrite the first signature identity"
    );
    let mut changed = command.clone();
    changed["fact"]["content"] = json!("Changed content");
    assert_eq!(
        post(&x.app, &f.operator, PATH, &changed).await.0,
        StatusCode::CONFLICT
    );
    let wrong = "/api/v1/employees/other/reviewed-memory";
    assert_eq!(
        post(&x.app, &f.operator, wrong, &command).await.0,
        StatusCode::FORBIDDEN
    );
    let stop = json!({"operation_id":Uuid::new_v4(),"expected_version":1});
    assert_eq!(
        post(&x.app, &f.operator, &format!("{wrong}/{id}/stop"), &stop)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, stopped) = post(&x.app, &f.operator, &format!("{PATH}/{id}/stop"), &stop).await;
    assert_eq!(status, StatusCode::OK, "{stopped}");
    assert_eq!(stopped["fact"]["version"], 2);
    assert_eq!(
        post(&x.app, &f.operator, &format!("{wrong}/{id}/stop"), &stop)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (_, old_effect) = post(&x.app, &f.operator, PATH, &command).await;
    assert_eq!(old_effect["effect"]["result_version"], 1);
    assert_eq!(old_effect["fact"]["version"], 2);
    assert_eq!(old_effect["fact"]["content"], command["fact"]["content"]);
    assert_eq!(x.counts().await, (1, 2, 0));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 plus employee-memory candidate assembly"]
async fn employee_memory_signed_hidden_source_capability_loss_and_inactive_stop_recovery() {
    let x = MemoryFixture::new(false).await;
    let f = &x.f;
    let command = x.command(&x.preview(&f.operator, "experience").await);
    let (status, approved) = post(&x.app, &f.operator, PATH, &command).await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let id = approved["fact"]["id"].as_str().unwrap();
    let no_cap = app(
        f,
        &f.operator,
        false,
        Role::Reader,
        vec![f.channel, f.hidden],
        vec!["cem"],
    );
    let (_, cap_hidden) = get(&no_cap, &f.operator, PATH).await;
    assert_eq!(cap_hidden["can_approve"], false);
    assert_eq!(cap_hidden["facts"][0]["content"], Value::Null);
    assert_eq!(cap_hidden["facts"][0]["can_stop"], true);
    // A current source may not restore a revoked deployment ceiling.
    let no_channel = app(
        f,
        &f.operator,
        true,
        Role::Reader,
        vec![f.channel],
        vec!["cem"],
    );
    assert_eq!(
        get(&no_channel, &f.operator, PATH).await.1["facts"][0]["content"],
        Value::Null
    );
    let epochs_before:Vec<(Uuid,i64)>=sqlx::query_as("SELECT channel_id,epoch FROM employee_memory_channel_authorities WHERE company_id=$1 ORDER BY channel_id")
        .bind(f.company).fetch_all(&f.pool).await.unwrap();
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(hex::decode(&x.source).unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    disable_employee(f).await;
    let epochs_after:Vec<(Uuid,i64)>=sqlx::query_as("SELECT channel_id,epoch FROM employee_memory_channel_authorities WHERE company_id=$1 ORDER BY channel_id")
        .bind(f.company).fetch_all(&f.pool).await.unwrap();
    assert_eq!(epochs_before.len(), 2);
    assert!(epochs_after
        .iter()
        .zip(&epochs_before)
        .all(|(a, b)| a.0 == b.0 && a.1 > b.1));
    let narrow = app(
        f,
        &f.operator,
        false,
        Role::Reader,
        vec![f.channel],
        vec!["cem"],
    );
    let (status, replay) = post(&narrow, &f.operator, PATH, &command).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["created"], false);
    for key in [
        "content",
        "source",
        "source_hash",
        "audience",
        "audience_hash",
        "provenance",
        "sharing_hash",
    ] {
        assert_eq!(replay["fact"][key], Value::Null, "{key}");
    }
    let another = app(
        f,
        &f.reader,
        true,
        Role::Operator,
        vec![f.channel, f.hidden],
        vec!["cem"],
    );
    assert_eq!(get(&another, &f.reader, PATH).await.1["facts"], json!([]));
    let stop = json!({"operation_id":Uuid::new_v4(),"expected_version":1});
    assert_eq!(
        post(&another, &f.reader, &format!("{PATH}/{id}/stop"), &stop)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let denied = app(
        f,
        &f.operator,
        false,
        Role::Reader,
        vec![f.channel],
        vec!["other"],
    );
    assert_eq!(
        get(&denied, &f.operator, PATH).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(&denied, &f.operator, &format!("{PATH}/{id}/stop"), &stop)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, stopped) = post(&narrow, &f.operator, &format!("{PATH}/{id}/stop"), &stop).await;
    assert_eq!(status, StatusCode::OK, "{stopped}");
    assert_eq!(stopped["fact"]["version"], 2);
    let (status, replay) = post(&narrow, &f.operator, &format!("{PATH}/{id}/stop"), &stop).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["created"], false);
    assert_eq!(x.counts().await, (1, 2, 0));
}

async fn disable_employee(f: &Fixture) {
    // Follow the same signed admission and sealed runner as management_lifecycle;
    // direct status/epoch writes are correctly rejected by the production guard.
    let (revision, epoch): (Uuid, i64) = sqlx::query_as(
        "SELECT active_revision_id,lifecycle_epoch FROM employees WHERE company_id=$1 AND id='cem'",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    let mut management = config(f.community, &f.operator, f.channel);
    management.humans[0].can_manage_employees = true;
    management.humans[0].can_execute_provisioning = true;
    ortak_server::management::synchronize_authorizations(&f.control, &management)
        .await
        .unwrap();
    let app = product_router(f.control.clone(), management, Arc::new(Replay::default())).unwrap();
    let body = json!({"idempotency_key":Uuid::new_v4(),"action":"disable",
        "draft_id":null,"operation_id":null,"expected_revision_id":revision,
        "expected_lifecycle_epoch":epoch});
    let (status, receipt) = post(
        &app,
        &f.operator,
        "/api/v1/employees/cem/management-commands",
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{receipt}");
    ortak_server::management::execute_next(&f.control, f.community)
        .await
        .unwrap();
    let current: (String, Uuid, i64) = sqlx::query_as(
        "SELECT status,active_revision_id,lifecycle_epoch FROM employees WHERE company_id=$1 AND id='cem'",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(current, ("disabled".into(), revision, epoch + 1));
}
