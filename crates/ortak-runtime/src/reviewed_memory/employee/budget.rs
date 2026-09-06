use super::*;

/// Limits the old remote requests to the slots left by actual employee results.
/// It does not change old selection authority, query semantics or record bytes.
pub(crate) struct Remaining<'a, M> {
    pub adapter: &'a M,
    pub records: usize,
}

impl<M: ReviewedRunAdapter> ReviewedRunAdapter for Remaining<'_, M> {
    fn reviewed_enabled(&self, a: &DispatchAuthority) -> Result<bool, DispatchRefusal> {
        self.adapter.reviewed_enabled(a)
    }
    fn conversation_project(&self, a: &DispatchAuthority) -> Result<Option<Uuid>, DispatchRefusal> {
        self.adapter.conversation_project(a)
    }
    async fn recall_selected(
        &self,
        s: &ReviewedMemorySelection,
        q: &str,
    ) -> Result<ReviewedMemoryContext, DispatchRefusal> {
        let mut selected = s.clone();
        selected.pins.truncate(self.records);
        let omitted = selected.pins.len() != s.pins.len();
        let mut result = if selected.pins.is_empty() {
            ReviewedMemoryContext::default()
        } else {
            self.adapter.recall_selected(&selected, q).await?
        };
        if result
            .records
            .iter()
            .any(|r| !selected.pins.contains(&r.pin))
        {
            return Err(DispatchRefusal::MemoryContextRejected);
        }
        result.truncated |= omitted;
        Ok(result)
    }
    async fn recall_selected_conversation(
        &self,
        s: &ReviewedConversationSelection,
        q: &str,
    ) -> Result<ReviewedSelectedRecall, DispatchRefusal> {
        let mut selected = s.clone();
        selected.records.truncate(self.records);
        let omitted = selected.records.len() != s.records.len();
        let mut result = if selected.records.is_empty() {
            ReviewedSelectedRecall::default()
        } else {
            self.adapter
                .recall_selected_conversation(&selected, q)
                .await?
        };
        if result
            .records
            .iter()
            .any(|r| !selected.records.iter().any(|p| p.fact_id() == r.fact_id()))
        {
            return Err(DispatchRefusal::MemoryContextRejected);
        }
        result.truncated |= omitted;
        Ok(result)
    }
}
