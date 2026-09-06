//! Actual descriptor reader and spawned-process seam; only fresh synthetic files.
use chrono::Utc;
use ortak_control::workspace::*;
use ortak_domain::EmployeeId;
use ortak_server::worker_workspace_tools::{ProcessWorkspaceAdapter, WorkspaceConfig};
use ortak_server::workspace_reader::{execute, ReaderAction, ReaderRequest, ReaderResponse};
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
use uuid::Uuid;

struct Fixture {
    root: PathBuf,
    input: PathBuf,
    runs: PathBuf,
    grant: WorkspaceGrant,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("ortak-reader-fixture-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let input = root.join("inputs");
        let runs = root.join("runs");
        for path in [&input, &runs] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let company = Uuid::new_v4();
        private(
            input.join(".ortak-workspace-inputs-v1"),
            format!("ortak-workspace/v1:{company}\n").as_bytes(),
        );
        private(
            runs.join(".ortak-workspace-runs-v1"),
            format!("ortak-workspace/v1:{company}\n").as_bytes(),
        );
        let revision = Uuid::new_v4();
        let directory = input.join(revision.to_string());
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let file = Uuid::new_v4();
        private(
            directory.join(file.to_string()),
            b"Approved synthetic input",
        );
        let mut grant = WorkspaceGrant {
            format: WORKSPACE_FORMAT.into(),
            company_id: company,
            project_id: Uuid::new_v4(),
            employee_id: EmployeeId::parse("ada").unwrap(),
            workspace_ref: "input:brief".into(),
            revision,
            manifest_hash: String::new(),
            files: vec![WorkspaceFile {
                file_id: file,
                name: "brief.txt".into(),
                media_type: "text/plain".into(),
                bytes: 24,
                sha256: hex::encode(Sha256::digest(b"Approved synthetic input")),
            }],
        };
        grant.manifest_hash = grant.compute_hash().unwrap();
        grant.validate().unwrap();
        Self {
            root,
            input,
            runs,
            grant,
        }
    }
    fn request(&self, action: ReaderAction) -> ReaderRequest {
        ReaderRequest {
            execution_token: Uuid::new_v4(),
            input_root: self.input.to_str().unwrap().into(),
            run_root: self.runs.to_str().unwrap().into(),
            grant: self.grant.clone(),
            action,
        }
    }
    fn source(&self) -> PathBuf {
        self.input
            .join(self.grant.revision.to_string())
            .join(self.grant.files[0].file_id.to_string())
    }
    fn call(&self) -> WorkspaceToolRequest {
        let id = self.grant.files[0].file_id;
        WorkspaceToolRequest {
            call_id: "call_1".into(),
            file_id: id,
            arguments_hash: WorkspaceToolRequest::hash_arguments(id),
            ordinal: 1,
        }
    }
}
fn private(path: PathBuf, bytes: &[u8]) {
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o400)).unwrap();
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fn writable(path: &std::path::Path) {
            if fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
                if let Ok(entries) = fs::read_dir(path) {
                    for entry in entries.flatten() {
                        writable(&entry.path());
                    }
                }
            }
        }
        writable(&self.root);
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn immutable_run_copy_is_idempotent_and_survives_original_input_change() {
    let f = Fixture::new();
    let run = Uuid::new_v4();
    assert!(matches!(
        execute(f.request(ReaderAction::Verify {})).unwrap(),
        ReaderResponse::Verified {}
    ));
    for _ in 0..2 {
        assert!(
            matches!(execute(f.request(ReaderAction::Prepare {run_id:run})).unwrap(),ReaderResponse::Prepared {run_id,..} if run_id==run)
        );
    }
    fs::set_permissions(f.source(), fs::Permissions::from_mode(0o600)).unwrap();
    private(f.source(), b"Changed original source!");
    assert!(execute(f.request(ReaderAction::Verify {})).is_err());
    let ReaderResponse::Read { result } = execute(f.request(ReaderAction::Read {
        run_id: run,
        request: f.call(),
    }))
    .unwrap() else {
        panic!()
    };
    assert!(
        matches!(result,WorkspaceResult::Completed {content,..} if content=="Approved synthetic input")
    );
}

