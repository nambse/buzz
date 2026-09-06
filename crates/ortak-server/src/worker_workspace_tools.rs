//! Explicit worker-owned immutable input roots and bounded reader composition.
//! Configuration is operator-only; public requests contain no host paths.

use crate::workspace_reader::{ReaderAction, ReaderRequest, ReaderResponse};
use chrono::{DateTime, Utc};
use ortak_control::adapter::Detail;
use ortak_control::outbox::OutboxLease;
use ortak_control::runtime::RuntimeError;
use ortak_control::workspace::{
    empty_policy, PreparedWorkspace, WorkspaceAdapter, WorkspaceExecutionObserver, WorkspaceGrant,
    WorkspaceReaderIdentity, WorkspaceResult, WorkspaceToolPort, WorkspaceToolRequest,
};
use ortak_control::{CompanyScope, PgControlPlane};
use ortak_runtime::workspace_tools::{ConfiguredRunWorkspace, RunWorkspace, WorkspaceStep};
use ortak_runtime::{DispatchAuthority, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{io::Read, path::PathBuf, time::Duration};

mod recovery;
pub use recovery::recover_reader;

/// Explicit local process and finite selected input configuration.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceConfig {
    /// Absolute pinned reader artifact path; never resolved from PATH.
    pub reader_binary: PathBuf,
    /// Exact SHA-256 of the operator's selected reader artifact.
    pub reader_sha256: String,
    /// Dedicated private input root; requires an exact company marker.
    pub input_root: PathBuf,
    /// Separate dedicated private per-run store root.
    pub run_root: PathBuf,
    /// At most sixteen explicit employee/project/ref selections.
    pub grants: Vec<WorkspaceGrant>,
    /// Immutable expiry for a new registry publication.
    pub expires_at: DateTime<Utc>,
    /// Explicit operator publication; absent/false only uses existing registry.
    #[serde(default)]
    pub register_selected_inputs: bool,
}

/// Selected process adapter. Debug intentionally excludes all local paths.
#[derive(Clone)]
pub struct ProcessWorkspaceAdapter {
    binary: PathBuf,
    sha256: String,
    input_root: String,
    run_root: String,
}
impl std::fmt::Debug for ProcessWorkspaceAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessWorkspaceAdapter")
            .finish_non_exhaustive()
    }
}
fn invalid() -> RuntimeError {
    RuntimeError::InvalidSpec {
        detail: Detail::new("invalid selected workspace reader"),
    }
}
fn unavailable() -> RuntimeError {
    RuntimeError::Unavailable {
        detail: Detail::new("selected workspace reader unavailable"),
    }
}

fn verify_executable(
    binary: &std::path::Path,
    expected_hash: &str,
    uid: u32,
) -> std::result::Result<(), RuntimeError> {
    let fd = rustix::fs::open(
        binary,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| invalid())?;
    let stat = rustix::fs::fstat(&fd).map_err(|_| invalid())?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile
        || stat.st_nlink != 1
        || stat.st_uid != uid
        || stat.st_mode & 0o022 != 0
        || stat.st_mode & 0o100 == 0
        || stat.st_size < 1
        || stat.st_size > 268435456
    {
        return Err(invalid());
    }
    let mut file = std::fs::File::from(fd);
    let mut digest = Sha256::new();
    let mut bytes = [0u8; 65536];
    let mut count = 0usize;
    loop {
        let read = file.read(&mut bytes).map_err(|_| invalid())?;
        if read == 0 {
            break;
        }
        count += read;
        if count > 268435456 {
            return Err(invalid());
        }
        digest.update(&bytes[..read]);
    }
    if count as i64 != stat.st_size || hex::encode(digest.finalize()) != expected_hash {
        return Err(invalid());
    }
    Ok(())
}

impl ProcessWorkspaceAdapter {
    /// Verifies only an explicit pinned executable and root syntax. Input file
    /// ownership/hash verification is performed by the bounded child itself.
    pub fn new(config: &WorkspaceConfig) -> std::result::Result<Self, RuntimeError> {
        if !config.reader_binary.is_absolute()
            || !config.input_root.is_absolute()
            || !config.run_root.is_absolute()
            || config.input_root.starts_with(&config.run_root)
            || config.run_root.starts_with(&config.input_root)
            || [&config.reader_binary, &config.input_root, &config.run_root]
                .iter()
                .any(|p| {
                    p.components().any(|c| {
                        !matches!(
                            c,
                            std::path::Component::RootDir | std::path::Component::Normal(_)
                        )
                    })
                })
            || config.reader_sha256.len() != 64
        {
            return Err(invalid());
        }
        verify_executable(
            &config.reader_binary,
            &config.reader_sha256,
            rustix::process::getuid().as_raw(),
        )?;
        Ok(Self {
            binary: config.reader_binary.clone(),
            sha256: config.reader_sha256.clone(),
            input_root: config.input_root.to_str().ok_or_else(invalid)?.into(),
            run_root: config.run_root.to_str().ok_or_else(invalid)?.into(),
        })
    }

