use super::*;

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_rechecks_expiry_after_delayed_owned_inspection() {
    let server = Server::start().await;
    let (service, config) = provision(&server).await;
    service
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    let allowed = &config.employees[0];
    let refresh_short_witness = || {
        service
            .witnesses
            .lock()
            .unwrap()
            .get_mut(&allowed.employee_id)
            .unwrap()
            .expires = Some(Instant::now() + Duration::from_millis(100));
    };
    server.state.lock().unwrap().fault = Some("delay_inspect");
    let write = MemoryWriteRequest {
        employee_id: allowed.employee_id.clone(),
        binding: allowed.binding.clone(),
        scope: MemoryScope::EmployeeExperience,
        idempotency_key: "expiry-check".into(),
        facts: vec![MemoryFact {
            content: "must never be sent".into(),
            provenance: MemoryProvenance {
                employee_id: allowed.employee_id.clone(),
                run_id: None,
                source: "review".into(),
                recorded_at: Utc::now(),
            },
        }],
    };
    let before = server.state.lock().unwrap().calls.len();
    refresh_short_witness();
    assert!(matches!(
        service.remember(&write).await,
        Err(MemoryError::Unsupported { .. })
    ));
    refresh_short_witness();
    let recall = MemoryRecallRequest {
        employee_id: allowed.employee_id.clone(),
        binding: allowed.binding.clone(),
        scope: MemoryScope::EmployeeExperience,
        query: "never send".into(),
        budget: MemoryBudget::default(),
    };
    assert!(matches!(
        service.recall(&recall).await,
        Err(MemoryError::Unsupported { .. })
    ));
    assert!(
        server.state.lock().unwrap().calls[before..]
            .iter()
            .all(|(_, path, _)| !path.ends_with("/remember") && !path.ends_with("/recall"))
    );
    refresh_short_witness();
    assert!(!service.health(&allowed.binding).await.unwrap().is_healthy());
    refresh_short_witness();
    let caps = service.probe_capabilities(&allowed.binding).await.unwrap();
    assert!(!caps.capabilities.contains(&MemoryCapability::Recall));
    assert!(!caps.capabilities.contains(&MemoryCapability::Remember));
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_replaced_native_ids_cannot_reuse_matching_metadata() {
    let server = Server::start().await;
    let (service, config) = provision(&server).await;
    service
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    let allowed = &config.employees[0];
    // Native list names and metadata stay unchanged; only the immutable ID changes.
    server.state.lock().unwrap().fault = Some("native_identity");
    assert!(matches!(
        service.health(&allowed.binding).await,
        Err(MemoryError::Rejected { .. })
    ));
    assert!(matches!(
        service.probe_capabilities(&allowed.binding).await,
        Err(MemoryError::Rejected { .. })
    ));
    let before = server.state.lock().unwrap().calls.len();
    assert!(matches!(
        service
            .recall(&MemoryRecallRequest {
                employee_id: allowed.employee_id.clone(),
                binding: allowed.binding.clone(),
                scope: MemoryScope::EmployeeExperience,
                query: "test".into(),
                budget: MemoryBudget::default(),
            })
            .await,
        Err(MemoryError::Rejected { .. })
    ));
    assert!(
        server.state.lock().unwrap().calls[before..]
            .iter()
            .all(|(_, path, _)| !path.ends_with("/recall"))
    );
}

#[tokio::test]
#[ignore = "requires local HTTP sockets; run centrally"]
async fn http_contract_resume_is_read_only_and_requires_original_receipt() {
    let server = Server::start().await;
    let (_, config) = provision(&server).await;
    let restarted = adapter(Uuid::from_u128(1), config.clone());
    let allowed = &config.employees[0];
    let request = MemoryResourceRequest {
        employee_id: allowed.employee_id.clone(),
        binding: allowed.binding.clone(),
        mode: ProvisioningMode::Create,
        idempotency_key: "fresh-create".into(),
    };
    let before = server.state.lock().unwrap().calls.len();
    restarted.resume_created_resources(&request).await.unwrap();
    assert!(!restarted.witnessed(allowed).unwrap());
    assert!(
        server.state.lock().unwrap().calls[before..]
            .iter()
            .all(
                |(_, path, _)| path == "/v3/ortak/protocol" || path.ends_with("/resources/inspect")
            )
    );
    restarted
        .validate_memory_roundtrip(&gate(&config))
        .await
        .unwrap();
    let mut wrong = request.clone();
    wrong.idempotency_key = "other-create".into();
    assert!(
        adapter(Uuid::from_u128(1), config.clone())
            .resume_created_resources(&wrong)
            .await
            .is_err()
    );
    server.state.lock().unwrap().fault = Some("inspect_absent");
    assert!(
        adapter(Uuid::from_u128(1), config.clone())
            .resume_created_resources(&request)
            .await
            .is_err()
    );
    assert_eq!(
        server
            .state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|(_, path, _)| path == "/v3/ortak/resources/create")
            .count(),
        1
    );
}