#[test]
fn prepare_finishes_only_an_exact_copy_after_interrupted_final_seal() {
    let f = Fixture::new();
    let run = Uuid::new_v4();
    execute(f.request(ReaderAction::Prepare { run_id: run })).unwrap();
    let final_dir = f
        .runs
        .join(f.grant.company_id.to_string())
        .join(run.to_string());
    // The durable rename happened but its final seal did not. Reads refuse it;
    // Prepare owns the same lock and may finish only after verifying all bytes.
    fs::set_permissions(&final_dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(execute(f.request(ReaderAction::Read {
        run_id: run,
        request: f.call()
    }))
    .is_err());
    execute(f.request(ReaderAction::Prepare { run_id: run })).unwrap();
    assert_eq!(
        fs::metadata(&final_dir).unwrap().permissions().mode() & 0o777,
        0o500
    );
    fs::set_permissions(&final_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let file = final_dir.join(f.grant.files[0].file_id.to_string());
    fs::remove_file(&file).unwrap();
    private(file, b"Different synthetic data");
    assert!(execute(f.request(ReaderAction::Prepare { run_id: run })).is_err());
    assert_eq!(
        fs::metadata(&final_dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn safe_reader_refuses_symlinks_hardlinks_unsealed_files_and_traversal() {
    let f = Fixture::new();
    let source = f.source();
    let other = f.root.join("outside");
    private(other.clone(), b"Approved synthetic input");
    fs::remove_file(&source).unwrap();
    std::os::unix::fs::symlink(&other, &source).unwrap();
    assert!(execute(f.request(ReaderAction::Verify {})).is_err());
    fs::remove_file(&source).unwrap();
    fs::hard_link(&other, &source).unwrap();
    assert!(execute(f.request(ReaderAction::Verify {})).is_err());
    fs::remove_file(&source).unwrap();
    private(source.clone(), b"Approved synthetic input");
    fs::set_permissions(&source, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(execute(f.request(ReaderAction::Verify {})).is_err());
    let mut request = f.request(ReaderAction::Verify {});
    request.grant.files[0].name = "../outside".into();
    request.grant.manifest_hash = request.grant.compute_hash().unwrap();
    assert!(execute(request).is_err());
    let linked = f.root.join("linked");
    std::os::unix::fs::symlink(&f.input, &linked).unwrap();
    let mut request = f.request(ReaderAction::Verify {});
    request.input_root = linked.to_str().unwrap().into();
    assert!(execute(request).is_err());
}

#[tokio::test]
async fn actual_pinned_reader_process_prepares_and_returns_exact_private_bytes() {
    let f = Fixture::new();
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_ortak-workspace-reader"));
    let bytes = fs::read(&binary).unwrap();
    let config = WorkspaceConfig {
        reader_binary: binary,
        reader_sha256: hex::encode(Sha256::digest(&bytes)),
        input_root: f.input.clone(),
        run_root: f.runs.clone(),
        grants: vec![f.grant.clone()],
        expires_at: Utc::now() + chrono::Duration::days(1),
        register_selected_inputs: false,
    };
    let adapter = ProcessWorkspaceAdapter::new(&config).unwrap();
    adapter.verify(&f.grant).await.unwrap();
    let prepared = adapter.prepare(&f.grant, Uuid::new_v4()).await.unwrap();
    let result = adapter.read(&f.grant, &prepared, &f.call()).await.unwrap();
    result.validate(&f.grant, &f.call()).unwrap();
    let mut changed = config.clone();
    changed.reader_sha256 = "0".repeat(64);
    assert!(ProcessWorkspaceAdapter::new(&changed).is_err());
}

struct Observer {
    token: Uuid,
    started: std::sync::atomic::AtomicUsize,
    stopped: std::sync::atomic::AtomicUsize,
}
impl Observer {
    fn new() -> Self {
        Self {
            token: Uuid::new_v4(),
            started: 0.into(),
            stopped: 0.into(),
        }
    }
}
impl WorkspaceExecutionObserver for Observer {
    fn execution_token(&self) -> Uuid {
        self.token
    }
    async fn started(&self, pid: Option<u32>) -> Result<(), ortak_control::runtime::RuntimeError> {
        assert!(pid.is_some());
        self.started
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    async fn stopped(&self) -> Result<(), ortak_control::runtime::RuntimeError> {
        self.stopped
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn actual_reader_failure_is_reaped_before_its_observer_claims_stopped() {
    let f = Fixture::new();
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_ortak-workspace-reader"));
    let config = WorkspaceConfig {
        reader_sha256: hex::encode(Sha256::digest(fs::read(&binary).unwrap())),
        reader_binary: binary,
        input_root: f.input.clone(),
        run_root: f.runs.clone(),
        grants: vec![f.grant.clone()],
        expires_at: Utc::now() + chrono::Duration::days(1),
        register_selected_inputs: false,
    };
    let adapter = ProcessWorkspaceAdapter::new(&config).unwrap();
    let observer = Observer::new();
    let run = Uuid::new_v4();
    let prepared = adapter
        .prepare_observed(&f.grant, run, &observer)
        .await
        .unwrap();
    let final_dir = f
        .runs
        .join(f.grant.company_id.to_string())
        .join(run.to_string());
    fs::set_permissions(&final_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let file = final_dir.join(f.grant.files[0].file_id.to_string());
    fs::remove_file(&file).unwrap();
    private(file, b"Changed synthetic bytes!");
    fs::set_permissions(&final_dir, fs::Permissions::from_mode(0o500)).unwrap();
    assert!(adapter
        .read_observed(&f.grant, &prepared, &f.call(), &observer)
        .await
        .is_err());
    assert_eq!(
        observer.started.load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    assert_eq!(
        observer.stopped.load(std::sync::atomic::Ordering::SeqCst),
        2
    );
}

#[tokio::test]
async fn actual_reader_watchdog_exits_even_while_stdin_never_reaches_eof() {
    let token = Uuid::new_v4();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_ortak-workspace-reader"))
        .arg(format!("--ortak-workspace-child={token}"))
        .env_clear()
        .current_dir("/")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let _held_stdin = child.stdin.take().unwrap();
    // Wait/reap is the proof. The test supplies no roots or input document.
    let status = tokio::time::timeout(std::time::Duration::from_secs(11), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.code(), Some(124));
}
