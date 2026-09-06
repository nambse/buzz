use super::RunWorkspace;
use crate::postgres::{invalid, workspace_tools as store};
use crate::{DispatchAuthority, Result};
use ortak_control::outbox::OutboxLease;
use ortak_control::workspace::{
    empty_policy, workspace_read_policy, WorkspaceAdapter, WorkspaceExecutionObserver,
    WorkspaceFailure, WorkspaceGrant, WorkspaceResult, WorkspaceToolPort,
};
use ortak_control::{CompanyScope, PgControlPlane};
use std::time::Duration;
use uuid::Uuid;

/// One explicit selected adapter and immutable grant allowlist. No ambient
/// filesystem or provider configuration is discovered by this composition.
#[derive(Clone, Debug)]
pub struct ConfiguredRunWorkspace<A> {
    control: PgControlPlane,
    adapter: A,
    grants: Vec<WorkspaceGrant>,
    cursor: std::sync::Arc<std::sync::Mutex<Option<Uuid>>>,
}

/// Result of a bounded worker step; contains no selected input bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceStep {
    /// No pending journal request or due retained receipt.
    Idle,
    /// Exactly one result was acknowledged or safely interrupted.
    Settled,
    /// A retry remains durably scheduled.
    Retry,
    /// No new result was delivered; durable cancellation/ownership recovery remains.
    RecoveryPending,
}

impl<A: WorkspaceAdapter> ConfiguredRunWorkspace<A> {
    /// Validates a finite explicit allowlist before any adapter/file I/O.
    pub fn new(
        control: PgControlPlane,
        adapter: A,
        scope: &CompanyScope,
        grants: Vec<WorkspaceGrant>,
    ) -> Result<Self> {
        if grants.is_empty() || grants.len() > 16 {
            return Err(invalid("invalid workspace selection count".into()));
        }
        let mut selections = std::collections::BTreeSet::new();
        for grant in &grants {
            grant.validate()?;
            if grant.company_id != scope.company_id()
                || !selections.insert((
                    grant.project_id,
                    grant.employee_id.clone(),
                    grant.workspace_ref.clone(),
                ))
            {
                return Err(invalid(
                    "workspace selection scope or uniqueness differs".into(),
                ));
            }
        }
        Ok(Self {
            control,
            adapter,
            grants,
            cursor: Default::default(),
        })
    }

    /// Explicit selected input revisions; metadata only, used to stop old runs
    /// when their current configured reader selection has been removed.
    pub fn selected_revisions(&self) -> Vec<Uuid> {
        self.grants.iter().map(|g| g.revision).collect()
    }

    /// Performs actual adapter verification before the registry publication.
    /// This explicit operator action never imports an unselected existing path.
    pub async fn register(
        &self,
        scope: &CompanyScope,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        for grant in &self.grants {
            self.adapter.verify(grant).await?;
            store::register(&self.control, scope, grant, expires_at).await?;
        }
        Ok(())
    }

    /// Handles at most one selected run and one tool call. It first retries a
    /// retained result; otherwise reads one pending request and claims it before
    /// I/O. Transport errors retain the action with bounded retry/backoff.
    pub async fn step<P: WorkspaceToolPort>(
        &self,
        scope: &CompanyScope,
        port: &P,
    ) -> Result<WorkspaceStep> {
        let after = *self
            .cursor
            .lock()
            .map_err(|_| invalid("workspace scan cursor unavailable".into()))?;
        let Some(run) = store::next_run(&self.control, scope, after).await? else {
            return Ok(WorkspaceStep::Idle);
        };
        *self
            .cursor
            .lock()
            .map_err(|_| invalid("workspace scan cursor unavailable".into()))? = Some(run.run_id);
        let key = crate::run_idempotency_key(scope.company_id(), run.run_id);
        let Some(grant) = self
            .grants
            .iter()
            .find(|g| g.revision == run.grant.revision && *g == &run.grant)
        else {
            store::interrupt_run(&self.control, scope, run.run_id).await?;
            return Ok(WorkspaceStep::RecoveryPending);
        };
        let request = if let Some(request) = run.request {
            request
        } else {
            let Some(request) = port.pending_workspace_tool(&key, grant).await? else {
                return Ok(WorkspaceStep::Idle);
            };
            request
        };
        let Some(claim) = store::claim(&self.control, scope, run.run_id, &request).await? else {
            return Ok(WorkspaceStep::Idle);
        };
        let result = match claim.result {
            Some(result) => result,
            None => {
                // The adapter owns kill/reap on cancellation. Do not drop a
                // blocking read future behind timeout and pretend it stopped.
                let observer = store::plan_reader(
                    &self.control,
                    scope,
                    run.run_id,
                    grant,
                    &format!("read:{}", request.call_id),
                    claim.lease.token,
                    self.adapter.reader_identity(),
                )
                .await?;
                let result = match self
                    .adapter
                    .read_observed(grant, &claim.prepared, &request, &observer)
                    .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        if !store::reader_stopped(&self.control, scope, observer.execution_token())
                            .await?
                        {
                            return Err(error.into());
                        }
                        WorkspaceResult::Failed {
                            code: WorkspaceFailure::WorkspaceUnavailable,
                        }
                    }
                };
                result.validate(grant, &request)?;
                if !store::record(&self.control, scope, &claim.lease, &result).await? {
                    return Ok(WorkspaceStep::RecoveryPending);
                }
                result
            }
        };
        if !store::delivery_current(&self.control, scope, &claim.lease).await? {
            return Ok(WorkspaceStep::RecoveryPending);
        }
        match tokio::time::timeout(
            Duration::from_secs(2),
            port.resolve_workspace_tool(&key, grant, &request, &result),
        )
        .await
        {
            Ok(Ok(_ack)) => {
                store::acknowledge(&self.control, scope, &claim.lease).await?;
                Ok(WorkspaceStep::Settled)
            }
            Ok(Err(_)) | Err(_) => {
                store::retry(&self.control, scope, &claim.lease).await?;
                Ok(WorkspaceStep::Retry)
            }
        }
    }
}

impl<A: WorkspaceAdapter> RunWorkspace for ConfiguredRunWorkspace<A> {
    async fn prepare(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        authority: &DispatchAuthority,
        run_id: Uuid,
    ) -> Result<Option<WorkspaceGrant>> {
        if empty_policy(authority.permissions()) {
            return Ok(None);
        }
        let work = authority
            .work_origin()
            .ok_or_else(|| invalid("workspace read requires Work origin".into()))?;
        let grant = self
            .grants
            .iter()
            .find(|g| {
                g.company_id == scope.company_id()
                    && g.project_id == work.project_id
                    && &g.employee_id == authority.employee_id()
                    && authority.binding().workspace_ref == g.workspace_ref
                    && workspace_read_policy(authority.permissions(), &g.workspace_ref)
            })
            .ok_or_else(|| invalid("workspace policy is not explicitly configured".into()))?;
        store::preflight(&self.control, scope, lease, authority, grant).await?;
        let observer = store::plan_reader(
            &self.control,
            scope,
            run_id,
            grant,
            "prepare",
            lease.lease_token,
            self.adapter.reader_identity(),
        )
        .await?;
        let prepared = self
            .adapter
            .prepare_observed(grant, run_id, &observer)
            .await?;
        store::freeze(&self.control, scope, lease, authority, grant, &prepared).await?;
        Ok(Some(grant.clone()))
    }
}
