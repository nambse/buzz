use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 plus employee-memory candidate assembly"]
async fn employee_memory_signed_receipt_failure_rolls_back_fact_and_stop() {
    let x = MemoryFixture::new(false).await;
    let f = &x.f;
    let command = x.command(&x.preview(&f.operator, "experience").await);
    // Audited test-only DDL: identifiers and scope are generated UUIDs, never request text.
    let name = format!("test_employee_memory_{}", Uuid::new_v4().simple());
    let install=format!("CREATE FUNCTION {name}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN
        IF NEW.company_id='{}'::uuid THEN RAISE EXCEPTION 'synthetic receipt failure'; END IF; RETURN NEW; END $$;
        CREATE TRIGGER {name} AFTER INSERT ON employee_reviewed_memory_operations FOR EACH ROW EXECUTE FUNCTION {name}();",f.company);
    let remove = format!(
        "DROP TRIGGER {name} ON employee_reviewed_memory_operations; DROP FUNCTION {name}();"
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(install.clone()))
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        post(&x.app, &f.operator, PATH, &command).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(x.counts().await, (0, 0, 0));
    let scopes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM employee_memory_channel_authorities WHERE company_id=$1",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(scopes, 0, "registration must roll back with approval");
    sqlx::raw_sql(sqlx::AssertSqlSafe(remove.clone()))
        .execute(&f.pool)
        .await
        .unwrap();
    let (status, approved) = post(&x.app, &f.operator, PATH, &command).await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let id = approved["fact"]["id"].as_str().unwrap();
    let stop = json!({"operation_id":Uuid::new_v4(),"expected_version":1});
    sqlx::raw_sql(sqlx::AssertSqlSafe(install.clone()))
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        post(&x.app, &f.operator, &format!("{PATH}/{id}/stop"), &stop)
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        get(&x.app, &f.operator, PATH).await.1["facts"][0]["version"],
        1
    );
    assert_eq!(x.counts().await, (1, 1, 0));
    sqlx::raw_sql(sqlx::AssertSqlSafe(remove.clone()))
        .execute(&f.pool)
        .await
        .unwrap();
    let (status, stopped) = post(&x.app, &f.operator, &format!("{PATH}/{id}/stop"), &stop).await;
    assert_eq!(status, StatusCode::OK, "{stopped}");
    assert_eq!(x.counts().await, (1, 2, 0));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 plus employee-memory candidate assembly"]
async fn employee_memory_signed_current_source_deadline_and_body_bounds() {
    let x = MemoryFixture::new(false).await;
    let f = &x.f;
    let preview = x.preview(&f.operator, "experience").await;
    let command = x.command(&preview);
    // Mutation between preview and write must not turn a preview into authority.
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.hidden).bind(f.operator.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap();
    assert_eq!(
        post(&x.app, &f.operator, PATH, &command).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE channel_members SET removed_at=NULL WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.hidden).bind(f.operator.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE channels SET ttl_deadline=clock_timestamp()+interval '10 minutes' WHERE community_id=$1 AND id=$2")
        .bind(f.community).bind(f.hidden).execute(&f.pool).await.unwrap();
    assert_eq!(
        post(&x.app, &f.operator, PATH, &command).await.0,
        StatusCode::BAD_REQUEST,
        "fact expiry exceeds current audience deadline"
    );
    let mut too_large = command.clone();
    too_large["fact"]["content"] = json!("x".repeat(33_000));
    assert_eq!(
        post(&x.app, &f.operator, PATH, &too_large).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let old_path = format!("/api/v1/runs/{}/cancel", Uuid::new_v4());
    assert_eq!(
        post(
            &x.app,
            &f.operator,
            &old_path,
            &json!({"reason":"x".repeat(5000)})
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(x.counts().await, (0, 0, 0));
}

#[test]
fn employee_memory_capability_is_default_off_and_independent_of_operator() {
    let keys = Keys::generate();
    let config = config(Uuid::new_v4(), &keys, Uuid::new_v4());
    let mut json = serde_json::to_value(config).unwrap();
    json["humans"][0]
        .as_object_mut()
        .unwrap()
        .remove("can_review_employee_memory");
    let legacy: ApiConfig = serde_json::from_value(json.clone()).unwrap();
    assert!(!legacy.humans[0].can_review_employee_memory);
    assert!(legacy.validate().is_ok());
    json["humans"][0]["role"] = json!("reader");
    json["humans"][0]["can_review_employee_memory"] = json!(true);
    let granted: ApiConfig = serde_json::from_value(json).unwrap();
    assert!(granted.humans[0].can_review_employee_memory);
    assert!(granted.validate().is_ok());
}
