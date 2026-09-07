//! Real employee HTTP read plus the existing controlled legacy transport.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct MixedMemory<'a> {
    pub c: &'a ConversationFixture,
    pub remote: &'a Remote,
    pub destination: Option<EmployeeReviewedDestination>,
    pub stop_after_read: AtomicBool,
    pub app: &'a Router,
    pub fact: Uuid,
}
impl MemoryAdapter for MixedMemory<'_> {
    fn adapter_name(&self) -> &str {
        "honcho"
    }
    async fn probe_capabilities(
        &self,
        binding: &ortak_domain::MemoryBinding,
    ) -> Result<MemoryCapabilities, MemoryError> {
        self.c.memory.probe_capabilities(binding).await
    }
    async fn health(
        &self,
        binding: &ortak_domain::MemoryBinding,
    ) -> Result<MemoryHealthReport, MemoryError> {
        self.c.memory.health(binding).await
    }
    async fn ensure_resources(
        &self,
        request: &MemoryResourceRequest,
    ) -> Result<MemoryResourceOutcome, MemoryError> {
        self.c.memory.ensure_resources(request).await
    }
    async fn delete_created_resource(&self, reference: &str, key: &str) -> Result<(), MemoryError> {
        self.c.memory.delete_created_resource(reference, key).await
    }
    async fn recall(&self, request: &MemoryRecallRequest) -> Result<MemoryRecall, MemoryError> {
        self.c.memory.recall(request).await
    }
    async fn remember(
        &self,
        request: &MemoryWriteRequest,
    ) -> Result<MemoryWriteReceipt, MemoryError> {
        self.c.memory.remember(request).await
    }
}
impl ReviewedRunAdapter for MixedMemory<'_> {
    fn employee_destination(
        &self,
        _: &DispatchAuthority,
    ) -> Result<Option<EmployeeReviewedDestination>, DispatchRefusal> {
        Ok(self.destination)
    }
    fn reviewed_enabled(&self, authority: &DispatchAuthority) -> Result<bool, DispatchRefusal> {
        self.c.memory.reviewed_enabled(authority)
    }
    fn conversation_project(
        &self,
        authority: &DispatchAuthority,
    ) -> Result<Option<Uuid>, DispatchRefusal> {
        self.c.memory.conversation_project(authority)
    }
    async fn recall_selected(
        &self,
        selected: &ReviewedMemorySelection,
        _: &str,
    ) -> Result<ReviewedMemoryContext, DispatchRefusal> {
        // The already published legacy mock owns these bytes; this is also the
        // manual Work fallback. No employee registry content is available here.
        let contents = self.c.memory.contents.lock().unwrap();
        Ok(ReviewedMemoryContext {
            records: selected
                .pins
                .iter()
                .map(|pin| ReviewedMemoryRecord {
                    pin: pin.clone(),
                    content: contents.get(&pin.fact_id).unwrap().clone(),
                })
                .collect(),
            truncated: false,
        })
    }
    async fn recall_selected_conversation(
        &self,
        selected: &ReviewedConversationSelection,
        query: &str,
    ) -> Result<ReviewedSelectedRecall, DispatchRefusal> {
        self.c
            .memory
            .recall_selected_conversation(selected, query)
            .await
    }
    async fn recall_selected_employee(
        &self,
        selected: &ReviewedEmployeeSelection,
    ) -> Result<ReviewedEmployeeRecall, DispatchRefusal> {
        assert_eq!(Some(selected.destination), self.destination);
        assert_eq!(selected.company_id, self.c.x.f.company);
        let requested = selected
            .records
            .iter()
            .map(|s| ortak_memory::ReviewedEmployeeCommitment {
                target_id: s.pin.target_id,
                fact_id: s.pin.fact_id,
                destination_channel_id: selected.destination.destination_channel_id,
                content_hash: s.pin.content_hash.clone(),
                source_hash: s.pin.source_hash.clone(),
                sharing_hash: s.pin.sharing_hash.clone(),
            })
            .collect::<Vec<_>>();
        let human = selected.origin.requester_public_key().unwrap();
        let result = self
            .remote
            .service
            .recall_selected_reviewed_employee(
                &self.remote.namespace,
                selected.destination.destination_channel_id,
                Some(&human),
                &requested,
            )
            .await
            .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
        let records = result
            .records
            .into_iter()
            .map(|record| {
                let expected = selected
                    .records
                    .iter()
                    .find(|p| p.pin.fact_id == record.record_id)
                    .unwrap();
                assert_eq!(
                    record.provenance.as_deref(),
                    Some(expected.provenance.as_str())
                );
                assert_eq!(record.content_hash, expected.pin.content_hash);
                ReviewedEmployeeRecord {
                    pin: expected.pin.clone(),
                    content: record.content.unwrap(),
                    provenance: record.provenance.unwrap(),
                }
            })
            .collect();
        if self.stop_after_read.swap(false, Ordering::SeqCst) {
            stop(self.c, self.app, self.fact).await;
        }
        Ok(ReviewedEmployeeRecall {
            records,
            truncated: result.truncated,
        })
    }
}
