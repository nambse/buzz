//! Controlled HTTP bytes reach the production owned adapter; no test-only
//! witness or SQL ACK is minted. These socket tests are executed by root only.
use super::*;
use chrono::SecondsFormat;
use ortak_control::{MessageId, memory::employee::*, office_identity::OfficePublicKey};
use sha2::{Digest, Sha256};

fn identity(body: &Value) -> Value {
    let namespace = String::from_utf8(
        wire::canonical(
            &json!({"company_id":body["company_id"],"employee_id":body["employee_id"],
        "format":"ortak-reviewed-employee-namespace/1"}),
        )
        .unwrap(),
    )
    .unwrap();
    let nh = hex::encode(Sha256::digest(namespace.as_bytes()));
    json!({"company_id":body["company_id"],"employee_id":body["employee_id"],"deployment_id":body["deployment_id"],
        "binding":body["binding"],"ownership":body["ownership"],"protocol":"reviewed-employee/1","namespace":namespace,
        "namespace_hash":nh,"binding_hash":wire::fingerprint(&json!({"binding":body["binding"],"namespace_hash":nh,"protocol":"reviewed-employee/1"})).unwrap()})
}
pub(super) fn respond(
    state: &mut State,
    path: &str,
    body: Value,
) -> (u16, Value, String, Option<usize>) {
    let mut result = identity(&body);
    if path.ends_with("/namespace") {
        if state.fault == Some("employee_namespace") {
            result["ownership"]["native_ids"]["workspace"] = json!("replaced");
        }
        return (200, result, String::new(), None);
    }
    if path.contains("/diagnostics/") {
        let mut parts = path.rsplit('/');
        let action = parts.next().unwrap();
        let operation = parts.next().unwrap();
        let hash = if action == "write" {
            hex::encode(Sha256::digest(
                body["challenge"].as_str().unwrap().as_bytes(),
            ))
        } else {
            body["challenge_hash"].as_str().unwrap().to_owned()
        };
        let old = state
            .employee_diagnostics
            .entry(operation.into())
            .or_insert_with(|| {
                json!({"write_request_hash":null,"withdraw_request_hash":null,
            "challenge":null,"erased":false,"tombstone_at":null})
            });
        let mut commitment = json!({"format":if action=="write"{"ortak-reviewed-employee-diagnostic/1"}else{"ortak-reviewed-employee-diagnostic-withdraw/1"},
            "operation_id":operation,"namespace_hash":result["namespace_hash"],"binding_hash":result["binding_hash"],
            "employee_revision_id":body["employee_revision_id"],"employee_lifecycle_epoch":body["employee_lifecycle_epoch"]});
        if action == "write" {
            commitment["challenge"] = body["challenge"].clone();
            old["write_request_hash"] = json!(wire::fingerprint(&commitment).unwrap());
            if old["erased"] != true {
                old["challenge"] = body["challenge"].clone();
            }
        } else if action == "withdraw" {
            if state.fault == Some("employee_cleanup") {
                return (
                    409,
                    json!({"detail":"cleanup_refused"}),
                    String::new(),
                    None,
                );
            }
            commitment["challenge_hash"] = json!(hash);
            old["withdraw_request_hash"] = json!(wire::fingerprint(&commitment).unwrap());
            old["challenge"] = Value::Null;
            old["erased"] = json!(true);
            if old["tombstone_at"].is_null() {
                old["tombstone_at"] =
                    json!(Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true));
            }
        }
        let mut fields = old.clone();
        if action != "read" {
            fields["challenge"] = Value::Null;
        }
        if action == "read" && state.fault == Some("employee_read") {
            fields["challenge"] = json!("ee".repeat(32));
        }
        result
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        result["operation_id"] = json!(operation);
        result["employee_revision_id"] = body["employee_revision_id"].clone();
        result["employee_lifecycle_epoch"] = body["employee_lifecycle_epoch"].clone();
        result["challenge_hash"] = json!(hash);
        return (
            if action == "write" { 201 } else { 200 },
            result,
            String::new(),
            None,
        );
    }
    let value = state
        .employee_reply
        .clone()
        .expect("explicit employee response");
    let declared = if state.fault == Some("employee_large") {
        Some(65537)
    } else {
        None
    };
    (200, value, String::new(), declared)
}
async fn selected(
    server: &Server,
) -> (
    HonchoMemoryAdapter,
    HonchoMemoryConfig,
    HonchoCreatedResourcesReceipt,
    ReviewedEmployeeNamespace,
) {
    let (service, config) = provision(server).await;
    let original = service
        .created_resources_receipt(&MemoryResourceRequest {
            employee_id: config.employees[0].employee_id.clone(),
            binding: config.employees[0].binding.clone(),
            mode: ProvisioningMode::Create,
            idempotency_key: "fresh-create".into(),
        })
        .await
        .unwrap();
    let namespace = service
        .inspect_reviewed_employee_namespace(&original)
        .await
        .unwrap();
    (service, config, original, namespace)
}
fn diagnostic() -> EmployeeNamespaceDiagnostic {
    EmployeeNamespaceDiagnostic {
        operation_id: Uuid::new_v4(),
        employee_revision_id: Uuid::from_u128(31),
        employee_lifecycle_epoch: 2,
        challenge: "aa".repeat(32),
    }
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn employee_namespace_diagnostic_requires_exact_readback_cleanup_and_adapter_identity() {
    let server = Server::start().await;
    let (service, config, _, namespace) = selected(&server).await;
    let d = diagnostic();
    let witness = service
        .validate_reviewed_employee_namespace(&namespace, &d)
        .await
        .unwrap();
    service.employee_witness_current(&witness).unwrap();
    assert!(witness.diagnostic().erased);
    assert!(witness.remaining() <= Duration::from_secs(55));
    assert!(
        adapter(Uuid::from_u128(1), config)
            .employee_witness_current(&witness)
            .is_err()
    );
    let text = serde_json::to_string(witness.diagnostic()).unwrap();
    assert!(!text.contains(&d.challenge));
    let calls = server
        .state
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|(_, p, _)| p.contains("/diagnostics/"))
        .map(|(_, p, _)| p.rsplit('/').next().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(calls, ["write", "read", "withdraw"]);
    assert!(
        service
            .validate_reviewed_employee_namespace(&namespace, &d)
            .await
            .is_err(),
        "a cleaned operation is not a fresh readback"
    );
    server.state.lock().unwrap().fault = Some("employee_read");
    let bad = diagnostic();
    assert!(
        service
            .validate_reviewed_employee_namespace(&namespace, &bad)
            .await
            .is_err()
    );
    assert_eq!(
        server.state.lock().unwrap().employee_diagnostics[&bad.operation_id.to_string()]["erased"],
        true
    );
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn employee_namespace_uncertain_cleanup_recovers_same_key_without_new_write() {
    let server = Server::start().await;
    let (service, _, _, namespace) = selected(&server).await;
    let d = diagnostic();
    server.state.lock().unwrap().fault = Some("employee_cleanup");
    assert!(
        service
            .validate_reviewed_employee_namespace(&namespace, &d)
            .await
            .is_err()
    );
    let before = server.state.lock().unwrap().calls.len();
    server.state.lock().unwrap().fault = None;
    assert!(
        service
            .recover_employee_namespace_diagnostic(&namespace, &d)
            .await
            .unwrap()
            .erased
    );
    assert!(
        server.state.lock().unwrap().calls[before..]
            .iter()
            .all(|(_, p, _)| !p.ends_with("/write") && !p.ends_with("/read"))
    );
    server.state.lock().unwrap().fault = Some("employee_namespace");
    let before = server.state.lock().unwrap().calls.len();
    assert!(
        service
            .validate_reviewed_employee_namespace(&namespace, &diagnostic())
            .await
            .is_err()
    );
    assert!(
        server.state.lock().unwrap().calls[before..]
            .iter()
            .all(|(_, p, _)| !p.contains("/diagnostics/"))
    );
}

fn publication(namespace: &ReviewedEmployeeNamespace) -> (ReviewedEmployeePublication, Value) {
    let original = namespace.original();
    let human = OfficePublicKey::parse_hex(&"bb".repeat(32)).unwrap();
    let content = "The human edited this relationship fact.\n".to_owned();
    let audience = EmployeeMemoryAudienceV1::relationship(
        original.company_id,
        original.employee_id.clone(),
        Uuid::from_u128(10),
        Uuid::from_u128(11),
        human,
    )
    .unwrap();
    let source = EmployeeMemorySourceV1::new(
        Uuid::from_u128(10),
        Uuid::from_u128(12),
        MessageId::from_bytes([0xcc; 32]),
        chrono::DateTime::from_timestamp(1_780_000_000, 0).unwrap(),
        human,
        EmployeeMemoryDigest::from_bytes([0xdd; 32]),
    )
    .unwrap();
    let expires = chrono::DateTime::from_timestamp(Utc::now().timestamp() + 3600, 0).unwrap();
    let approval = EmployeeSharingApprovalV1::new(
        Uuid::from_u128(13),
        human,
        EmployeeMemoryDigest::from_bytes(Sha256::digest(content.as_bytes()).into()),
        expires,
    )
    .unwrap();
    let provenance = EmployeeMemoryProvenanceV1::new(audience, source, approval).unwrap();
    let commitment = ReviewedEmployeeCommitment {
        target_id: Uuid::from_u128(14),
        fact_id: Uuid::from_u128(15),
        destination_channel_id: Uuid::from_u128(11),
        content_hash: provenance.approval().content_hash().to_hex(),
        source_hash: provenance.source_hash().unwrap().to_hex(),
        sharing_hash: provenance.sharing_hash().unwrap().to_hex(),
    };
    let record = json!({"protocol":"reviewed-employee/1","company_id":original.company_id,"employee_id":original.employee_id,"deployment_id":original.deployment_id,
        "workspace_id":original.binding.workspace,"record_id":commitment.fact_id,"target_id":commitment.target_id,"destination_channel_id":commitment.destination_channel_id,
        "namespace_hash":namespace.namespace_hash(),"binding_hash":namespace.binding_hash(),"status":"active","content":content,
        "content_hash":commitment.content_hash,"source_hash":commitment.source_hash,"sharing_hash":commitment.sharing_hash,
        "provenance":String::from_utf8(provenance.canonical_bytes().unwrap()).unwrap(),"expires_at":expires.to_rfc3339_opts(SecondsFormat::Micros,true),
        "erased_from_reviewed_store":false,"tombstone_at":null});
    (
        ReviewedEmployeePublication {
            commitment,
            content,
            provenance,
        },
        record,
    )
}
#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn employee_publication_recall_and_cleanup_bind_exact_pins_without_recurring_diagnostic() {
    let server = Server::start().await;
    let (service, _, _, namespace) = selected(&server).await;
    let d = diagnostic();
    let _registration = service
        .validate_reviewed_employee_namespace(&namespace, &d)
        .await
        .unwrap();
    let before = server.state.lock().unwrap().calls.len();
    let (value, record) = publication(&namespace);
    let mut ack = record.clone();
    ack["content"] = Value::Null;
    ack["request_hash"] = json!(
        employee_reviewed_request_hash(
            namespace.namespace_hash(),
            namespace.binding_hash(),
            namespace.original().company_id,
            &namespace.original().employee_id,
            &value.commitment,
            false
        )
        .unwrap()
    );
    server.state.lock().unwrap().employee_reply = Some(ack.clone());
    assert!(
        service
            .publish_reviewed_employee(&namespace, &value)
            .await
            .unwrap()
            .record
            .content
            .is_none()
    );
    for (key, wrong) in [
        ("sharing_hash", json!("ee".repeat(32))),
        ("target_id", json!(Uuid::from_u128(55))),
        ("content", json!("ACK must not contain text")),
    ] {
        let mut bad = ack.clone();
        bad[key] = wrong;
        server.state.lock().unwrap().employee_reply = Some(bad);
        assert!(
            service
                .publish_reviewed_employee(&namespace, &value)
                .await
                .is_err(),
            "{key}"
        );
    }
    server.state.lock().unwrap().employee_reply =
        Some(json!({"records":[record.clone()],"truncated":false}));
    let selected = std::slice::from_ref(&value.commitment);
    assert_eq!(
        service
            .recall_selected_reviewed_employee(
                &namespace,
                value.commitment.destination_channel_id,
                Some(&"bb".repeat(32)),
                selected
            )
            .await
            .unwrap()
            .records[0]
            .content
            .as_deref(),
        Some(value.content.as_str())
    );
    assert!(
        service
            .recall_selected_reviewed_employee(
                &namespace,
                value.commitment.destination_channel_id,
                Some(&"cc".repeat(32)),
                selected
            )
            .await
            .is_err()
    );
    server.state.lock().unwrap().fault = Some("employee_large");
    assert!(
        service
            .recall_selected_reviewed_employee(
                &namespace,
                value.commitment.destination_channel_id,
                Some(&"bb".repeat(32)),
                selected
            )
            .await
            .is_err()
    );
    server.state.lock().unwrap().fault = None;
    ack["status"] = json!("withdrawn");
    ack["erased_from_reviewed_store"] = json!(true);
    ack["tombstone_at"] = json!(Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true));
    ack["request_hash"] = json!(
        employee_reviewed_request_hash(
            namespace.namespace_hash(),
            namespace.binding_hash(),
            namespace.original().company_id,
            &namespace.original().employee_id,
            &value.commitment,
            true
        )
        .unwrap()
    );
    server.state.lock().unwrap().employee_reply = Some(ack);
    assert!(
        service
            .withdraw_reviewed_employee(&namespace, &value.commitment)
            .await
            .unwrap()
            .record
            .erased_from_reviewed_store
    );
    let calls = &server.state.lock().unwrap().calls[before..];
    assert!(calls.iter().all(|(_, p, _)| !p.contains("/diagnostics/")));
    let body = &calls.last().unwrap().2;
    assert!(body.get("content").is_none() && body.get("provenance").is_none());
}
