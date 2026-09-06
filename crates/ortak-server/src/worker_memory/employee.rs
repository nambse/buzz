//! Exact configured employee-owned namespace reads; no diagnostic or local text.
use super::*;
use ortak_runtime::memory_context::ReviewedEmployeeRecord;
use ortak_runtime::reviewed_memory::{
    EmployeeReviewedDestination, ReviewedEmployeeRecall, ReviewedEmployeeSelection,
};
use ortak_runtime::{DispatchAuthority, DispatchRefusal};

pub(super) fn destination(
    worker: &WorkerMemory,
    authority: &DispatchAuthority,
) -> Result<Option<EmployeeReviewedDestination>, DispatchRefusal> {
    let Some(channel) = authority.input().channel_id else {
        return Ok(None);
    };
    let values = worker
        .validations
        .lock()
        .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
    let mut found = None;
    for value in values.iter().filter(|v| {
        v.resource.employee_id == *authority.employee_id()
            && Some(&v.resource.binding) == authority.memory_binding()
            && v.creation_receipt
                .as_ref()
                .is_some_and(|r| r.company_id == authority.company_id())
    }) {
        for selected in value
            .reviewed_employee_destinations
            .iter()
            .filter(|s| s.destination_channel_id == channel)
        {
            if found.is_some() {
                return Err(DispatchRefusal::MemoryContextRejected);
            };
            found = Some(*selected);
        }
    }
    Ok(found)
}

pub(super) async fn recall(
    worker: &WorkerMemory,
    selected: &ReviewedEmployeeSelection,
) -> Result<ReviewedEmployeeRecall, DispatchRefusal> {
    let refused = || DispatchRefusal::MemoryContextRejected;
    let original = {
        let values = worker
            .validations
            .lock()
            .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
        let value = values
            .iter()
            .find(|v| {
                v.resource.employee_id == selected.employee_id
                    && v.resource.binding == selected.binding
                    && v.reviewed_employee_destinations
                        .contains(&selected.destination)
            })
            .ok_or_else(refused)?;
        value.creation_receipt.clone().ok_or_else(refused)?
    };
    let mut stored = selected.creation_receipt.clone();
    let object = stored.as_object_mut().ok_or_else(refused)?;
    if object.remove("protocol").as_ref() != Some(&serde_json::json!("reviewed-employee/1")) {
        return Err(refused());
    }
    let namespace_hash = object
        .remove("namespace_hash")
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or_else(refused)?;
    if serde_json::from_value::<HonchoCreatedResourcesReceipt>(stored).map_err(|_| refused())?
        != original
        || original.company_id != selected.company_id
        || original.employee_id != selected.employee_id
        || original.deployment_id != selected.deployment_id
        || original.binding != selected.binding
        || selected.records.is_empty()
        || selected.records.len() > 8
        || selected
            .origin
            .destination_channel_id()
            .map_err(|_| refused())?
            != selected.destination.destination_channel_id
    {
        return Err(refused());
    }
    let human = selected
        .origin
        .requester_public_key()
        .map_err(|_| refused())?;
    let adapter = worker
        .adapter
        .as_ref()
        .ok_or(DispatchRefusal::MemoryAdapterUnavailable)?;
    // A read-only native ownership inspection is sufficient after the retained
    // initial namespace registration. Never run a fresh write/read/delete probe.
    let namespace = adapter
        .inspect_reviewed_employee_namespace(&original)
        .await
        .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
    if namespace.namespace_hash() != namespace_hash
        || selected.records.iter().any(|s| {
            s.pin.target_id != selected.destination.target_id
                || s.pin.namespace_hash != namespace.namespace_hash()
                || s.pin.binding_hash != namespace.binding_hash()
        })
    {
        return Err(refused());
    }
    let commitments = selected
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
    let remote = adapter
        .recall_selected_reviewed_employee(
            &namespace,
            selected.destination.destination_channel_id,
            Some(&human),
            &commitments,
        )
        .await
        .map_err(|_| DispatchRefusal::MemoryUnavailable)?;
    let mut records = Vec::new();
    for r in remote.records {
        let expected = selected
            .records
            .iter()
            .find(|s| s.pin.fact_id == r.record_id)
            .ok_or_else(refused)?;
        if r.target_id != expected.pin.target_id
            || r.content_hash != expected.pin.content_hash
            || r.source_hash != expected.pin.source_hash
            || r.sharing_hash != expected.pin.sharing_hash
            || r.binding_hash != expected.pin.binding_hash
            || r.namespace_hash != expected.pin.namespace_hash
            || r.expires_at != Some(expected.pin.expires_at)
            || r.provenance.as_deref() != Some(expected.provenance.as_str())
            || r.status != ortak_memory::ReviewedProjectStatus::Active
            || r.erased_from_reviewed_store
        {
            return Err(refused());
        }
        records.push(ReviewedEmployeeRecord {
            pin: expected.pin.clone(),
            content: r.content.ok_or_else(refused)?,
            provenance: expected.provenance.clone(),
        });
    }
    Ok(ReviewedEmployeeRecall {
        records,
        truncated: remote.truncated,
    })
}
