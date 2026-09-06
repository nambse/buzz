use super::{WorkspaceGrant, WorkspaceResult, WorkspaceToolAck, WorkspaceToolRequest};
use crate::runtime::RuntimeError;
use uuid::Uuid;

/// Actual selected-store verification returned only by the configured adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedWorkspace {
    /// Run owning the immutable copy and any recoverable partial preparation.
    pub run_id: Uuid,
    /// Exact selected manifest digest verified by actual bounded I/O.
    pub manifest_hash: String,
    /// Adapter-owned opaque recovery identity, never a host path.
    pub store_ref: String,
}

/// Isolated, bounded immutable input I/O. Implementations must use safe opens,
/// verify owner/type/link/hash/size, and stop/reap their owned reader on every
/// interruption before returning. No caller/model string selects a host root.
#[allow(async_fn_in_trait)]
pub trait WorkspaceAdapter {
    /// Exact owned process identity, or None for adapters with no child process.
    fn reader_identity(&self) -> Option<WorkspaceReaderIdentity> {
        None
    }
    /// Observed preparation. A process adapter overrides this to record its PID
    /// before delivering stdin; the default owns no separate process.
    async fn prepare_observed<O: WorkspaceExecutionObserver>(
        &self,
        grant: &WorkspaceGrant,
        run_id: Uuid,
        observer: &O,
    ) -> Result<PreparedWorkspace, RuntimeError> {
        observer.started(None).await?;
        let prepared = self.prepare(grant, run_id).await?;
        observer.stopped().await?;
        Ok(prepared)
    }
    /// Observed read with the same durable lifecycle contract.
    async fn read_observed<O: WorkspaceExecutionObserver>(
        &self,
        grant: &WorkspaceGrant,
        prepared: &PreparedWorkspace,
        request: &WorkspaceToolRequest,
        observer: &O,
    ) -> Result<WorkspaceResult, RuntimeError> {
        observer.started(None).await?;
        let result = self.read(grant, prepared, request).await?;
        observer.stopped().await?;
        Ok(result)
    }

    /// Verify an operator-prepared immutable revision using actual file I/O.
    async fn verify(&self, grant: &WorkspaceGrant) -> Result<(), RuntimeError>;
    /// Create or verify the exact per-run immutable copy, idempotently by run.
    async fn prepare(
        &self,
        grant: &WorkspaceGrant,
        run_id: Uuid,
    ) -> Result<PreparedWorkspace, RuntimeError>;
    /// Read only the selected file from that run's previously prepared copy.
    async fn read(
        &self,
        grant: &WorkspaceGrant,
        prepared: &PreparedWorkspace,
        request: &WorkspaceToolRequest,
    ) -> Result<WorkspaceResult, RuntimeError>;
}

/// Bounded central-worker pull/resolve transport on the authenticated bridge.
#[allow(async_fn_in_trait)]
pub trait WorkspaceToolPort {
    /// Reads at most the single journal-reserved pending request for this run.
    async fn pending_workspace_tool(
        &self,
        idempotency_key: &str,
        grant: &WorkspaceGrant,
    ) -> Result<Option<WorkspaceToolRequest>, RuntimeError>;
    /// Resolves exactly one call. Identical replay is acknowledgement recovery,
    /// including after terminal state; changed or new late results are refused.
    async fn resolve_workspace_tool(
        &self,
        idempotency_key: &str,
        grant: &WorkspaceGrant,
        request: &WorkspaceToolRequest,
        result: &WorkspaceResult,
    ) -> Result<WorkspaceToolAck, RuntimeError>;
}

/// Exact configured owned reader executable; private control-plane metadata.
#[derive(Clone, Eq, PartialEq)]
pub struct WorkspaceReaderIdentity {
    /// Absolute pinned executable path, never included in tool/model arguments.
    pub executable: String,
    /// Exact artifact SHA-256.
    pub sha256: String,
    /// Expected local operating-system owner.
    pub uid: u32,
}
impl std::fmt::Debug for WorkspaceReaderIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceReaderIdentity")
            .field("sha256", &self.sha256)
            .field("uid", &self.uid)
            .finish_non_exhaustive()
    }
}

/// Durable lifecycle witness for one already reserved reader execution.
/// The process must wait for `started` to commit before receiving input roots.
#[allow(async_fn_in_trait)]
pub trait WorkspaceExecutionObserver {
    /// Unique durable execution marker; the only non-program argv value.
    fn execution_token(&self) -> Uuid;
    /// Pins the actual child PID before any filesystem input is delivered.
    /// None is reserved for adapters with no separately spawned process.
    async fn started(&self, pid: Option<u32>) -> Result<(), RuntimeError>;
    /// Call only after wait/reap, or after an in-process operation has returned
    /// without any owned asynchronous resource remaining.
    async fn stopped(&self) -> Result<(), RuntimeError>;
}
