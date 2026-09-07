//! Selected Work workspace admission and one bounded central tool action.
//! Database authority is released before I/O, then derived again at commit.

use crate::{DispatchAuthority, Result};
use ortak_control::outbox::OutboxLease;
use ortak_control::workspace::WorkspaceGrant;
use ortak_control::CompanyScope;

mod driver;
pub use driver::{ConfiguredRunWorkspace, WorkspaceStep};

/// Optional selected workspace preparation alongside the frozen memory snapshot.
#[allow(async_fn_in_trait)]
pub trait RunWorkspace {
    /// Verify/copy exact inputs outside a transaction, then persist their use
    /// under the current dispatch lease. An absent grant means no tools.
    async fn prepare(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        authority: &DispatchAuthority,
        run_id: uuid::Uuid,
    ) -> Result<Option<WorkspaceGrant>>;
}

/// Compatibility path for adapters that do not implement selected workspaces.
/// The production Hermes adapter refuses a Files policy without an actual grant.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoRunWorkspace;
impl RunWorkspace for NoRunWorkspace {
    async fn prepare(
        &self,
        _scope: &CompanyScope,
        _lease: &OutboxLease,
        _authority: &DispatchAuthority,
        _run_id: uuid::Uuid,
    ) -> Result<Option<WorkspaceGrant>> {
        Ok(None)
    }
}
