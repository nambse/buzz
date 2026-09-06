//! Real signed HTTP requests through the production router, backed by disposable PG.
use std::{
    collections::HashSet,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use buzz_auth::{AuthError, Nip98ReplayGuard};
use chrono::Utc;
use nostr::{EventBuilder, EventId, Keys, Kind, Tag};
use ortak_control::{
    ports::{CompanyDirectory, RunEventRepository},
    run_event::{RedactionPolicy, RunEvent, RunEventPayload},
    PgControlPlane,
};
use ortak_domain::EmployeeId;
use ortak_server::{product_router, ApiConfig, HumanGrant, Role};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Default)]
struct Replay(Mutex<HashSet<(String, String)>>);
impl Nip98ReplayGuard for Replay {
    fn try_mark_in_scope<'a>(
        &'a self,
        scope: &'a str,
        event: &'a EventId,
        _: u64,
    ) -> Pin<Box<dyn Future<Output = Result<bool, AuthError>> + Send + 'a>> {
        Box::pin(async move {
            Ok(self
                .0
                .lock()
                .expect("replay lock")
                .insert((scope.to_owned(), event.to_hex())))
        })
    }
}

fn signed(keys: &Keys, method: &str, path: &str, body: &str, payload: bool) -> Request<Body> {
    let mut tags = vec![
        Tag::parse(["u", &format!("http://localhost:8787{path}")]).unwrap(),
        Tag::parse(["method", method]).unwrap(),
        Tag::parse(["nonce", &Uuid::new_v4().to_string()]).unwrap(),
    ];
    if payload {
        tags.push(Tag::parse(["payload", &hex::encode(Sha256::digest(body.as_bytes()))]).unwrap());
    }
    let event = EventBuilder::new(Kind::HttpAuth, "")
        .tags(tags)
        .sign_with_keys(keys)
        .unwrap();
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost:8787")
        .header(
            "authorization",
            format!(
                "Nostr {}",
                STANDARD.encode(serde_json::to_vec(&event).unwrap())
            ),
        )
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

async fn response(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&body)
            .unwrap_or_else(|_| json!({"text": String::from_utf8_lossy(&body)})),
    )
}

fn config(community: Uuid, keys: &Keys, channel: Uuid) -> ApiConfig {
    ApiConfig {
        allowed_web_origins: vec!["http://localhost:5173".into()],
        origin: "http://localhost:8787".into(),
        community_id: community,
        humans: vec![HumanGrant {
            public_key: keys.public_key().to_hex(),
            role: Role::Operator,
            can_create_projects: false,
            can_manage_employees: false,
            can_execute_provisioning: false,
            can_review_employee_memory: false,
            channel_ids: vec![channel],
            employee_ids: vec![EmployeeId::parse("cem").unwrap()],
        }],
    }
}

