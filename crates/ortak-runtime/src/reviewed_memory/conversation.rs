//! A selected conversation path never substitutes approval-registry text.
use super::*;
use crate::memory_context::{
    ConversationMemoryOrigin, ReviewedContextRecord, ReviewedConversationContext,
    ReviewedConversationPin, ReviewedConversationRecord, ReviewedMemoryRecord,
};

mod response;
mod selection;
mod types;
pub use types::{ReviewedConversationSelection, ReviewedSelectedRecall, ReviewedSelectionPin};

pub(super) enum Outcome {
    Ready(FrozenRunSnapshot),
    Legacy(FrozenRunSnapshot),
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn snapshot<M: ReviewedRunAdapter>(
    adapter: &M,
    control: &PgControlPlane,
    scope: &CompanyScope,
    authority: &DispatchAuthority,
    run_id: Uuid,
    project: Uuid,
    redaction: &RedactionPolicy,
    scratch: FrozenRunSnapshot,
) -> Result<Outcome, DispatchRefusal> {
    if project.is_nil()
        || authority
            .work_origin()
            .is_some_and(|work| work.project_id != project)
    {
        return Err(DispatchRefusal::MemoryContextRejected);
    }
    let query = if authority.work_origin().is_some() {
        work_query(&authority.input().body, redaction)?
    } else {
        office_query(&authority.input().body, redaction)
    };
    let include_project =
        authority.work_origin().is_some() && adapter.reviewed_enabled(authority)?;
    let Some(selected) = selection::select(
        control,
        scope,
        authority,
        run_id,
        project,
        &query,
        include_project,
    )
    .await
    .map_err(|_| DispatchRefusal::MemoryContextRejected)?
    else {
        // Only a positively identified manual Work or nonhuman Office
        // source can use this branch. Lost human source authority refuses.
        return Ok(Outcome::Legacy(scratch));
    };
    let remote = if selected.records.is_empty() {
        ReviewedSelectedRecall::default()
    } else {
        tokio::time::timeout(
            Duration::from_secs(10),
            adapter.recall_selected_conversation(&selected, &query),
        )
        .await
        .map_err(|_| DispatchRefusal::MemoryUnavailable)??
    };
    response::compose(scratch, authority, &selected, remote, redaction).map(Outcome::Ready)
}

fn office_query(input: &str, redaction: &RedactionPolicy) -> String {
    let clean = redaction.redact(&strip_control_characters(input));
    let mut terms = std::collections::BTreeSet::new();
    clean
        .split(|c: char| !c.is_alphanumeric())
        .filter_map(|value| {
            let term = value.to_lowercase();
            (term.len() >= 2 && term.len() <= 32 && terms.insert(term.clone())).then_some(term)
        })
        .take(16)
        .collect::<Vec<_>>()
        .join(" OR ")
}

#[cfg(test)]
mod tests;
