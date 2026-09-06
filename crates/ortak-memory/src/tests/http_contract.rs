use super::*;
use serde_json::Value;
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Default)]
struct State {
    calls: Vec<(String, String, Value)>,
    created: bool,
    create_body: Value,
    records: BTreeMap<String, Value>,
    fault: Option<&'static str>,
    reviewed_reply: Option<Value>,
    employee_diagnostics: BTreeMap<String, Value>,
    employee_reply: Option<Value>,
}
struct Server {
    origin: String,
    state: Arc<Mutex<State>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State::default()));
        let shared = state.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0; 4096];
                let end = loop {
                    let count = socket.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break end + 4;
                    }
                    assert!(bytes.len() < 32 * 1024);
                };
                let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                let line = headers.lines().next().unwrap();
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap().to_owned();
                let path = parts.next().unwrap().to_owned();
                assert!(
                    headers
                        .to_ascii_lowercase()
                        .contains("authorization: bearer fresh-test-token")
                );
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|n| n.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                assert!(length <= 1152 * 1024);
                while bytes.len() - end < length {
                    let count = socket.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                let body = if length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes[end..end + length]).unwrap()
                };
                let (status, body, extra, declared) = {
                    let mut state = shared.lock().unwrap();
                    state
                        .calls
                        .push((method.clone(), path.clone(), body.clone()));
                    respond(&mut state, &method, &path, body)
                };
                let delay = shared.lock().unwrap().fault == Some("delay_inspect")
                    && path.ends_with("/resources/inspect");
                if delay {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                let body = serde_json::to_vec(&body).unwrap();
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
                    declared.unwrap_or(body.len())
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            }
        });
        Self {
            origin,
            state,
            task,
        }
    }
}

