use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with decomposition schema"]
async fn decomposition_hidden_parent_never_grants_content_and_current_role_scope_fence_replays() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = super::super::boundaries::source_message(&f, f.channel).await;
    let mut promotion = item_body("Parent content canary");
    promotion["source_message_id"] = json!(source);
    let parent = post(
        &app,
        &f.operator,
        &format!("/api/v1/projects/{project}/promotions"),
        &promotion,
    )
    .await
    .1["work_item"]
        .clone();
    let request = body(&parent);
    let created = post(&app, &f.operator, &path(&parent), &request).await;
    assert_eq!(created.0, StatusCode::CREATED);
    let child = created.1["child"].clone();
    assert_eq!(child["source_message_id"], Value::Null);
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(hex::decode(&source).unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    let visible = read(&f, &app, &child).await;
    assert_eq!(visible["parent"], Value::Null);
    for canary in [
        "Parent content canary",
        parent["id"].as_str().unwrap(),
        &source,
    ] {
        assert!(!visible.to_string().contains(canary));
    }
    assert_eq!(
        post(&app, &f.operator, &path(&parent), &request).await.0,
        StatusCode::NOT_FOUND
    );
    let before = snapshot(&f).await;
    grant(&f, project, &f.reader, "viewer").await;
    assert_eq!(
        post(&app, &f.reader, &path(&child), &body(&child)).await.0,
        StatusCode::FORBIDDEN
    );
    let mut injected = body(&child);
    injected["child"]["source_message_id"] = json!(source);
    assert_eq!(
        post(&app, &f.operator, &path(&child), &injected).await.0,
        StatusCode::BAD_REQUEST
    );
    let foreign = Fixture::new().await;
    let foreign_app = work_app(&foreign, true, Role::Operator, vec![foreign.channel]);
    assert_eq!(
        post(
            &foreign_app,
            &foreign.operator,
            &path(&child),
            &body(&child)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(f.operator.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap();
    assert!(matches!(
        post(&app, &f.operator, &path(&child), &body(&child))
            .await
            .0,
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    ));
    assert!(matches!(
        get(
            &app,
            &f.operator,
            &format!("/api/v1/work-items/{}/decomposition", id(&child))
        )
        .await
        .0,
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
    ));
    assert_eq!(snapshot(&f).await, before);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with decomposition schema"]
async fn decomposition_serializes_parent_cas_and_enforces_depth_count_terminal_and_archive_bounds()
{
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let root = item(&f, &app, project).await;
    let path = path(&root);
    let left = body(&root);
    let right = body(&root);
    let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::join!(
            post(&app, &f.operator, &path, &left),
            post(&app, &f.operator, &path, &right)
        )
    })
    .await
    .unwrap();
    let mut statuses = vec![a.0.as_u16(), b.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, vec![201, 409]);
    let first = if a.0 == StatusCode::CREATED { a.1 } else { b.1 };
    let mut current = first["work_item"].clone();
    let mut deepest = first["child"].clone();
    for _ in 1..8 {
        deepest = create(&f, &app, &deepest).await["child"].clone();
    }
    assert_eq!(
        post(&app, &f.operator, &super::path(&deepest), &body(&deepest))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    for _ in 1..32 {
        current = create(&f, &app, &current).await["work_item"].clone();
    }
    assert_eq!(
        read(&f, &app, &current).await["children"]
            .as_array()
            .unwrap()
            .len(),
        32
    );
    assert_eq!(
        post(&app, &f.operator, &path, &body(&current)).await.0,
        StatusCode::BAD_REQUEST
    );
    let terminal = transition(&f, &app, deepest, "cancelled").await;
    assert_eq!(
        post(&app, &f.operator, &super::path(&terminal), &body(&terminal))
            .await
            .0,
        StatusCode::CONFLICT
    );
    let other = item(&f, &app, project).await;
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
                actor: WorkActor::Human(f.operator.public_key().to_hex()),
                reason: Some("Freeze project".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        post(&app, &f.operator, &super::path(&other), &body(&other))
            .await
            .0,
        StatusCode::CONFLICT
    );
}
