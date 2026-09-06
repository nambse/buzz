use super::*;
#[path = "../../../../../ortak-control/tests/lifecycle_support.rs"]
mod lifecycle_support;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal69"]
async fn reviewed_export_failed_cleanup_retry_keeps_the_original_target_after_source_loss() {
    let x = ExportFixture::new(Duration::from_secs(86400), true).await;
    x.publish().await;
    x.stop().await;
    let lease = exports::claim(&x.f.control, &x.scope)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.action, ReviewedExportAction::Withdraw);
    let original = exports::prepare(&x.f.control, &x.scope, &lease)
        .await
        .unwrap()
        .unwrap();
    assert!(original.content.is_none());
    assert!(
        exports::fail(&x.f.control, &x.scope, &lease, "service_refused", true)
            .await
            .unwrap()
    );
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(x.f.community)
        .bind(hex::decode(&x.source).unwrap())
        .execute(&x.f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE employees SET status='paused' WHERE company_id=$1 AND id='cem'")
        .bind(x.f.company)
        .execute(&x.f.pool)
        .await
        .unwrap();
    WorkService::new(x.f.control.clone())
        .archive_project(
            &x.scope,
            ortak_work::ArchiveProject {
                project_id: x.project,
                expected_version: 1,
                reason: Some("Retained cleanup recovery".into()),
                actor: WorkActor::Human(x.f.operator.public_key().to_hex()),
            },
        )
        .await
        .unwrap();
    exports::advertise_targets(&x.f.control, &x.scope, &[])
        .await
        .unwrap();
    let path = format!("{}/exports/withdraw/retry", x.path());
    let retry = json!({"operation_id":Uuid::new_v4(),"retry_version":0});
    for _ in 0..2 {
        let response = post(&x.app, &x.f.operator, &path, &retry).await;
        assert_eq!(response.0, StatusCode::OK, "{response:?}");
    }
    let next = exports::claim(&x.f.control, &x.scope)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.action, ReviewedExportAction::Withdraw);
    let retained = exports::prepare(&x.f.control, &x.scope, &next)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retained.binding, original.binding);
    assert_eq!(retained.creation_receipt, original.creation_receipt);
    assert_eq!(retained.idempotency_key, original.idempotency_key);
    assert_eq!(retained.request_hash, original.request_hash);
    assert!(
        !exports::acknowledge(&x.f.control, &x.scope, &lease, &acknowledgement(&original))
            .await
            .unwrap()
    );
    assert!(
        exports::acknowledge(&x.f.control, &x.scope, &next, &acknowledgement(&retained))
            .await
            .unwrap()
    );
    let page = x.page().await;
    assert!(page["facts"][0]["content"].is_null());
    assert_eq!(
        page["facts"][0]["export"]["erased_from_reviewed_store"],
        true
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal69"]
async fn reviewed_export_stale_target_and_source_cannot_reach_the_worker_adapter() {
    let x = ExportFixture::new(Duration::from_secs(86400), true).await;
    x.publish().await;
    // Replacing the selected worker allowlist retires its advertisement but not cleanup identity.
    assert_eq!(
        exports::advertise_targets(&x.f.control, &x.scope, &[])
            .await
            .unwrap(),
        0
    );
    let adapter = ObservedAdapter::default();
    assert!(!schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    assert!(adapter.calls.lock().unwrap().is_empty());
    assert_eq!(
        x.page().await["facts"][0]["export"]["publication"]["error_code"],
        "authority_refused"
    );
    x.stop().await;
    assert!(schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    assert_eq!(
        adapter.calls.lock().unwrap()[0].0,
        ReviewedExportAction::Withdraw
    );

    let y = ExportFixture::new(Duration::from_secs(86400), true).await;
    y.publish().await;
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(y.f.community)
        .bind(hex::decode(&y.source).unwrap())
        .execute(&y.f.pool)
        .await
        .unwrap();
    let held = ObservedAdapter::default();
    assert!(!schedule_one(&y.f.control, &y.scope, &held).await.unwrap());
    assert!(held.calls.lock().unwrap().is_empty());
    let page = y.page().await;
    assert!(page["facts"][0]["content"].is_null());
    y.stop().await;
    assert!(schedule_one(&y.f.control, &y.scope, &held).await.unwrap());
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal69"]
async fn reviewed_export_cleanup_survives_company_and_source_admission_loss() {
    let x = ExportFixture::new(Duration::from_secs(86400), true).await;
    x.publish().await;
    let adapter = ObservedAdapter::default();
    assert!(schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(x.f.community)
        .bind(hex::decode(&x.source).unwrap())
        .execute(&x.f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE employees SET status='paused' WHERE company_id=$1 AND id='cem'")
        .bind(x.f.company)
        .execute(&x.f.pool)
        .await
        .unwrap();
    x.stop().await;
    // Core scheduler is the same unconditional cancellation-first worker pass.
    // Company admission loss must not prevent exact retained-binding cleanup.
    sqlx::query("UPDATE companies SET status='suspended' WHERE id=$1")
        .bind(x.f.company)
        .execute(&x.f.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE project_access_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND project_id=$2")
        .bind(x.f.company).bind(x.project).execute(&x.f.pool).await.unwrap();
    assert!(schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    let erased:bool=sqlx::query_scalar("SELECT erased_from_reviewed_store FROM reviewed_memory_export_receipts WHERE company_id=$1 AND fact_id=$2 AND action='withdraw'")
        .bind(x.f.company).bind(x.fact).fetch_one(&x.f.pool).await.unwrap();
    assert!(erased);
    assert_eq!(adapter.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal69"]
async fn reviewed_export_old_revision_cannot_publish_after_sealed_disable_reenable() {
    let x = ExportFixture::new(Duration::from_secs(86400), true).await;
    x.publish().await;
    let memory = NamedMemory(
        ortak_control::fakes::FakeMemoryAdapter::new()
            .with_existing_binding(x.employee.memory.as_ref().unwrap()),
    );
    lifecycle_support::cycle_with_memory(&x.f.pool, &x.f.control, &x.scope, &x.employee, &memory)
        .await;
    x.advertise().await;
    let adapter = ObservedAdapter::default();
    assert!(!schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    assert!(
        adapter.calls.lock().unwrap().is_empty(),
        "renewed advertisement cannot revive the original revision's job"
    );
    x.stop().await;
    assert!(schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    assert_eq!(
        adapter.calls.lock().unwrap()[0].0,
        ReviewedExportAction::Withdraw
    );
}

// Only the fixture's advertised adapter name changes; all health/resource
// behavior remains the standard fake. The saga still seals current evidence.
pub(super) struct NamedMemory(pub(super) ortak_control::fakes::FakeMemoryAdapter);
impl ortak_control::memory::MemoryAdapter for NamedMemory {
    fn adapter_name(&self) -> &str {
        "honcho"
    }
    async fn probe_capabilities(
        &self,
        binding: &ortak_domain::MemoryBinding,
    ) -> Result<ortak_control::memory::MemoryCapabilities, MemoryError> {
        let mut value = self.0.probe_capabilities(binding).await?;
        value.adapter = "honcho".into();
        Ok(value)
    }
    async fn health(
        &self,
        binding: &ortak_domain::MemoryBinding,
    ) -> Result<ortak_control::memory::MemoryHealthReport, MemoryError> {
        self.0.health(binding).await
    }
    async fn ensure_resources(
        &self,
        request: &ortak_control::memory::MemoryResourceRequest,
    ) -> Result<ortak_control::memory::MemoryResourceOutcome, MemoryError> {
        self.0.ensure_resources(request).await
    }
    async fn delete_created_resource(&self, resource: &str, key: &str) -> Result<(), MemoryError> {
        self.0.delete_created_resource(resource, key).await
    }
    async fn recall(
        &self,
        request: &ortak_control::memory::MemoryRecallRequest,
    ) -> Result<ortak_control::memory::MemoryRecall, MemoryError> {
        self.0.recall(request).await
    }
    async fn remember(
        &self,
        request: &ortak_control::memory::MemoryWriteRequest,
    ) -> Result<ortak_control::memory::MemoryWriteReceipt, MemoryError> {
        self.0.remember(request).await
    }
}
