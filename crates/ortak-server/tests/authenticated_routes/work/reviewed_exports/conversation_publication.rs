//! Signed conversation publication and retained cleanup through the production scheduler.
use super::*;

pub(super) async fn approve(x: &ExportFixture, kind: &str, source: &str, content: &str) -> Uuid {
    let path = format!("/api/v1/projects/{}/conversation-memory", x.project);
    let preview = post(
        &x.app,
        &x.f.operator,
        &format!("{path}/preview"),
        &json!({"employee_id":"cem","source_message_id":source,"audience":{"kind":kind}}),
    )
    .await;
    assert_eq!(preview.0, StatusCode::OK, "{preview:?}");
    let approved = post(&x.app, &x.f.operator, &path, &json!({"operation_id":Uuid::new_v4(),"fact":{
        "employee_id":"cem","source_message_id":source,"audience":{"kind":kind},
        "expected_audience_hash":preview.1["preview"]["audience_hash"],"content":content,"reviewed":true,
        "expires_at":(Utc::now()+chrono::Duration::hours(1)).to_rfc3339_opts(chrono::SecondsFormat::Micros,true)}})).await;
    assert_eq!(approved.0, StatusCode::OK, "{approved:?}");
    id(&approved.1["fact"]["fact"])
}

pub(super) async fn advertise(x: &ExportFixture) {
    assert_eq!(
        exports::advertise_targets_with_conversations(
            &x.f.control,
            &x.scope,
            &[x.target.clone()],
            &[ReviewedConversationTarget {
                project_id: x.project,
                employee_id: x.employee.id.clone(),
                channel_id: x.f.channel
            }]
        )
        .await
        .unwrap(),
        1
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with conversation76"]
async fn conversation_publication_is_atomic_scoped_and_stop_keeps_remote_cleanup() {
    let x = ExportFixture::new(Duration::from_secs(86400), false).await;
    let fact = approve(
        &x,
        "thread",
        &x.source,
        "Reviewed conversation deployment memory",
    )
    .await;
    let path = format!("/api/v1/projects/{}/conversation-memory/{fact}", x.project);
    let publish = format!("{path}/publish");
    let command = x.command();
    assert_ne!(
        post(&x.app, &x.f.operator, &publish, &command).await.0,
        StatusCode::OK
    );
    advertise(&x).await;
    assert_eq!(x.counts().await, (0, 0, 0));
    assert_eq!(
        post(
            &x.app,
            &x.f.operator,
            &format!(
                "/api/v1/projects/{}/reviewed-memory/{fact}/publish",
                x.project
            ),
            &command
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut unconfirmed = command.clone();
    unconfirmed["confirmed"] = json!(false);
    assert_ne!(
        post(&x.app, &x.f.operator, &publish, &unconfirmed).await.0,
        StatusCode::OK
    );
    let function = format!("conversation_export_atomic_{}", Uuid::new_v4().simple());
    let operation = Uuid::parse_str(command["operation_id"].as_str().unwrap()).unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {function}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture receipt failure'; END $$;
        CREATE TRIGGER {function} BEFORE INSERT ON reviewed_memory_export_commands FOR EACH ROW WHEN(NEW.operation_id='{operation}'::uuid) EXECUTE FUNCTION {function}();")))
        .execute(&x.f.pool).await.unwrap();
    let failed = post(&x.app, &x.f.operator, &publish, &command).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {function} ON reviewed_memory_export_commands; DROP FUNCTION {function}();"
    )))
    .execute(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(failed.0, StatusCode::SERVICE_UNAVAILABLE, "{failed:?}");
    assert_eq!(x.counts().await, (0, 0, 0));
    let (a, b) = tokio::join!(
        post(&x.app, &x.f.operator, &publish, &command),
        post(&x.app, &x.f.operator, &publish, &command)
    );
    assert_eq!(a.0, StatusCode::OK, "{a:?}");
    assert_eq!(a, b);
    assert_eq!(a.1["export"]["runtime_consumption_enabled"], false);
    assert_eq!(x.counts().await, (1, 2, 1));
    let source_hash: (Vec<u8>,Vec<u8>) = sqlx::query_as("SELECT x.source_hash,a.source_hash FROM reviewed_memory_exports x
        JOIN reviewed_memory_conversation_audiences a ON a.company_id=x.company_id AND a.fact_id=x.fact_id WHERE x.company_id=$1 AND x.fact_id=$2")
        .bind(x.f.company).bind(fact).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(source_hash.0, source_hash.1);
    let remote = ObservedAdapter::default();
    assert!(schedule_one(&x.f.control, &x.scope, &remote).await.unwrap());
    let view: Value = sqlx::query_scalar("SELECT ortak_reviewed_export_view($1,$2)")
        .bind(x.f.company)
        .bind(fact)
        .fetch_one(&x.f.pool)
        .await
        .unwrap();
    assert_eq!(view["runtime_consumption_enabled"], true);
    let project_enabled:bool = sqlx::query_scalar("SELECT runtime_consumption_enabled FROM reviewed_memory_targets WHERE company_id=$1 AND project_id=$2")
        .bind(x.f.company).bind(x.project).fetch_one(&x.f.pool).await.unwrap();
    assert!(
        !project_enabled,
        "conversation opt-in does not opt into legacy project context"
    );
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(x.f.community)
        .bind(hex::decode(&x.source).unwrap())
        .execute(&x.f.pool)
        .await
        .unwrap();
    let replay = post(&x.app, &x.f.operator, &publish, &command).await;
    assert_eq!(replay.0, StatusCode::OK, "{replay:?}");
    assert_eq!(replay.1["export"]["runtime_consumption_enabled"], false);
    assert!(
        !schedule_one(&x.f.control, &x.scope, &remote).await.unwrap(),
        "source loss is not a fabricated withdrawal"
    );
    let stopped = post(&x.app,&x.f.operator,&format!("{path}/stop"),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":1,"reason":"Stop after source removal"})).await;
    assert_eq!(stopped.0, StatusCode::OK, "{stopped:?}");
    assert!(stopped.1["fact"]["fact"]["content"].is_null());
    exports::advertise_targets_with_conversations(&x.f.control, &x.scope, &[], &[])
        .await
        .unwrap();
    assert!(schedule_one(&x.f.control, &x.scope, &remote).await.unwrap());
    let view: Value = sqlx::query_scalar("SELECT ortak_reviewed_export_view($1,$2)")
        .bind(x.f.company)
        .bind(fact)
        .fetch_one(&x.f.pool)
        .await
        .unwrap();
    assert_eq!(view["cleanup"]["state"], "acknowledged");
    assert_eq!(view["erased_from_reviewed_store"], true);
    assert_eq!(view["runtime_consumption_enabled"], false);
    let calls = remote.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, ReviewedExportAction::Publish);
    assert_eq!(calls[1].0, ReviewedExportAction::Withdraw);
    assert_ne!(calls[0].1, calls[1].1);
}
