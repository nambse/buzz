//! Bounded immutable text reader with an explicit platform boundary.
//! The descriptor implementation requires Unix ownership and no-follow semantics.
use ortak_control::workspace::{WorkspaceGrant, WorkspaceResult, WorkspaceToolRequest};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::{execute, main};

/// Explicit operator roots and an exact server-derived operation; stdin only.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderRequest {
    /// Exact argv execution marker, checked before filesystem access.
    pub execution_token: Uuid,
    /// Private prepared inputs root with an exact company marker.
    pub input_root: String,
    /// Separate private immutable per-run root with an exact company marker.
    pub run_root: String,
    /// Exact selected manifest; display names never form host paths.
    pub grant: WorkspaceGrant,
    /// Closed command.
    pub action: ReaderAction,
}
/// Only verify, per-run copy and one selected immutable read are supported.
#[derive(Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReaderAction {
    /// Actual I/O verification before registry publication.
    Verify {},
    /// Idempotent per-run immutable copy.
    Prepare {
        /// Durable owning run.
        run_id: Uuid,
    },
    /// Read exactly one selected file from an already prepared run.
    Read {
        /// Durable owning run.
        run_id: Uuid,
        /// Exact reserved request.
        request: WorkspaceToolRequest,
    },
}
/// Private bounded result, never logged by the reader or central worker.
#[derive(Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReaderResponse {
    /// All selected inputs were actually verified.
    Verified {},
    /// The same run store exists and its exact manifest was verified.
    Prepared {
        /// Durable run.
        run_id: Uuid,
        /// Exact selected digest.
        manifest_hash: String,
        /// Opaque run recovery identity.
        store_ref: String,
    },
    /// Exact private selected text result.
    Read {
        /// File bytes with their bounded manifest metadata.
        result: WorkspaceResult,
    },
}

/// Unsupported hosts refuse before inspecting paths or reading stdin.
#[cfg(not(unix))]
pub fn execute(_request: ReaderRequest) -> Result<ReaderResponse, &'static str> {
    Err("unsupported_workspace_platform")
}

/// Unsupported hosts cannot start the Unix descriptor reader.
#[cfg(not(unix))]
pub fn main() -> Result<(), &'static str> {
    Err("unsupported_workspace_platform")
}
