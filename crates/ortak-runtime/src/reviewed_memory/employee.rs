//! Explicit employee-owned recall; no model/request parameter selects a namespace.
use super::*;
use crate::Result as RuntimeResult;
use crate::memory_context::{
    EmployeeContextRecord, EmployeeMemoryOrigin, ReviewedEmployeeContext, ReviewedEmployeePin,
    ReviewedEmployeeRecord,
};

mod budget;
mod selection;
pub(super) use budget::Remaining;
mod types;
pub use types::{
    EmployeeReviewedDestination, ReviewedEmployeeRecall, ReviewedEmployeeSelection,
    ReviewedEmployeeSelectionPin,
};

pub(super) async fn recall<M: ReviewedRunAdapter>(
    adapter: &M,
    control: &PgControlPlane,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    run: Uuid,
    destination: EmployeeReviewedDestination,
    redaction: &RedactionPolicy,
) -> Result<Option<ReviewedEmployeeContext>, DispatchRefusal> {
    let selected = selection::select(control, scope, authority, run, destination)
        .await
        .map_err(|_| DispatchRefusal::MemoryContextRejected)?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    if selected.records.is_empty() {
        return Ok(None);
    };
    let remote = tokio::time::timeout(
        Duration::from_secs(10),
        adapter.recall_selected_employee(&selected),
    )
    .await
    .map_err(|_| DispatchRefusal::MemoryUnavailable)??;
    if remote.records.len() > selected.records.len() {
        return Err(DispatchRefusal::MemoryContextRejected);
    };
    let mut previous = None;
    for record in &remote.records {
        let index = selected
            .records
            .iter()
            .position(|s| s.pin == record.pin && s.provenance == record.provenance)
            .ok_or(DispatchRefusal::MemoryContextRejected)?;
        if previous.is_some_and(|old| old >= index)
            || redaction.redact(&record.content) != record.content
        {
            return Err(DispatchRefusal::MemoryContextRejected);
        }
        record
            .validate()
            .map_err(|_| DispatchRefusal::MemoryContextRejected)?;
        previous = Some(index);
    }
    if remote.records.is_empty() {
        return Ok(None);
    };
    // HTTP cannot carry a transactional witness. Re-resolve all permissions and
    // exact returned pins after it, then freeze rechecks them once more atomically.
    let current = selection::select(control, scope, authority, run, destination)
        .await
        .map_err(|_| DispatchRefusal::MemoryContextRejected)?
        .ok_or(DispatchRefusal::MemoryContextRejected)?;
    if selected.origin != current.origin
        || remote.records.iter().any(|r| {
            !current
                .records
                .iter()
                .any(|s| s.pin == r.pin && s.provenance == r.provenance)
        })
    {
        return Err(DispatchRefusal::MemoryContextRejected);
    }
    let context = ReviewedEmployeeContext {
        origin: selected.origin,
        conversation_origin: None,
        records: remote
            .records
            .into_iter()
            .map(|record| EmployeeContextRecord::Employee { record })
            .collect(),
        truncated: selected.truncated || remote.truncated,
    };
    context
        .validate_for(authority)
        .map_err(|_| DispatchRefusal::MemoryContextRejected)?;
    Ok(Some(context))
}
