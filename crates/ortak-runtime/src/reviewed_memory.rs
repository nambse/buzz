//! Explicit reviewed recall composed with the existing scratch adapter.
mod conversation;
mod employee;
mod selection;
pub use conversation::{
    ReviewedConversationSelection, ReviewedSelectedRecall, ReviewedSelectionPin,
};
pub use employee::{
    EmployeeReviewedDestination, ReviewedEmployeeRecall, ReviewedEmployeeSelection,
    ReviewedEmployeeSelectionPin,
};

use crate::memory_context::{
    AdapterRunMemory, FrozenRunSnapshot, ReviewedMemoryContext, ReviewedMemoryPin, RunMemory,
};
use crate::{DispatchAuthority, DispatchRefusal};
use ortak_control::memory::MemoryAdapter;
use ortak_control::run_event::{RedactionPolicy, strip_control_characters};
use ortak_control::{CompanyScope, PgControlPlane};
use std::time::Duration;
use uuid::Uuid;

/// Selected current authority, with no approval text or secret material.
#[derive(Clone)]
pub struct ReviewedMemorySelection {
    /// Server-resolved company.
    pub company_id: Uuid,
    /// Work's canonical project.
    pub project_id: Uuid,
    /// Stable employee.
    pub employee_id: ortak_domain::EmployeeId,
    /// Current full memory binding, never supplied by the model.
    pub binding: ortak_domain::MemoryBinding,
    /// At most 32 current approvals; remote search applies these before its limit.
    pub pins: Vec<ReviewedMemoryPin>,
}

/// Optional selected-project read boundary. Implementations must never replace a
/// failed remote result with approval-registry text or broaden the exact IDs.
#[allow(async_fn_in_trait)]
pub trait ReviewedRunAdapter: Send + Sync {
    /// Explicit employee-owned target/destination selection. Absence is the
    /// unchanged legacy path; a model, source event or peer cannot opt itself in.
    fn employee_destination(
        &self,
        _authority: &DispatchAuthority,
    ) -> Result<Option<EmployeeReviewedDestination>, DispatchRefusal> {
        Ok(None)
    }

    /// Read only centrally selected current employee IDs from the exact retained
    /// owned namespace. Default implementations keep the new protocol closed.
    async fn recall_selected_employee(
        &self,
        _selection: &ReviewedEmployeeSelection,
    ) -> Result<ReviewedEmployeeRecall, DispatchRefusal> {
        Err(DispatchRefusal::MemoryAdapterUnavailable)
    }
    /// Explicit operator recipe opt-in for this exact Work project and employee.
    fn reviewed_enabled(&self, authority: &DispatchAuthority) -> Result<bool, DispatchRefusal>;
    /// Read only the selected current IDs using the existing owned I/O witness.
    async fn recall_selected(
        &self,
        selection: &ReviewedMemorySelection,
        query: &str,
    ) -> Result<ReviewedMemoryContext, DispatchRefusal>;

    /// Explicit configured project for this employee/channel (or the same Work
    /// project). Absence preserves existing behavior; no project is inferred.
    fn conversation_project(
        &self,
        _authority: &DispatchAuthority,
    ) -> Result<Option<Uuid>, DispatchRefusal> {
        Ok(None)
    }

    /// Read only the centrally selected, audience-checked IDs. The default
    /// refuses an opted-in path rather than falling back to local fact text.
    async fn recall_selected_conversation(
        &self,
        _selection: &ReviewedConversationSelection,
        _query: &str,
    ) -> Result<ReviewedSelectedRecall, DispatchRefusal> {
        Err(DispatchRefusal::MemoryAdapterUnavailable)
    }
}

/// Explicit project/conversation provider; unmapped scopes retain scratch behavior.
pub struct ReviewedRunMemory<'a, M> {
    adapter: &'a M,
    control: PgControlPlane,
    scope: CompanyScope,
}

impl<'a, M> ReviewedRunMemory<'a, M> {
    /// Uses the already selected worker adapter and server-owned company scope.
    pub fn new(adapter: &'a M, control: PgControlPlane, scope: CompanyScope) -> Self {
        Self {
            adapter,
            control,
            scope,
        }
    }
}

impl<M: MemoryAdapter + ReviewedRunAdapter> RunMemory for ReviewedRunMemory<'_, M> {
    async fn check(&self, authority: &DispatchAuthority) -> Result<(), DispatchRefusal> {
        AdapterRunMemory::new(self.adapter).check(authority).await
    }

    async fn snapshot(
        &self,
        authority: &DispatchAuthority,
        run_id: Uuid,
        redaction: &RedactionPolicy,
    ) -> Result<FrozenRunSnapshot, DispatchRefusal> {
        let scratch = AdapterRunMemory::new(self.adapter)
            .snapshot(authority, run_id, redaction)
            .await?;
        if let Some(destination) = self.adapter.employee_destination(authority)? {
            if let Some(context) = employee::recall(
                self.adapter,
                &self.control,
                &self.scope,
                authority,
                run_id,
                destination,
                redaction,
            )
            .await?
            {
                let limited = employee::Remaining {
                    adapter: self.adapter,
                    records: 8 - context.records.len(),
                };
                let legacy =
                    ReviewedRunMemory::new(&limited, self.control.clone(), self.scope.clone())
                        .legacy_snapshot(authority, run_id, redaction, scratch)
                        .await?;
                return legacy
                    .with_employee(authority, context)
                    .map_err(|_| DispatchRefusal::MemoryContextRejected);
            }
        }
        self.legacy_snapshot(authority, run_id, redaction, scratch)
            .await
    }
}

