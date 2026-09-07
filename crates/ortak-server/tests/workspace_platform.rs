#![cfg(not(unix))]
//! Unsupported local readers must refuse without weakening Unix ownership checks.
use chrono::Utc;
use ortak_control::workspace::{WorkspaceFile, WorkspaceGrant, WORKSPACE_FORMAT};
use ortak_domain::EmployeeId;
use ortak_server::worker_workspace_tools::{ProcessWorkspaceAdapter, WorkspaceConfig};
use ortak_server::workspace_reader::{execute, ReaderAction, ReaderRequest};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[test]
fn unsupported_reader_refuses_preparation_before_touching_the_selected_roots() {
    let root = std::env::temp_dir().join(format!("ortak-platform-{}", Uuid::new_v4()));
    let mut grant = WorkspaceGrant {
        format: WORKSPACE_FORMAT.into(),
        company_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        employee_id: EmployeeId::parse("ada").unwrap(),
        workspace_ref: "input:brief".into(),
        revision: Uuid::new_v4(),
        manifest_hash: String::new(),
        files: vec![WorkspaceFile {
            file_id: Uuid::new_v4(),
            name: "brief.txt".into(),
            media_type: "text/plain".into(),
            bytes: 4,
            sha256: hex::encode(Sha256::digest(b"text")),
        }],
    };
    grant.manifest_hash = grant.compute_hash().unwrap();
    grant.validate().unwrap();
    for action in [
        ReaderAction::Verify {},
        ReaderAction::Prepare {
            run_id: Uuid::new_v4(),
        },
    ] {
        let request = ReaderRequest {
            execution_token: Uuid::new_v4(),
            input_root: root.join("inputs").to_string_lossy().into(),
            run_root: root.join("runs").to_string_lossy().into(),
            grant: grant.clone(),
            action,
        };
        assert_eq!(
            execute(request).err(),
            Some("unsupported_workspace_platform")
        );
    }
    let config = WorkspaceConfig {
        reader_binary: root.join("reader.exe"),
        reader_sha256: "0".repeat(64),
        input_root: root.join("inputs"),
        run_root: root.join("runs"),
        grants: vec![grant],
        expires_at: Utc::now() + chrono::Duration::hours(1),
        register_selected_inputs: true,
    };
    assert!(ProcessWorkspaceAdapter::new(&config).is_err());
    assert!(!root.exists());
}

#[test]
fn unsupported_reader_binary_exits_without_waiting_for_stdin() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ortak-workspace-reader"))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("unsupported_workspace_platform"));
}
