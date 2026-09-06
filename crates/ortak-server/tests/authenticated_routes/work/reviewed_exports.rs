//! Signed publication, real durable scheduler and retained-binding cleanup.
use super::*;
use ortak_control::{adapter::Detail, memory::MemoryError, CompanyScope};
use ortak_domain::Employee;
use ortak_server::reviewed_export_worker::{schedule_one, ReviewedExportAdapter};
use ortak_work::reviewed_exports::{self as exports, *};
use std::sync::Mutex;
use std::time::Duration;

#[path = "reviewed_exports/authority.rs"]
mod authority;
#[path = "reviewed_exports/fixture.rs"]
mod fixture;
#[path = "reviewed_exports/jobs.rs"]
mod jobs;
#[path = "reviewed_exports/retention.rs"]
mod retention;
#[path = "reviewed_exports/runtime.rs"]
mod runtime;
#[path = "reviewed_exports/conversation_targets.rs"]
mod conversation_targets;
#[path = "reviewed_exports/conversation_publication.rs"]
mod conversation_publication;
#[path = "reviewed_exports/conversation_runtime.rs"]
mod conversation_runtime;
use fixture::*;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal69"]
async fn reviewed_export_is_explicit_authorized_atomic_and_idempotent() {
    let x = ExportFixture::new(Duration::from_secs(86400), false).await;
    assert_eq!(x.counts().await, (0, 0, 0));
    let command = x.command();
    assert_ne!(
        post(&x.app, &x.f.operator, &x.publish_path(), &command)
            .await
            .0,
        StatusCode::OK
    );
    x.advertise().await;
    assert_eq!(
        x.counts().await,
        (0, 0, 0),
        "advertising never publishes existing facts"
    );
    grant(&x.f, x.project, &x.f.reader, "contributor").await;
    assert_eq!(
        post(&x.app, &x.f.reader, &x.publish_path(), &command)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let mut invalid = command.clone();
    invalid["confirmed"] = json!(false);
    assert_ne!(
        post(&x.app, &x.f.operator, &x.publish_path(), &invalid)
            .await
            .0,
        StatusCode::OK
    );
    invalid = command.clone();
    invalid["binding"] = json!(x.target.binding);
    assert_ne!(
        post(&x.app, &x.f.operator, &x.publish_path(), &invalid)
            .await
            .0,
        StatusCode::OK
    );
    let trigger = format!("reviewed_export_atomic_{}", Uuid::new_v4().simple());
    let operation = command["operation_id"].as_str().unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {trigger}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture command failure'; END $$;
        CREATE TRIGGER {trigger} BEFORE INSERT ON reviewed_memory_export_commands FOR EACH ROW WHEN(NEW.operation_id='{operation}'::uuid) EXECUTE FUNCTION {trigger}();")))
        .execute(&x.f.pool).await.unwrap();
    let result = post(&x.app, &x.f.operator, &x.publish_path(), &command).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {trigger} ON reviewed_memory_export_commands; DROP FUNCTION {trigger}();"
    )))
    .execute(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(result.0, StatusCode::SERVICE_UNAVAILABLE, "{result:?}");
    assert_eq!(x.counts().await, (0, 0, 0));
    let publish_path = x.publish_path();
    let (a, b) = tokio::join!(
        post(&x.app, &x.f.operator, &publish_path, &command),
        post(&x.app, &x.f.operator, &publish_path, &command)
    );
    assert_eq!(a.0, StatusCode::OK, "{a:?}");
    assert_eq!(b.0, StatusCode::OK, "{b:?}");
    assert_eq!(a.1, b.1);
    assert_eq!(x.counts().await, (1, 2, 1));
    assert_eq!(a.1["export"]["runtime_consumption_enabled"], false);
    let projection = a.1.to_string();
    for private in [
        "creation_receipt",
        "binding_hash",
        "endpoint_ref",
        "Reviewed deployment fact",
    ] {
        assert!(!projection.contains(private));
    }
    assert_eq!(
        post(&x.app, &x.f.operator, &x.publish_path(), &x.command())
            .await
            .0,
        StatusCode::CONFLICT
    );
    let other = project(&x.f, &x.app, x.f.channel).await;
    assert_ne!(
        post(
            &x.app,
            &x.f.operator,
            &format!(
                "/api/v1/projects/{other}/reviewed-memory/{}/publish",
                x.fact
            ),
            &command
        )
        .await
        .0,
        StatusCode::OK
    );
}
