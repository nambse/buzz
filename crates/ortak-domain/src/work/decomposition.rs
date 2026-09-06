//! Structural decomposition never changes acceptance, assignment or context.
use super::*;

/// Maximum direct children retained under one work item.
pub const MAX_WORK_CHILDREN: usize = 32;
/// Maximum child depth, with a root at zero.
pub const MAX_WORK_DEPTH: i16 = 8;

/// Independent human definition of a new manual child; its scope is derived.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NewChildWork {
    /// Bounded, secret-free title.
    pub title: String,
    /// Independent description; parent content is never copied implicitly.
    #[serde(default)]
    pub description: String,
    /// Independent priority.
    #[serde(default)]
    pub priority: WorkPriority,
    /// The child's own human acceptance criteria.
    #[serde(default)]
    pub criteria: Vec<String>,
    /// The child's own human approval gates.
    #[serde(default)]
    pub approvals: Vec<ApprovalGateSpec>,
}
impl NewChildWork {
    /// Validate the existing Work definition bounds without looking up scope.
    pub fn validate(&self) -> Result<(), DomainError> {
        self.clone().into_item(Uuid::nil()).validate()
    }
    /// Derive a manual item in the authorized parent's project.
    pub fn into_item(self, project_id: Uuid) -> NewWorkItem {
        NewWorkItem {
            project_id,
            title: self.title,
            description: self.description,
            priority: self.priority,
            criteria: self.criteria,
            approvals: self.approvals,
            source_message_id: None,
        }
    }
}
impl WorkItem {
    /// Record a fresh structural child, preserving all current review facts.
    /// The repository must create the child and retained link in the same commit.
    pub fn record_child_created(&mut self, child_id: Uuid) -> Result<WorkEvent, DomainError> {
        self.ensure_mutable()?;
        if child_id.is_nil() || child_id == self.id {
            return Err(DomainError::InvalidField {
                field: "work.child",
            });
        }
        self.version = self
            .version
            .checked_add(1)
            .ok_or(DomainError::InvalidField {
                field: "work.version",
            })?;
        Ok(WorkEvent::ChildCreated { child_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> WorkItem {
        WorkItem::create(
            NewWorkItemIds {
                id: Uuid::new_v4(),
                criterion_ids: vec![Uuid::new_v4()],
                approval_ids: vec![],
            },
            &NewWorkItem {
                project_id: Uuid::new_v4(),
                title: "Parent".into(),
                description: "Original context".into(),
                priority: WorkPriority::High,
                criteria: vec!["Human accepts parent".into()],
                approvals: vec![],
                source_message_id: None,
            },
        )
        .unwrap()
        .0
    }
    #[test]
    fn decomposition_advances_only_parent_history_and_child_definition_stays_independent() {
        let mut parent = fixture();
        let before = parent.clone();
        let child_id = Uuid::new_v4();
        assert_eq!(
            parent.record_child_created(child_id).unwrap(),
            WorkEvent::ChildCreated { child_id }
        );
        assert_eq!(parent.version, before.version + 1);
        parent.version = before.version;
        assert_eq!(parent, before);
        let input = NewChildWork {
            title: "Independent child".into(),
            description: String::new(),
            priority: WorkPriority::Normal,
            criteria: vec!["Human accepts child".into()],
            approvals: vec![],
        }
        .into_item(parent.project_id);
        let child = WorkItem::create(
            NewWorkItemIds {
                id: child_id,
                criterion_ids: vec![Uuid::new_v4()],
                approval_ids: vec![],
            },
            &input,
        )
        .unwrap()
        .0;
        assert_eq!(child.state, WorkState::Proposed);
        assert_eq!(child.version, 1);
        assert!(
            child.source_message_id.is_none()
                && child.attachments.is_empty()
                && child.assignments.is_empty()
        );
        assert_ne!(child.criteria, parent.criteria);
        assert!(child.description.is_empty());
    }
    #[test]
    fn decomposition_invalid_or_terminal_child_creation_preserves_every_parent_fact() {
        let mut parent = fixture();
        for child in [Uuid::nil(), parent.id] {
            let before = parent.clone();
            assert!(parent.record_child_created(child).is_err());
            assert_eq!(parent, before);
        }
        parent.state = WorkState::Cancelled;
        let before = parent.clone();
        assert!(parent.record_child_created(Uuid::new_v4()).is_err());
        assert_eq!(parent, before);
    }
}
