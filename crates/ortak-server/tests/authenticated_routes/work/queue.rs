//! Signed employee queues authorize before pagination and never perform Work writes.
use super::boundaries::{source_message, trusted_item};
use super::*;

const QUEUE: &str = "/api/v1/employees/cem/work-items";

async fn assign_row(f: &Fixture, item: Uuid, role: &str) {
    // Seed retained manual assignments without requiring an executable employee.
    sqlx::query("INSERT INTO work_assignments(company_id,work_item_id,employee_id,role,assigned_by_type,assigned_by_id) VALUES($1,$2,'cem',$3,'human',$4)")
        .bind(f.company).bind(item).bind(role).bind(f.operator.public_key().to_hex())
        .execute(&f.pool).await.unwrap();
}

async fn read_counts(f: &Fixture) -> (i64, i64, i64, i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT
        (SELECT count(*) FROM work_items WHERE company_id=$1),
        (SELECT count(*) FROM work_item_history WHERE company_id=$1),
        (SELECT count(*) FROM work_api_operations WHERE company_id=$1),
        (SELECT count(*) FROM work_assignments WHERE company_id=$1),
        (SELECT count(*) FROM runs WHERE company_id=$1),
        (SELECT count(*) FROM outbox WHERE company_id=$1),
        (SELECT COALESCE(sum(version),0)::bigint FROM work_items WHERE company_id=$1)",
    )
    .bind(f.company)
    .fetch_one(&f.pool)
    .await
    .unwrap()
}

fn page_url(cursor: &str) -> String {
    let query: String = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("limit", "1")
        .append_pair("cursor", cursor)
        .finish();
    format!("{QUEUE}?{query}")
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn signed_employee_queue_is_scoped_bounded_read_only_and_safe_for_paused_employee() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let wide = work_app(&f, true, Role::Reader, vec![f.channel, f.hidden]);
    let visible = project(&f, &app, f.channel).await;
    grant(&f, visible, &f.reader, "viewer").await;
    let mut expected = Vec::new();
    for role in ["owner", "contributor", "reviewer"] {
        let work = item(&f, &app, visible).await;
        assign_row(&f, id(&work), role).await;
        expected.push((id(&work), role));
    }
    // These newer rows must be excluded before the one-row page limit.
    let ungranted = project(&f, &app, f.channel).await;
    let work = item(&f, &app, ungranted).await;
    assign_row(&f, id(&work), "owner").await;
    let outside = project(&f, &wide, f.hidden).await;
    grant(&f, outside, &f.reader, "viewer").await;
    let work = item(&f, &wide, outside).await;
    assign_row(&f, id(&work), "owner").await;
    let foreign = source_message(&f, f.hidden).await;
    let work = trusted_item(
        &f,
        visible,
        "Private source must stay hidden",
        Some(foreign),
    )
    .await;
    assign_row(&f, work, "owner").await;
    sqlx::query("UPDATE employees SET status='paused' WHERE company_id=$1 AND id='cem'")
        .bind(f.company)
        .execute(&f.pool)
        .await
        .unwrap();
    let before = read_counts(&f).await;
    expected.reverse();
    let mut path = format!("{QUEUE}?limit=1");
    let mut first_cursor = None;
    for (index, (expected_id, role)) in expected.iter().enumerate() {
        let (status, body) = get(&app, &f.reader, &path).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["employee_id"], "cem");
        assert_eq!(body["execution_available"], false);
        let rows = body["work_items"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(id(&rows[0]), *expected_id);
        assert_eq!(rows[0]["assignment_role"], *role);
        let keys: std::collections::BTreeSet<_> = rows[0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            std::collections::BTreeSet::from([
                "id",
                "project_id",
                "title",
                "priority",
                "state",
                "version",
                "source_message_id",
                "created_at",
                "updated_at",
                "assignment_role"
            ]),
            "no description/history/runtime/artifact fields enter the queue"
        );
        if index < expected.len() - 1 {
            let cursor = body["next_cursor"].as_str().unwrap();
            if first_cursor.is_none() {
                first_cursor = Some(cursor.to_owned());
            }
            path = page_url(cursor);
        } else {
            assert!(body["next_cursor"].is_null());
        }
    }
    let cursor = first_cursor.unwrap();
    assert_eq!(
        get(&app, &f.operator, &page_url(&cursor)).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&wide, &f.reader, &page_url(&cursor)).await.0,
        StatusCode::BAD_REQUEST
    );
    for employee in ["zeynep", "unconfigured"] {
        assert_eq!(
            get(
                &app,
                &f.reader,
                &format!("/api/v1/employees/{employee}/work-items")
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        get(&app, &f.reader, &format!("{QUEUE}?state=completed"))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        read_counts(&f).await,
        before,
        "queue reads must not create history, receipts, assignments, runtime starts or outbox jobs"
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn signed_queue_reconnect_rechecks_source_membership_and_project_grants() {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    grant(&f, project, &f.reader, "viewer").await;
    let manual = item(&f, &app, project).await;
    assign_row(&f, id(&manual), "contributor").await;
    let source = source_message(&f, f.channel).await;
    let promoted = trusted_item(&f, project, "Visible promoted work", Some(source.clone())).await;
    assign_row(&f, promoted, "reviewer").await;
    let (status, first) = get(&app, &f.reader, &format!("{QUEUE}?limit=1")).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(id(&first["work_items"][0]), promoted);
    assert_eq!(first["work_items"][0]["source_message_id"], source);
    let next = page_url(first["next_cursor"].as_str().unwrap());
    sqlx::query("UPDATE events SET deleted_at=now() WHERE community_id=$1 AND id=$2")
        .bind(f.community)
        .bind(hex::decode(source).unwrap())
        .execute(&f.pool)
        .await
        .unwrap();
    let (status, body) = get(&app, &f.reader, QUEUE).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["work_items"].as_array().unwrap().len(), 1);
    assert_eq!(id(&body["work_items"][0]), id(&manual));
    assert!(body["next_cursor"].is_null());
    let (status, body) = get(&app, &f.reader, &next).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(id(&body["work_items"][0]), id(&manual));
    sqlx::query("UPDATE channel_members SET removed_at=now() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(f.reader.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap();
    let (status, body) = get(&app, &f.reader, &next).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["work_items"].as_array().unwrap().is_empty());
    sqlx::query("UPDATE channel_members SET removed_at=NULL WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(f.reader.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE project_access_grants SET revoked_at=now() WHERE company_id=$1 AND project_id=$2 AND actor_pubkey=$3")
        .bind(f.company).bind(project).bind(f.reader.public_key().to_hex()).execute(&f.pool).await.unwrap();
    let (status, body) = get(&app, &f.reader, &next).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["work_items"].as_array().unwrap().is_empty());
    assert!(body["next_cursor"].is_null());
}
