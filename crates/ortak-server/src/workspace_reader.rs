//! Bounded descriptor-based immutable text reader, invoked only by the selected
//! central worker. It has no network, credential or subprocess operations.

use ortak_control::workspace::{WorkspaceGrant, WorkspaceResult, WorkspaceToolRequest};
use rustix::fs::{self, FileType, Mode, OFlags};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::path::{Component, Path};
use uuid::Uuid;

mod files;
use files::*;

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

/// Execute one stdin-selected operation. All errors are static and contain no
/// paths, file content, credentials or OS exception text.
pub fn execute(request: ReaderRequest) -> Result<ReaderResponse, &'static str> {
    request.grant.validate().map_err(|_| "invalid_selection")?;
    let input_path = Path::new(&request.input_root);
    let run_path = Path::new(&request.run_root);
    if !input_path.is_absolute()
        || !run_path.is_absolute()
        || input_path.starts_with(run_path)
        || run_path.starts_with(input_path)
    {
        return Err("invalid_roots");
    }
    let input = open_root(input_path)?;
    let runs = open_root(run_path)?;
    marker(
        &input,
        ".ortak-workspace-inputs-v1",
        request.grant.company_id,
    )?;
    marker(&runs, ".ortak-workspace-runs-v1", request.grant.company_id)?;
    let grant = &request.grant;
    match request.action {
        ReaderAction::Verify {} => {
            let revision = directory(&input, &grant.revision.to_string())?;
            for file in &grant.files {
                read_selected(&revision, file)?;
            }
            Ok(ReaderResponse::Verified {})
        }
        ReaderAction::Prepare { run_id } => {
            if run_id.is_nil() {
                return Err("invalid_run");
            }
            let company = ensure_directory(&runs, &grant.company_id.to_string())?;
            // Stable retained lock identity, bounded nonblocking acquisition.
            let lock = lock_run(&company, run_id)?;
            let final_name = run_id.to_string();
            match fs::openat(
                &company,
                &final_name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            ) {
                Ok(fd) => {
                    // A crash after rename but before chmod leaves an owned
                    // 0700 final directory. Only an exact verified copy may
                    // finish sealing; a reader still requires 0500.
                    check_directory(&fd, false)?;
                    verify_run(&fd, grant)?;
                    fs::fchmod(&fd, Mode::RUSR | Mode::XUSR).map_err(|_| "store_unavailable")?;
                    fs::fsync(&fd).map_err(|_| "store_unavailable")?;
                    fs::fsync(&company).map_err(|_| "store_unavailable")?;
                }
                Err(rustix::io::Errno::NOENT) => {
                    let stage_name = format!("{run_id}.preparing");
                    let stage = ensure_directory(&company, &stage_name)?;
                    let revision = directory(&input, &grant.revision.to_string())?;
                    for file in &grant.files {
                        let bytes = read_selected(&revision, file)?;
                        write_once(&stage, &file.file_id.to_string(), &bytes)?;
                    }
                    let wire = serde_json::to_value(grant).map_err(|_| "invalid_selection")?;
                    let bytes = serde_json::to_vec(&wire).map_err(|_| "invalid_selection")?;
                    write_once(&stage, "manifest.json", &bytes)?;
                    verify_run(&stage, grant)?;
                    // Darwin requires a writable source directory for rename.
                    // The private parent and retained flock own this whole
                    // publish. No read accepts it until the final seal below.
                    fs::fchmod(&stage, Mode::RUSR | Mode::WUSR | Mode::XUSR)
                        .map_err(|_| "store_unavailable")?;
                    fs::fsync(&stage).map_err(|_| "store_unavailable")?;
                    fs::renameat_with(
                        &company,
                        &stage_name,
                        &company,
                        &final_name,
                        fs::RenameFlags::NOREPLACE,
                    )
                    .map_err(|_| "store_unavailable")?;
                    fs::fchmod(&stage, Mode::RUSR | Mode::XUSR).map_err(|_| "store_unavailable")?;
                    fs::fsync(&stage).map_err(|_| "store_unavailable")?;
                    fs::fsync(&company).map_err(|_| "store_unavailable")?;
                }
                Err(_) => return Err("store_unavailable"),
            }
            drop(lock);
            Ok(ReaderResponse::Prepared {
                run_id,
                manifest_hash: grant.manifest_hash.clone(),
                store_ref: format!("workspace-run:{}:{run_id}", grant.company_id),
            })
        }
        ReaderAction::Read { run_id, request } => {
            request.validate(grant).map_err(|_| "invalid_call")?;
            let company = directory(&runs, &grant.company_id.to_string())?;
            let run = directory(&company, &run_id.to_string())?;
            check_directory(&run, true)?;
            verify_manifest(&run, grant)?;
            let file = grant.file(request.file_id).map_err(|_| "invalid_call")?;
            let bytes = read_selected(&run, file)?;
            let content = String::from_utf8(bytes).map_err(|_| "input_changed")?;
            let result = WorkspaceResult::Completed {
                content,
                sha256: file.sha256.clone(),
                bytes: file.bytes,
                name: file.name.clone(),
            };
            result
                .validate(grant, &request)
                .map_err(|_| "input_changed")?;
            Ok(ReaderResponse::Read { result })
        }
    }
}

