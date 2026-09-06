//! Production preparation against disposable PG and an actual HTTP transport.
//! The fixture supplies provider outcomes; it does not claim real inference.
use super::*;
use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use nostr::Keys;
use ortak_control::{
    ports::CompanyDirectory,
    provisioning::{OperationMode, ProvisioningRequest},
};
use serde_json::{json, Value};
use sqlx::{postgres::PgConnectOptions, PgPool};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::sync::Notify;

#[path = "../../tests/authenticated_routes/management_fixture.rs"]
mod fixture;

#[path = "retention_test.rs"]
mod retention;

#[derive(Clone)]
struct Bridge {
    company: Uuid,
    binding: Value,
    pool: PgPool,
    known: Arc<Mutex<Option<(Uuid, String)>>>,
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
    stop_allowed: Arc<AtomicBool>,
    workspace_capable: Arc<AtomicBool>,
    started: Arc<Notify>,
}
impl Bridge {
    fn receipt(&self) -> Value {
        let guard = self.known.lock().unwrap();
        let (id, status) = guard.as_ref().unwrap();
        json!({"runtime_run_ref":format!("ortak:{}:{id}",self.company),"started_at":Utc::now(),"status":status})
    }
    fn auth(headers: &HeaderMap) {
        assert_eq!(headers["authorization"], "Bearer selected-fixture-token");
    }
}
struct Fixture {
    control: PgControlPlane,
    scope: CompanyScope,
    operation: Uuid,
    config: Value,
    runtime: HermesAdapter,
    bridge: Bridge,
    server: tokio::task::JoinHandle<()>,
}
impl Fixture {
    async fn new() -> Self {
        Self::new_with_workspace(false).await
    }
    async fn new_with_workspace(workspace: bool) -> Self {
        let url = std::env::var("ORTAK_TEST_DATABASE_URL").unwrap();
        let options: PgConnectOptions = url.parse().unwrap();
        assert_eq!(options.get_port(), 55432);
        assert!(matches!(options.get_host(), "localhost" | "127.0.0.1"));
        let pool = crate::connect_private_database(&url).await.unwrap();
        buzz_db::migration::run_migrations(&pool).await.unwrap();
        let company = Uuid::new_v4();
        let community = Uuid::new_v4();
        sqlx::query("INSERT INTO communities(id,host) VALUES($1,$2)")
            .bind(community)
            .bind(format!("probe-{}.example", community.simple()))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO companies(id,slug,display_name) VALUES($1,$2,'Probe fixture')")
            .bind(company)
            .bind(format!("probe-{}", company.simple()))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO office_company_bindings(company_id,community_id) VALUES($1,$2)")
            .bind(company)
            .bind(community)
            .execute(&pool)
            .await
            .unwrap();
        let control = PgControlPlane::new(pool.clone());
        let scope = control
            .resolve_company_for_community(community)
            .await
            .unwrap();
        let mut config = fixture::prepared(&scope, Uuid::new_v4());
        config["runtime_credentials"] = json!({"source":"hermes_profile"});
        if workspace {
            config["manifest"]["employee"]["runtime"]["workspace_ref"] = json!("input:probe");
            config["manifest"]["employee"]["permissions"] =
                json!({"allowed_tools":["files"],"allowed_workspaces":["input:probe"]});
        }
        let selected: ProvisioningConfig = serde_json::from_value(config.clone()).unwrap();
        let bridge = Bridge {
            company,
            binding: serde_json::to_value(&selected.manifest.employee.runtime).unwrap(),
            pool: pool.clone(),
            known: Arc::default(),
            starts: Arc::default(),
            stops: Arc::default(),
            stop_allowed: Arc::new(AtomicBool::new(true)),
            workspace_capable: Arc::default(),
            started: Arc::new(Notify::new()),
        };
        async fn inspect(
            State(f): State<Bridge>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            Bridge::auth(&headers);
            assert_eq!(body, json!({"company_id":f.company,"binding":f.binding}));
            let completed = f
                .known
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|(_, s)| s == "completed");
            Json(
                json!({"profile_ref":"fixture-profile","healthy":completed&&f.stops.load(Ordering::SeqCst)>0}),
            )
        }
        async fn start(
            State(f): State<Bridge>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            Bridge::auth(&headers);
            let id = Uuid::parse_str(body["probe_id"].as_str().unwrap()).unwrap();
            assert_eq!(
                body,
                json!({"company_id":f.company,"binding":f.binding,"probe_id":id})
            );
            let persisted:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM provisioning_runtime_probes WHERE company_id=$1 AND probe_id=$2 AND state='running')").bind(f.company).bind(id).fetch_one(&f.pool).await.unwrap();
            assert!(persisted, "bridge cannot run before durable admission");
            {
                let mut known = f.known.lock().unwrap();
                if let Some((previous, _)) = known.as_ref() {
                    assert_eq!(*previous, id);
                } else {
                    *known = Some((id, "running".into()));
                    f.starts.fetch_add(1, Ordering::SeqCst);
                }
            }
            f.started.notify_one();
            Json(f.receipt())
        }
        async fn lookup(
            State(f): State<Bridge>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> (axum::http::StatusCode, Json<Value>) {
            Bridge::auth(&headers);
            assert_eq!(body["company_id"], json!(f.company));
            let id = Uuid::parse_str(body["run_id"].as_str().unwrap()).unwrap();
            assert_eq!(
                body["idempotency_key"],
                json!(format!("ortak-run:{}:{id}", f.company))
            );
            let known = f.known.lock().unwrap().as_ref().map(|(known, _)| *known);
            if let Some(known) = known {
                assert_eq!(known, id);
                (axum::http::StatusCode::OK, Json(f.receipt()))
            } else {
                (axum::http::StatusCode::NOT_FOUND, Json(json!({})))
            }
        }
        async fn stop(
            State(f): State<Bridge>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> (axum::http::StatusCode, Json<Value>) {
            Bridge::auth(&headers);
            let id = Uuid::parse_str(body["run_id"].as_str().unwrap()).unwrap();
            assert_eq!(
                body["idempotency_key"],
                json!(format!("ortak-run:{}:{id}", f.company))
            );
            if !f.stop_allowed.load(Ordering::SeqCst) {
                return (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({})));
            }
            f.stops.fetch_add(1, Ordering::SeqCst);
            (
                axum::http::StatusCode::OK,
                Json(
                    json!({"runtime_run_ref":format!("ortak:{}:{id}",f.company),"outcome":"already_terminal"}),
                ),
            )
        }
        async fn capabilities(State(f): State<Bridge>, headers: HeaderMap) -> Json<Value> {
            Bridge::auth(&headers);
            let capabilities = if f.workspace_capable.load(Ordering::SeqCst) {
                vec!["workspace_text_read"]
            } else {
                vec![]
            };
            Json(
                json!({"adapter":"hermes","api_version":"ortak-hermes-bridge/v1","capabilities":capabilities}),
            )
        }
        let app = Router::new()
            .route("/v1/capabilities", axum::routing::get(capabilities))
            .route("/v1/profiles/inspect", post(inspect))
            .route("/v1/profiles/probe", post(start))
            .route("/v1/runs/lookup", post(lookup))
            .route("/v1/runs/cancel", post(stop))
            .with_state(bridge.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        config["bridge_origin"] = json!(origin);
        let runtime = HermesAdapter::new(company, &origin, "selected-fixture-token").unwrap();
        let typed: ProvisioningConfig = serde_json::from_value(config.clone()).unwrap();
        let operation = control
            .begin_operation(
                &scope,
                &ProvisioningRequest {
                    employee_id: typed.manifest.employee.id.clone(),
                    mode: OperationMode::Adopt,
                    dry_run: false,
                    idempotency_key: typed.operation_key,
                    manifest: typed.manifest,
                },
            )
            .await
            .unwrap()
            .id;
        sqlx::query("INSERT INTO provisioning_runner_selections(company_id,operation_id,configuration_fingerprint) VALUES($1,$2,$3)").bind(company).bind(operation).bind([0x68_u8;32].as_slice()).execute(&pool).await.unwrap();
        Self {
            control,
            scope,
            operation,
            config,
            runtime,
            bridge,
            server: tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }),
        }
    }
    fn start(&self) -> tokio::task::JoinHandle<Result<(), &'static str>> {
        let control = self.control.clone();
        let scope = self.scope.clone();
        let runtime = self.runtime.clone();
        let operation = self.operation;
        let config: ProvisioningConfig = serde_json::from_value(self.config.clone()).unwrap();
        tokio::spawn(async move { prepare(&control, &scope, operation, &runtime, &config).await })
    }
    async fn wait_started(&self, run: &mut tokio::task::JoinHandle<Result<(), &'static str>>) {
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                result = run => panic!("preparation ended before HTTP admission: {result:?}"),
                _ = self.bridge.started.notified() => (),
            }
        })
        .await
        .expect("explicit preparation must reach its selected fixture transport");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with proposal68"]
