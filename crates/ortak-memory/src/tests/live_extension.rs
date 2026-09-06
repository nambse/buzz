use super::*;

#[tokio::test]
#[ignore = "requires explicit fresh Honcho extension URL/admin token; creates isolated disposable resources"]
async fn live_extension_create_roundtrip_replay_and_scoped_recall() {
    let origin = std::env::var("ORTAK_HONCHO_TEST_URL")
        .expect("set ORTAK_HONCHO_TEST_URL to the fresh extension");
    let company = Uuid::new_v4();
    let (_, mut config) = fixture(&origin, ProvisioningMode::Create);
    config.employees[0].binding.workspace = format!("ortak_rust_{}", company.simple());
    config.request_timeout = Duration::from_secs(15);
    let build = || {
        let token = ResolvedHonchoToken::from_env(
            config.deployment.token_ref.clone(),
            "ORTAK_HONCHO_TEST_TOKEN",
        )
        .unwrap();
        HonchoMemoryAdapter::for_company(company, config.clone(), token).unwrap()
    };
    let service = build();
    let allowed = &config.employees[0];
    let create = MemoryResourceRequest {
        employee_id: allowed.employee_id.clone(),
        binding: allowed.binding.clone(),
        mode: ProvisioningMode::Create,
        idempotency_key: format!("rust-create-{company}"),
    };
    let resources = service.ensure_resources(&create).await.unwrap();
    let original = service.created_resources_receipt(&create).await.unwrap();
    assert!(resources
        .outcomes()
        .iter()
        .all(|r| !r.ownership.is_adopted()));
    let mut validation = gate(&config);
    validation.run_id = Uuid::new_v4();
    validation.recorded_at = Utc::now();
    let validated = service
        .validate_memory_roundtrip(&validation)
        .await
        .unwrap();
    assert!(service.health(&allowed.binding).await.unwrap().is_healthy());
    let text = format!("Scoped employee memory validation {}", company.simple());
    let write = MemoryWriteRequest {
        employee_id: allowed.employee_id.clone(),
        binding: allowed.binding.clone(),
        scope: MemoryScope::EmployeeExperience,
        facts: vec![MemoryFact {
            content: text.clone(),
            provenance: MemoryProvenance {
                employee_id: allowed.employee_id.clone(),
                run_id: Some(validation.run_id),
                source: "ortak_rust_contract".into(),
                recorded_at: validation.recorded_at,
            },
        }],
        idempotency_key: format!("rust-memory-{company}"),
    };
    let receipt = service.remember(&write).await.unwrap();
    assert_eq!(receipt, service.remember(&write).await.unwrap());
    let query = MemoryRecallRequest {
        employee_id: allowed.employee_id.clone(),
        binding: allowed.binding.clone(),
        scope: write.scope.clone(),
        query: text.clone(),
        budget: MemoryBudget::default(),
    };
    let result = service.recall(&query).await.unwrap();
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].content, text);
    let mut other = query.clone();
    other.scope = MemoryScope::ProjectContext {
        project_id: Uuid::from_u128(4),
    };
    assert!(service.recall(&other).await.unwrap().records.is_empty());
    let restarted = build();
    assert!(matches!(
        restarted.recall(&query).await,
        Err(MemoryError::Unsupported { .. })
    ));
    assert!(matches!(
        restarted.validate_memory_roundtrip(&validation).await,
        Err(MemoryError::Unsupported { .. })
    ));
    assert_eq!(
        resources,
        restarted
            .recover_created_resources(&original)
            .await
            .unwrap()
    );
    assert_eq!(
        validated.write_receipt,
        restarted
            .validate_memory_roundtrip(&validation)
            .await
            .unwrap()
            .write_receipt
    );
    assert_eq!(receipt, restarted.remember(&write).await.unwrap());
    let mut adoption_config = config.clone();
    adoption_config.employees[0].mode = ProvisioningMode::Adopt;
    let token = ResolvedHonchoToken::from_env(
        adoption_config.deployment.token_ref.clone(),
        "ORTAK_HONCHO_TEST_TOKEN",
    )
    .unwrap();
    let adopted = HonchoMemoryAdapter::for_company(company, adoption_config, token).unwrap();
    let mut inspect = create.clone();
    inspect.mode = ProvisioningMode::Adopt;
    assert!(adopted
        .ensure_resources(&inspect)
        .await
        .unwrap()
        .any_adopted());
    assert!(matches!(
        adopted.recall(&query).await,
        Err(MemoryError::Unsupported { .. })
    ));
    assert!(adopted
        .recover_created_resources(&original)
        .await
        .unwrap()
        .outcomes()
        .iter()
        .all(|resource| resource.ownership.is_adopted()));
    assert!(matches!(
        adopted.recall(&query).await,
        Err(MemoryError::Unsupported { .. })
    ));
    assert_eq!(
        validated.write_receipt,
        adopted
            .validate_memory_roundtrip(&validation)
            .await
            .unwrap()
            .write_receipt
    );
    assert_eq!(receipt, adopted.remember(&write).await.unwrap());
    assert!(adopted
        .ensure_resources(&inspect)
        .await
        .unwrap()
        .outcomes()
        .iter()
        .all(|resource| resource.ownership.is_adopted()));
}
