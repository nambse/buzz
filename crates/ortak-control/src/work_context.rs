//! Frozen Work reference data. Current project/source authority remains external.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use ortak_domain::EmployeeId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::conversation_context::{
    event_id, optional_event_id, text, ContextEmployee, ContextMessage, ContextSelection,
};

/// Maximum encoded Work reference envelope, including the complete prior output.
pub const MAX_WORK_CONTEXT_BYTES: usize = 128 * 1024;
/// Maximum canonical messages from the explicitly linked thread.
pub const MAX_WORK_MESSAGES: usize = 16;
/// Maximum text bytes retained from one canonical message.
pub const MAX_WORK_MESSAGE_BYTES: usize = 2 * 1024;
/// Maximum aggregate canonical message text.
pub const MAX_WORK_HISTORY_BYTES: usize = 16 * 1024;
/// Maximum currently visible coworkers, excluding the receiver.
pub const MAX_WORK_TEAMMATES: usize = 16;
/// Prior text deliverables are complete, never truncated to fit the envelope.
pub const MAX_ARTIFACT_BYTES: usize = 32 * 1024;

/// Exact immutable output chosen by the authenticated Work request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextArtifact {
    /// Same-item artifact identity; this is not a path or a tool argument.
    pub artifact_id: Uuid,
    /// Run that produced the selected output.
    pub run_id: Uuid,
    /// Employee that authored it, never coerced into the receiver's identity.
    pub employee_id: EmployeeId,
    /// Work version of the producing execution.
    pub execution_version: i64,
    /// Complete untrusted UTF-8 text.
    pub content: String,
    /// SHA-256 of the exact UTF-8 content bytes.
    pub sha256: String,
}

impl ContextArtifact {
    fn valid(&self, run: Uuid, version: i64) -> bool {
        !self.artifact_id.is_nil()
            && !self.run_id.is_nil()
            && self.run_id != run
            && self.execution_version > 0
            && self.execution_version < version
            && !self.content.trim().is_empty()
            && self.content.len() <= MAX_ARTIFACT_BYTES
            && event_id(&self.sha256)
            && self.sha256 == hex::encode(Sha256::digest(self.content.as_bytes()))
    }
}

/// Bounded same-item reference material supplied separately from the Work request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkContext {
    /// Closed wire version.
    pub version: u8,
    /// Run owning this immutable snapshot.
    pub snapshot_id: Uuid,
    /// Work whose saved definition is the current request.
    pub work_item_id: Uuid,
    /// Project granting current contribution authority.
    pub project_id: Uuid,
    /// Work version immediately before the authenticated execution request.
    pub requested_version: i64,
    /// Work version atomically created by that request.
    pub execution_version: i64,
    /// Project's exact plaintext Office channel.
    pub channel_id: Uuid,
    /// Explicitly linked source; absent means no Office history is supplied.
    pub source_message_id: Option<String>,
    /// Canonical root of that source, including when the source is itself a root.
    pub thread_root_message_id: Option<String>,
    /// Saved execution request time; later received events cannot enter the input.
    pub cutoff_received_at: DateTime<Utc>,
    /// Receiver's pinned public identity.
    pub employee: ContextEmployee,
    /// Current eligible public team facts at first snapshot admission.
    pub teammates: Vec<ContextEmployee>,
    /// Attributed, chronologically ordered reference messages from the linked thread.
    pub messages: Vec<ContextMessage>,
    /// True when older messages or content could not fit the finite budget.
    pub omitted_history: bool,
    /// Exact complete previous deliverable, when selected at request time.
    pub prior_artifact: Option<ContextArtifact>,
}

impl WorkContext {
    /// Validate wire bounds and attribution, never permission to read its sources.
    pub fn valid_for(&self, run: Uuid, employee: &EmployeeId, revision: Uuid, item: Uuid) -> bool {
        if self.version != 1
            || run.is_nil()
            || self.snapshot_id != run
            || self.work_item_id != item
            || item.is_nil()
            || self.project_id.is_nil()
            || self.channel_id.is_nil()
            || self.requested_version < 1
            || self.requested_version.checked_add(1) != Some(self.execution_version)
            || self.employee.employee_id != *employee
            || self.employee.revision_id != revision
            || !self.employee.valid()
            || self.teammates.len() > MAX_WORK_TEAMMATES
            || self.messages.len() > MAX_WORK_MESSAGES
            || !optional_event_id(&self.source_message_id)
            || !optional_event_id(&self.thread_root_message_id)
            || self.source_message_id.is_some() != self.thread_root_message_id.is_some()
            || self
                .prior_artifact
                .as_ref()
                .is_some_and(|a| !a.valid(run, self.execution_version))
        {
            return false;
        }
        let mut team = BTreeSet::from([employee.clone()]);
        if self
            .teammates
            .iter()
            .any(|e| !e.valid() || !team.insert(e.employee_id.clone()))
        {
            return false;
        }
        let mut ids = BTreeSet::new();
        let mut bytes = 0;
        let mut previous = None;
        for message in &self.messages {
            let Some(root) = self.thread_root_message_id.as_ref() else {
                return false;
            };
            let order = (message.created_at, message.message_id.as_str());
            bytes += message.content.len();
            if !event_id(&message.message_id)
                || !ids.insert(message.message_id.as_str())
                || !event_id(&message.source_content_hash)
                || !event_id(&message.author_public_key)
                || !text(&message.author_name, 200, false)
                || !text(&message.content, MAX_WORK_MESSAGE_BYTES, false)
                || !optional_event_id(&message.parent_message_id)
                || !optional_event_id(&message.thread_root_message_id)
                || previous.is_some_and(|p| p >= order)
                || if message.message_id == *root {
                    message.selection != ContextSelection::ThreadRoot
                } else {
                    message.selection != ContextSelection::ThreadRecent
                        || message.thread_root_message_id.as_ref() != Some(root)
                }
            {
                return false;
            }
            previous = Some(order);
        }
        if let Some(source) = self.source_message_id.as_ref() {
            if !ids.contains(source.as_str())
                || !self
                    .thread_root_message_id
                    .as_ref()
                    .is_some_and(|root| ids.contains(root.as_str()))
            {
                return false;
            }
            if self
                .messages
                .iter()
                .find(|m| &m.message_id == source)
                .and_then(|m| m.parent_message_id.as_deref())
                .is_some_and(|parent| !ids.contains(parent))
            {
                return false;
            }
        } else if !self.messages.is_empty() || self.omitted_history {
            return false;
        }
        bytes <= MAX_WORK_HISTORY_BYTES
            && serde_json::to_vec(self).is_ok_and(|v| v.len() <= MAX_WORK_CONTEXT_BYTES)
    }
}

#[cfg(test)]
mod tests;
