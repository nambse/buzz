use super::*;
use ortak_control::memory::{
    MemoryAdapter, MemoryFact, MemoryProvenance, MemoryScope, MemoryWriteRequest,
};

async fn remember(fixture: &Fixture, run_id: Uuid, key: &str, scope: MemoryScope) {
    fixture
        .memory
        .remember(&MemoryWriteRequest {
            employee_id: fixture.employee.id.clone(),
            binding: fixture.employee.memory.clone().expect("memory binding"),
            scope,
            facts: vec![MemoryFact {
                content: "Cem, stable memory query".to_owned(),
                provenance: MemoryProvenance {
                    employee_id: fixture.employee.id.clone(),
                    run_id: Some(run_id),
                    source: "run_scratch".to_owned(),
                    recorded_at: Utc::now(),
                },
            }],
            idempotency_key: key.to_owned(),
        })
        .await
        .expect("remember test data");
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn retry_reuses_frozen_spec_when_memory_changes_after_first_start() {
    let fixture = Fixture::new().await;
    fixture.route("Cem, stable memory query").await;
    let lease = fixture.lease(Duration::from_secs(60)).await;
    fixture.adapter.set_unavailable(true);
    let supervisor = fixture.supervisor(fixture.config());
    let run_id = match supervisor
        .dispatch(&fixture.scope, &lease)
        .await
        .expect("first attempt")
    {
        DispatchOutcome::RuntimeFailed { run_id, .. } => run_id,
        other => panic!("runtime refusal expected: {other:?}"),
    };
    let before: Vec<u8> = sqlx::query_scalar(
        "SELECT spec_bytes FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(fixture.scope.company_id())
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("frozen bytes");
    remember(
        &fixture,
        run_id,
        "changed-memory",
        MemoryScope::RunScratch { run_id },
    )
    .await;
    fixture.adapter.set_unavailable(false);
    let retry = fixture.lease(Duration::from_secs(60)).await;
    assert!(matches!(
        supervisor
            .dispatch(&fixture.scope, &retry)
            .await
            .expect("retry"),
        DispatchOutcome::Started { .. }
    ));
    let specs = fixture.adapter.start_specs();
    assert_eq!(specs.len(), 2);
    assert!(specs[0].context.memory_context.is_empty());
    assert_eq!(
        serde_json::to_vec(&specs[0]).expect("first bytes"),
        serde_json::to_vec(&specs[1]).expect("retry bytes")
    );
    let after: Vec<u8> = sqlx::query_scalar(
        "SELECT spec_bytes FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(fixture.scope.company_id())
    .bind(run_id)
    .fetch_one(&fixture.pool)
    .await
    .expect("same frozen bytes");
    assert_eq!(before, after);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn required_memory_failure_does_not_strand_pump_or_cancel() {
    let fixture = Fixture::new().await;
    fixture.route("Cem, stable memory query").await;
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let no_memory = RunSupervisor::new(fixture.control.clone(), &fixture.adapter, fixture.config());
    assert!(matches!(
        no_memory
            .dispatch(&fixture.scope, &lease)
            .await
            .expect("refused"),
        DispatchOutcome::Refused {
            refusal: DispatchRefusal::MemoryAdapterUnavailable,
            ..
        }
    ));
    assert!(fixture.adapter.start_specs().is_empty());
    let retry = fixture.lease(Duration::from_secs(60)).await;
    let run_id = match fixture
        .supervisor(fixture.config())
        .dispatch(&fixture.scope, &retry)
        .await
        .expect("configured retry")
    {
        DispatchOutcome::Started { run_id, .. } => run_id,
        other => panic!("configured start expected: {other:?}"),
    };
    fixture.memory.set_unavailable(true);
    assert!(matches!(
        no_memory
            .pump(&fixture.scope, run_id)
            .await
            .expect("pump without memory"),
        PumpOutcome::Appended { .. }
    ));
    assert!(matches!(
        no_memory
            .cancel(&fixture.scope, run_id, "operator stop")
            .await
            .expect("cancel without memory"),
        CancellationOutcome::Cancelled { .. }
    ));
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn pre_run_recall_excludes_employee_experience_even_when_query_matches() {
    let fixture = Fixture::new().await;
    fixture.route("Cem, stable memory query").await;
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let authority = authorized(
        fixture
            .control
            .authorize_dispatch(&fixture.scope, &lease)
            .await
            .expect("authority"),
    );
    let run_id = match fixture
        .control
        .prepare_run(&fixture.scope, &authority)
        .await
        .expect("prepare")
    {
        PrepareOutcome::Prepared(run) => run.run_id,
        other => panic!("prepared: {other:?}"),
    };
    remember(
        &fixture,
        run_id,
        "cross-channel-experience",
        MemoryScope::EmployeeExperience,
    )
    .await;
    remember(
        &fixture,
        run_id,
        "same-run-scratch",
        MemoryScope::RunScratch { run_id },
    )
    .await;
    assert!(matches!(
        fixture
            .supervisor(fixture.config())
            .dispatch(&fixture.scope, &lease)
            .await
            .expect("start"),
        DispatchOutcome::Started { .. }
    ));
    let specs = fixture.adapter.start_specs();
    assert_eq!(
        specs[0].context.memory_context.len(),
        1,
        "employee-global memory is excluded"
    );
    assert!(specs[0].context.memory_context[0].contains("run_scratch"));
}

struct RevokingMemory<'a>(&'a Fixture);

impl MemoryAdapter for RevokingMemory<'_> {
    fn adapter_name(&self) -> &str {
        self.0.memory.adapter_name()
    }
    async fn probe_capabilities(
        &self,
        binding: &ortak_domain::MemoryBinding,
    ) -> Result<ortak_control::memory::MemoryCapabilities, ortak_control::memory::MemoryError> {
        self.0.memory.probe_capabilities(binding).await
    }
    async fn health(
        &self,
        binding: &ortak_domain::MemoryBinding,
    ) -> Result<ortak_control::memory::MemoryHealthReport, ortak_control::memory::MemoryError> {
        self.0.memory.health(binding).await
    }
    async fn ensure_resources(
        &self,
        _: &ortak_control::memory::MemoryResourceRequest,
    ) -> Result<ortak_control::memory::MemoryResourceOutcome, ortak_control::memory::MemoryError>
    {
        panic!("read path must not provision")
    }
    async fn delete_created_resource(
        &self,
        _: &str,
        _: &str,
    ) -> Result<(), ortak_control::memory::MemoryError> {
        panic!("read path must not delete")
    }
    async fn remember(
        &self,
        _: &MemoryWriteRequest,
    ) -> Result<ortak_control::memory::MemoryWriteReceipt, ortak_control::memory::MemoryError> {
        panic!("read path must not write")
    }
    async fn recall(
        &self,
        request: &ortak_control::memory::MemoryRecallRequest,
    ) -> Result<ortak_control::memory::MemoryRecall, ortak_control::memory::MemoryError> {
        let result = self.0.memory.recall(request).await?;
        sqlx::query("UPDATE employee_memory_bindings SET validated_at=NULL WHERE company_id=$1 AND revision_id=$2")
            .bind(self.0.scope.company_id()).bind(self.0.revision_id)
            .execute(&self.0.pool).await.expect("revoke memory during external recall");
        Ok(result)
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn memory_revoked_during_external_recall_is_rechecked_before_snapshot_and_start() {
    let fixture = Fixture::new().await;
    fixture.route("Cem, stable memory query").await;
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let memory = RevokingMemory(&fixture);
    let supervisor =
        RunSupervisor::new(fixture.control.clone(), &fixture.adapter, fixture.config())
            .with_memory(&memory);
    assert!(matches!(
        supervisor
            .dispatch(&fixture.scope, &lease)
            .await
            .expect("fresh refusal"),
        DispatchOutcome::Refused {
            refusal: DispatchRefusal::MemoryBindingUnvalidated,
            ..
        }
    ));
    assert!(fixture.adapter.start_specs().is_empty());
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM run_context_snapshots WHERE company_id=$1")
            .bind(fixture.scope.company_id())
            .fetch_one(&fixture.pool)
            .await
            .expect("no frozen snapshot");
    assert_eq!(count, 0);
}