impl<M: ReviewedRunAdapter> ReviewedRunMemory<'_, M> {
    async fn legacy_snapshot(
        &self,
        authority: &DispatchAuthority,
        run_id: Uuid,
        redaction: &RedactionPolicy,
        scratch: FrozenRunSnapshot,
    ) -> Result<FrozenRunSnapshot, DispatchRefusal> {
        if let Some(project) = self.adapter.conversation_project(authority)? {
            match conversation::snapshot(
                self.adapter,
                &self.control,
                &self.scope,
                authority,
                run_id,
                project,
                redaction,
                scratch,
            )
            .await?
            {
                conversation::Outcome::Ready(snapshot) => return Ok(snapshot),
                conversation::Outcome::Legacy(scratch) => {
                    return self
                        .project_snapshot(authority, run_id, redaction, scratch)
                        .await;
                }
            }
        }
        self.project_snapshot(authority, run_id, redaction, scratch)
            .await
    }
}

impl<M: ReviewedRunAdapter> ReviewedRunMemory<'_, M> {
    async fn project_snapshot(
        &self,
        authority: &DispatchAuthority,
        run_id: Uuid,
        redaction: &RedactionPolicy,
        scratch: FrozenRunSnapshot,
    ) -> Result<FrozenRunSnapshot, DispatchRefusal> {
        if authority.work_origin().is_none() || !self.adapter.reviewed_enabled(authority)? {
            return Ok(scratch);
        }
        let query = work_query(&authority.input().body, redaction)?;
        if query.is_empty() {
            return scratch
                .with_reviewed(authority, ReviewedMemoryContext::default())
                .map_err(|_| DispatchRefusal::MemoryContextRejected);
        }
        let selected = selection::select(&self.control, &self.scope, authority, run_id, &query)
            .await
            .map_err(|_| DispatchRefusal::MemoryContextRejected)?;
        let context = if selected.pins.is_empty() {
            ReviewedMemoryContext::default()
        } else {
            let context = tokio::time::timeout(
                Duration::from_secs(10),
                self.adapter.recall_selected(&selected, &query),
            )
            .await
            .map_err(|_| DispatchRefusal::MemoryUnavailable)??;
            // A custom redaction policy may refuse approved text. It cannot
            // silently substitute unapproved bytes under the approved digest.
            context
                .validate()
                .map_err(|_| DispatchRefusal::MemoryContextRejected)?;
            for record in &context.records {
                if !selected.pins.contains(&record.pin)
                    || redaction.redact(&record.content) != record.content
                {
                    return Err(DispatchRefusal::MemoryContextRejected);
                }
            }
            context
        };
        scratch
            .with_reviewed(authority, context)
            .map_err(|_| DispatchRefusal::MemoryContextRejected)
    }
}

// Only human Work text contributes search terms. UUIDs, JSON field names and
// runtime instructions must not become mandatory conjuncts in a lexical query.
fn work_query(input: &str, redaction: &RedactionPolicy) -> Result<String, DispatchRefusal> {
    let value: serde_json::Value =
        serde_json::from_str(input).map_err(|_| DispatchRefusal::MemoryContextRejected)?;
    let mut terms = std::collections::BTreeSet::new();
    let mut selected = Vec::new();
    for field in ["title", "description"] {
        let text = value[field]
            .as_str()
            .ok_or(DispatchRefusal::MemoryContextRejected)?;
        let clean = redaction.redact(&strip_control_characters(text));
        for term in clean.split(|c: char| !c.is_alphanumeric()) {
            let term = term.to_lowercase();
            if term.len() >= 2 && term.len() <= 32 && terms.insert(term.clone()) {
                selected.push(term);
                if selected.len() == 16 {
                    return Ok(selected.join(" OR "));
                }
            }
        }
    }
    Ok(selected.join(" OR "))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_search_uses_bounded_human_terms_without_mandatory_uuid_conjuncts() {
        let value = serde_json::json!({"title":"Deployment DEPLOYMENT plan","description":"Verify café retry",
            "work_item_id":Uuid::new_v4(),"instructions":"Never search this synthetic instruction"});
        assert_eq!(
            work_query(&value.to_string(), &RedactionPolicy::new()).unwrap(),
            "deployment OR plan OR verify OR café OR retry"
        );
        let long = serde_json::json!({"title":(0..200).map(|i|format!("term{i}")).collect::<Vec<_>>().join(" "),"description":"unused"});
        assert_eq!(
            work_query(&long.to_string(), &RedactionPolicy::new())
                .unwrap()
                .split(" OR ")
                .count(),
            16
        );
    }
}
