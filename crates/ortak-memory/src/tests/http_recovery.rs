use super::*;

fn create_request(config: &HonchoMemoryConfig) -> MemoryResourceRequest {
    let allowed = &config.employees[0];
    MemoryResourceRequest {
        employee_id: allowed.employee_id.clone(),
        binding: allowed.binding.clone(),
        mode: ProvisioningMode::Create,
        idempotency_key: "fresh-create".into(),
    }
}

async fn prepared(server: &Server) -> (HonchoCreatedResourcesReceipt, HonchoMemoryConfig) {
    let (created, mut config) = provision(server).await;
    let receipt = created
        .created_resources_receipt(&create_request(&config))
        .await
        .unwrap();
    // Both activation and execution consume the same durable serialized evidence.
    let bytes = serde_json::to_vec(&receipt).unwrap();
    let restored = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(receipt, restored);
    config.employees[0].mode = ProvisioningMode::Adopt;
    (restored, config)
}

fn read_only(calls: &[(String, String, Value)]) -> bool {
    calls.iter().all(|(_, path, _)| {
        path == "/v3/ortak/protocol"
            || path.ends_with("/resources/inspect")
            || path.contains("/list?")
    })
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_recovery_separates_adopted_acquisition_from_explicit_io() {
    let server = Server::start().await;
    let (receipt, config) = prepared(&server).await;
    let service = adapter(receipt.company_id, config.clone());
    let allowed = &config.employees[0];
    let before = server.state.lock().unwrap().calls.len();
    let outcome = service.recover_created_resources(&receipt).await.unwrap();
    assert!(outcome.outcomes().iter().all(|r| r.ownership.is_adopted()));
    assert!(receipt
        .resources
        .outcomes()
        .iter()
        .all(|r| !r.ownership.is_adopted()));
    assert!(!service.witnessed(allowed).unwrap());
    assert!(!service.health(&allowed.binding).await.unwrap().is_healthy());
    let capabilities = service.probe_capabilities(&allowed.binding).await.unwrap();
    assert_eq!(
        capabilities.capabilities,
        BTreeSet::from([
            MemoryCapability::HealthProbe,
            MemoryCapability::ResourceInspect
        ])
    );
    let mut adopt = create_request(&config);
    adopt.mode = ProvisioningMode::Adopt;
    adopt.idempotency_key = "separate-saga-adopt-key".into();
    assert_eq!(service.ensure_resources(&adopt).await.unwrap(), outcome);
    assert!(!service.witnessed(allowed).unwrap());
    assert!(read_only(&server.state.lock().unwrap().calls[before..]));

    let diagnostic = service
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    assert!(service.health(&allowed.binding).await.unwrap().is_healthy());
    let capabilities = service.probe_capabilities(&allowed.binding).await.unwrap();
    assert!(capabilities
        .capabilities
        .contains(&MemoryCapability::Recall));
    assert!(capabilities
        .capabilities
        .contains(&MemoryCapability::Remember));
    assert!(!capabilities
        .capabilities
        .contains(&MemoryCapability::ResourceCreate));
    assert!(!capabilities
        .capabilities
        .contains(&MemoryCapability::ResourceDelete));
    assert_eq!(service.ensure_resources(&adopt).await.unwrap(), outcome);
    let recall = MemoryRecallRequest {
        employee_id: allowed.employee_id.clone(),
        binding: allowed.binding.clone(),
        scope: diagnostic.scope,
        query: "roundtrip".into(),
        budget: MemoryBudget::default(),
    };
    assert_eq!(service.recall(&recall).await.unwrap().records.len(), 1);
    let before = server.state.lock().unwrap().calls.len();
    assert!(matches!(
        service
            .delete_created_resource("workspace:private_employee_one", "compensate")
            .await,
        Err(MemoryError::Unsupported {
            capability: MemoryCapability::ResourceDelete
        })
    ));
    assert!(service
        .ensure_resources(&create_request(&config))
        .await
        .is_err());
    assert!(service
        .created_resources_receipt(&create_request(&config))
        .await
        .is_err());
    assert_eq!(server.state.lock().unwrap().calls.len(), before);
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_recovery_restart_requires_frozen_ids_and_fresh_witness() {
    let server = Server::start().await;
    let (receipt, config) = prepared(&server).await;
    let first = adapter(receipt.company_id, config.clone());
    first.recover_created_resources(&receipt).await.unwrap();
    let diagnostic = first
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    let restarted = adapter(receipt.company_id, config.clone());
    assert!(restarted
        .validate_memory_roundtrip(&gate(&config))
        .await
        .is_err());
    restarted.recover_created_resources(&receipt).await.unwrap();
    assert!(!restarted.witnessed(&config.employees[0]).unwrap());
    let repeated = restarted
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    assert_eq!(repeated.write_receipt, diagnostic.write_receipt);
    let writes: Vec<_> = server
        .state
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|(_, path, _)| path.ends_with("/remember"))
        .map(|(_, _, body)| body.clone())
        .collect();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0], writes[1]);

    // A new adapter may not treat a replacement with the same names/hash as
    // its first valid identity: it must match the original durable native IDs.
    server.state.lock().unwrap().fault = Some("native_identity");
    let replaced = adapter(receipt.company_id, config.clone());
    let before = server.state.lock().unwrap().calls.len();
    assert!(matches!(
        replaced.recover_created_resources(&receipt).await,
        Err(MemoryError::Rejected { .. })
    ));
    assert!(replaced
        .validate_memory_roundtrip(&gate(&config))
        .await
        .is_err());
    assert!(!replaced.witnessed(&config.employees[0]).unwrap());
    assert!(read_only(&server.state.lock().unwrap().calls[before..]));
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_recovery_refuses_tampered_selection_before_http() {
    let server = Server::start().await;
    let (receipt, config) = prepared(&server).await;
    for field in [
        "company",
        "deployment",
        "employee",
        "binding",
        "key",
        "hash",
        "outcome",
        "native_shape",
    ] {
        let mut altered = receipt.clone();
        match field {
            "company" => altered.company_id = Uuid::from_u128(99),
            "deployment" => altered.deployment_id = Uuid::from_u128(99),
            "employee" => altered.employee_id = EmployeeId::parse("other-employee").unwrap(),
            "binding" => altered.binding.endpoint_ref = "service://other".into(),
            "key" => altered.creation_key = "new-create-key".into(),
            "hash" => altered.request_hash = "0".repeat(64),
            "outcome" => altered.resources = resources::outcome(&config.employees[0], false),
            "native_shape" => altered.native_ids.peers.clear(),
            _ => unreachable!(),
        }
        let service = adapter(receipt.company_id, config.clone());
        let before = server.state.lock().unwrap().calls.len();
        assert!(
            service.recover_created_resources(&altered).await.is_err(),
            "{field}"
        );
        assert!(service
            .validate_memory_roundtrip(&gate(&config))
            .await
            .is_err());
        assert_eq!(server.state.lock().unwrap().calls.len(), before, "{field}");
    }
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_recovery_self_asserted_receipt_never_replaces_server_evidence() {
    let server = Server::start().await;
    let (receipt, config) = prepared(&server).await;
    for fault in ["native_receipt", "create_hash", "absent"] {
        let mut altered = receipt.clone();
        let service = adapter(receipt.company_id, config.clone());
        match fault {
            "native_receipt" => altered.native_ids.workspace = "self_asserted_native_id".into(),
            "create_hash" => {
                altered.creation_key = "self-asserted-create-key".into();
                altered.request_hash = wire::fingerprint(
                    &service.creation_body(&config.employees[0], &altered.creation_key),
                )
                .unwrap();
            }
            "absent" => server.state.lock().unwrap().fault = Some("inspect_absent"),
            _ => unreachable!(),
        }
        let before = server.state.lock().unwrap().calls.len();
        assert!(
            matches!(
                service.recover_created_resources(&altered).await,
                Err(MemoryError::Rejected { .. })
            ),
            "{fault}"
        );
        assert!(service
            .validate_memory_roundtrip(&gate(&config))
            .await
            .is_err());
        let state = server.state.lock().unwrap();
        assert!(state.calls.len() > before);
        assert!(read_only(&state.calls[before..]));
    }
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_recovered_adoption_enforces_expiry_at_actual_io() {
    let server = Server::start().await;
    let (receipt, config) = prepared(&server).await;
    let service = adapter(receipt.company_id, config.clone());
    service.recover_created_resources(&receipt).await.unwrap();
    service
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    let allowed = &config.employees[0];
    service
        .witnesses
        .lock()
        .unwrap()
        .get_mut(&allowed.employee_id)
        .unwrap()
        .expires = Some(Instant::now() + Duration::from_millis(100));
    server.state.lock().unwrap().fault = Some("delay_inspect");
    let before = server.state.lock().unwrap().calls.len();
    assert!(matches!(
        service
            .remember(&MemoryWriteRequest {
                employee_id: allowed.employee_id.clone(),
                binding: allowed.binding.clone(),
                scope: MemoryScope::EmployeeExperience,
                idempotency_key: "must-not-write-expired".into(),
                facts: vec![MemoryFact {
                    content: "must not be sent".into(),
                    provenance: MemoryProvenance {
                        employee_id: allowed.employee_id.clone(),
                        run_id: None,
                        source: "recovery_test".into(),
                        recorded_at: Utc::now(),
                    },
                }],
            })
            .await,
        Err(MemoryError::Unsupported {
            capability: MemoryCapability::Remember
        })
    ));
    assert!(read_only(&server.state.lock().unwrap().calls[before..]));
}
