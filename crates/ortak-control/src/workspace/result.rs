use super::{digest, invalid, valid_hash, WorkspaceGrant};
use crate::runtime::RuntimeError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Exact journal-reserved request. No model-selected path or command exists.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceToolRequest {
    /// Bounded provider call identity within the stable start key.
    pub call_id: String,
    /// Selected immutable input identity.
    pub file_id: Uuid,
    /// SHA-256 of canonical one-key file_id argument JSON.
    pub arguments_hash: String,
    /// Durable call budget position, from one through four.
    pub ordinal: u8,
}
impl WorkspaceToolRequest {
    /// Computes the only accepted argument digest.
    pub fn hash_arguments(file_id: Uuid) -> String {
        digest(format!("{{\"file_id\":\"{file_id}\"}}").as_bytes())
    }
    /// Validates the journal request before any selected file is opened.
    pub fn validate(&self, grant: &WorkspaceGrant) -> Result<(), RuntimeError> {
        grant.validate()?;
        if self.call_id.is_empty()
            || self.call_id.len() > 128
            || !self.call_id.as_bytes()[0].is_ascii_alphanumeric()
            || !self
                .call_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
            || !(1..=4).contains(&self.ordinal)
            || !valid_hash(&self.arguments_hash)
            || self.arguments_hash != Self::hash_arguments(self.file_id)
        {
            return Err(invalid());
        }
        grant.file(self.file_id)?;
        Ok(())
    }
}

/// Closed failure vocabulary; no raw filesystem/provider error reaches clients.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceFailure {
    /// Current Work, employee, source or workspace permission changed.
    AuthorityChanged,
    /// The explicitly configured isolated input store is unavailable.
    WorkspaceUnavailable,
    /// A selected file is absent or cannot be safely opened.
    FileUnavailable,
    /// Exact selected bytes, size, name or digest changed.
    InputChanged,
    /// The bounded read or run deadline was reached.
    DeadlineExceeded,
    /// A durable cancellation prevents delivery.
    Cancelled,
}

/// Private exact tool result retained for same-key acknowledgement recovery.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceResult {
    /// Exact immutable selected UTF-8 bytes; never a public Activity payload.
    Completed {
        /// Exact text, excluding NUL.
        content: String,
        /// SHA-256 of content UTF-8 bytes.
        sha256: String,
        /// Exact content byte count.
        bytes: u32,
        /// Logical manifest name.
        name: String,
    },
    /// Bounded failure; contains no exception text or input bytes.
    Failed {
        /// Closed reason.
        code: WorkspaceFailure,
    },
}
impl std::fmt::Debug for WorkspaceResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Completed { bytes, .. } => f
                .debug_struct("Completed")
                .field("bytes", bytes)
                .finish_non_exhaustive(),
            Self::Failed { code } => f.debug_struct("Failed").field("code", code).finish(),
        }
    }
}
impl WorkspaceResult {
    /// Checks exact bytes against the selected manifest before commit/delivery.
    pub fn validate(
        &self,
        grant: &WorkspaceGrant,
        request: &WorkspaceToolRequest,
    ) -> Result<(), RuntimeError> {
        request.validate(grant)?;
        if let Self::Completed {
            content,
            sha256,
            bytes,
            name,
        } = self
        {
            let file = grant.file(request.file_id)?;
            if content.contains('\0')
                || content.len() != *bytes as usize
                || *bytes != file.bytes
                || name != &file.name
                || sha256 != &file.sha256
                || digest(content.as_bytes()) != *sha256
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    /// Canonical private receipt bytes; bounds must have been checked first.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, RuntimeError> {
        let value = serde_json::to_value(self).map_err(|_| invalid())?;
        serde_json::to_vec(&value).map_err(|_| invalid())
    }
}

/// Explicit bridge acknowledgement; an identical retry never redelivers bytes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceToolAck {
    /// True only after exact receipt retention in the owning bridge journal.
    pub acknowledged: bool,
    /// Exact request call identity.
    pub call_id: String,
    /// Exact request argument digest.
    pub arguments_hash: String,
}
