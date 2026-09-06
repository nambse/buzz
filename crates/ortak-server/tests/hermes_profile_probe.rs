//! The real Rust transport talks to a bounded authenticated HTTP fixture.
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use chrono::Utc;
use ortak_control::runtime::{
    RuntimeAdapter, RuntimeCapability, RuntimeError, RuntimeResourceRequest,
};
use ortak_domain::{EmployeeId, ProvisioningMode, RuntimeBinding};
use ortak_runtime::hermes::{HermesAdapter, ProfileProbeStatus};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone)]
struct Fixture {
    company: Uuid,
    id: Uuid,
    binding: RuntimeBinding,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    reply: Arc<Mutex<Value>>,
}
impl Fixture {
    fn new() -> Self {
        let company = Uuid::new_v4();
        let id = Uuid::new_v4();
        Self {company,id,binding:serde_json::from_value(json!({"adapter":"hermes","profile_ref":"selected-profile","model":"selected-model","workspace_ref":"none","credential_refs":["secret://fixture/oauth"]})).unwrap(),
            calls:Arc::default(),reply:Arc::new(Mutex::new(json!({"runtime_run_ref":format!("ortak:{company}:{id}"),"started_at":Utc::now(),"status":"accepted"})))}
    }
    async fn server(&self) -> (HermesAdapter, tokio::task::JoinHandle<()>) {
        async fn inspect(
            State(f): State<Fixture>,
            h: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            assert_eq!(h["authorization"], "Bearer fixture-only-token");
            assert_eq!(body, json!({"company_id":f.company,"binding":f.binding}));
            f.calls.lock().unwrap().push(("inspect".into(), body));
            Json(
                json!({"profile_ref":"selected-profile","healthy":false,"credential_references":["secret://fixture/oauth"]}),
            )
        }
        async fn probe(
            State(f): State<Fixture>,
            h: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            assert_eq!(h["authorization"], "Bearer fixture-only-token");
            assert_eq!(
                body,
                json!({"company_id":f.company,"binding":f.binding,"probe_id":f.id})
            );
            f.calls.lock().unwrap().push(("probe".into(), body));
            Json(f.reply.lock().unwrap().clone())
        }
        async fn lookup(
            State(f): State<Fixture>,
            h: HeaderMap,
            Json(body): Json<Value>,
        ) -> (StatusCode, Json<Value>) {
            assert_eq!(h["authorization"], "Bearer fixture-only-token");
            assert_eq!(
                body,
                json!({"company_id":f.company,"run_id":f.id,"idempotency_key":format!("ortak-run:{}:{}",f.company,f.id)})
            );
            f.calls.lock().unwrap().push(("lookup".into(), body));
            let value = f.reply.lock().unwrap().clone();
            (
                if value.is_null() {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::OK
                },
                Json(value),
            )
        }
        async fn cancel(
            State(f): State<Fixture>,
            h: HeaderMap,
            Json(mut body): Json<Value>,
        ) -> Json<Value> {
            assert_eq!(h["authorization"], "Bearer fixture-only-token");
            assert!(body
                .as_object_mut()
                .unwrap()
                .remove("reason")
                .unwrap()
                .is_string());
            assert_eq!(
                body,
                json!({"company_id":f.company,"run_id":f.id,"idempotency_key":format!("ortak-run:{}:{}",f.company,f.id)})
            );
            f.calls.lock().unwrap().push(("cancel".into(), body));
            Json(
                json!({"runtime_run_ref":f.reply.lock().unwrap()["runtime_run_ref"],"outcome":"already_terminal"}),
            )
        }
        let app = Router::new()
            .route("/v1/profiles/inspect", post(inspect))
            .route("/v1/profiles/probe", post(probe))
            .route("/v1/runs/lookup", post(lookup))
            .route("/v1/runs/cancel", post(cancel))
            .with_state(self.clone());
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let runtime = HermesAdapter::new(
            self.company,
            &format!("http://{}", socket.local_addr().unwrap()),
            "fixture-only-token",
        )
        .unwrap();
        (
            runtime,
            tokio::spawn(async move { axum::serve(socket, app).await.unwrap() }),
        )
    }
}

#[tokio::test]
async fn ordinary_profile_reads_never_start_a_diagnostic_and_create_stays_refused() {
    let f = Fixture::new();
    let (runtime, server) = f.server().await;
    assert!(!runtime.health(&f.binding).await.unwrap().is_healthy());
    assert_eq!(
        runtime
            .resolvable_credential_references(&f.binding)
            .await
            .unwrap(),
        f.binding.credential_refs
    );
    let mut request = RuntimeResourceRequest {
        employee_id: EmployeeId::parse("fixture").unwrap(),
        mode: ProvisioningMode::Adopt,
        binding: f.binding.clone(),
        idempotency_key: "retained-step".into(),
    };
    assert!(runtime.ensure_profile(&request).await.is_err());
    request.mode = ProvisioningMode::Create;
    assert!(matches!(
        runtime.ensure_profile(&request).await,
        Err(RuntimeError::Unsupported {
            capability: RuntimeCapability::ProfileCreate
        })
    ));
    assert_eq!(
        f.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _)| p.as_str())
            .collect::<Vec<_>>(),
        vec!["inspect"; 3]
    );
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn explicit_probe_uses_exact_persisted_identity_and_terminal_still_needs_containment() {
    let f = Fixture::new();
    let (runtime, server) = f.server().await;
    for _ in 0..2 {
        assert_eq!(
            runtime.start_profile_probe(&f.binding, f.id).await.unwrap(),
            ProfileProbeStatus::Accepted
        );
    }
    f.reply.lock().unwrap()["status"] = json!("completed");
    assert_eq!(
        runtime.profile_probe_status(f.id).await.unwrap(),
        Some(ProfileProbeStatus::Completed)
    );
    runtime.stop_profile_probe(f.id).await.unwrap();
    let calls = f.calls.lock().unwrap();
    assert_eq!(calls[0].1, calls[1].1);
    assert_eq!(
        calls.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
        vec!["probe", "probe", "lookup", "cancel"]
    );
    drop(calls);
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn probe_refuses_unknown_or_cross_company_receipts_and_missing_lookup_is_read_only() {
    let f = Fixture::new();
    let (runtime, server) = f.server().await;
    f.reply.lock().unwrap()["status"] = json!("unknown");
    assert!(runtime.start_profile_probe(&f.binding, f.id).await.is_err());
    f.reply.lock().unwrap()["status"] = json!("completed");
    f.reply.lock().unwrap()["runtime_run_ref"] =
        json!(format!("ortak:{}:{}", Uuid::new_v4(), f.id));
    assert!(runtime.profile_probe_status(f.id).await.is_err());
    assert!(runtime.stop_profile_probe(f.id).await.is_err());
    *f.reply.lock().unwrap() = Value::Null;
    assert_eq!(runtime.profile_probe_status(f.id).await.unwrap(), None);
    assert!(runtime
        .start_profile_probe(&f.binding, Uuid::nil())
        .await
        .is_err());
    let mut foreign = f.binding.clone();
    foreign.adapter = "other".into();
    assert!(runtime.start_profile_probe(&foreign, f.id).await.is_err());
    assert_eq!(f.calls.lock().unwrap().len(), 4);
    server.abort();
    let _ = server.await;
}
