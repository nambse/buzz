//! V4 conversation context is historical, typed input; current ACL stays in SQL.
use super::*;

mod origin;
mod record;
pub use origin::ConversationMemoryOrigin;
pub use record::{ReviewedContextRecord, ReviewedConversationPin, ReviewedConversationRecord};

/// Existing control/bridge per-string ceiling, including rendered JSON metadata.
pub const MAX_RENDERED_CONTEXT_BYTES: usize = 8192;

/// Ordered combined reviewed context for v4; at least one conversation fact is required.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedConversationContext {
    /// Database-observed human and thread source; never a concurrency witness.
    pub origin: ConversationMemoryOrigin,
    /// At most eight project/conversation records in their admitted use order.
    pub records: Vec<ReviewedContextRecord>,
    /// More matches existed than the selected finite record/content budget.
    pub truncated: bool,
}

impl ReviewedConversationContext {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.records.is_empty() || self.records.len() > MAX_CONTEXT_RECORDS {
            return Err(rejected());
        }
        let origin = self.origin.parsed_provenance()?;
        let expected = origin.audience();
        let mut ids = std::collections::BTreeSet::new();
        let mut content_bytes = 0usize;
        let mut conversations = 0usize;
        for record in &self.records {
            if !ids.insert(record.fact_id()) {
                return Err(rejected());
            }
            content_bytes = content_bytes
                .checked_add(record.content().len())
                .ok_or_else(rejected)?;
            if content_bytes > 8192 {
                return Err(rejected());
            }
            if let ReviewedContextRecord::Conversation { record } = record {
                conversations += 1;
                let parsed = record.validate()?;
                let audience = parsed.audience();
                if audience.company_id() != expected.company_id()
                    || audience.community_id() != expected.community_id()
                    || audience.project_id() != expected.project_id()
                    || audience.employee_id() != expected.employee_id()
                    || audience.channel_id() != expected.channel_id()
                    || audience
                        .thread_root()
                        .is_some_and(|root| Some(root) != expected.thread_root())
                {
                    return Err(rejected());
                }
            }
            record.rendered()?;
        }
        if conversations == 0 {
            return Err(rejected());
        }
        Ok(())
    }

    pub(crate) fn validate_for(&self, authority: &DispatchAuthority) -> Result<()> {
        self.validate()?;
        self.origin.validate_for(authority)?;
        if authority.work_origin().is_none()
            && self
                .records
                .iter()
                .any(|record| matches!(record, ReviewedContextRecord::Project { .. }))
        {
            return Err(rejected());
        }
        Ok(())
    }

    pub(crate) fn rendered(&self) -> Result<Vec<String>> {
        self.validate()?;
        self.records
            .iter()
            .map(ReviewedContextRecord::rendered)
            .collect()
    }
}

impl FrozenRunSnapshot {
    /// Exact ordered conversation attribution in v4, absent on all legacy snapshots.
    pub fn conversation(&self) -> Option<&ReviewedConversationContext> {
        self.wire.conversation.as_ref()
    }

    /// Build v4 from a plain scratch snapshot and the explicitly selected reviewed set.
    /// Project-only/empty results must use the existing v1–3 construction path.
    pub(crate) fn with_conversation(
        mut self,
        authority: &DispatchAuthority,
        context: ReviewedConversationContext,
    ) -> Result<Self> {
        if self.wire.reviewed.is_some()
            || self.wire.conversation.is_some()
            || self.wire.employee.is_some()
        {
            return Err(rejected());
        }
        context.validate_for(authority)?;
        let remaining = MAX_CONTEXT_RECORDS.saturating_sub(context.records.len());
        if self.wire.recall.records.len() > remaining {
            self.wire.recall.records.truncate(remaining);
            self.wire.recall.truncated = true;
        }
        let reviewed_bytes: usize = context.records.iter().map(|r| r.content().len()).sum();
        while self
            .wire
            .recall
            .records
            .iter()
            .map(|r| r.content.len())
            .sum::<usize>()
            + reviewed_bytes
            > MAX_CONTEXT_BYTES
        {
            self.wire.recall.records.pop();
            self.wire.recall.truncated = true;
        }
        self.wire.version = 4;
        self.wire.spec.context.memory_context = rendered(&self.wire.recall)?;
        self.wire
            .spec
            .context
            .memory_context
            .extend(context.rendered()?);
        self.wire.conversation = Some(context);
        let bytes = serde_json::to_vec(&self.wire).map_err(|_| rejected())?;
        Self::decode(&bytes, authority, self.wire.spec.run_id)
    }
}

#[cfg(test)]
mod tests;