fn respond(
    state: &mut State,
    method: &str,
    path: &str,
    body: Value,
) -> (u16, Value, String, Option<usize>) {
    if state.fault == Some("auth") {
        return (
            401,
            json!({"secret":"fresh-test-token"}),
            String::new(),
            None,
        );
    }
    if state.fault == Some("redirect") {
        return (
            307,
            json!({}),
            "Location: http://127.0.0.1:1/stolen\r\n".into(),
            None,
        );
    }
    if state.fault == Some("large") {
        return (200, json!({}), String::new(), Some(2 * 1024 * 1024 + 1));
    }
    let company = Uuid::from_u128(1);
    let owner = json!({"protocol":PROTOCOL,"company_id":company,"employee_id":"employee-one"});
    let value = if path == "/v3/ortak/protocol" {
        assert_eq!(method, "GET");
        json!({"protocol":PROTOCOL,"honcho_version":HONCHO_VERSION})
    } else if path.starts_with("/v3/workspaces/list?") {
        assert_eq!(method, "POST");
        assert_eq!(body, json!({}));
        let items = if state.created {
            vec![json!({"id":"private_employee_one","metadata":{"ortak":owner}})]
        } else {
            vec![]
        };
        json!({"total":items.len(),"pages":usize::from(!items.is_empty()),"page":1,"size":100,"items":items})
    } else if path.starts_with("/v3/workspaces/private_employee_one/peers/list?") {
        assert_eq!(method, "POST");
        assert_eq!(body, json!({}));
        json!({"total":2,"pages":1,"page":1,"size":100,"items":[{"id":"operator","workspace_id":"private_employee_one","metadata":{"ortak":owner}},{"id":"employee","workspace_id":"private_employee_one","metadata":{"ortak":owner}}]})
    } else if path == "/v3/ortak/resources/create" {
        assert_eq!(method, "POST");
        assert_eq!(body["company_id"], json!(company));
        assert_eq!(body["employee_id"], "employee-one");
        state.created = true;
        state.create_body = body.clone();
        json!({"protocol":PROTOCOL,"workspace_id":"private_employee_one","user_peer":"operator","employee_peer":"employee","ownership":"created"})
    } else if path.ends_with("/resources/inspect") {
        if !state.created || state.fault == Some("inspect_absent") {
            return (
                409,
                json!({"detail":"owned_bundle_required"}),
                String::new(),
                None,
            );
        }
        assert_eq!(method, "POST");
        assert_eq!(
            body,
            json!({"company_id":company,"employee_id":"employee-one",
            "user_peer":"operator","employee_peer":"employee"})
        );
        let workspace_id = if state.fault == Some("native_identity") {
            "replaced_workspace"
        } else {
            "native_workspace"
        };
        json!({"protocol":PROTOCOL,"company_id":company,"employee_id":"employee-one",
            "workspace_id":"private_employee_one","user_peer":"operator","employee_peer":"employee",
            "ownership":"created","request_hash":wire::fingerprint(&state.create_body).unwrap(),
            "native_ids":{"workspace":workspace_id,"peers":{"operator":"native_operator","employee":"native_employee"}}})
    } else if path.contains("/reviewed-employees/") {
        return employee::respond(state, path, body);
    } else if path.contains("/reviewed-projects/") {
        state
            .reviewed_reply
            .clone()
            .expect("explicit reviewed response fixture")
    } else if path.ends_with("/remember") {
        assert_eq!(method, "POST");
        assert_eq!(body["company_id"], json!(company));
        let session = path.split('/').nth(6).unwrap();
        assert!(session.starts_with("ortak_"));
        let mut hashed = body.clone();
        hashed["workspace_id"] = json!("private_employee_one");
        hashed["session_id"] = json!(session);
        let hash = wire::fingerprint(&hashed).unwrap();
        let facts = body["facts"].as_array().unwrap();
        let records:Vec<_>=facts.iter().enumerate().map(|(index,fact)|json!({"record_ref":format!("record_{index}"),"content":fact["content"],"scope":body["scope"],"provenance":fact["provenance"],"metadata":{"ortak":{"protocol":PROTOCOL,"company_id":body["company_id"],"employee_id":body["employee_id"],"scope":body["scope"],"write_key":body["idempotency_key"],"request_hash":hash,"fact_index":index,"provenance":fact["provenance"]}}})).collect();
        let mut response = json!({"protocol":PROTOCOL,"workspace_id":"private_employee_one","session_id":session,"request_hash":hash,"record_refs":records.iter().map(|v|v["record_ref"].clone()).collect::<Vec<_>>(),"records":records});
        state.records.insert(session.into(), response.clone());
        if state.fault == Some("hash") {
            response["request_hash"] = json!("0".repeat(64));
        }
        if state.fault == Some("metadata") {
            response["records"][0]["metadata"]["ortak"]["company_id"] = json!(Uuid::from_u128(99));
        }
        response
    } else if path.ends_with("/recall") {
        assert_eq!(method, "POST");
        assert_eq!(body["company_id"], json!(company));
        let session = path.split('/').nth(6).unwrap();
        let mut records = state
            .records
            .get(session)
            .map(|v| v["records"].as_array().unwrap().clone())
            .unwrap_or_default();
        for record in &mut records {
            record.as_object_mut().unwrap().remove("metadata");
        }
        if state.fault == Some("scope") && !records.is_empty() {
            records[0]["scope"] = json!({"scope":"company_truth"});
        }
        if state.fault == Some("empty") {
            records.clear();
        }
        json!({"records":records,"truncated":false})
    } else {
        panic!("unexpected non-extension operation {path}")
    };
    (
        if path.ends_with("/remember") || path.ends_with("/resources/create") {
            201
        } else {
            200
        },
        value,
        String::new(),
        None,
    )
}

