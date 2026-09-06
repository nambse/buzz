//! Local-socket transport only. Real adapter owns creation, diagnostics and
//! response validation; production PG/signature code owns every durable effect.
use super::*;
use ortak_control::CompanyScope;
use ortak_control::memory::{MemoryAdapter, MemoryResourceRequest};
use ortak_domain::{CredentialRef, MemoryBinding, ProvisioningMode};
use ortak_memory::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub struct Remote {
    pub service: HonchoMemoryAdapter,
    pub namespace: ReviewedEmployeeNamespace,
    pub scope: CompanyScope,
    pub state: Arc<Mutex<State>>,
    task: tokio::task::JoinHandle<()>,
}
pub struct State {
    binding: MemoryBinding,
    company: Uuid,
    created: Option<Value>,
    diagnostics: BTreeMap<String, Value>,
    records: BTreeMap<String, Value>,
    content: BTreeMap<String, Value>,
    pub calls: Vec<(String, Value)>,
    pub forge_ack: bool,
}
impl Drop for Remote {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Remote {
    pub(in crate::employee_memory) async fn new(x: &MemoryFixture) -> Self {
        Self::new_on(&x.f).await
    }
    pub async fn new_on(f: &Fixture) -> Self {
        let binding:MemoryBinding=serde_json::from_value(sqlx::query_scalar::<_,Value>("SELECT r.manifest->'memory' FROM employee_revisions r
            JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id AND e.active_revision_id=r.id WHERE e.company_id=$1 AND e.id='cem'")
            .bind(f.company).fetch_one(&f.pool).await.unwrap()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State {
            binding: binding.clone(),
            company: f.company,
            created: None,
            diagnostics: BTreeMap::new(),
            records: BTreeMap::new(),
            content: BTreeMap::new(),
            calls: vec![],
            forge_ack: false,
        }));
        let shared = state.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut data = vec![];
                let mut chunk = [0; 4096];
                let end = loop {
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    data.extend_from_slice(&chunk[..n]);
                    if let Some(p) = data.windows(4).position(|v| v == b"\r\n\r\n") {
                        break p + 4;
                    }
                    assert!(data.len() < 16384);
                };
                let headers = String::from_utf8(data[..end].to_vec()).unwrap();
                assert!(
                    headers
                        .to_ascii_lowercase()
                        .contains("authorization: bearer fixture-owned-token")
                );
                let path = headers
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                let len = headers
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|s| s.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                assert!(len <= 32768);
                while data.len() - end < len {
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    data.extend_from_slice(&chunk[..n]);
                }
                let body = if len == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&data[end..end + len]).unwrap()
                };
                let (status, response) = {
                    let mut s = shared.lock().unwrap();
                    s.calls.push((path.clone(), body.clone()));
                    respond(&mut s, &path, body)
                };
                let body = serde_json::to_vec(&response).unwrap();
                assert!(body.len() <= 65536);
                let header = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            }
        });
        let scope = f
            .control
            .resolve_company_for_community(f.community)
            .await
            .unwrap();
        let token = CredentialRef::parse("secret://fixture/owned-employee").unwrap();
        let config = HonchoMemoryConfig {
            deployment: HonchoDeploymentSelection {
                deployment_id: Uuid::new_v4(),
                protocol: PROTOCOL.into(),
                honcho_version: HONCHO_VERSION.into(),
                endpoint_ref: binding.endpoint_ref.clone(),
                origin,
                token_ref: token.clone(),
            },
            employees: vec![HonchoEmployeeBinding {
                employee_id: EmployeeId::parse("cem").unwrap(),
                binding: binding.clone(),
                mode: ProvisioningMode::Create,
                allow_company_truth: false,
                allowed_projects: BTreeSet::new(),
            }],
            request_timeout: std::time::Duration::from_secs(3),
            witness_lifetime: std::time::Duration::from_secs(60),
        };
        let service = HonchoMemoryAdapter::new(
            &scope,
            config,
            ResolvedHonchoToken::new(token, "fixture-owned-token".to_owned().into()),
        )
        .unwrap();
        let request = MemoryResourceRequest {
            employee_id: EmployeeId::parse("cem").unwrap(),
            binding,
            mode: ProvisioningMode::Create,
            idempotency_key: "fixture-create".into(),
        };
        service.ensure_resources(&request).await.unwrap();
        let original = service.created_resources_receipt(&request).await.unwrap();
        let namespace = service
            .inspect_reviewed_employee_namespace(&original)
            .await
            .unwrap();
        Self {
            service,
            namespace,
            scope,
            state,
            task,
        }
    }
    pub(in crate::employee_memory) async fn register(&self, x: &MemoryFixture) -> (Uuid, DateTime<Utc>) {
        self.register_on(&x.f, x.f.hidden).await
    }
    pub async fn register_on(&self, f: &Fixture, destination: Uuid) -> (Uuid, DateTime<Utc>) {
        let(revision,epoch):(Uuid,i64)=sqlx::query_as("SELECT active_revision_id,lifecycle_epoch FROM employees WHERE company_id=$1 AND id='cem'")
            .bind(f.company).fetch_one(&f.pool).await.unwrap();
        let request = EmployeeNamespaceDiagnostic {
            operation_id: Uuid::new_v4(),
            employee_revision_id: revision,
            employee_lifecycle_epoch: epoch,
            challenge: "aa".repeat(32),
        };
        let witness = self
            .service
            .validate_reviewed_employee_namespace(&self.namespace, &request)
            .await
            .unwrap();
        let until = DateTime::from_timestamp(Utc::now().timestamp() + 86400, 0).unwrap();
        let id = ortak_server::employee_memory_exports::register_target(
            &f.control,
            &self.scope,
            &self.service,
            &witness,
            destination,
            until,
        )
        .await
        .unwrap();
        assert_eq!(
            id,
            ortak_server::employee_memory_exports::register_target(
                &f.control,
                &self.scope,
                &self.service,
                &witness,
                destination,
                until
            )
            .await
            .unwrap()
        );
        (id, until)
    }
    pub fn diagnostic_count(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(p, _)| p.contains("/diagnostics/"))
            .count()
    }
}
fn normalized(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let sorted: BTreeMap<_, _> = map
                .iter()
                .map(|(k, v)| (k.clone(), normalized(v)))
                .collect();
            serde_json::to_value(sorted).unwrap()
        }
        Value::Array(a) => Value::Array(a.iter().map(normalized).collect()),
        _ => v.clone(),
    }
}
fn hash(v: &Value) -> String {
    hex::encode(Sha256::digest(serde_json::to_vec(&normalized(v)).unwrap()))
}
fn identity(b: &Value) -> Value {
    let n=serde_json::to_string(&normalized(&json!({"format":"ortak-reviewed-employee-namespace/1","company_id":b["company_id"],"employee_id":b["employee_id"]}))).unwrap();
    let nh = hex::encode(Sha256::digest(n.as_bytes()));
    let bh =
        hash(&json!({"binding":b["binding"],"namespace_hash":nh,"protocol":"reviewed-employee/1"}));
    json!({"company_id":b["company_id"],"employee_id":b["employee_id"],"deployment_id":b["deployment_id"],"binding":b["binding"],"ownership":b["ownership"],
        "protocol":"reviewed-employee/1","namespace":n,"namespace_hash":nh,"binding_hash":bh})
}
fn respond(s: &mut State, p: &str, b: Value) -> (u16, Value) {
    let owner = json!({"protocol":PROTOCOL,"company_id":s.company,"employee_id":"cem"});
    let m = &s.binding;
    if p == "/v3/ortak/protocol" {
        return (
            200,
            json!({"protocol":PROTOCOL,"honcho_version":HONCHO_VERSION}),
        );
    }
    if p.starts_with("/v3/workspaces/list?") {
        let items = if s.created.is_some() {
            vec![json!({"id":m.workspace,"metadata":{"ortak":owner}})]
        } else {
            vec![]
        };
        return (
            200,
            json!({"total":items.len(),"pages":usize::from(!items.is_empty()),"page":1,"size":100,"items":items}),
        );
    }
    if p.contains("/peers/list?") {
        return (
            200,
            json!({"total":2,"pages":1,"page":1,"size":100,"items":[
        {"id":m.user_peer,"workspace_id":m.workspace,"metadata":{"ortak":owner}},{"id":m.employee_peer,"workspace_id":m.workspace,"metadata":{"ortak":owner}}]}),
        );
    }
    if p == "/v3/ortak/resources/create" {
        s.created = Some(b);
        return (
            201,
            json!({"protocol":PROTOCOL,"workspace_id":m.workspace,"user_peer":m.user_peer,"employee_peer":m.employee_peer,"ownership":"created"}),
        );
    }
    if p.ends_with("/resources/inspect") {
        return (
            200,
            json!({"protocol":PROTOCOL,"company_id":s.company,"employee_id":"cem","workspace_id":m.workspace,
        "user_peer":m.user_peer,"employee_peer":m.employee_peer,"ownership":"created","request_hash":hash(s.created.as_ref().unwrap()),
        "native_ids":{"workspace":"native_workspace","peers":{m.user_peer.clone():"native_human",m.employee_peer.clone():"native_employee"}}}),
        );
    }
    let mut id = identity(&b);
    if p.ends_with("/namespace") {
        return (200, id);
    }
    if p.ends_with("/recall-selected") {
        let ids = b["record_ids"].as_array().unwrap();
        assert!(!ids.is_empty() && ids.len() <= 8);
        let records: Vec<Value> = ids
            .iter()
            .filter_map(|id| {
                let key = id.as_str().unwrap();
                let mut record = s.records.get(key)?.clone();
                if record["status"] != "active"
                    || record["destination_channel_id"] != b["destination_channel_id"]
                {
                    return None;
                }
                let provenance: Value =
                    serde_json::from_str(record["provenance"].as_str().unwrap()).unwrap();
                if provenance["audience"]["kind"] == "relationship"
                    && provenance["audience"]["human_public_key"] != b["human_public_key"]
                {
                    return None;
                }
                record["content"] = s.content.get(key)?.clone();
                Some(record)
            })
            .collect();
        return (200, json!({"records":records,"truncated":false}));
    }
    let mut parts = p.rsplit('/');
    let action = parts.next().unwrap();
    let record = parts.next().unwrap();
    if p.contains("/diagnostics/") {
        let ch = if action == "write" {
            hex::encode(Sha256::digest(b["challenge"].as_str().unwrap().as_bytes()))
        } else {
            b["challenge_hash"].as_str().unwrap().into()
        };
        let d=s.diagnostics.entry(record.into()).or_insert_with(||json!({"write_request_hash":null,"withdraw_request_hash":null,"challenge":null,"erased":false,"tombstone_at":null}));
        let mut wire = json!({"format":if action=="write"{"ortak-reviewed-employee-diagnostic/1"}else{"ortak-reviewed-employee-diagnostic-withdraw/1"},
            "operation_id":record,"namespace_hash":id["namespace_hash"],"binding_hash":id["binding_hash"],"employee_revision_id":b["employee_revision_id"],
            "employee_lifecycle_epoch":b["employee_lifecycle_epoch"]});
        if action == "write" {
            wire["challenge"] = b["challenge"].clone();
            d["write_request_hash"] = json!(hash(&wire));
            if d["erased"] != true {
                d["challenge"] = b["challenge"].clone();
            }
        }
        if action == "withdraw" {
            wire["challenge_hash"] = json!(ch);
            d["withdraw_request_hash"] = json!(hash(&wire));
            d["challenge"] = Value::Null;
            d["erased"] = json!(true);
            if d["tombstone_at"].is_null() {
                d["tombstone_at"] = json!(Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true));
            }
        }
        let mut metadata = d.clone();
        if action != "read" {
            metadata["challenge"] = Value::Null;
        }
        id.as_object_mut()
            .unwrap()
            .extend(metadata.as_object().unwrap().clone());
        id["operation_id"] = json!(record);
        id["employee_revision_id"] = b["employee_revision_id"].clone();
        id["employee_lifecycle_epoch"] = b["employee_lifecycle_epoch"].clone();
        id["challenge_hash"] = json!(ch);
        return (200, id);
    }
    assert!(p.contains("/records/"));
    let first = !s.records.contains_key(record);
    let r=s.records.entry(record.into()).or_insert_with(||json!({"protocol":"reviewed-employee/1","company_id":b["company_id"],"employee_id":b["employee_id"],
        "deployment_id":b["deployment_id"],"workspace_id":b["binding"]["workspace"],"record_id":record,"target_id":b["target_id"],"destination_channel_id":b["destination_channel_id"],
        "namespace_hash":id["namespace_hash"],"binding_hash":id["binding_hash"],"status":"withdrawn","content":null,"content_hash":b["content_hash"],"source_hash":b["source_hash"],
        "sharing_hash":b["sharing_hash"],"provenance":null,"expires_at":null,"erased_from_reviewed_store":false,"tombstone_at":null}));
    if action == "publish" {
        r["provenance"] = b["provenance"].clone();
        r["expires_at"] = serde_json::from_str::<Value>(b["provenance"].as_str().unwrap()).unwrap()
            ["approval"]["expires_at"]
            .clone();
        if r["tombstone_at"].is_null() {
            r["status"] = json!("active");
            s.content.insert(record.into(), b["content"].clone());
        }
    } else {
        assert_eq!(action, "withdraw");
        r["status"] = json!("withdrawn");
        r["erased_from_reviewed_store"] = json!(true);
        s.content.remove(record);
        if r["tombstone_at"].is_null() {
            r["tombstone_at"] = json!(Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true));
        }
    }
    let mut response = r.clone();
    response["request_hash"] = json!(hash(
        &json!({"action":action,"binding_hash":id["binding_hash"],"company_id":b["company_id"],
        "content_hash":b["content_hash"],"employee_id":b["employee_id"],"fact_id":record,"format":"ortak-reviewed-employee-remote-request/1",
        "namespace_hash":id["namespace_hash"],"sharing_hash":b["sharing_hash"],"source_hash":b["source_hash"],"target_id":b["target_id"]})
    ));
    if s.forge_ack {
        response["request_hash"] = json!("00".repeat(32));
    }
    (
        if first && action == "publish" {
            201
        } else {
            200
        },
        response,
    )
}
