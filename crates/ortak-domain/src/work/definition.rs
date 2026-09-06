//! Atomic amendments to work that has not acquired review evidence.
use super::*;
use sha2::{Digest, Sha256};

/// One existing criterion, retaining its durable identity and order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionEdit {
    /// Existing criterion identifier.
    pub id: Uuid,
    /// Replacement bounded, secret-free text; null preserves canonical text.
    pub text: Option<String>,
}

/// Complete editable definition with explicit append-only criterion additions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditWorkDefinition {
    /// Replacement title; null preserves the canonical title.
    pub title: Option<String>,
    /// Replacement description; null preserves it and an empty string clears it.
    pub description: Option<String>,
    /// Every existing criterion in original order; none may be omitted.
    pub criteria: Vec<CriterionEdit>,
    /// New pending criteria appended in order; the control layer allocates IDs.
    #[serde(default)]
    pub additional_criteria: Vec<String>,
}
impl EditWorkDefinition {
    /// Validate bounded text and input identities before opening a transaction.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.criteria.len() > MAX_WORK_CRITERIA
            || self.additional_criteria.len() > MAX_WORK_CRITERIA - self.criteria.len()
        {
            return Err(DomainError::InvalidField {
                field: "work.criteria",
            });
        }
        NewWorkItem {
            project_id: Uuid::nil(),
            title: self.title.clone().unwrap_or_else(|| "unchanged".into()),
            description: self.description.clone().unwrap_or_default(),
            priority: WorkPriority::Normal,
            criteria: self
                .criteria
                .iter()
                .map(|c| c.text.clone().unwrap_or_else(|| "unchanged".into()))
                .chain(self.additional_criteria.iter().cloned())
                .collect(),
            approvals: Vec::new(),
            source_message_id: None,
        }
        .validate()?;
        let mut seen = BTreeSet::new();
        if self
            .criteria
            .iter()
            .any(|c| c.id.is_nil() || !seen.insert(c.id))
        {
            return Err(DomainError::InvalidField {
                field: "work.criteria",
            });
        }
        Ok(())
    }
}
impl NewWorkItem {
    /// Stable hash of creation fields, ignoring review state and allocated child IDs.
    /// Approval gate order is canonicalized, matching promotion replay semantics.
    pub fn definition_fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        let mut field = |value: &[u8]| {
            hash.update((value.len() as u64).to_be_bytes());
            hash.update(value);
        };
        field(b"ortak-work-definition-v1");
        field(self.project_id.as_bytes());
        field(self.title.as_bytes());
        field(self.description.as_bytes());
        field(self.priority.as_str().as_bytes());
        field(self.source_message_id.as_deref().unwrap_or("").as_bytes());
        field(&(self.criteria.len() as u64).to_be_bytes());
        for text in &self.criteria {
            field(text.as_bytes());
        }
        let gates: BTreeMap<_, _> = self
            .approvals
            .iter()
            .map(|a| (a.gate.as_str(), a.required))
            .collect();
        field(&(gates.len() as u64).to_be_bytes());
        for (gate, required) in gates {
            field(gate.as_bytes());
            field(&[u8::from(required)]);
        }
        hex::encode(hash.finalize())
    }
}
impl WorkItem {
    /// Whether definition edits can preserve every existing review fact unchanged.
    pub fn definition_editable(&self) -> bool {
        matches!(
            self.state,
            WorkState::Proposed | WorkState::Ready | WorkState::InProgress | WorkState::Blocked
        ) && self
            .criteria
            .iter()
            .all(|c| c.status == CriterionStatus::Pending)
            && self
                .approvals
                .iter()
                .all(|a| a.status == ApprovalStatus::Pending)
    }
    fn definition_fingerprint(&self) -> String {
        NewWorkItem {
            project_id: self.project_id,
            title: self.title.clone(),
            description: self.description.clone(),
            priority: self.priority,
            criteria: self.criteria.iter().map(|c| c.text.clone()).collect(),
            approvals: self
                .approvals
                .iter()
                .map(|a| ApprovalGateSpec {
                    gate: a.gate.clone(),
                    required: a.required,
                })
                .collect(),
            source_message_id: self.source_message_id.clone(),
        }
        .definition_fingerprint()
    }
    /// Amend title, description and criteria with one version/history event.
    /// Retains existing IDs/order and review evidence; refusals leave `self` unchanged.
    pub fn edit_definition(
        &mut self,
        input: &EditWorkDefinition,
        additional_ids: &[Uuid],
    ) -> Result<WorkEvent, DomainError> {
        self.ensure_mutable()?;
        input.validate()?;
        if !self.definition_editable() {
            return Err(DomainError::InvalidField {
                field: "work.definition_frozen",
            });
        }
        if self.criteria.len() != input.criteria.len()
            || self
                .criteria
                .iter()
                .zip(&input.criteria)
                .any(|(old, new)| old.id != new.id)
            || additional_ids.len() != input.additional_criteria.len()
        {
            return Err(DomainError::InvalidField {
                field: "work.criteria",
            });
        }
        let mut ids: BTreeSet<_> = self.criteria.iter().map(|c| c.id).collect();
        if additional_ids
            .iter()
            .any(|id| id.is_nil() || !ids.insert(*id))
        {
            return Err(DomainError::InvalidField {
                field: "work.criteria",
            });
        }
        let title_changed = input
            .title
            .as_ref()
            .is_some_and(|value| value != &self.title);
        let description_changed = input
            .description
            .as_ref()
            .is_some_and(|value| value != &self.description);
        let edited_criterion_ids: Vec<_> = self
            .criteria
            .iter()
            .zip(&input.criteria)
            .filter(|(old, new)| new.text.as_ref().is_some_and(|value| value != &old.text))
            .map(|(old, _)| old.id)
            .collect();
        if !title_changed
            && !description_changed
            && edited_criterion_ids.is_empty()
            && additional_ids.is_empty()
        {
            return Err(DomainError::InvalidField {
                field: "work.definition_unchanged",
            });
        }
        let next_version = self
            .version
            .checked_add(1)
            .ok_or(DomainError::InvalidField {
                field: "work.version",
            })?;
        let previous_definition_hash = self.definition_fingerprint();
        if let Some(value) = &input.title {
            self.title = value.clone();
        }
        if let Some(value) = &input.description {
            self.description = value.clone();
        }
        for (criterion, edit) in self.criteria.iter_mut().zip(&input.criteria) {
            if let Some(value) = &edit.text {
                criterion.text = value.clone();
            }
        }
        for (id, text) in additional_ids.iter().zip(&input.additional_criteria) {
            self.criteria.push(AcceptanceCriterion {
                id: *id,
                position: self.criteria.len() as u16,
                text: text.clone(),
                status: CriterionStatus::Pending,
                satisfied_by: None,
            });
        }
        self.version = next_version;
        Ok(WorkEvent::DefinitionEdited {
            previous_definition_hash,
            title_changed,
            description_changed,
            edited_criterion_ids,
            added_criterion_ids: additional_ids.to_vec(),
        })
    }
}
#[cfg(test)]
mod tests;
