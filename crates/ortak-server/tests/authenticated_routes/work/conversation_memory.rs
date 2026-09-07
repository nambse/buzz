//! A signed audience preview resolves current Office state without approving a fact.

use super::*;

#[path = "conversation_memory/approval.rs"]
mod approval;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn conversation_memory_signed_preview_is_current_scoped_and_read_only() {
    let f = Fixture::new().await;
    execution::fixture::employee(&f).await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = boundaries::source_message(&f, f.channel).await;
    let sibling = boundaries::source_message(&f, f.channel).await;
    let path = format!("/api/v1/projects/{project}/conversation-memory/preview");
    let body = json!({"employee_id":"cem","source_message_id":source,
        "audience":{"kind":"thread"}});

    let preview = post(&app, &f.operator, &path, &body).await;
    assert_eq!(preview.0, StatusCode::OK, "{preview:?}");
    let audience = &preview.1["preview"]["audience"];
    assert_eq!(audience["kind"], "thread");
    assert_eq!(audience["thread_root_event_id"], source);
    assert_eq!(audience["channel_id"], f.channel.to_string());
    assert!(!preview.1.to_string().contains("Canonical source fixture"));
    let original_hash = preview.1["preview"]["audience_hash"].clone();
    assert_eq!(original_hash.as_str().unwrap().len(), 64);

    let mut other = body.clone();
    other["source_message_id"] = json!(sibling);
    let other = post(&app, &f.operator, &path, &other).await;
    assert_eq!(other.0, StatusCode::OK, "{other:?}");
    assert_ne!(other.1["preview"]["audience_hash"], original_hash);

    let mut channel = body.clone();
    channel["audience"] = json!({"kind":"channel"});
    let channel = post(&app, &f.operator, &path, &channel).await;
    assert_eq!(channel.0, StatusCode::OK, "{channel:?}");
    assert!(channel.1["preview"]["audience"]["thread_root_event_id"].is_null());
    assert_ne!(channel.1["preview"]["audience_hash"], original_hash);

    let mut forged = body.clone();
    forged["audience"]["thread_root_event_id"] = json!(source);
    assert_eq!(
        post(&app, &f.operator, &path, &forged).await.0,
        StatusCode::BAD_REQUEST
    );
    grant(&f, project, &f.reader, "contributor").await;
    assert_eq!(
        post(&app, &f.reader, &path, &body).await.0,
        StatusCode::FORBIDDEN
    );
    let hidden = boundaries::source_message(&f, f.hidden).await;
    let mut hidden_body = body.clone();
    hidden_body["source_message_id"] = json!(hidden);
    assert_eq!(
        post(&app, &f.operator, &path, &hidden_body).await.0,
        StatusCode::FORBIDDEN
    );

    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(hex::decode(&source).unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        post(&app, &f.operator, &path, &body).await.0,
        StatusCode::FORBIDDEN
    );
    let persisted: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM reviewed_memory_facts WHERE company_id=$1),
        (SELECT count(*) FROM reviewed_memory_operations WHERE company_id=$1),
        (SELECT count(*) FROM reviewed_memory_exports WHERE company_id=$1)",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(persisted, (0, 0, 0));
}
