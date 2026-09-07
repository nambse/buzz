//! Deterministic provider fixtures advertise the exact selected binding names.
//! The real saga and repository still issue and validate their sealed evidence.
use ortak_control::{
    adapter::{HealthReport, ResourceOutcome},
    fakes::{FakeMemoryAdapter, FakeRuntimeAdapter},
    memory::*,
    runtime::*,
};
use ortak_domain::{MemoryBinding, RuntimeBinding};

pub(super) struct SelectedRuntime(pub(super) FakeRuntimeAdapter);
impl RuntimeAdapter for SelectedRuntime {
    fn adapter_name(&self) -> &str {
        "hermes"
    }
    async fn probe_capabilities(&self) -> Result<RuntimeCapabilities, RuntimeError> {
        let mut capabilities = self.0.probe_capabilities().await?;
        capabilities.adapter = "hermes".into();
        capabilities
            .capabilities
            .insert(RuntimeCapability::WorkspaceTextRead);
        Ok(capabilities)
    }
    async fn health(&self, binding: &RuntimeBinding) -> Result<HealthReport, RuntimeError> {
        self.0.health(binding).await
    }
    async fn ensure_profile(
        &self,
        request: &RuntimeResourceRequest,
    ) -> Result<ResourceOutcome, RuntimeError> {
        self.0.ensure_profile(request).await
    }
    async fn delete_created_profile(&self, resource: &str, key: &str) -> Result<(), RuntimeError> {
        self.0.delete_created_profile(resource, key).await
    }
    async fn start_run(&self, spec: &RunSpec) -> Result<RunStartReceipt, RuntimeError> {
        self.0.start_run(spec).await
    }
    async fn next_events(
        &self,
        reference: &RuntimeRunRef,
        cursor: Option<&RuntimeCursor>,
        limit: usize,
    ) -> Result<RuntimeEventBatch, RuntimeError> {
        self.0.next_events(reference, cursor, limit).await
    }
    async fn cancel_run(
        &self,
        reference: &RuntimeRunRef,
        reason: &str,
    ) -> Result<CancelOutcome, RuntimeError> {
        self.0.cancel_run(reference, reason).await
    }
}

pub(super) struct SelectedMemory(pub(super) FakeMemoryAdapter);
impl MemoryAdapter for SelectedMemory {
    fn adapter_name(&self) -> &str {
        "honcho"
    }
    async fn probe_capabilities(
        &self,
        binding: &MemoryBinding,
    ) -> Result<MemoryCapabilities, MemoryError> {
        let mut capabilities = self.0.probe_capabilities(binding).await?;
        capabilities.adapter = "honcho".into();
        Ok(capabilities)
    }
    async fn health(&self, binding: &MemoryBinding) -> Result<MemoryHealthReport, MemoryError> {
        self.0.health(binding).await
    }
    async fn ensure_resources(
        &self,
        request: &MemoryResourceRequest,
    ) -> Result<MemoryResourceOutcome, MemoryError> {
        self.0.ensure_resources(request).await
    }
    async fn delete_created_resource(&self, resource: &str, key: &str) -> Result<(), MemoryError> {
        self.0.delete_created_resource(resource, key).await
    }
    async fn recall(&self, request: &MemoryRecallRequest) -> Result<MemoryRecall, MemoryError> {
        self.0.recall(request).await
    }
    async fn remember(
        &self,
        request: &MemoryWriteRequest,
    ) -> Result<MemoryWriteReceipt, MemoryError> {
        self.0.remember(request).await
    }
}
