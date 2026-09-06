use super::*;

fn audience(config: &HonchoMemoryConfig) -> ReviewedProjectScope {
    ReviewedProjectScope {
        employee_id: config.employees[0].employee_id.clone(),
        binding: config.employees[0].binding.clone(),
        project_id: Uuid::from_u128(4),
    }
}
fn publication() -> ReviewedProjectPublication {
    ReviewedProjectPublication {
        record_id: Uuid::from_u128(91),
        idempotency_key: "reviewed-publish".into(),
        content: "Human approved deployment fact".into(),
        source_hash: "a".repeat(64),
        approval_id: Uuid::from_u128(92),
        approved_by: "b".repeat(64),
        expires_at: chrono::DateTime::from_timestamp(Utc::now().timestamp() + 86400, 0).unwrap(),
    }
}
fn body(scope: &ReviewedProjectScope, value: &ReviewedProjectPublication) -> Value {
    use sha2::{Digest, Sha256};
    json!({"company_id":Uuid::from_u128(1),"employee_id":scope.employee_id,"idempotency_key":value.idempotency_key,
        "content":value.content,"content_hash":hex::encode(Sha256::digest(value.content.as_bytes())),
        "source_hash":value.source_hash,"approval_id":value.approval_id,"approved_by":value.approved_by,
        "expires_at":value.expires_at.to_rfc3339_opts(chrono::SecondsFormat::Secs,true)})
}
fn record(
    service: &HonchoMemoryAdapter,
    scope: &ReviewedProjectScope,
    value: &ReviewedProjectPublication,
) -> Value {
    let identity = service
        .creation_receipts
        .lock()
        .unwrap()
        .get(&scope.employee_id)
        .cloned()
        .unwrap();
    json!({"protocol":PROTOCOL,"record_family":"reviewed-project/1","workspace_id":scope.binding.workspace,
        "project_id":scope.project_id,"record_id":value.record_id,"company_id":Uuid::from_u128(1),"employee_id":scope.employee_id,
        "binding_hash":wire::fingerprint(&json!({"request_hash":identity.request_hash,"native_ids":identity.native_ids})).unwrap(),
        "status":"active","content":value.content,"content_hash":body(scope,value)["content_hash"],"expires_at":value.expires_at,
        "provenance":{"approval_id":value.approval_id,"approved_by":value.approved_by,"source_hash":value.source_hash,"created_at":Utc::now()},
        "erased_from_reviewed_store":false,"tombstone_at":null})
}
fn acknowledgement(
    mut record: Value,
    scope: &ReviewedProjectScope,
    value: &ReviewedProjectPublication,
    request: &Value,
    action: &str,
) -> Value {
    let mut all = request.clone();
    for (key, value) in [
        ("family", json!("reviewed-project/1")),
        ("workspace_id", json!(scope.binding.workspace)),
        ("project_id", json!(scope.project_id)),
        ("record_id", json!(value.record_id)),
        ("action", json!(action)),
    ] {
        all[key] = value;
    }
    record["content"] = Value::Null;
    record["request_hash"] = json!(wire::fingerprint(&all).unwrap());
    record
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_reviewed_uses_exact_wire_and_rejects_forged_receipts() {
    let server = Server::start().await;
    let (service, config) = provision(&server).await;
    let scope = audience(&config);
    let value = publication();
    assert!(
        service
            .publish_reviewed_project(&scope, &value)
            .await
            .is_err()
    );
    service
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    let baseline = record(&service, &scope, &value);
    let ack = acknowledgement(
        baseline.clone(),
        &scope,
        &value,
        &body(&scope, &value),
        "publish",
    );
    server.state.lock().unwrap().reviewed_reply = Some(ack.clone());
    let receipt = service
        .publish_reviewed_project(&scope, &value)
        .await
        .unwrap();
    assert_eq!(receipt.record.record_id, value.record_id);
    assert!(receipt.record.content.is_none());
    assert_eq!(
        server.state.lock().unwrap().calls.last().unwrap().2,
        body(&scope, &value)
    );
    for (field, bad) in [
        ("request_hash", json!("c".repeat(64))),
        ("binding_hash", json!("d".repeat(64))),
        ("employee_id", json!("other")),
        ("project_id", json!(Uuid::from_u128(98))),
        ("record_family", json!("native")),
        ("content_hash", json!("e".repeat(64))),
        ("content", json!("unexpected acknowledgement text")),
    ] {
        let mut bad_ack = ack.clone();
        bad_ack[field] = bad;
        server.state.lock().unwrap().reviewed_reply = Some(bad_ack);
        assert!(
            service
                .publish_reviewed_project(&scope, &value)
                .await
                .is_err(),
            "{field}"
        );
    }
    let mut wrong = scope.clone();
    wrong.project_id = Uuid::from_u128(99);
    let before = server.state.lock().unwrap().calls.len();
    assert!(
        service
            .publish_reviewed_project(&wrong, &value)
            .await
            .is_err()
    );
    assert_eq!(server.state.lock().unwrap().calls.len(), before);
    server.state.lock().unwrap().reviewed_reply =
        Some(json!({"records":[baseline.clone()],"next_after":null}));
    assert_eq!(
        service
            .inspect_reviewed_project(&scope, None)
            .await
            .unwrap()
            .records
            .len(),
        1
    );
    server.state.lock().unwrap().reviewed_reply =
        Some(json!({"records":[baseline.clone()],"truncated":false}));
    assert_eq!(
        service
            .recall_reviewed_project(&scope, "deployment")
            .await
            .unwrap()
            .records
            .len(),
        1
    );
    server.state.lock().unwrap().reviewed_reply =
        Some(json!({"records":[baseline.clone(),baseline.clone()],"truncated":false}));
    assert!(
        service
            .recall_reviewed_project(&scope, "deployment")
            .await
            .is_err()
    );
    let removal_body = json!({"company_id":Uuid::from_u128(1),"employee_id":scope.employee_id,"idempotency_key":"stop-one"});
    let mut stopped = baseline;
    stopped["status"] = json!("withdrawn");
    stopped["erased_from_reviewed_store"] = json!(true);
    stopped["tombstone_at"] = json!(Utc::now());
    server.state.lock().unwrap().reviewed_reply = Some(acknowledgement(
        stopped.clone(),
        &scope,
        &value,
        &removal_body,
        "withdraw",
    ));
    assert!(
        service
            .remove_reviewed_project(
                &scope,
                value.record_id,
                "stop-one",
                ReviewedProjectRemoval::Withdraw
            )
            .await
            .unwrap()
            .record
            .erased_from_reviewed_store
    );
    stopped["erased_from_reviewed_store"] = json!(false);
    server.state.lock().unwrap().reviewed_reply = Some(acknowledgement(
        stopped,
        &scope,
        &value,
        &removal_body,
        "withdraw",
    ));
    assert!(
        service
            .remove_reviewed_project(
                &scope,
                value.record_id,
                "stop-one",
                ReviewedProjectRemoval::Withdraw
            )
            .await
            .is_err()
    );
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_reviewed_rechecks_io_generation_after_current_ownership_read() {
    let server = Server::start().await;
    let (service, config) = provision(&server).await;
    service
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    let scope = audience(&config);
    server.state.lock().unwrap().fault = Some("delay_inspect");
    let before = server.state.lock().unwrap().calls.len();
    for operation in 0..5 {
        service
            .witnesses
            .lock()
            .unwrap()
            .get_mut(&scope.employee_id)
            .unwrap()
            .expires = Some(Instant::now() + Duration::from_millis(100));
        let refused = match operation {
            0 => service
                .publish_reviewed_project(&scope, &publication())
                .await
                .is_err(),
            1 => service
                .remove_reviewed_project(
                    &scope,
                    Uuid::from_u128(91),
                    "stop",
                    ReviewedProjectRemoval::Withdraw,
                )
                .await
                .is_err(),
            2 => service
                .inspect_reviewed_project(&scope, None)
                .await
                .is_err(),
            3 => service
                .recall_reviewed_project(&scope, "deployment")
                .await
                .is_err(),
            _ => service
                .recall_selected_reviewed_project(
                    &scope,
                    "deployment",
                    &BTreeSet::from([Uuid::from_u128(91)]),
                )
                .await
                .is_err(),
        };
        assert!(refused);
    }
    assert!(
        server.state.lock().unwrap().calls[before..]
            .iter()
            .all(|(_, path, _)| !path.contains("/reviewed-projects/"))
    );
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_reviewed_selected_pins_exact_ids_and_rejects_foreign_results() {
    let server = Server::start().await;
    let (service, config) = provision(&server).await;
    service
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    let scope = audience(&config);
    let value = publication();
    let ids = BTreeSet::from([value.record_id]);
    let baseline = record(&service, &scope, &value);
    server.state.lock().unwrap().reviewed_reply =
        Some(json!({"records":[baseline.clone()],"truncated":false}));
    let recalled = service
        .recall_selected_reviewed_project(&scope, "deployment", &ids)
        .await
        .unwrap();
    assert_eq!(recalled.records[0].record_id, value.record_id);
    let call = server.state.lock().unwrap().calls.last().unwrap().clone();
    assert!(call.1.ends_with("/recall-selected"));
    assert_eq!(
        call.2,
        json!({"company_id":Uuid::from_u128(1),"employee_id":scope.employee_id,
        "query":"deployment","record_ids":[value.record_id]})
    );
    let mut foreign = baseline;
    foreign["record_id"] = json!(Uuid::from_u128(999));
    server.state.lock().unwrap().reviewed_reply =
        Some(json!({"records":[foreign],"truncated":false}));
    assert!(
        service
            .recall_selected_reviewed_project(&scope, "deployment", &ids)
            .await
            .is_err()
    );
    let before = server.state.lock().unwrap().calls.len();
    for invalid in [
        BTreeSet::new(),
        BTreeSet::from([Uuid::nil()]),
        (1..=33).map(Uuid::from_u128).collect(),
    ] {
        assert!(
            service
                .recall_selected_reviewed_project(&scope, "deployment", &invalid)
                .await
                .is_err()
        );
    }
    assert_eq!(server.state.lock().unwrap().calls.len(), before);
}