    async fn invoke<O: WorkspaceExecutionObserver>(
        &self,
        grant: &WorkspaceGrant,
        action: ReaderAction,
        observer: Option<&O>,
    ) -> std::result::Result<ReaderResponse, RuntimeError> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        grant.validate()?;
        let token = observer.map_or_else(uuid::Uuid::new_v4, |o| o.execution_token());
        let bytes = serde_json::to_vec(&ReaderRequest {
            execution_token: token,
            input_root: self.input_root.clone(),
            run_root: self.run_root.clone(),
            grant: grant.clone(),
            action,
        })
        .map_err(|_| invalid())?;
        if bytes.len() > 32768 {
            return Err(invalid());
        }
        let mut child = tokio::process::Command::new(&self.binary)
            .arg(format!("--ortak-workspace-child={token}"))
            .env_clear()
            .current_dir("/")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| unavailable())?;
        if let Some(observer) = observer {
            if let Err(error) = observer.started(child.id()).await {
                child.start_kill().map_err(|_| unavailable())?;
                tokio::time::timeout(Duration::from_secs(3), child.wait())
                    .await
                    .map_err(|_| unavailable())?
                    .map_err(|_| unavailable())?;
                observer.stopped().await?;
                return Err(error);
            }
        }
        let mut stdin = child.stdin.take().ok_or_else(unavailable)?;
        let stdout = child.stdout.take().ok_or_else(unavailable)?;
        let mut output = Vec::new();
        let operation = async {
            let send = async move {
                stdin.write_all(&bytes).await?;
                stdin.shutdown().await?;
                // Unix pipe shutdown does not close ChildStdin. Own and drop
                // it here so the bounded reader receives EOF before wait().
                drop(stdin);
                Ok::<_, std::io::Error>(())
            };
            let mut bounded_stdout = stdout.take(131073);
            let receive = bounded_stdout.read_to_end(&mut output);
            let (sent, read, status) = tokio::join!(send, receive, child.wait());
            sent.map_err(|_| unavailable())?;
            read.map_err(|_| unavailable())?;
            let status = status.map_err(|_| unavailable())?;
            if !status.success() || output.len() > 131072 {
                return Err(unavailable());
            }
            serde_json::from_slice(&output).map_err(|_| unavailable())
        };
        match tokio::time::timeout(Duration::from_secs(5), operation).await {
            Ok(Ok(response)) => {
                if let Some(observer) = observer {
                    observer.stopped().await?;
                }
                Ok(response)
            }
            failed => {
                // A failed/expired invocation never masquerades as successful
                // containment. Kill and reap before returning; failure leaves
                // the durable owning run/action recoverable.
                child.start_kill().map_err(|_| unavailable())?;
                tokio::time::timeout(Duration::from_secs(3), child.wait())
                    .await
                    .map_err(|_| unavailable())?
                    .map_err(|_| unavailable())?;
                if let Some(observer) = observer {
                    observer.stopped().await?;
                }
                match failed {
                    Ok(Err(error)) => Err(error),
                    _ => Err(unavailable()),
                }
            }
        }
    }
}
struct NoObserver;
impl WorkspaceExecutionObserver for NoObserver {
    fn execution_token(&self) -> uuid::Uuid {
        uuid::Uuid::nil()
    }
    async fn started(&self, _: Option<u32>) -> std::result::Result<(), RuntimeError> {
        Ok(())
    }
    async fn stopped(&self) -> std::result::Result<(), RuntimeError> {
        Ok(())
    }
}
impl WorkspaceAdapter for ProcessWorkspaceAdapter {
    fn reader_identity(&self) -> Option<WorkspaceReaderIdentity> {
        Some(WorkspaceReaderIdentity {
            executable: self.binary.to_string_lossy().into(),
            sha256: self.sha256.clone(),
            uid: rustix::process::getuid().as_raw(),
        })
    }
    async fn prepare_observed<O: WorkspaceExecutionObserver>(
        &self,
        grant: &WorkspaceGrant,
        run_id: uuid::Uuid,
        observer: &O,
    ) -> std::result::Result<PreparedWorkspace, RuntimeError> {
        match self
            .invoke(grant, ReaderAction::Prepare { run_id }, Some(observer))
            .await?
        {
            ReaderResponse::Prepared {
                run_id: found,
                manifest_hash,
                store_ref,
            } if found == run_id
                && manifest_hash == grant.manifest_hash
                && store_ref == format!("workspace-run:{}:{run_id}", grant.company_id) =>
            {
                Ok(PreparedWorkspace {
                    run_id,
                    manifest_hash,
                    store_ref,
                })
            }
            _ => Err(invalid()),
        }
    }
    async fn read_observed<O: WorkspaceExecutionObserver>(
        &self,
        grant: &WorkspaceGrant,
        prepared: &PreparedWorkspace,
        request: &WorkspaceToolRequest,
        observer: &O,
    ) -> std::result::Result<WorkspaceResult, RuntimeError> {
        if prepared.manifest_hash != grant.manifest_hash
            || prepared.store_ref
                != format!("workspace-run:{}:{}", grant.company_id, prepared.run_id)
        {
            return Err(invalid());
        }
        request.validate(grant)?;
        match self
            .invoke(
                grant,
                ReaderAction::Read {
                    run_id: prepared.run_id,
                    request: request.clone(),
                },
                Some(observer),
            )
            .await?
        {
            ReaderResponse::Read { result } => {
                result.validate(grant, request)?;
                Ok(result)
            }
            _ => Err(invalid()),
        }
    }

