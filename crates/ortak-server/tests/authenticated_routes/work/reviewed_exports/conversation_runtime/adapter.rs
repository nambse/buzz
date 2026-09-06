use super::super::authority::NamedMemory;
use super::*;
use std::collections::HashMap;

pub(super) struct SelectedMemory {
    pub inner: NamedMemory,
    pub project: Uuid,
    pub project_enabled: bool,
    pub contents: Mutex<HashMap<Uuid, String>>,
    pub selected: Mutex<Vec<Vec<Uuid>>>,
}
impl MemoryAdapter for SelectedMemory {
    fn adapter_name(&self) -> &str {
        "honcho"
    }
    async fn probe_capabilities(
        &self,
        b: &ortak_domain::MemoryBinding,
    ) -> Result<MemoryCapabilities, MemoryError> {
        self.inner.probe_capabilities(b).await
    }
    async fn health(
        &self,
        b: &ortak_domain::MemoryBinding,
    ) -> Result<MemoryHealthReport, MemoryError> {
        self.inner.health(b).await
    }
    async fn ensure_resources(
        &self,
        r: &MemoryResourceRequest,
    ) -> Result<MemoryResourceOutcome, MemoryError> {
        self.inner.ensure_resources(r).await
    }
    async fn delete_created_resource(&self, r: &str, k: &str) -> Result<(), MemoryError> {
        self.inner.delete_created_resource(r, k).await
    }
    async fn recall(&self, r: &MemoryRecallRequest) -> Result<MemoryRecall, MemoryError> {
        let MemoryScope::RunScratch { run_id } = r.scope else {
            panic!("scratch adapter must never receive a broader namespace")
        };
        Ok(MemoryRecall {
            records: vec![MemoryRecord {
                record_ref: "synthetic-scratch-before-v4".into(),
                scope: r.scope.clone(),
                content: SCRATCH.into(),
                provenance: MemoryProvenance {
                    employee_id: r.employee_id.clone(),
                    run_id: Some(run_id),
                    source: "run_scratch".into(),
                    recorded_at: Utc::now(),
                },
            }],
            truncated: false,
        })
    }
    async fn remember(&self, r: &MemoryWriteRequest) -> Result<MemoryWriteReceipt, MemoryError> {
        self.inner.remember(r).await
    }
}
impl ReviewedRunAdapter for SelectedMemory {
    fn reviewed_enabled(&self, authority: &DispatchAuthority) -> Result<bool, DispatchRefusal> {
        Ok(self.project_enabled
            && authority
                .work_origin()
                .is_some_and(|work| work.project_id == self.project))
    }
    async fn recall_selected(
        &self,
        _: &ReviewedMemorySelection,
        _: &str,
    ) -> Result<ReviewedMemoryContext, DispatchRefusal> {
        panic!("project-only recall must not replace the selected conversation transport")
    }
    fn conversation_project(&self, _: &DispatchAuthority) -> Result<Option<Uuid>, DispatchRefusal> {
        Ok(Some(self.project))
    }
    async fn recall_selected_conversation(
        &self,
        s: &ReviewedConversationSelection,
        _: &str,
    ) -> Result<ReviewedSelectedRecall, DispatchRefusal> {
        // Only this controlled remote port owns the approval bytes. Production
        // selection supplies IDs/provenance and verifies every returned hash.
        self.selected
            .lock()
            .unwrap()
            .push(s.records.iter().map(|p| p.fact_id()).collect());
        let contents = self.contents.lock().unwrap();
        let records = s
            .records
            .iter()
            .map(|p| {
                let content = contents
                    .get(&p.fact_id())
                    .expect("published remote record")
                    .clone();
                match p {
                    ReviewedSelectionPin::Project { pin } => ReviewedContextRecord::Project {
                        record: ReviewedMemoryRecord {
                            pin: pin.clone(),
                            content,
                        },
                    },
                    ReviewedSelectionPin::Conversation { pin, provenance } => {
                        ReviewedContextRecord::Conversation {
                            record: ReviewedConversationRecord {
                                pin: pin.clone(),
                                content,
                                provenance: provenance.clone(),
                            },
                        }
                    }
                }
            })
            .collect();
        Ok(ReviewedSelectedRecall {
            records,
            truncated: false,
        })
    }
}
