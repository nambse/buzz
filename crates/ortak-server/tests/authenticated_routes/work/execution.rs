//! Signed conversation promotion → Work → shared supervisor → review evidence.
use super::*;
use futures_util::StreamExt;
use ortak_control::{
    fakes::{FakeMemoryAdapter, FakeRuntimeAdapter},
    run_event::{BoundedText, DeliveryIntentKind},
    runtime::RuntimeRunRef,
};
use ortak_runtime::{DispatchOutcome, RunSupervisor, SupervisorConfig};
use ortak_work::schedule_work_outputs;
use std::time::Duration;
#[path = "execution/fixture.rs"]
pub(super) mod fixture;
use fixture::*;
#[path = "execution/assignments.rs"]
mod assignments;
#[path = "execution/dependencies.rs"]
mod dependencies;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn signed_work_start_is_atomic_idempotent_and_refuses_overrides_stale_state_and_missing_role()
{
    let f = Fixture::new().await;
    employee(&f).await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let (project, current) = ready(&f, &app).await;
    let path = format!("/api/v1/work-items/{}/executions", id(&current));
    let command = request(&current);
    let mut injected = command.clone();
    injected["model"] = json!("caller-override");
    assert_eq!(
        post(&app, &f.operator, &path, &injected).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post(&app, &f.reader, &path, &command).await.0,
        StatusCode::NOT_FOUND
    );
    grant(&f, project, &f.reader, "viewer").await;
    assert_eq!(
        post(&app, &f.reader, &path, &command).await.0,
        StatusCode::FORBIDDEN
    );
    let failing_op = command["operation_id"].as_str().unwrap();
    let name = format!("work_start_failure_{}", Uuid::new_v4().simple());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {name}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture storage failure'; END $$;
        CREATE TRIGGER {name} BEFORE INSERT ON work_api_operations FOR EACH ROW WHEN(NEW.operation_id='{failing_op}'::uuid) EXECUTE FUNCTION {name}();"))).execute(&f.pool).await.unwrap();
    let failed = post(&app, &f.operator, &path, &command).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON work_api_operations; DROP FUNCTION {name}();"
    )))
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(failed.0, StatusCode::SERVICE_UNAVAILABLE, "{failed:?}");
    assert_eq!(runtime_counts(&f).await, (0, 0, 0));
    let (a, b) = tokio::join!(
        post(&app, &f.operator, &path, &command),
        post(&app, &f.operator, &path, &command)
    );
    assert_eq!(a.0, StatusCode::OK, "{a:?}");
    assert_eq!(b.0, StatusCode::OK, "{b:?}");
    assert_eq!(a.1["execution"]["run_id"], b.1["execution"]["run_id"]);
    assert_ne!(a.1["execution"]["created"], b.1["execution"]["created"]);
    assert_eq!(runtime_counts(&f).await, (1, 1, 0));
    let (_, saved) = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}", id(&current)),
    )
    .await;
    assert_eq!(saved["work_item"]["version"], version(&current) + 1);
    assert_eq!(saved["work_item"]["state"], "in_progress");
    assert_eq!(saved["work_item"]["criteria"], current["criteria"]);
    assert_eq!(saved["work_item"]["approvals"], current["approvals"]);
    assert_eq!(
        post(&app, &f.operator, &path, &request(&current)).await.0,
        StatusCode::CONFLICT
    );
    let run = Uuid::parse_str(a.1["execution"]["run_id"].as_str().unwrap()).unwrap();
    let detail = format!("/api/v1/runs/{run}");
    assert_eq!(get(&app, &f.reader, &detail).await.0, StatusCode::OK);
    assert_eq!(
        post(&app, &f.reader, &format!("{detail}/cancel"), &json!({}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    grant(&f, project, &f.reader, "contributor").await;
    assert_eq!(
        post(&app, &f.reader, &format!("{detail}/cancel"), &json!({}))
            .await
            .0,
        StatusCode::ACCEPTED
    );
    let (_, list) = get(&app, &f.reader, "/api/v1/runs?limit=1").await;
    assert_eq!(list["runs"][0]["run_id"], run.to_string());
    sqlx::query("UPDATE project_access_grants SET revoked_at=now() WHERE company_id=$1 AND project_id=$2 AND actor_pubkey=$3")
        .bind(f.company).bind(project).bind(f.reader.public_key().to_hex()).execute(&f.pool).await.unwrap();
    assert_eq!(get(&app, &f.reader, &detail).await.0, StatusCode::NOT_FOUND);
    let (_, list) = get(&app, &f.reader, "/api/v1/runs?limit=1").await;
    assert_eq!(list["runs"], json!([]));
}

async fn frame(stream: &mut axum::body::BodyDataStream) -> String {
    String::from_utf8(
        tokio::time::timeout(Duration::from_secs(3), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
fn activity(frame: &str) -> Value {
    assert!(frame.contains("event: activity"), "{frame}");
    serde_json::from_str(
        frame
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn shared_work_runtime_saves_one_verified_artifact_and_review_and_streams_late_output_under_project_authority(
) {
    let f = Fixture::new().await;
    let employee = employee(&f).await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let (project, current) = ready(&f, &app).await;
    let (run, command) = queue(&f, &app, &current).await;
    let (adapter, memory, reference) = start(&f, &employee, run).await;
    assert_eq!(adapter.start_specs().len(), 1);
    let spec = &adapter.start_specs()[0];
    assert_eq!(spec.context.work_item_id, Some(id(&current)));
    assert!(spec.context.conversation_ref.is_none() && spec.context.reply_to_message_id.is_none());
    assert!(spec.input.contains("Produce the actual deliverable"));
    let (_, replay) = post(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}/executions", id(&current)),
        &command,
    )
    .await;
    assert_eq!(replay["execution"]["run_id"], run.to_string());
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with((*f.pool.connect_options()).clone())
        .await
        .unwrap();
    let stream_app = product_router(
        PgControlPlane::new(pool),
        config(f.community, &f.operator, f.channel),
        Arc::new(Replay::default()),
    )
    .unwrap();
    let response = stream_app
        .clone()
        .oneshot(signed(
            &f.operator,
            "GET",
            &format!("/api/v1/runs/{run}/stream"),
            "",
            false,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    let initial = activity(&frame(&mut stream).await);
    assert_eq!(initial["detail"]["memory"]["recall"]["status"], "prepared");
    assert!(initial["detail"]["memory"]["write"].is_null());
    assert!(!initial.to_string().contains("credential://"));
    complete(
        &f,
        &adapter,
        &memory,
        run,
        &reference,
        BoundedText::raw("Complete <plain> deliverable"),
    )
    .await;
    let terminal = activity(&frame(&mut stream).await);
    assert_eq!(terminal["detail"]["work_output"]["status"], "pending");
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let report = schedule_work_outputs(&f.control, &scope, 8).await.unwrap();
    assert_eq!((report.attempted, report.materialized), (1, 1));
    let late = activity(&frame(&mut stream).await);
    assert_eq!(late["detail"]["work_output"]["status"], "materialized");
    assert_eq!(late["page"]["entries"], json!([]));
    assert_eq!(
        late["page"]["next_after_sequence"],
        terminal["page"]["next_after_sequence"]
    );
    assert_eq!(
        schedule_work_outputs(&f.control, &scope, 8)
            .await
            .unwrap()
            .attempted,
        0
    );
    let (_, saved) = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}", id(&current)),
    )
    .await;
    assert_eq!(saved["work_item"]["state"], "review");
    assert_eq!(saved["work_item"]["version"], version(&current) + 2);
    assert_eq!(saved["work_item"]["criteria"], current["criteria"]);
    assert_eq!(saved["work_item"]["approvals"], current["approvals"]);
    let artifact = late["detail"]["work_output"]["artifact_id"]
        .as_str()
        .unwrap();
    let path = format!("/api/v1/work-items/{}/artifacts/{artifact}", id(&current));
    let output = app
        .clone()
        .oneshot(signed(&f.operator, "GET", &path, "", false))
        .await
        .unwrap();
    assert_eq!(output.status(), StatusCode::OK);
    assert_eq!(
        output.headers()["content-type"],
        "text/plain; charset=utf-8"
    );
    assert_eq!(output.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        to_bytes(output.into_body(), 32768).await.unwrap(),
        "Complete <plain> deliverable"
    );
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM artifacts WHERE company_id=$1),(SELECT count(*) FROM runtime_office_outputs WHERE company_id=$1),(SELECT count(*) FROM runtime_memory_writes WHERE company_id=$1)").bind(f.company).fetch_one(&f.pool).await.unwrap();
    assert_eq!(counts, (1, 0, 0));
    // A fresh stream projection may still hold its short shared authority fence.
    // The real grant guard deliberately rejects that race with NOWAIT; retry
    // only that transient conflict, then assert the stream's post-commit fence.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = sqlx::query("UPDATE project_access_grants SET revoked_at=now() WHERE company_id=$1 AND project_id=$2 AND actor_pubkey=$3")
                .bind(f.company).bind(project).bind(f.operator.public_key().to_hex()).execute(&f.pool).await;
            match result {
                Ok(result) => {
                    assert_eq!(result.rows_affected(), 1);
                    break;
                }
                Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("55P03") => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => panic!("project revocation failed: {error}"),
            }
        }
    }).await.expect("project revocation must acquire its authority fence within three seconds");
    let mut revoked = frame(&mut stream).await;
    if revoked.contains("event: activity") {
        // The producer has exactly one buffered slot. A snapshot authorized
        // before the project grant's exclusive lock committed may already be
        // in that slot; it must only repeat the previously visible terminal
        // state. The very next fresh read must close on current authority.
        let buffered = activity(&revoked);
        assert_eq!(
            buffered, late,
            "only the already visible snapshot may drain"
        );
        revoked = frame(&mut stream).await;
    }
    assert!(revoked.contains("\"code\":\"revoked\""), "{revoked}");
    assert!(!revoked.contains("entries"));
    assert!(stream.next().await.is_none());
    assert_eq!(get(&app, &f.operator, &path).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn work_change_revokes_execution_and_partial_final_text_never_creates_review() {
    for edited in [false, true] {
        let f = Fixture::new().await;
        let employee = employee(&f).await;
        let app = work_app(&f, true, Role::Reader, vec![f.channel]);
        let (_, current) = ready(&f, &app).await;
        let (run, _) = queue(&f, &app, &current).await;
        let (adapter, memory, reference) = start(&f, &employee, run).await;
        let scope = f
            .control
            .resolve_company_for_community(f.community)
            .await
            .unwrap();
        if edited {
            let (status,body)=post(&app,&f.operator,&format!("/api/v1/work-items/{}/transitions",id(&current)),
                &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&current)+1,"target":"ready"})).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            let report = ortak_runtime::reconciliation::reconcile_runtime(
                &f.control,
                &adapter,
                &scope,
                &SupervisorConfig::default(),
                8,
            )
            .await
            .unwrap();
            assert_eq!((report.revocations, report.stop_attempts), (1, 1));
            let reason: String =
                sqlx::query_scalar("SELECT cancel_reason FROM runs WHERE company_id=$1 AND id=$2")
                    .bind(f.company)
                    .bind(run)
                    .fetch_one(&f.pool)
                    .await
                    .unwrap();
            assert_eq!(reason, "work_revoked");
        } else {
            let mut delta = BoundedText::raw("Partial deliverable");
            delta.truncated = true;
            complete(&f, &adapter, &memory, run, &reference, delta).await;
        }
        let report = schedule_work_outputs(&f.control, &scope, 8).await.unwrap();
        assert_eq!((report.attempted, report.materialized), (1, 0));
        let (_, saved) = get(
            &app,
            &f.operator,
            &format!("/api/v1/work-items/{}", id(&current)),
        )
        .await;
        assert_ne!(saved["work_item"]["state"], "review");
        let (_, executions) = get(
            &app,
            &f.operator,
            &format!("/api/v1/work-items/{}/executions", id(&current)),
        )
        .await;
        assert_eq!(executions["executions"][0]["reconciled"], true);
        assert!(executions["executions"][0]["artifact_id"].is_null());
    }
}

#[path = "execution/lifecycle.rs"]
mod lifecycle;
