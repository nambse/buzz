//! Retry authorization follows the original committed transition and current role.
use super::*;

async fn receipt_count(f: &Fixture, actor: &Keys, operation: &Value) -> i64 {
    let operation = Uuid::parse_str(operation.as_str().unwrap()).unwrap();
    sqlx::query_scalar("SELECT count(*) FROM work_api_operations WHERE company_id=$1 AND actor_pubkey=$2 AND operation_id=$3")
        .bind(f.company).bind(actor.public_key().to_hex()).bind(operation)
        .fetch_one(&f.pool).await.unwrap()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn contributor_start_replay_after_later_review_preserves_original_permission() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let current = item(&f, &app, project).await;
    let ready = transition(&f, &app, current, "ready").await;
    grant(&f, project, &f.reader, "contributor").await;
    let path = format!("/api/v1/work-items/{}/transitions", id(&ready));
    let start = json!({"operation_id":Uuid::new_v4(),"expected_version":version(&ready),"target":"in_progress"});
    let (status, started) = post(&app, &f.reader, &path, &start).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let reviewed = transition(&f, &app, started["work_item"].clone(), "review").await;
    assert_eq!(version(&reviewed), 4);

    // Replaying Ready→InProgress is not a new Review→InProgress decision.
    let (status, replay) = post(&app, &f.reader, &path, &start).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["work_item"], reviewed);
    assert_eq!(replay["work_item"]["state"], "review");
    assert_eq!(replay["work_item"]["history"].as_array().unwrap().len(), 4);
    assert_eq!(
        receipt_count(&f, &f.reader, &start["operation_id"]).await,
        1
    );
    let history: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work_item_history WHERE company_id=$1 AND work_item_id=$2",
    )
    .bind(f.company)
    .bind(id(&ready))
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(history, 4);
    assert_eq!(runtime_counts(&f).await, (0, 0, 0));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn review_rejection_replay_cannot_inherit_weaker_permission_after_downgrade() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let mut current = item(&f, &app, project).await;
    for state in ["ready", "in_progress", "review"] {
        current = transition(&f, &app, current, state).await;
    }
    grant(&f, project, &f.reader, "reviewer").await;
    let path = format!("/api/v1/work-items/{}/transitions", id(&current));
    let reject = json!({"operation_id":Uuid::new_v4(),"expected_version":version(&current),
        "target":"in_progress","reason":"Another review pass is required"});
    let (status, rejected) = post(&app, &f.reader, &path, &reject).await;
    assert_eq!(status, StatusCode::OK, "{rejected}");
    assert_eq!(rejected["work_item"]["state"], "in_progress");
    assert_eq!(rejected["work_item"]["version"], 5);
    grant(&f, project, &f.reader, "contributor").await;

    // Current InProgress no longer reveals the original review-only action.
    let (status, replay) = post(&app, &f.reader, &path, &reject).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{replay}");
    assert_eq!(replay, json!({"error":{"code":"forbidden"}}));
    let (status, after) = get(
        &app,
        &f.reader,
        &format!("/api/v1/work-items/{}", id(&current)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(after, rejected);
    assert_eq!(after["work_item"]["history"].as_array().unwrap().len(), 5);
    assert_eq!(
        receipt_count(&f, &f.reader, &reject["operation_id"]).await,
        1
    );
    let history: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work_item_history WHERE company_id=$1 AND work_item_id=$2",
    )
    .bind(f.company)
    .bind(id(&current))
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(history, 5);
    assert_eq!(runtime_counts(&f).await, (0, 0, 0));
}
