//! Selected immutable text inputs and the bounded runtime tool port.
//!
//! References and file IDs are logical identities. They never select a host
//! path. A grant is a snapshot; current authorization belongs to the durable
//! Work admission and tool-result transaction.

use ortak_domain::{EmployeeId, PermissionPolicy, ToolCapability};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{adapter::Detail, runtime::RuntimeError};

mod ports;
mod result;
pub use ports::{
    PreparedWorkspace, WorkspaceAdapter, WorkspaceExecutionObserver, WorkspaceReaderIdentity,
    WorkspaceToolPort,
};
pub use result::{WorkspaceFailure, WorkspaceResult, WorkspaceToolAck, WorkspaceToolRequest};

/// Maximum selected inputs in one immutable workspace revision.
pub const MAX_WORKSPACE_FILES: usize = 8;
/// Maximum UTF-8 bytes in one input and one completed read.
pub const MAX_WORKSPACE_FILE_BYTES: usize = 16 * 1024;
/// Maximum combined bytes across a selected input manifest.
pub const MAX_WORKSPACE_TOTAL_BYTES: usize = 64 * 1024;
/// Exact grant format understood by the selected Hermes tool executor.
pub const WORKSPACE_FORMAT: &str = "ortak-workspace-read/v1";

/// One explicitly selected text file; the name is display metadata only.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFile {
    /// Opaque identity accepted by `read_workspace_text`.
    pub file_id: Uuid,
    /// Bounded relative logical name, never used as a filesystem path.
    pub name: String,
    /// Currently exactly `text/plain`.
    pub media_type: String,
    /// Exact UTF-8 size.
    pub bytes: u32,
    /// Lowercase SHA-256 of exact input bytes.
    pub sha256: String,
}

/// Server-selected workspace snapshot sent alongside, never inside, RunSpec.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceGrant {
    /// Exact protocol format.
    pub format: String,
    /// Owning company.
    pub company_id: Uuid,
    /// Project whose current contribution/source permissions are required.
    pub project_id: Uuid,
    /// Only employee allowed to consume this selection.
    pub employee_id: EmployeeId,
    /// Opaque reference explicitly present in the employee permission policy.
    pub workspace_ref: String,
    /// Immutable input revision, independent of employee/model revision.
    pub revision: Uuid,
    /// SHA-256 of canonical sorted-key JSON excluding this field.
    pub manifest_hash: String,
    /// Files sorted by file ID, unique IDs and names.
    pub files: Vec<WorkspaceFile>,
}

pub(crate) fn invalid() -> RuntimeError {
    RuntimeError::InvalidSpec {
        detail: Detail::new("invalid selected workspace contract"),
    }
}

/// Reject unsupported Hermes policy combinations before credentials/provider I/O.
/// Returns true only when the selected workspace-read capability is required.
pub fn validate_hermes_policy(
    binding: &ortak_domain::RuntimeBinding,
    policy: &PermissionPolicy,
) -> Result<bool, RuntimeError> {
    if empty_policy(policy) {
        return Ok(false);
    }
    if binding.adapter == "hermes" && workspace_read_policy(policy, &binding.workspace_ref) {
        return Ok(true);
    }
    Err(invalid())
}

/// True only for bounded ASCII opaque references; host path syntax is excluded.
pub fn valid_workspace_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        && value.as_bytes()[0].is_ascii_alphanumeric()
}

/// Whether a policy is the existing exact no-tools policy.
pub fn empty_policy(policy: &PermissionPolicy) -> bool {
    policy == &PermissionPolicy::default()
}

/// The one C2 policy shape; Files is a ceiling for the sole read-text operation.
pub fn workspace_read_policy(policy: &PermissionPolicy, reference: &str) -> bool {
    valid_workspace_reference(reference)
        && policy.allowed_tools == [ToolCapability::Files]
        && policy.allowed_workspaces == [reference]
        && policy.allowed_networks.is_empty()
        && policy.approval_required.is_empty()
}

pub(crate) fn digest(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}
pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl WorkspaceGrant {
    /// Computes the versioned manifest digest from the exact selected fields.
    pub fn compute_hash(&self) -> Result<String, RuntimeError> {
        let mut value = serde_json::to_value(self).map_err(|_| invalid())?;
        value
            .as_object_mut()
            .ok_or_else(invalid)?
            .remove("manifest_hash");
        // serde_json's default map is a BTreeMap: keys are recursively sorted.
        Ok(digest(&serde_json::to_vec(&value).map_err(|_| invalid())?))
    }

    /// Validates bounds, canonical ordering, names and the exact manifest hash.
    pub fn validate(&self) -> Result<(), RuntimeError> {
        if EmployeeId::parse(self.employee_id.as_str()).is_err()
            || self.format != WORKSPACE_FORMAT
            || self.company_id.is_nil()
            || self.project_id.is_nil()
            || self.revision.is_nil()
            || !valid_workspace_reference(&self.workspace_ref)
            || self.files.is_empty()
            || self.files.len() > MAX_WORKSPACE_FILES
            || !valid_hash(&self.manifest_hash)
            || self.compute_hash()? != self.manifest_hash
        {
            return Err(invalid());
        }
        let mut names = std::collections::BTreeSet::new();
        let mut total = 0usize;
        let mut previous = None;
        for file in &self.files {
            if file.file_id.is_nil()
                || previous.is_some_and(|id| id >= file.file_id)
                || !names.insert(&file.name)
                || file.name.len() > 256
                || file.name.is_empty()
                || !file.name.as_bytes()[0].is_ascii_alphanumeric()
                || file.name.split('/').any(|part| {
                    part.is_empty()
                        || part == "."
                        || part == ".."
                        || !part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                })
                || file.media_type != "text/plain"
                || file.bytes as usize > MAX_WORKSPACE_FILE_BYTES
                || !valid_hash(&file.sha256)
            {
                return Err(invalid());
            }
            total += file.bytes as usize;
            previous = Some(file.file_id);
        }
        if total > MAX_WORKSPACE_TOTAL_BYTES {
            return Err(invalid());
        }
        Ok(())
    }

    /// Finds only an explicitly selected file.
    pub fn file(&self, file_id: Uuid) -> Result<&WorkspaceFile, RuntimeError> {
        self.files
            .iter()
            .find(|f| f.file_id == file_id)
            .ok_or_else(invalid)
    }
}

#[cfg(test)]
mod tests;
