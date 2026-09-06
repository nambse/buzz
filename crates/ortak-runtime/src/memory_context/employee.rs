//! V5 explicitly combines employee-owned approvals with unchanged older records.
//! Canonical claims are historical bytes; SQL supplies current authority.
use super::*;
use ortak_control::memory::employee::EmployeeMemoryKind;
use sha2::{Digest, Sha256};

mod origin;
mod record;
pub use origin::EmployeeMemoryOrigin;
pub use record::{EmployeeContextRecord, ReviewedEmployeePin, ReviewedEmployeeRecord};

#[cfg(test)]
mod tests;

/// Mixed reviewed context. At least one actual remote employee record is required.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedEmployeeContext {
    /// Canonical actual requester/source/destination and selected authority epochs.
    pub origin: EmployeeMemoryOrigin,
    /// Required only when the mixed set contains a conversation record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_origin: Option<ConversationMemoryOrigin>,
    /// Relationship, experience, then the existing reviewed records; scratch is separate.
    pub records: Vec<EmployeeContextRecord>,
    /// More selected matches existed than the finite context budget allowed.
    pub truncated: bool,
}

impl ReviewedEmployeeContext {
    pub(crate) fn validate(&self) -> Result<()> {
        let origin = self.origin.parsed()?;
        if self.records.is_empty() || self.records.len() > MAX_CONTEXT_RECORDS {
            return Err(rejected());
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut bytes = 0usize;
        let mut employees = 0usize;
        let mut prior = None;
        let mut legacy = Vec::new();
        for item in &self.records {
            if !ids.insert(item.fact_id()) {
                return Err(rejected());
            }
            bytes = bytes
                .checked_add(item.content().len())
                .ok_or_else(rejected)?;
            if bytes > 8192 {
                return Err(rejected());
            }
            if let EmployeeContextRecord::Employee { record } = item {
                if !legacy.is_empty() {
                    return Err(rejected());
                }
                employees += 1;
                let parsed = record.validate()?;
                let a = parsed.audience();
                let priority = if a.kind() == EmployeeMemoryKind::Relationship {
                    0
                } else {
                    1
                };
                let order = (priority, record.pin.fact_id);
                if prior.is_some_and(|old| old >= order) {
                    return Err(rejected());
                }
                prior = Some(order);
                let namespace = serde_json::to_vec(&serde_json::json!({
                    "company_id":origin.company_id,"employee_id":origin.employee_id,
                    "format":"ortak-reviewed-employee-namespace/1"}))
                .map_err(|_| rejected())?;
                if a.company_id() != origin.company_id
                    || a.employee_id() != &origin.employee_id
                    || a.destination_community_id() != origin.source.community_id
                    || a.destination_channel_id() != origin.destination_channel_id
                    || a.human_public_key()
                        .is_some_and(|key| key.to_hex() != origin.requester_public_key)
                    || record.pin.destination_authority_epoch != origin.destination_authority_epoch
                    || record.pin.namespace_hash != hex::encode(Sha256::digest(namespace))
                {
                    return Err(rejected());
                }
            } else {
                legacy.push(item.legacy().ok_or_else(rejected)?);
            }
            item.rendered()?;
        }
        if employees == 0 {
            return Err(rejected());
        }
        let has_conversation = legacy
            .iter()
            .any(|r| matches!(r, ReviewedContextRecord::Conversation { .. }));
        match (&self.conversation_origin, has_conversation) {
            (Some(conversation), true) => {
                let parsed = conversation.parsed_provenance()?;
                if conversation.requester_public_key() != origin.requester_public_key
                    || parsed.source().event_id().to_hex() != origin.source.event_id
                    || parsed
                        .source()
                        .created_at()
                        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
                        != origin.source.event_created_at
                    || parsed.audience().company_id() != origin.company_id
                    || parsed.audience().community_id() != origin.source.community_id
                    || parsed.audience().employee_id() != &origin.employee_id
                    || parsed.audience().channel_id() != origin.destination_channel_id
                {
                    return Err(rejected());
                }
                ReviewedConversationContext {
                    origin: conversation.clone(),
                    records: legacy,
                    truncated: self.truncated,
                }
                .validate()?;
            }
            (None, false) => {}
            _ => return Err(rejected()),
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
                .any(|r| matches!(r, EmployeeContextRecord::Project { .. }))
        {
            return Err(rejected());
        }
        if let Some(origin) = &self.conversation_origin {
            // Use the unchanged v4 validator for its actual Work/thread binding.
            ReviewedConversationContext {
                origin: origin.clone(),
                records: self
                    .records
                    .iter()
                    .filter_map(EmployeeContextRecord::legacy)
                    .collect(),
                truncated: self.truncated,
            }
            .validate_for(authority)?;
        }
        Ok(())
    }

    pub(crate) fn rendered(&self) -> Result<Vec<String>> {
        self.validate()?;
        self.records
            .iter()
            .map(EmployeeContextRecord::rendered)
            .collect()
    }
}

impl FrozenRunSnapshot {
    /// Exact mixed employee attribution, present only in version five.
    pub fn employee(&self) -> Option<&ReviewedEmployeeContext> {
        self.wire.employee.as_ref()
    }

    /// In-memory validation projection only: it is never persisted or sent to
    /// a runtime. It lets the unchanged legacy validators check their own rows.
    pub(crate) fn employee_legacy_projection(&self, authority: &DispatchAuthority) -> Result<Self> {
        let context = self.employee().ok_or_else(rejected)?;
        let plain = Self::from_recall(authority, self.spec().run_id, self.wire.recall.clone())?;
        if let Some(origin) = &context.conversation_origin {
            plain.with_conversation(
                authority,
                ReviewedConversationContext {
                    origin: origin.clone(),
                    records: context
                        .records
                        .iter()
                        .filter_map(EmployeeContextRecord::legacy)
                        .collect(),
                    truncated: context.truncated,
                },
            )
        } else {
            let records = context
                .records
                .iter()
                .filter_map(|r| match r {
                    EmployeeContextRecord::Project { record } => Some(record.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if records.is_empty() {
                Ok(plain)
            } else {
                plain.with_reviewed(
                    authority,
                    ReviewedMemoryContext {
                        records,
                        truncated: context.truncated,
                    },
                )
            }
        }
    }

    /// Upgrades a previously valid legacy snapshot only after actual remote
    /// employee records arrived. Empty responses must return the legacy winner.
    pub(crate) fn with_employee(
        mut self,
        authority: &DispatchAuthority,
        mut context: ReviewedEmployeeContext,
    ) -> Result<Self> {
        if self.wire.employee.is_some()
            || context.conversation_origin.is_some()
            || context
                .records
                .iter()
                .any(|r| !matches!(r, EmployeeContextRecord::Employee { .. }))
        {
            return Err(rejected());
        }
        context.validate_for(authority)?;
        let old = if let Some(conversation) = self.wire.conversation.take() {
            context.conversation_origin = Some(conversation.origin);
            context.truncated |= conversation.truncated;
            conversation
                .records
                .into_iter()
                .map(|r| match r {
                    ReviewedContextRecord::Project { record } => {
                        EmployeeContextRecord::Project { record }
                    }
                    ReviewedContextRecord::Conversation { record } => {
                        EmployeeContextRecord::Conversation { record }
                    }
                })
                .collect::<Vec<_>>()
        } else if let Some(reviewed) = self.wire.reviewed.take() {
            context.truncated |= reviewed.truncated;
            reviewed
                .records
                .into_iter()
                .map(|record| EmployeeContextRecord::Project { record })
                .collect()
        } else {
            vec![]
        };
        let mut bytes = context
            .records
            .iter()
            .map(|r| r.content().len())
            .sum::<usize>();
        for item in old {
            if context.records.len() == MAX_CONTEXT_RECORDS || bytes + item.content().len() > 8192 {
                context.truncated = true;
                // Keep a stable prefix; a later smaller record may not jump ahead.
                break;
            }
            bytes += item.content().len();
            context.records.push(item);
        }
        if !context
            .records
            .iter()
            .any(|r| matches!(r, EmployeeContextRecord::Conversation { .. }))
        {
            context.conversation_origin = None;
        }
        context.validate_for(authority)?;
        let remaining = MAX_CONTEXT_RECORDS - context.records.len();
        if self.wire.recall.records.len() > remaining {
            self.wire.recall.records.truncate(remaining);
            self.wire.recall.truncated = true;
        }
        while bytes
            + self
                .wire
                .recall
                .records
                .iter()
                .map(|r| r.content.len())
                .sum::<usize>()
            > MAX_CONTEXT_BYTES
        {
            self.wire.recall.records.pop();
            self.wire.recall.truncated = true;
        }
        self.wire.version = 5;
        self.wire.spec.context.memory_context = context.rendered()?;
        self.wire
            .spec
            .context
            .memory_context
            .extend(rendered(&self.wire.recall)?);
        if self
            .wire
            .spec
            .context
            .memory_context
            .iter()
            .any(|s| s.len() > MAX_RENDERED_CONTEXT_BYTES)
        {
            return Err(rejected());
        }
        self.wire.employee = Some(context);
        let bytes = serde_json::to_vec(&self.wire).map_err(|_| rejected())?;
        Self::decode(&bytes, authority, self.wire.spec.run_id)
    }
}
