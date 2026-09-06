//! Exact selected approved-record recall; no approval text is substituted locally.
use super::*;
use ortak_memory::{ReviewedProjectScope, ReviewedProjectStatus};
use ortak_runtime::memory_context::{
    ReviewedContextRecord, ReviewedConversationRecord, ReviewedMemoryContext, ReviewedMemoryRecord,
};
use ortak_runtime::reviewed_memory::{
    ReviewedConversationSelection, ReviewedMemorySelection, ReviewedRunAdapter,
    ReviewedSelectedRecall, ReviewedSelectionPin,
};
use ortak_runtime::{DispatchAuthority, DispatchRefusal};

impl ReviewedRunAdapter for WorkerMemory {
    fn employee_destination(
        &self,
        authority: &DispatchAuthority,
    ) -> Result<Option<ortak_runtime::reviewed_memory::EmployeeReviewedDestination>, DispatchRefusal>
    {
        employee::destination(self, authority)
    }

    async fn recall_selected_employee(
        &self,
        selected: &ortak_runtime::reviewed_memory::ReviewedEmployeeSelection,
    ) -> Result<ortak_runtime::reviewed_memory::ReviewedEmployeeRecall, DispatchRefusal> {
        employee::recall(self, selected).await
    }
    fn conversation_project(
        &self,
        authority: &DispatchAuthority,
    ) -> Result<Option<Uuid>, DispatchRefusal> {
        let Some(channel) = authority.input().channel_id else {
            return Ok(None);
        };
        let values = self
            .validations
            .lock()
            .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
        let selected: BTreeSet<_> = values
            .iter()
            .filter(|v| {
                v.resource.employee_id == *authority.employee_id()
                    && Some(&v.resource.binding) == authority.memory_binding()
                    && v.creation_receipt
                        .as_ref()
                        .is_some_and(|r| r.company_id == authority.company_id())
            })
            .flat_map(|v| v.reviewed_conversations.iter())
            .filter(|s| {
                s.channel_id == channel
                    && authority
                        .work_origin()
                        .is_none_or(|w| w.project_id == s.project_id)
            })
            .map(|s| s.project_id)
            .collect();
        if selected.len() > 1 {
            return Err(DispatchRefusal::MemoryContextRejected);
        }
        Ok(selected.into_iter().next())
    }

    async fn recall_selected_conversation(
        &self,
        selection: &ReviewedConversationSelection,
        query: &str,
    ) -> Result<ReviewedSelectedRecall, DispatchRefusal> {
        let parsed = selection
            .origin
            .parsed_provenance()
            .map_err(|_| DispatchRefusal::MemoryContextRejected)?;
        let channel = parsed.audience().channel_id();
        let configured = {
            let values = self
                .validations
                .lock()
                .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
            values.iter().any(|v| {
                v.resource.employee_id == selection.employee_id
                    && v.resource.binding == selection.binding
                    && v.creation_receipt
                        .as_ref()
                        .is_some_and(|r| r.company_id == selection.company_id)
                    && v.reviewed_conversations
                        .iter()
                        .any(|s| s.project_id == selection.project_id && s.channel_id == channel)
                    && (selection
                        .records
                        .iter()
                        .all(|r| !matches!(r, ReviewedSelectionPin::Project { .. }))
                        || v.reviewed_runtime_projects.contains(&selection.project_id))
            })
        };
        let ids: BTreeSet<_> = selection
            .records
            .iter()
            .map(ReviewedSelectionPin::fact_id)
            .collect();
        if !configured
            || ids.len() != selection.records.len()
            || ids.len() > 8
            || parsed.audience().company_id() != selection.company_id
            || parsed.audience().project_id() != selection.project_id
            || parsed.audience().employee_id() != &selection.employee_id
        {
            return Err(DispatchRefusal::MemoryContextRejected);
        }
        if ids.is_empty() {
            return Ok(ReviewedSelectedRecall {
                records: Vec::new(),
                truncated: selection.truncated,
            });
        }
        let result = self
            .selected(MemoryCapability::Recall)
            .map_err(|_| DispatchRefusal::MemoryUnavailable)?
            .recall_selected_reviewed_project(
                &ReviewedProjectScope {
                    employee_id: selection.employee_id.clone(),
                    binding: selection.binding.clone(),
                    project_id: selection.project_id,
                },
                query,
                &ids,
            )
            .await
            .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
        let mut verified = std::collections::BTreeMap::new();
        for remote in result.records {
            let selected = selection
                .records
                .iter()
                .find(|p| p.fact_id() == remote.record_id)
                .ok_or(DispatchRefusal::MemoryContextRejected)?;
            let pin = selected.common_pin();
            let provenance = remote
                .provenance
                .ok_or(DispatchRefusal::MemoryContextRejected)?;
            if remote.status != ReviewedProjectStatus::Active
                || remote.binding_hash != pin.binding_hash
                || remote.content_hash.as_ref() != Some(&pin.content_hash)
                || remote.expires_at != Some(pin.expires_at)
                || provenance.approval_id != pin.approval_id
                || provenance.approved_by != pin.approved_by
                || provenance.source_hash != pin.source_hash
            {
                return Err(DispatchRefusal::MemoryContextRejected);
            }
            let content = remote
                .content
                .ok_or(DispatchRefusal::MemoryContextRejected)?;
            let record = match selected {
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
            };
            if verified.insert(remote.record_id, record).is_some() {
                return Err(DispatchRefusal::MemoryContextRejected);
            }
        }
        // Preserve the central scope order; missing remote bytes stay absent.
        let records = selection
            .records
            .iter()
            .filter_map(|pin| verified.remove(&pin.fact_id()))
            .collect();
        Ok(ReviewedSelectedRecall {
            records,
            truncated: selection.truncated || result.truncated,
        })
    }