fn open_root(path: &Path) -> Result<OwnedFd, &'static str> {
    let mut fd = fs::open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "root_unavailable")?;
    for component in path.components() {
        match component {
            Component::RootDir => (),
            Component::Normal(name) => {
                fd = fs::openat(
                    &fd,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|_| "root_unavailable")?;
                let stat = fs::fstat(&fd).map_err(|_| "root_unavailable")?;
                let current = rustix::process::getuid().as_raw();
                if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
                    || !(stat.st_uid == current || stat.st_uid == 0)
                    || (stat.st_mode & 0o022 != 0
                        && !(stat.st_uid == 0 && stat.st_mode & 0o1000 != 0))
                {
                    return Err("root_unavailable");
                }
            }
            _ => return Err("invalid_roots"),
        }
    }
    check_directory(&fd, false)?;
    let stat = fs::fstat(&fd).map_err(|_| "root_unavailable")?;
    if stat.st_mode & 0o777 != 0o700 {
        return Err("root_unavailable");
    }
    Ok(fd)
}

fn marker(root: &OwnedFd, name: &str, company: Uuid) -> Result<(), &'static str> {
    let bytes = read_private(root, name, 128)?;
    if bytes != format!("ortak-workspace/v1:{company}\n").as_bytes() {
        return Err("root_marker_differs");
    }
    Ok(())
}

/// Reader entry point: one bounded stdin document and one bounded JSON result.
/// A failure prints only a closed code, never a deserialization/OS exception.
pub fn main() -> Result<(), &'static str> {
    // A dedicated thread exits the entire process even when the main reader
    // blocks in filesystem I/O. Parent wait/reap or later absence is still
    // required; elapsed watchdog time alone is never a containment receipt.
    std::thread::Builder::new()
        .name("workspace-deadline".into())
        .spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(8));
            std::process::exit(124);
        })
        .map_err(|_| "watchdog_unavailable")?;
    let args: Vec<_> = std::env::args().take(3).collect();
    if args.len() != 2 {
        return Err("execution_identity_required");
    }
    let token = args[1]
        .strip_prefix("--ortak-workspace-child=")
        .and_then(|v| Uuid::parse_str(v).ok())
        .filter(|v| !v.is_nil() && args[1] == format!("--ortak-workspace-child={v}"))
        .ok_or("execution_identity_required")?;

    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|_| "input_unavailable")?;
    if bytes.len() > 32768 {
        return Err("input_too_large");
    }
    let request: ReaderRequest = serde_json::from_slice(&bytes).map_err(|_| "invalid_request")?;
    if request.execution_token != token {
        return Err("execution_identity_differs");
    }
    let result = execute(request)?;
    let bytes = serde_json::to_vec(&result).map_err(|_| "result_unavailable")?;
    if bytes.len() > 131072 {
        return Err("result_too_large");
    }
    std::io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|_| "result_unavailable")?;
    Ok(())
}
