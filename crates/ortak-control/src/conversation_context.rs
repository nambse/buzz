//! Bounded, attributed Office reference data. This wire never grants authority.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use ortak_domain::EmployeeId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Maximum number of canonical prior messages in a run.
pub const MAX_MESSAGES: usize = 32;
/// Maximum UTF-8 bytes in one prior message.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024;
/// Maximum aggregate prior-message text bytes.
pub const MAX_HISTORY_BYTES: usize = 48 * 1024;
/// Maximum encoded context bytes, including attribution and team information.
pub const MAX_CONTEXT_BYTES: usize = 64 * 1024;
/// Maximum visible colleagues; the receiving employee is separate.
pub const MAX_TEAMMATES: usize = 32;

/// Public employee facts selected from a validated revision; no runtime secrets.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextEmployee {
    /// Stable identity, independent from the current model or session.
    pub employee_id: EmployeeId,
    /// Revision supplying these facts.
    pub revision_id: Uuid,
    /// Display name.
    pub name: String,
    /// Company role.
    pub title: String,
    /// Role description, not a system instruction.
    pub biography: String,
    /// Declared responsibilities.
    pub responsibilities: Vec<String>,
    /// Declared domains, not proof of completed work.
    pub domains: Vec<String>,
}

impl ContextEmployee {
    fn valid(&self) -> bool {
        !self.revision_id.is_nil()
            && text(&self.name, 200, false)
            && text(&self.title, 200, false)
            && text(&self.biography, 4096, true)
            && self.responsibilities.len() <= 32
            && self.responsibilities.iter().all(|v| text(v, 512, false))
            && self.domains.len() <= 32
            && self.domains.iter().all(|v| text(v, 128, false))
    }
}

/// Why a canonical message entered this snapshot, never a routing instruction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSelection {
    /// Explicit relay-persisted reply parent.
    ReplyParent,
    /// Root of the explicitly addressed thread.
    ThreadRoot,
    /// Nearby message inside that same thread.
    ThreadRecent,
    /// Nearby channel message for a new, unthreaded request.
    ChannelRecent,
}

/// One prior message; authors are never coerced into the receiver's assistant role.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMessage {
    /// Canonical signed event id, lowercase hexadecimal.
    pub message_id: String,
    /// Canonical event timestamp.
    pub created_at: DateTime<Utc>,
    /// Author's verified public key.
    pub author_public_key: String,
    /// Employee identity when the key belongs to a company employee.
    pub author_employee_id: Option<EmployeeId>,
    /// Source display name; a missing profile may use the public key.
    pub author_name: String,
    /// Canonical parent, when present.
    pub parent_message_id: Option<String>,
    /// Canonical containing thread root, when present.
    pub thread_root_message_id: Option<String>,
    /// Bounded untrusted text, never instructions or authorization.
    pub content: String,
    /// SHA-256 of the complete canonical source text for fresh deletion/edit checks.
    pub source_content_hash: String,
    /// Whether this text is only a prefix of the canonical message.
    pub truncated: bool,
    /// Server selection reason.
    pub selection: ContextSelection,
}

/// Frozen ordinary Office context, selected by the control plane before start.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationContext {
    /// Closed wire version.
    pub version: u8,
    /// Run id owning the frozen selection.
    pub snapshot_id: Uuid,
    /// Exact plaintext Office channel; never an encrypted DM scope.
    pub channel_id: Uuid,
    /// Current request, excluded from prior messages.
    pub trigger_message_id: String,
    /// Explicit containing thread; absent for a new channel request.
    pub thread_root_message_id: Option<String>,
    /// Server receipt-time upper bound for prior source selection.
    pub cutoff_received_at: DateTime<Utc>,
    /// The receiving employee's pinned role facts.
    pub employee: ContextEmployee,
    /// Currently visible company/channel colleagues.
    pub teammates: Vec<ContextEmployee>,
    /// Selected sources in stable chronological order.
    pub messages: Vec<ContextMessage>,
    /// Earlier messages were omitted due to count or byte budgets.
    pub omitted_history: bool,
}

impl ConversationContext {
    /// Checks closed wire bounds and same-run attribution, not database access.
    pub fn valid_for(&self, run: Uuid, employee: &EmployeeId, revision: Uuid) -> bool {
        if self.version != 1
            || self.snapshot_id != run
            || run.is_nil()
            || self.channel_id.is_nil()
            || !event_id(&self.trigger_message_id)
            || !optional_event_id(&self.thread_root_message_id)
            || self.employee.employee_id != *employee
            || self.employee.revision_id != revision
            || !self.employee.valid()
            || self.teammates.len() > MAX_TEAMMATES
            || self.messages.len() > MAX_MESSAGES
        {
            return false;
        }
        let mut team_ids = BTreeSet::from([employee.clone()]);
        if self
            .teammates
            .iter()
            .any(|e| !e.valid() || !team_ids.insert(e.employee_id.clone()))
        {
            return false;
        }
        let mut message_ids = BTreeSet::from([self.trigger_message_id.as_str()]);
        let mut bytes = 0;
        let mut previous = None;
        for m in &self.messages {
            let order = (m.created_at, m.message_id.as_str());
            bytes += m.content.len();
            if !event_id(&m.source_content_hash)
                || !event_id(&m.message_id)
                || !message_ids.insert(&m.message_id)
                || !event_id(&m.author_public_key)
                || !text(&m.author_name, 200, false)
                || !optional_event_id(&m.parent_message_id)
                || !optional_event_id(&m.thread_root_message_id)
                || !text(&m.content, MAX_MESSAGE_BYTES, false)
                || previous.is_some_and(|p| p >= order)
                || match &self.thread_root_message_id {
                    Some(root) => {
                        m.selection == ContextSelection::ChannelRecent
                            || (m.message_id != *root
                                && m.thread_root_message_id.as_ref() != Some(root))
                    }
                    None => m.selection != ContextSelection::ChannelRecent,
                }
            {
                return false;
            }
            previous = Some(order);
        }
        bytes <= MAX_HISTORY_BYTES
            && serde_json::to_vec(self).is_ok_and(|v| v.len() <= MAX_CONTEXT_BYTES)
    }
}

fn text(value: &str, limit: usize, empty: bool) -> bool {
    (empty || !value.trim().is_empty())
        && value.len() <= limit
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

fn event_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn optional_event_id(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(event_id)
}

#[cfg(test)]
mod tests;
