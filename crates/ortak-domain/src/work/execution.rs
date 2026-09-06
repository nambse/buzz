//! Atomic Work execution events; runtime output never resolves human review facts.
use super::*;

impl WorkItem {
    /// Queue one assigned employee and retain its run reference in one version.
    /// Current identity, configuration, authority and run uniqueness are storage gates.
    pub fn request_execution(
        &mut self,
        run_id: Uuid,
        employee_id: &EmployeeId,
        attachment_id: Uuid,
    ) -> Result<WorkEvent, DomainError> {
        if !matches!(self.state, WorkState::Ready | WorkState::InProgress)
            || !self.definition_editable()
            || run_id.is_nil()
            || attachment_id.is_nil()
            || self.attachments.len() >= MAX_WORK_ATTACHMENTS
            || self
                .attachments
                .iter()
                .any(|a| a.id == attachment_id || a.reference == (AttachmentRef::Run { run_id }))
            || !self.assignments.iter().any(|a| {
                &a.employee_id == employee_id
                    && a.status == AssignmentStatus::Active
                    && matches!(a.role, AssignmentRole::Owner | AssignmentRole::Contributor)
            })
        {
            return Err(DomainError::InvalidField {
                field: "work.execution",
            });
        }
        let blocking = self.blocking_dependencies();
        if !blocking.is_empty() {
            return Err(DomainError::DependenciesUnresolved {
                count: blocking.len(),
            });
        }
        let version = self
            .version
            .checked_add(1)
            .ok_or(DomainError::InvalidField {
                field: "work.version",
            })?;
        let from = self.state;
        self.state = WorkState::InProgress;
        self.version = version;
        self.attachments.push(WorkAttachment {
            id: attachment_id,
            reference: AttachmentRef::Run { run_id },
            label: None,
        });
        Ok(WorkEvent::ExecutionRequested {
            run_id,
            employee_id: employee_id.clone(),
            from,
        })
    }

    /// Attach a verified artifact and enter review atomically. Criteria and approval
    /// decisions remain unchanged; current execution/version checks belong to storage.
    pub fn execution_result_ready(
        &mut self,
        run_id: Uuid,
        artifact_id: Uuid,
        attachment_id: Uuid,
    ) -> Result<WorkEvent, DomainError> {
        if self.state != WorkState::InProgress
            || !self.definition_editable()
            || artifact_id.is_nil()
            || attachment_id.is_nil()
            || self.attachments.len() >= MAX_WORK_ATTACHMENTS
            || !self
                .attachments
                .iter()
                .any(|a| a.reference == (AttachmentRef::Run { run_id }))
            || self.attachments.iter().any(|a| {
                a.id == attachment_id || a.reference == (AttachmentRef::Artifact { artifact_id })
            })
        {
            return Err(DomainError::InvalidField {
                field: "work.execution_result",
            });
        }
        let version = self
            .version
            .checked_add(1)
            .ok_or(DomainError::InvalidField {
                field: "work.version",
            })?;
        self.attachments.push(WorkAttachment {
            id: attachment_id,
            reference: AttachmentRef::Artifact { artifact_id },
            label: None,
        });
        self.state = WorkState::Review;
        self.version = version;
        Ok(WorkEvent::ExecutionResultReady {
            run_id,
            artifact_id,
        })
    }

    /// Canonical, bounded definition for a pinned execution. No text is truncated.
    pub fn execution_input(&self) -> Result<Vec<u8>, DomainError> {
        let criteria: Vec<_> = self
            .criteria
            .iter()
            .map(|c| serde_json::json!({"id": c.id, "text": c.text}))
            .collect();
        let bytes = serde_json::to_vec(&serde_json::json!({
            "type": "work_item", "work_item_id": self.id, "project_id": self.project_id,
            "title": self.title, "description": self.description,
            "acceptance_criteria": criteria,
            "instructions": "Produce a complete text deliverable for human review. Do not claim acceptance or approval."
        })).map_err(|_| DomainError::InvalidField { field: "work.execution_input" })?;
        if bytes.len() > 32 * 1024 {
            return Err(DomainError::InvalidField {
                field: "work.execution_input",
            });
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests;