#[tokio::test]
async fn signature_host_and_payload_are_required_before_database_access() {
    let keys = Keys::generate();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    let app = product_router(
        PgControlPlane::new(pool),
        config(Uuid::new_v4(), &keys, Uuid::new_v4()),
        Arc::new(Replay::default()),
    )
    .unwrap();
    let preflight = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/runs")
        .header("host", "localhost:8787")
        .header("origin", "http://localhost:5173")
        .header("access-control-request-method", "GET")
        .header("access-control-request-headers", "authorization")
        .body(Body::empty())
        .unwrap();
    let preflight = app.clone().oneshot(preflight).await.unwrap();
    assert_eq!(preflight.status(), StatusCode::OK);
    assert_eq!(
        preflight.headers()["access-control-allow-origin"],
        "http://localhost:5173"
    );
    let no_auth = Request::builder()
        .uri("/api/v1/runs")
        .header("host", "localhost:8787")
        .body(Body::empty())
        .unwrap();
    assert_eq!(response(&app, no_auth).await.0, StatusCode::UNAUTHORIZED);
    let mut wrong_host = signed(&keys, "GET", "/api/v1/runs", "", false);
    wrong_host
        .headers_mut()
        .insert("host", "another.example".parse().unwrap());
    assert_eq!(response(&app, wrong_host).await.0, StatusCode::UNAUTHORIZED);
    let path = format!("/api/v1/runs/{}/cancel", Uuid::new_v4());
    assert_eq!(
        response(&app, signed(&keys, "POST", &path, "{}", false))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let mut changed = signed(&keys, "POST", &path, "{}", true);
    *changed.body_mut() = Body::from("{\"role\":\"operator\"}");
    assert_eq!(response(&app, changed).await.0, StatusCode::UNAUTHORIZED);
}

struct Fixture {
    pool: PgPool,
    control: PgControlPlane,
    company: Uuid,
    community: Uuid,
    channel: Uuid,
    hidden: Uuid,
    revision: Uuid,
    operator: Keys,
    reader: Keys,
    app: Router,
}
impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("ORTAK_TEST_DATABASE_URL")
            .expect("explicit disposable ORTAK_TEST_DATABASE_URL required");
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(
            parsed.port(),
            Some(55432),
            "test DB must be the explicitly disposable port55432"
        );
        let pool = PgPool::connect(&url).await.unwrap();
        buzz_db::migration::run_migrations(&pool).await.unwrap();
        let control = PgControlPlane::new(pool.clone());
        let community = Uuid::new_v4();
        let company = Uuid::new_v4();
        let operator = Keys::generate();
        let reader = Keys::generate();
        sqlx::query("INSERT INTO communities (id,host) VALUES ($1,$2)")
            .bind(community)
            .bind(format!("api-{}.example", community.simple()))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO companies (id,slug,display_name) VALUES ($1,$2,'API test')")
            .bind(company)
            .bind(format!("api-{}", company.simple()))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO office_company_bindings (community_id,company_id) VALUES ($1,$2)")
            .bind(community)
            .bind(company)
            .execute(&pool)
            .await
            .unwrap();
        let channel = Uuid::new_v4();
        let hidden = Uuid::new_v4();
        for id in [channel, hidden] {
            sqlx::query("INSERT INTO channels (community_id,id,name,created_by,visibility) VALUES ($1,$2,$3,$4,'private')")
                .bind(community).bind(id).bind(format!("api-{}", id.simple())).bind(operator.public_key().to_bytes().as_slice()).execute(&pool).await.unwrap();
            for key in [&operator, &reader] {
                sqlx::query("INSERT INTO channel_members (community_id,channel_id,pubkey) VALUES ($1,$2,$3)")
                    .bind(community).bind(id).bind(key.public_key().to_bytes().as_slice()).execute(&pool).await.unwrap();
            }
        }
        for key in [&operator, &reader] {
            sqlx::query(
                "INSERT INTO relay_members (community_id,pubkey,role) VALUES ($1,$2,'member')",
            )
            .bind(community)
            .bind(key.public_key().to_hex())
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO employees (company_id,id) VALUES ($1,'cem')")
            .bind(company)
            .execute(&pool)
            .await
            .unwrap();
        let revision = Uuid::new_v4();
        sqlx::query("INSERT INTO employee_revisions (company_id,id,employee_id,revision_number,manifest,manifest_fingerprint,provisioning_mode) VALUES ($1,$2,'cem',1,$3,$4,'create')")
            .bind(company).bind(revision).bind(json!({"name":"Cem", "title":"Test employee"})).bind([0_u8;32].as_slice()).execute(&pool).await.unwrap();
        sqlx::query(
            "UPDATE employees SET status='active', active_revision_id=$2 WHERE company_id=$1",
        )
        .bind(company)
        .bind(revision)
        .execute(&pool)
        .await
        .unwrap();
        let mut config = config(community, &operator, channel);
        let mut grant = config.humans[0].clone();
        grant.public_key = reader.public_key().to_hex();
        grant.role = Role::Reader;
        config.humans.push(grant);
        let app = product_router(control.clone(), config, Arc::new(Replay::default())).unwrap();
        Self {
            pool,
            control,
            company,
            community,
            channel,
            hidden,
            revision,
            operator,
            reader,
            app,
        }
    }

    async fn run(&self, channel: Uuid) -> Uuid {
        let event = EventBuilder::new(Kind::Custom(9), "Cem, hello")
            .tags([
                Tag::parse(["h", &channel.to_string()]).unwrap(),
                Tag::parse(["nonce", &Uuid::new_v4().to_string()]).unwrap(),
            ])
            .sign_with_keys(&self.operator)
            .unwrap();
        let created =
            chrono::DateTime::from_timestamp(event.created_at.as_secs() as i64, 0).unwrap();
        sqlx::query("INSERT INTO events (community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id) VALUES ($1,$2,$3,$4,9,$5,'Cem, hello',$6,$7)")
            .bind(self.community).bind(event.id.to_bytes().as_slice()).bind(self.operator.public_key().to_bytes().as_slice()).bind(created)
            .bind(serde_json::to_value(&event.tags).unwrap()).bind(event.sig.serialize().as_slice()).bind(channel).execute(&self.pool).await.unwrap();
        sqlx::query("INSERT INTO office_inbox (company_id,event_id,event_created_at,event_kind,author_pubkey,channel_id) VALUES ($1,$2,$3,9,$4,$5)")
            .bind(self.company).bind(event.id.to_bytes().as_slice()).bind(created).bind(self.operator.public_key().to_bytes().as_slice()).bind(channel).execute(&self.pool).await.unwrap();
        let run = Uuid::new_v4();
        sqlx::query("INSERT INTO runs (company_id,id,employee_id,employee_revision_id,message_id,runtime_adapter) VALUES ($1,$2,'cem',$3,$4,'hermes')")
            .bind(self.company).bind(run).bind(self.revision).bind(event.id.to_bytes().as_slice()).execute(&self.pool).await.unwrap();
        let scope = self
            .control
            .resolve_company_for_community(self.community)
            .await
            .unwrap();
        let event = RunEvent::normalize(
            run,
            Utc::now(),
            None,
            &RunEventPayload::RunQueued,
            &RedactionPolicy::new(),
        )
        .unwrap();
        self.control
            .append_run_events(&scope, run, &[event])
            .await
            .unwrap();
        run
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn real_routes_enforce_company_cohort_membership_and_audited_cancel() {
    let f = Fixture::new().await;
    let run = f.run(f.channel).await;
    let hidden = f.run(f.hidden).await;
    let foreign = Fixture::new().await;
    let foreign_run = foreign.run(foreign.channel).await;
    let (status, list) = response(
        &f.app,
        signed(&f.operator, "GET", "/api/v1/runs?limit=1", "", false),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["runs"].as_array().unwrap().len(), 1);
    assert_eq!(list["runs"][0]["run_id"], run.to_string());
    assert_eq!(list["has_more"], false);
    for denied in [hidden, foreign_run, Uuid::new_v4()] {
        let (status, body) = response(
            &f.app,
            signed(
                &f.operator,
                "GET",
                &format!("/api/v1/runs/{denied}"),
                "",
                false,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body, json!({"error":{"code":"not_found"}}));
    }
    let events = format!("/api/v1/runs/{run}/events?limit=1");
    let (status, page) = response(&f.app, signed(&f.operator, "GET", &events, "", false)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["next_after_sequence"], 0);
    let (_, next) = response(
        &f.app,
        signed(
            &f.operator,
            "GET",
            &format!("/api/v1/runs/{run}/events?after_sequence=0"),
            "",
            false,
        ),
    )
    .await;
    assert_eq!(next["entries"], json!([]));
    assert_eq!(next["next_after_sequence"], 0);
    let (_, employee) = response(
        &f.app,
        signed(&f.operator, "GET", "/api/v1/employees/cem", "", false),
    )
    .await;
    assert_eq!(employee["employee"]["name"], "Cem");
    assert_eq!(employee["runtime_health"], "not_probed");
    let path = format!("/api/v1/runs/{run}/cancel");
    assert_eq!(
        response(&f.app, signed(&f.reader, "POST", &path, "{}", true))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        response(
            &f.app,
            signed(
                &f.operator,
                "POST",
                &format!("/api/v1/runs/{foreign_run}/cancel"),
                "{}",
                true
            )
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let req = signed(&f.operator, "POST", &path, "{}", true);
    let auth = req.headers()["authorization"].clone();
    let (status, cancel) = response(&f.app, req).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{cancel}");
    assert_eq!(cancel["status"], "pending");
    let mut replay = signed(&f.operator, "POST", &path, "{}", true);
    replay.headers_mut().insert("authorization", auth);
    assert_eq!(response(&f.app, replay).await.0, StatusCode::UNAUTHORIZED);
    let (status, again) = response(&f.app, signed(&f.operator, "POST", &path, "{}", true)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(again["request_id"], cancel["request_id"]);
    let row=sqlx::query("SELECT (SELECT count(*) FROM run_cancel_requests WHERE company_id=$1 AND run_id=$2) AS requests,
        (SELECT status FROM runs WHERE company_id=$1 AND id=$2) AS status,
        (SELECT count(*) FROM ortak_api_audit WHERE company_id=$1 AND action='cancel_run' AND outcome='requested' AND actor_pubkey=$3) AS attributed,
        (SELECT count(*) FROM ortak_api_audit WHERE company_id=$1 AND action='cancel_run' AND outcome='denied' AND actor_pubkey=$4) AS denied")
        .bind(f.company).bind(run).bind(f.operator.public_key().to_hex()).bind(f.reader.public_key().to_hex()).fetch_one(&f.pool).await.unwrap();
    assert_eq!(row.get::<i64, _>("requests"), 1);
    assert_eq!(row.get::<String, _>("status"), "queued");
    assert_eq!(row.get::<i64, _>("attributed"), 1);
    assert_eq!(row.get::<i64, _>("denied"), 1);
    // A failing audit insert must roll back the pending request in the same tx.
    let audit_failure_run = f.run(f.channel).await;
    sqlx::query("ALTER TABLE ortak_api_audit ADD CONSTRAINT reject_test_requested CHECK (outcome <> 'requested') NOT VALID")
        .execute(&f.pool).await.unwrap();
    let failed_path = format!("/api/v1/runs/{audit_failure_run}/cancel");
    assert_eq!(
        response(
            &f.app,
            signed(&f.operator, "POST", &failed_path, "{}", true)
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM run_cancel_requests WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.company)
    .bind(audit_failure_run)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(count, 0, "audit failure must leave no cancellation request");
    sqlx::query("ALTER TABLE ortak_api_audit DROP CONSTRAINT reject_test_requested")
        .execute(&f.pool)
        .await
        .unwrap();

    // Hold the production run-row lock so cancellation waits after its authority
    // fence. Existing membership changes and absent-user deactivation INSERTs
    // must fail serialization; neither may commit behind the authorization read.
    let raced_run = f.run(f.channel).await;
    let mut blocker = f.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.company)
        .bind(raced_run)
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let request = signed(
        &f.operator,
        "POST",
        &format!("/api/v1/runs/{raced_run}/cancel"),
        "{}",
        true,
    );
    let app = f.app.clone();
    let task = tokio::spawn(async move { response(&app, request).await });
    let mut fenced = false;
    for _ in 0..100 {
        let mut probe = f.pool.begin().await.unwrap();
        let acquired: bool = sqlx::query_scalar(
            "SELECT pg_try_advisory_xact_lock(ortak_office_company_lock_key($1))",
        )
        .bind(f.company)
        .fetch_one(&mut *probe)
        .await
        .unwrap();
        probe.rollback().await.unwrap();
        if !acquired {
            fenced = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        fenced,
        "request must acquire authority before waiting for run lock"
    );
    let removed = sqlx::query("UPDATE channel_members SET removed_at=now() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(f.operator.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap_err();
    assert_eq!(
        removed
            .as_database_error()
            .and_then(|e| e.code())
            .as_deref(),
        Some("40001")
    );
    let deactivated =
        sqlx::query("INSERT INTO users (community_id,pubkey,deactivated_at) VALUES ($1,$2,now())")
            .bind(f.community)
            .bind(f.operator.public_key().to_bytes().as_slice())
            .execute(&f.pool)
            .await
            .unwrap_err();
    assert_eq!(
        deactivated
            .as_database_error()
            .and_then(|e| e.code())
            .as_deref(),
        Some("40001")
    );
    blocker.rollback().await.unwrap();
    let (status, body) = tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    sqlx::query("UPDATE channel_members SET removed_at=now() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community).bind(f.channel).bind(f.operator.public_key().to_bytes().as_slice()).execute(&f.pool).await.unwrap();
    assert_eq!(
        response(
            &f.app,
            signed(
                &f.operator,
                "GET",
                &format!("/api/v1/runs/{run}/events"),
                "",
                false
            )
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        response(&f.app, signed(&f.operator, "POST", &path, "{}", true))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, list) = response(
        &f.app,
        signed(&f.operator, "GET", "/api/v1/runs", "", false),
    )
    .await;
    assert_eq!(list["runs"], json!([]));
}

#[path = "authenticated_routes/memory.rs"]
mod memory;

#[path = "authenticated_routes/database.rs"]
mod database;

#[path = "authenticated_routes/binding_retirement.rs"]
mod binding_retirement;
#[path = "authenticated_routes/direct.rs"]
mod direct;
#[path = "authenticated_routes/routing_read.rs"]
mod routing_read;

#[path = "authenticated_routes/work.rs"]
mod work;

#[path = "authenticated_routes/activity_stream.rs"]
mod activity_stream;

#[path = "authenticated_routes/provisioning.rs"]
mod provisioning;

#[path = "authenticated_routes/management.rs"]
mod management;

#[path = "authenticated_routes/employee_memory.rs"]
mod employee_memory;

#[cfg(feature = "encrypted-dm")]
#[path = "authenticated_routes/encrypted_dm.rs"]
mod encrypted_dm;
