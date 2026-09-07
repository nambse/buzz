//! Signed Work → selected input → leased read/result → current-authority closure.
use super::*;
use crate::work::execution::fixture::{complete, employee_with_workspace, queue, ready};
use ortak_control::adapter::{HealthReport, ResourceOutcome};
use ortak_control::fakes::{FakeMemoryAdapter, FakeRuntimeAdapter};
use ortak_control::runtime::*;
use ortak_control::workspace::*;
use ortak_control::CompanyScope;
use ortak_domain::{EmployeeId, RuntimeBinding};
use ortak_runtime::workspace_tools::{ConfiguredRunWorkspace, WorkspaceStep};
use ortak_runtime::{DispatchOutcome, RunSupervisor, SupervisorConfig};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

#[derive(Clone, Default)]
struct Inputs {
    reads: Arc<AtomicUsize>,
    verified: Arc<AtomicUsize>,
    revoke_on_read: Arc<AtomicBool>,
    pool: Option<PgPool>,
    company: Uuid,
    project: Uuid,
    block: Arc<AtomicBool>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
impl WorkspaceAdapter for Inputs {
    async fn verify(&self, grant: &WorkspaceGrant) -> Result<(), RuntimeError> {
        grant.validate()?;
        self.verified.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn prepare(
        &self,
        grant: &WorkspaceGrant,
        run: Uuid,
    ) -> Result<PreparedWorkspace, RuntimeError> {
        Ok(PreparedWorkspace {
            run_id: run,
            manifest_hash: grant.manifest_hash.clone(),
            store_ref: format!("workspace-run:{}:{run}", grant.company_id),
        })
    }
    async fn read(
        &self,
        grant: &WorkspaceGrant,
        _: &PreparedWorkspace,
        request: &WorkspaceToolRequest,
    ) -> Result<WorkspaceResult, RuntimeError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        if self.block.load(Ordering::SeqCst) {
            self.release.notified().await;
        }
        if self.revoke_on_read.load(Ordering::SeqCst) {
            sqlx::query("UPDATE project_access_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND project_id=$2 AND role='owner'")
                .bind(self.company).bind(self.project).execute(self.pool.as_ref().unwrap()).await.unwrap();
        }
        let file = grant.file(request.file_id)?;
        Ok(WorkspaceResult::Completed {
            content: "Selected brief".into(),
            sha256: file.sha256.clone(),
            bytes: file.bytes,
            name: file.name.clone(),
        })
    }
}
#[derive(Clone)]
struct Runtime {
    inner: Arc<FakeRuntimeAdapter>,
    lose_start_ack: Arc<AtomicBool>,
    grants: Arc<std::sync::Mutex<Vec<WorkspaceGrant>>>,
}
impl RuntimeAdapter for Runtime {
    fn adapter_name(&self) -> &str {
        self.inner.adapter_name()
    }
    async fn probe_capabilities(&self) -> Result<RuntimeCapabilities, RuntimeError> {
        self.inner.probe_capabilities().await
    }
    async fn health(&self, b: &RuntimeBinding) -> Result<HealthReport, RuntimeError> {
        self.inner.health(b).await
    }
    async fn ensure_profile(
        &self,
        r: &RuntimeResourceRequest,
    ) -> Result<ResourceOutcome, RuntimeError> {
        self.inner.ensure_profile(r).await
    }
    async fn delete_created_profile(&self, r: &str, k: &str) -> Result<(), RuntimeError> {
        self.inner.delete_created_profile(r, k).await
    }
    async fn start_run(&self, s: &RunSpec) -> Result<RunStartReceipt, RuntimeError> {
        self.inner.start_run(s).await
    }
    async fn start_run_with_workspace(
        &self,
        s: &RunSpec,
        g: Option<&WorkspaceGrant>,
    ) -> Result<RunStartReceipt, RuntimeError> {
        let g = g.expect("supervisor must compose the frozen grant");
        g.validate()?;
        self.grants.lock().unwrap().push(g.clone());
        let receipt = self.inner.start_run(s).await?;
        if self.lose_start_ack.swap(false, Ordering::SeqCst) {
            return Err(RuntimeError::Unavailable {
                detail: ortak_control::adapter::Detail::new("controlled lost start response"),
            });
        }
        Ok(receipt)
    }
    async fn lookup_start(&self, k: &str) -> Result<Option<RunStartReceipt>, RuntimeError> {
        self.inner.lookup_start(k).await
    }
    async fn cancel_start(&self, k: &str, r: &str) -> Result<CancelStartReceipt, RuntimeError> {
        self.inner.cancel_start(k, r).await
    }
    async fn next_events(
        &self,
        r: &RuntimeRunRef,
        c: Option<&RuntimeCursor>,
        n: usize,
    ) -> Result<RuntimeEventBatch, RuntimeError> {
        self.inner.next_events(r, c, n).await
    }
    async fn cancel_run(&self, r: &RuntimeRunRef, s: &str) -> Result<CancelOutcome, RuntimeError> {
        self.inner.cancel_run(r, s).await
    }
}
#[derive(Clone)]
struct Port {
    deliveries: Arc<AtomicUsize>,
    request: WorkspaceToolRequest,
    fail_first: Arc<AtomicBool>,
    resolves: Arc<AtomicUsize>,
    acknowledged: Arc<AtomicBool>,
}
impl WorkspaceToolPort for Port {
    async fn pending_workspace_tool(
        &self,
        _: &str,
        _: &WorkspaceGrant,
    ) -> Result<Option<WorkspaceToolRequest>, RuntimeError> {
        Ok((!self.acknowledged.load(Ordering::SeqCst)).then(|| self.request.clone()))
    }
    async fn resolve_workspace_tool(
        &self,
        _: &str,
        g: &WorkspaceGrant,
        r: &WorkspaceToolRequest,
        result: &WorkspaceResult,
    ) -> Result<WorkspaceToolAck, RuntimeError> {
        result.validate(g, r)?;
        self.resolves.fetch_add(1, Ordering::SeqCst);
        if !self.acknowledged.swap(true, Ordering::SeqCst) {
            self.deliveries.fetch_add(1, Ordering::SeqCst);
        }

        if self.fail_first.swap(false, Ordering::SeqCst) {
            return Err(RuntimeError::Unavailable {
                detail: ortak_control::adapter::Detail::new("controlled lost response"),
            });
        }
        self.acknowledged.store(true, Ordering::SeqCst);
        Ok(WorkspaceToolAck {
            acknowledged: true,
            call_id: r.call_id.clone(),
            arguments_hash: r.arguments_hash.clone(),
        })
    }
}
struct Selected {
    f: Fixture,
    scope: CompanyScope,
    app: Router,
    item: Value,
    run: Uuid,
    grant: WorkspaceGrant,
    workspace: ConfiguredRunWorkspace<Inputs>,
    inputs: Inputs,
    port: Port,
    runtime: Runtime,
    memory: FakeMemoryAdapter,
    reference: RuntimeRunRef,
}
async fn selected() -> Selected {
    selected_with_lost_start(false).await
}
async fn selected_with_lost_start(lost_ack: bool) -> Selected {
    let f = Fixture::new().await;
    let employee = employee_with_workspace(&f, "input:brief").await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let (project, item) = ready(&f, &app).await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let file = Uuid::new_v4();
    let mut grant = WorkspaceGrant {
        format: WORKSPACE_FORMAT.into(),
        company_id: f.company,
        project_id: project,
        employee_id: EmployeeId::parse("cem").unwrap(),
        workspace_ref: "input:brief".into(),
        revision: Uuid::new_v4(),
        manifest_hash: String::new(),
        files: vec![WorkspaceFile {
            file_id: file,
            name: "brief.txt".into(),
            media_type: "text/plain".into(),
            bytes: 14,
            sha256: hex::encode(Sha256::digest(b"Selected brief")),
        }],
    };
    grant.manifest_hash = grant.compute_hash().unwrap();
    let inputs = Inputs {
        pool: Some(f.pool.clone()),
        company: f.company,
        project,
        ..Default::default()
    };
    let workspace = ConfiguredRunWorkspace::new(
        f.control.clone(),
        inputs.clone(),
        &scope,
        vec![grant.clone()],
    )
    .unwrap();
    workspace
        .register(&scope, chrono::Utc::now() + chrono::Duration::days(1))
        .await
        .unwrap();
    let (run, _) = queue(&f, &app, &item).await;
    let runtime = Runtime {
        inner: Arc::new(
            FakeRuntimeAdapter::new().with_existing_profile("fake://work-profile", true),
        ),
        lose_start_ack: Arc::new(AtomicBool::new(lost_ack)),
        grants: Arc::default(),
    };
    let memory = FakeMemoryAdapter::new().with_existing_binding(employee.memory.as_ref().unwrap());
    let lease = f
        .control
        .claim_runtime_dispatches(
            &scope,
            "fake-runtime",
            "workspace-fixture",
            Duration::from_secs(60),
            1,
        )
        .await
        .unwrap()
        .remove(0);
    let result = RunSupervisor::new(f.control.clone(), &runtime, SupervisorConfig::default())
        .with_memory(&memory)
        .with_workspace(workspace.clone())
        .dispatch(&scope, &lease)
        .await
        .unwrap();
    let reference = match result {
        DispatchOutcome::Started {
            runtime_run_ref, ..
        } if !lost_ack => runtime_run_ref,
        DispatchOutcome::RuntimeFailed { .. } if lost_ack => {
            runtime
                .lookup_start(&ortak_runtime::run_idempotency_key(f.company, run))
                .await
                .unwrap()
                .unwrap()
                .runtime_run_ref
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(runtime.grants.lock().unwrap().as_slice(), [grant.clone()]);
    let port = Port {
        deliveries: Arc::default(),
        request: WorkspaceToolRequest {
            call_id: "call_1".into(),
            file_id: file,
            arguments_hash: WorkspaceToolRequest::hash_arguments(file),
            ordinal: 1,
        },
        fail_first: Arc::new(AtomicBool::new(false)),
        resolves: Arc::default(),
        acknowledged: Arc::default(),
    };
    Selected {
        f,
        scope,
        app,
        item,
        run,
        grant,
        workspace,
        inputs,
        port,
        runtime,
        memory,
        reference,
    }
}

#[path = "workspace/tests.rs"]
mod tests;

#[path = "workspace/retention.rs"]
mod retention;