async fn runtime_probe_crash_reconnect_uses_one_durable_child_and_requires_containment() {
    let f = Fixture::new().await;
    let mut first = f.start();
    f.wait_started(&mut first).await;
    first.abort();
    let _ = first.await;
    let before = f
        .control
        .provisioning_runtime_probe(&f.scope, f.operation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.state(), "running");
    f.bridge.known.lock().unwrap().as_mut().unwrap().1 = "completed".into();
    f.bridge.stop_allowed.store(false, Ordering::SeqCst);
    assert_eq!(f.start().await.unwrap(), Err("probe_containment_pending"));
    assert_eq!(
        f.control
            .provisioning_runtime_probe(&f.scope, f.operation)
            .await
            .unwrap()
            .unwrap()
            .state(),
        "running"
    );
    f.bridge.stop_allowed.store(true, Ordering::SeqCst);
    assert_eq!(f.start().await.unwrap(), Ok(()));
    let after = f
        .control
        .provisioning_runtime_probe(&f.scope, f.operation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.id(), after.id());
    assert_eq!(after.state(), "succeeded");
    assert_eq!(f.bridge.starts.load(Ordering::SeqCst), 1);
    // An acknowledged fresh success is reused read-only after another crash.
    assert_eq!(f.start().await.unwrap(), Ok(()));
    assert_eq!(f.bridge.starts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with proposal68"]
async fn runtime_probe_scope_revocation_during_poll_stops_child_and_dry_run_never_admits() {
    let f = Fixture::new().await;
    let mut dry: ProvisioningConfig = serde_json::from_value(f.config.clone()).unwrap();
    dry.dry_run = true;
    assert_eq!(
        prepare(&f.control, &f.scope, f.operation, &f.runtime, &dry).await,
        Ok(())
    );
    assert_eq!(f.bridge.starts.load(Ordering::SeqCst), 0);
    let mut run = f.start();
    f.wait_started(&mut run).await;
    sqlx::query("DELETE FROM office_company_bindings WHERE company_id=$1")
        .bind(f.scope.company_id())
        .execute(f.control.pool())
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), run)
            .await
            .unwrap()
            .unwrap(),
        Err("probe_authority_changed")
    );
    assert_eq!(f.bridge.stops.load(Ordering::SeqCst), 1);
    let states: Vec<String> =
        sqlx::query_scalar("SELECT state FROM provisioning_runtime_probes WHERE company_id=$1")
            .bind(f.scope.company_id())
            .fetch_all(f.control.pool())
            .await
            .unwrap();
    assert_eq!(states, vec!["failed"]);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with proposal68"]
