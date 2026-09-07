//! Exact authenticated Rust→bridge tool wire; no provider or real workspace.
use axum::{
    extract::State,
    http::{HeaderMap, Uri},
    routing::post,
    Json, Router,
};
use chrono::Utc;
use ortak_control::runtime::{RunContext, RunSpec, RuntimeAdapter};
use ortak_control::workspace::*;
use ortak_domain::{EmployeeId, PermissionPolicy, RuntimeBinding, ToolCapability};
use ortak_runtime::hermes::HermesAdapter;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone)]
struct Fixture {
    spec: RunSpec,
    grant: WorkspaceGrant,
    request: WorkspaceToolRequest,
    result: WorkspaceResult,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    forged: Arc<std::sync::atomic::AtomicBool>,
}
impl Fixture {
    fn new() -> Self {
        let company = Uuid::new_v4();
        let run = Uuid::new_v4();
        let file = Uuid::new_v4();
        let employee = EmployeeId::parse("ada").unwrap();
        let mut grant = WorkspaceGrant {
            format: WORKSPACE_FORMAT.into(),
            company_id: company,
            project_id: Uuid::new_v4(),
            employee_id: employee.clone(),
            workspace_ref: "input:brief".into(),
            revision: Uuid::new_v4(),
            manifest_hash: String::new(),
            files: vec![WorkspaceFile {
                file_id: file,
                name: "brief.txt".into(),
                media_type: "text/plain".into(),
                bytes: 7,
                sha256: hex::encode(Sha256::digest(b"exact\n\n")),
            }],
        };
        grant.manifest_hash = grant.compute_hash().unwrap();
        let spec = RunSpec {
            run_id: run,
            employee_id: employee,
            revision_id: Uuid::new_v4(),
            binding: serde_json::from_value::<RuntimeBinding>(
                json!({"adapter":"hermes","profile_ref":"selected-profile","model":"selected-model",
    "workspace_ref":"input:brief","credential_refs":[]}),
            )
            .unwrap(),
            permissions: PermissionPolicy {
                allowed_tools: vec![ToolCapability::Files],
                allowed_workspaces: vec![grant.workspace_ref.clone()],
                ..Default::default()
            },
            input: "Read the selected brief".into(),
            context: RunContext {
                work_item_id: Some(Uuid::new_v4()),
                ..Default::default()
            },
            idempotency_key: ortak_runtime::run_idempotency_key(company, run),
        };
        let request = WorkspaceToolRequest {
            call_id: "call_1".into(),
            file_id: file,
            arguments_hash: WorkspaceToolRequest::hash_arguments(file),
            ordinal: 1,
        };
        let result = WorkspaceResult::Completed {
            content: "exact\n\n".into(),
            sha256: grant.files[0].sha256.clone(),
            bytes: 7,
            name: "brief.txt".into(),
        };
        Self {
            spec,
            grant,
            request,
            result,
            calls: Arc::default(),
            forged: Arc::default(),
        }
    }
    async fn server(&self) -> (HermesAdapter, tokio::task::JoinHandle<()>) {
        async fn handle(
            State(f): State<Fixture>,
            headers: HeaderMap,
            uri: Uri,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            assert_eq!(headers["authorization"], "Bearer synthetic-workspace-token");
            f.calls
                .lock()
                .unwrap()
                .push((uri.path().into(), body.clone()));
            let identity = json!({"company_id":f.grant.company_id,"run_id":f.spec.run_id,"idempotency_key":f.spec.idempotency_key});
            match uri.path() {
                "/v1/runs" => {
                    assert_eq!(
                        body,
                        json!({"company_id":f.grant.company_id,"spec":f.spec,"workspace":f.grant})
                    );
                    Json(
                        json!({"runtime_run_ref":format!("ortak:{}:{}",f.grant.company_id,f.spec.run_id),"started_at":Utc::now(),"status":"accepted"}),
                    )
                }
                "/v1/runs/tools/pending" => {
                    assert_eq!(body, identity);
                    let mut request = serde_json::to_value(&f.request).unwrap();
                    if f.forged.load(std::sync::atomic::Ordering::SeqCst) {
                        request["file_id"] = json!(Uuid::new_v4());
                    }
                    Json(json!({"request":request}))
                }
                "/v1/runs/tools/resolve" => {
                    let mut expected = identity;
                    expected["request"] = json!(f.request);
                    expected["result"] = json!(f.result);
                    assert_eq!(body, expected);
                    Json(
                        json!({"acknowledged":true,"call_id":if f.forged.load(std::sync::atomic::Ordering::SeqCst) {"other"} else {"call_1"},"arguments_hash":f.request.arguments_hash}),
                    )
                }
                _ => panic!("unexpected route"),
            }
        }
        let app = Router::new()
            .route("/v1/runs", post(handle))
            .route("/v1/runs/tools/pending", post(handle))
            .route("/v1/runs/tools/resolve", post(handle))
            .with_state(self.clone());
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let adapter = HermesAdapter::new(
            self.grant.company_id,
            &format!("http://{}", socket.local_addr().unwrap()),
            "synthetic-workspace-token",
        )
        .unwrap();
        (
            adapter,
            tokio::spawn(async move { axum::serve(socket, app).await.unwrap() }),
        )
    }
}
#[tokio::test]
async fn selected_workspace_transport_preserves_exact_grant_and_private_result_bytes() {
    let f = Fixture::new();
    let (adapter, server) = f.server().await;
    adapter
        .start_run_with_workspace(&f.spec, Some(&f.grant))
        .await
        .unwrap();
    assert_eq!(
        adapter
            .pending_workspace_tool(&f.spec.idempotency_key, &f.grant)
            .await
            .unwrap(),
        Some(f.request.clone())
    );
    let ack = adapter
        .resolve_workspace_tool(&f.spec.idempotency_key, &f.grant, &f.request, &f.result)
        .await
        .unwrap();
    assert!(ack.acknowledged);
    assert_eq!(f.calls.lock().unwrap().len(), 3);
    server.abort();
    let _ = server.await;
}
#[tokio::test]
async fn workspace_transport_refuses_crossed_or_ambient_policy_and_forged_bridge_receipts() {
    let f = Fixture::new();
    let (adapter, server) = f.server().await;
    assert!(adapter.start_run(&f.spec).await.is_err());
    let mut changed = f.spec.clone();
    changed.context.conversation_ref = Some("office:fixture".into());
    assert!(adapter
        .start_run_with_workspace(&changed, Some(&f.grant))
        .await
        .is_err());
    changed = f.spec.clone();
    changed
        .permissions
        .allowed_networks
        .push("https://example.invalid".into());
    assert!(adapter
        .start_run_with_workspace(&changed, Some(&f.grant))
        .await
        .is_err());
    let mut grant = f.grant.clone();
    grant.company_id = Uuid::new_v4();
    grant.manifest_hash = grant.compute_hash().unwrap();
    assert!(adapter
        .start_run_with_workspace(&f.spec, Some(&grant))
        .await
        .is_err());
    let mut result = f.result.clone();
    if let WorkspaceResult::Completed { content, .. } = &mut result {
        *content = "changed".into();
    }
    assert!(adapter
        .resolve_workspace_tool(&f.spec.idempotency_key, &f.grant, &f.request, &result)
        .await
        .is_err());
    assert!(f.calls.lock().unwrap().is_empty());
    f.forged.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(adapter
        .pending_workspace_tool(&f.spec.idempotency_key, &f.grant)
        .await
        .is_err());
    assert!(adapter
        .resolve_workspace_tool(&f.spec.idempotency_key, &f.grant, &f.request, &f.result)
        .await
        .is_err());
    assert_eq!(f.calls.lock().unwrap().len(), 2);
    server.abort();
    let _ = server.await;
}
