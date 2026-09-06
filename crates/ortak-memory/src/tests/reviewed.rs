use super::*;

#[tokio::test]
async fn reviewed_operations_never_turn_read_only_adoption_into_io_authority() {
    let (company, config) = fixture("http://127.0.0.1:1", ProvisioningMode::Adopt);
    let scope = ReviewedProjectScope {
        employee_id: config.employees[0].employee_id.clone(),
        binding: config.employees[0].binding.clone(),
        project_id: Uuid::from_u128(4),
    };
    let service = adapter(company, config);
    let publication = ReviewedProjectPublication {
        record_id: Uuid::from_u128(7),
        idempotency_key: "approval-one".into(),
        content: "Human reviewed fact".into(),
        source_hash: "a".repeat(64),
        approval_id: Uuid::from_u128(8),
        approved_by: "b".repeat(64),
        expires_at: Utc::now() + chrono::Duration::days(1),
    };
    assert!(matches!(
        service.publish_reviewed_project(&scope, &publication).await,
        Err(MemoryError::Unsupported { .. })
    ));
    assert!(matches!(
        service
            .remove_reviewed_project(
                &scope,
                publication.record_id,
                "stop",
                ReviewedProjectRemoval::Withdraw
            )
            .await,
        Err(MemoryError::Unsupported { .. })
    ));
    assert!(matches!(
        service.inspect_reviewed_project(&scope, None).await,
        Err(MemoryError::Unsupported { .. })
    ));
    assert!(matches!(
        service.recall_reviewed_project(&scope, "fact").await,
        Err(MemoryError::Unsupported { .. })
    ));
    let mut invalid_scope = scope.clone();
    invalid_scope.project_id = Uuid::from_u128(999);
    assert!(matches!(
        service.inspect_reviewed_project(&invalid_scope, None).await,
        Err(MemoryError::InvalidRequest { .. })
    ));
    for invalid_text in ["é".repeat(2049), "   ".into(), "control\0value".into()] {
        let mut request = publication.clone();
        request.content = invalid_text;
        assert!(matches!(
            service.publish_reviewed_project(&scope, &request).await,
            Err(MemoryError::InvalidRequest { .. })
        ));
    }
}