async fn runtime_probe_new_operation_cannot_abandon_old_child_when_recovery_credential_is_missing()
{
    let f = Fixture::new().await;
    let mut first = f.start();
    f.wait_started(&mut first).await;
    first.abort();
    let _ = first.await;
    let old = f
        .control
        .provisioning_runtime_probe(&f.scope, f.operation)
        .await
        .unwrap()
        .unwrap();
    assert!(std::env::var_os(old.token_environment()).is_none());
    let mut selected: ProvisioningConfig = serde_json::from_value(f.config.clone()).unwrap();
    selected.operation_key = Uuid::new_v4().to_string();
    let operation = f
        .control
        .begin_operation(
            &f.scope,
            &ProvisioningRequest {
                employee_id: selected.manifest.employee.id.clone(),
                mode: OperationMode::Adopt,
                dry_run: false,
                idempotency_key: selected.operation_key.clone(),
                manifest: selected.manifest.clone(),
            },
        )
        .await
        .unwrap()
        .id;
    sqlx::query("INSERT INTO provisioning_runner_selections(company_id,operation_id,configuration_fingerprint) VALUES($1,$2,$3)").bind(f.scope.company_id()).bind(operation).bind([0x69_u8;32].as_slice()).execute(f.control.pool()).await.unwrap();
    assert_eq!(
        prepare(&f.control, &f.scope, operation, &f.runtime, &selected).await,
        Err("probe_recovery_credential_unavailable")
    );
    let retained = f
        .control
        .provisioning_runtime_probe(&f.scope, operation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retained.id(), old.id());
    assert_eq!(retained.state(), "running");
    assert_eq!(f.bridge.starts.load(Ordering::SeqCst), 1);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM provisioning_runtime_probes WHERE company_id=$1")
            .bind(f.scope.company_id())
            .fetch_one(f.control.pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
    // Supplying the original transport can still recover the exact retained child.
    contain(&f.runtime, &old).await.unwrap();
    f.control
        .settle_provisioning_runtime_probe(&f.scope, &old, Some("probe_interrupted"))
        .await
        .unwrap();
}

#[path = "workspace_test.rs"]
mod workspace;