    async fn verify(&self, grant: &WorkspaceGrant) -> std::result::Result<(), RuntimeError> {
        match self
            .invoke(grant, ReaderAction::Verify {}, None::<&NoObserver>)
            .await?
        {
            ReaderResponse::Verified {} => Ok(()),
            _ => Err(invalid()),
        }
    }
    async fn prepare(
        &self,
        grant: &WorkspaceGrant,
        run_id: uuid::Uuid,
    ) -> std::result::Result<PreparedWorkspace, RuntimeError> {
        match self
            .invoke(grant, ReaderAction::Prepare { run_id }, None::<&NoObserver>)
            .await?
        {
            ReaderResponse::Prepared {
                run_id: found,
                manifest_hash,
                store_ref,
            } if found == run_id
                && manifest_hash == grant.manifest_hash
                && store_ref == format!("workspace-run:{}:{run_id}", grant.company_id) =>
            {
                Ok(PreparedWorkspace {
                    run_id,
                    manifest_hash,
                    store_ref,
                })
            }
            _ => Err(invalid()),
        }
    }
    async fn read(
        &self,
        grant: &WorkspaceGrant,
        prepared: &PreparedWorkspace,
        request: &WorkspaceToolRequest,
    ) -> std::result::Result<WorkspaceResult, RuntimeError> {
        if prepared.manifest_hash != grant.manifest_hash
            || prepared.store_ref
                != format!("workspace-run:{}:{}", grant.company_id, prepared.run_id)
        {
            return Err(invalid());
        }
        request.validate(grant)?;
        match self
            .invoke(
                grant,
                ReaderAction::Read {
                    run_id: prepared.run_id,
                    request: request.clone(),
                },
                None::<&NoObserver>,
            )
            .await?
        {
            ReaderResponse::Read { result } => {
                result.validate(grant, request)?;
                Ok(result)
            }
            _ => Err(invalid()),
        }
    }
}

/// Optional worker composition; disabled/unavailable input stores do not grant
/// Files authority and do not block the established no-tools recovery path.
#[derive(Clone, Debug, Default)]
pub struct WorkerWorkspace(Option<ConfiguredRunWorkspace<ProcessWorkspaceAdapter>>);
impl WorkerWorkspace {
    /// Builds an explicit finite selection and optionally publishes verified
    /// registry rows. Publication is a configured operator action, never implicit.
    pub async fn new(
        control: PgControlPlane,
        scope: &CompanyScope,
        config: WorkspaceConfig,
    ) -> Result<Self> {
        let adapter = ProcessWorkspaceAdapter::new(&config)?;
        let selected = ConfiguredRunWorkspace::new(control, adapter, scope, config.grants)?;
        if config.register_selected_inputs {
            selected.register(scope, config.expires_at).await?;
        }
        Ok(Self(Some(selected)))
    }
    /// Current selected revisions, empty when the reader is disabled/unavailable.
    pub fn selected_revisions(&self) -> Vec<uuid::Uuid> {
        self.0
            .as_ref()
            .map_or_else(Vec::new, |s| s.selected_revisions())
    }

    /// At most one bounded call/result retry per worker cycle.
    pub async fn step<P: WorkspaceToolPort>(
        &self,
        scope: &CompanyScope,
        port: &P,
    ) -> Result<WorkspaceStep> {
        match &self.0 {
            Some(selected) => selected.step(scope, port).await,
            None => Ok(WorkspaceStep::Idle),
        }
    }
}
impl RunWorkspace for &WorkerWorkspace {
    async fn prepare(
        &self,
        scope: &CompanyScope,
        lease: &OutboxLease,
        authority: &DispatchAuthority,
        run_id: uuid::Uuid,
    ) -> Result<Option<WorkspaceGrant>> {
        match &self.0 {
            Some(selected) => selected.prepare(scope, lease, authority, run_id).await,
            None if empty_policy(authority.permissions()) => Ok(None),
            None => Err(invalid().into()),
        }
    }
}