async fn provision(server: &Server) -> (HonchoMemoryAdapter, HonchoMemoryConfig) {
    let (company, config) = fixture(&server.origin, ProvisioningMode::Create);
    let service = adapter(company, config.clone());
    service
        .ensure_resources(&MemoryResourceRequest {
            employee_id: config.employees[0].employee_id.clone(),
            binding: config.employees[0].binding.clone(),
            mode: ProvisioningMode::Create,
            idempotency_key: "fresh-create".into(),
        })
        .await
        .unwrap();
    (service, config)
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_roundtrip_gates_binding_and_propagates_scoped_receipts() {
    let server = Server::start().await;
    let (service, config) = provision(&server).await;
    let binding = &config.employees[0].binding;
    let before = service.probe_capabilities(binding).await.unwrap();
    assert!(!before.capabilities.contains(&MemoryCapability::Remember));
    assert!(!service.health(binding).await.unwrap().is_healthy());
    assert!(
        !server
            .state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|(_, p, _)| p.ends_with("/remember"))
    );
    let result = service
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    assert_eq!(result.write_receipt.written, 1);
    let repeated = service
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    assert_eq!(repeated.write_receipt, result.write_receipt);
    let writes: Vec<_> = server
        .state
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|(_, p, _)| p.ends_with("/remember"))
        .map(|(_, _, b)| b.clone())
        .collect();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0], writes[1]);
    assert!(service.health(binding).await.unwrap().is_healthy());
    assert!(
        service
            .probe_capabilities(binding)
            .await
            .unwrap()
            .capabilities
            .contains(&MemoryCapability::Recall)
    );
    let query = MemoryRecallRequest {
        employee_id: config.employees[0].employee_id.clone(),
        binding: binding.clone(),
        scope: result.scope,
        query: "roundtrip".into(),
        budget: MemoryBudget::default(),
    };
    assert_eq!(service.recall(&query).await.unwrap().records.len(), 1);
    let before = server.state.lock().unwrap().calls.len();
    let mut wrong = query.clone();
    wrong.employee_id = EmployeeId::parse("another-employee").unwrap();
    assert!(service.recall(&wrong).await.is_err());
    assert_eq!(server.state.lock().unwrap().calls.len(), before);
    let restarted = adapter(Uuid::from_u128(1), config.clone());
    assert!(matches!(
        restarted.recall(&query).await,
        Err(MemoryError::Unsupported { .. })
    ));
    service
        .witnesses
        .lock()
        .unwrap()
        .get_mut(&query.employee_id)
        .unwrap()
        .expires = Some(Instant::now() - Duration::from_secs(1));
    assert!(matches!(
        service.recall(&query).await,
        Err(MemoryError::Unsupported { .. })
    ));
    assert!(!service.health(binding).await.unwrap().is_healthy());
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_adoption_is_list_only_and_never_get_or_create() {
    let server = Server::start().await;
    server.state.lock().unwrap().created = true;
    let (company, config) = fixture(&server.origin, ProvisioningMode::Adopt);
    let service = adapter(company, config.clone());
    let allowed = &config.employees[0];
    let result = service
        .ensure_resources(&MemoryResourceRequest {
            employee_id: allowed.employee_id.clone(),
            binding: allowed.binding.clone(),
            mode: ProvisioningMode::Adopt,
            idempotency_key: "read-only".into(),
        })
        .await
        .unwrap();
    assert!(result.outcomes().iter().all(|r| r.ownership.is_adopted()));
    assert!(
        service
            .validate_memory_roundtrip(&gate(&config))
            .await
            .is_err()
    );
    assert!(
        server
            .state
            .lock()
            .unwrap()
            .calls
            .iter()
            .all(|(_, p, _)| p == "/v3/ortak/protocol" || p.contains("/list?"))
    );
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_rejects_forged_receipts_and_cross_scope_recall() {
    for fault in ["hash", "metadata", "scope", "empty"] {
        let server = Server::start().await;
        let (service, config) = provision(&server).await;
        server.state.lock().unwrap().fault = Some(fault);
        assert!(
            matches!(
                service.validate_memory_roundtrip(&gate(&config)).await,
                Err(MemoryError::Rejected { .. })
            ),
            "{fault}"
        );
        assert!(!service.witnessed(&config.employees[0]).unwrap());
    }
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_bounds_bodies_rejects_redirect_and_sanitizes_server_errors() {
    for fault in ["large", "redirect", "auth"] {
        let server = Server::start().await;
        let (company, config) = fixture(&server.origin, ProvisioningMode::Create);
        let binding = config.employees[0].binding.clone();
        let service = adapter(company, config);
        server.state.lock().unwrap().fault = Some(fault);
        let error = service.probe_capabilities(&binding).await.unwrap_err();
        assert!(matches!(error, MemoryError::Rejected { .. }));
        assert!(!format!("{error:?}").contains("fresh-test-token"));
        assert_eq!(server.state.lock().unwrap().calls.len(), 1);
    }
}

#[path = "http_gates.rs"]
mod gates;

#[path = "http_recovery.rs"]
mod recovery;

#[path = "http_reviewed.rs"]
mod reviewed;

#[path = "http_employee.rs"]
mod employee;
