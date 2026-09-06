use super::*;

pub(super) fn compose(
    scratch: FrozenRunSnapshot,
    authority: &DispatchAuthority,
    selected: &ReviewedConversationSelection,
    remote: ReviewedSelectedRecall,
    redaction: &RedactionPolicy,
) -> Result<FrozenRunSnapshot, DispatchRefusal> {
    let rejected = || DispatchRefusal::MemoryContextRejected;
    if selected.company_id != authority.company_id()
        || selected.employee_id != *authority.employee_id()
        || Some(&selected.binding) != authority.memory_binding()
        || selected.records.len() > 8
        || remote.records.len() > selected.records.len()
        || authority
            .work_origin()
            .is_some_and(|work| work.project_id != selected.project_id)
    {
        return Err(rejected());
    }
    let selected_ids: std::collections::BTreeSet<_> = selected
        .records
        .iter()
        .map(ReviewedSelectionPin::fact_id)
        .collect();
    if selected_ids.len() != selected.records.len() {
        return Err(rejected());
    }
    let mut returned = std::collections::BTreeMap::new();
    let mut bytes = 0usize;
    for record in remote.records {
        let pin = selected
            .records
            .iter()
            .find(|pin| pin.fact_id() == record.fact_id())
            .ok_or_else(rejected)?;
        bytes = bytes
            .checked_add(record.content().len())
            .ok_or_else(rejected)?;
        if !pin.matches(&record)
            || bytes > 8192
            || record.rendered().is_err()
            || redaction.redact(record.content()) != record.content()
            || returned.insert(record.fact_id(), record).is_some()
        {
            return Err(rejected());
        }
    }
    let truncated =
        selected.truncated || remote.truncated || returned.len() < selected.records.len();
    // Remote UUID sorting must not replace thread/channel/project priority.
    let records: Vec<_> = selected
        .records
        .iter()
        .filter_map(|pin| returned.remove(&pin.fact_id()))
        .collect();
    if records
        .iter()
        .any(|record| matches!(record, ReviewedContextRecord::Conversation { .. }))
    {
        scratch
            .with_conversation(
                authority,
                ReviewedConversationContext {
                    origin: selected.origin.clone(),
                    records,
                    truncated,
                },
            )
            .map_err(|_| rejected())
    } else if authority.work_origin().is_some() {
        let mut project = Vec::new();
        for record in records {
            match record {
                ReviewedContextRecord::Project { record } => project.push(record),
                _ => return Err(rejected()),
            }
        }
        scratch
            .with_reviewed(
                authority,
                ReviewedMemoryContext {
                    records: project,
                    truncated,
                },
            )
            .map_err(|_| rejected())
    } else if records.is_empty() {
        Ok(scratch)
    } else {
        Err(rejected())
    }
}