    fn reviewed_enabled(&self, authority: &DispatchAuthority) -> Result<bool, DispatchRefusal> {
        let Some(work) = authority.work_origin() else {
            return Ok(false);
        };
        let values = self
            .validations
            .lock()
            .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
        Ok(values.iter().any(|v| {
            v.resource.employee_id == *authority.employee_id()
                && Some(&v.resource.binding) == authority.memory_binding()
                && v.reviewed_runtime_projects.contains(&work.project_id)
        }))
    }

    async fn recall_selected(
        &self,
        selection: &ReviewedMemorySelection,
        query: &str,
    ) -> Result<ReviewedMemoryContext, DispatchRefusal> {
        let configured = {
            let values = self
                .validations
                .lock()
                .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
            values.iter().any(|v| {
                v.resource.employee_id == selection.employee_id
                    && v.resource.binding == selection.binding
                    && v.reviewed_runtime_projects.contains(&selection.project_id)
                    && v.creation_receipt
                        .as_ref()
                        .is_some_and(|r| r.company_id == selection.company_id)
            })
        };
        if !configured {
            return Err(DispatchRefusal::MemoryContextRejected);
        }
        let ids: BTreeSet<_> = selection.pins.iter().map(|p| p.fact_id).collect();
        if ids.len() != selection.pins.len() {
            return Err(DispatchRefusal::MemoryContextRejected);
        }
        let result = self
            .selected(MemoryCapability::Recall)
            .map_err(|_| DispatchRefusal::MemoryUnavailable)?
            .recall_selected_reviewed_project(
                &ReviewedProjectScope {
                    employee_id: selection.employee_id.clone(),
                    binding: selection.binding.clone(),
                    project_id: selection.project_id,
                },
                query,
                &ids,
            )
            .await
            .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
        let mut records = Vec::new();
        for remote in result.records {
            let pin = selection
                .pins
                .iter()
                .find(|p| p.fact_id == remote.record_id)
                .ok_or(DispatchRefusal::MemoryContextRejected)?;
            let provenance = remote
                .provenance
                .ok_or(DispatchRefusal::MemoryContextRejected)?;
            if remote.status != ReviewedProjectStatus::Active
                || remote.binding_hash != pin.binding_hash
                || remote.content_hash.as_ref() != Some(&pin.content_hash)
                || remote.expires_at != Some(pin.expires_at)
                || provenance.approval_id != pin.approval_id
                || provenance.approved_by != pin.approved_by
                || provenance.source_hash != pin.source_hash
            {
                return Err(DispatchRefusal::MemoryContextRejected);
            }
            records.push(ReviewedMemoryRecord {
                pin: pin.clone(),
                content: remote
                    .content
                    .ok_or(DispatchRefusal::MemoryContextRejected)?,
            });
        }
        Ok(ReviewedMemoryContext {
            records,
            truncated: result.truncated,
        })
    }
}
