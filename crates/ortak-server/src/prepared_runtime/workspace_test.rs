//! Retained registry metadata cannot replace current project availability.
use super::*;
use ortak_control::workspace::{WorkspaceFile, WorkspaceGrant, WORKSPACE_FORMAT};
use sha2::{Digest, Sha256};

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL55432 with proposal74"]
async fn workspace_profile_probe_requires_exact_capability_and_current_project_binding() {
    let f = Fixture::new_with_workspace(true).await;
    assert_eq!(
        f.start().await.unwrap(),
        Err("workspace_capability_unavailable")
    );
    assert_eq!(f.bridge.starts.load(Ordering::SeqCst), 0);
    f.bridge.workspace_capable.store(true, Ordering::SeqCst);
    let grant = retained_registry(&f).await;
    assert_eq!(
        f.start().await.unwrap(),
        Err("workspace_registry_unavailable")
    );
    assert_eq!(f.bridge.starts.load(Ordering::SeqCst), 0);
    bind_project(&f, &grant).await;
    let mut running = f.start();
    f.wait_started(&mut running).await;
    f.bridge.known.lock().unwrap().as_mut().unwrap().1 = "completed".into();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), running)
            .await
            .unwrap()
            .unwrap(),
        Ok(())
    );
    assert_eq!(f.bridge.starts.load(Ordering::SeqCst), 1);
}

async fn retained_registry(f: &Fixture) -> WorkspaceGrant {
    retained_registry_for(f, 86_400_000).await
}

async fn retained_registry_for(f: &Fixture, lifetime_ms: i64) -> WorkspaceGrant {
    let project = Uuid::new_v4();
    let file = Uuid::new_v4();
    let company = f.scope.company_id();
    let community = f.scope.community_id().unwrap();
    sqlx::query("INSERT INTO projects(company_id,id,slug,name,created_by_type) VALUES($1,$2,'workspace-probe','Workspace probe','system')")
        .bind(company).bind(project).execute(f.control.pool()).await.unwrap();
    // A valid retained immutable manifest may outlive its transient API binding.
    // This fixture deliberately seeds that persisted state without claiming I/O.
    let mut grant = WorkspaceGrant {
        format: WORKSPACE_FORMAT.into(),
        company_id: company,
        project_id: project,
        employee_id: ortak_domain::EmployeeId::parse("prepared-fixture").unwrap(),
        workspace_ref: "input:probe".into(),
        revision: Uuid::new_v4(),
        manifest_hash: String::new(),
        files: vec![WorkspaceFile {
            file_id: file,
            name: "probe.txt".into(),
            media_type: "text/plain".into(),
            bytes: 4,
            sha256: hex::encode(Sha256::digest(b"test")),
        }],
    };
    grant.manifest_hash = grant.compute_hash().unwrap();
    let bytes = serde_json::to_vec(&serde_json::to_value(&grant).unwrap()).unwrap();
    let mut tx = f.control.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO workspace_bindings(company_id,community_id,project_id,employee_id,id,workspace_ref,grant_bytes,manifest_hash,verification_id,verified_at,expires_at)
        VALUES($1,$2,$3,'prepared-fixture',$4,'input:probe',$5,$6,$7,clock_timestamp(),clock_timestamp()+($8::bigint * INTERVAL '1 millisecond'))")
        .bind(company).bind(community).bind(project).bind(grant.revision).bind(bytes).bind(hex::decode(&grant.manifest_hash).unwrap()).bind(Uuid::new_v4()).bind(lifetime_ms).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workspace_files(company_id,community_id,workspace_id,id,ordinal,logical_name,media_type,byte_count,content_hash)
        VALUES($1,$2,$3,$4,0,'probe.txt','text/plain',4,$5)")
        .bind(company).bind(community).bind(grant.revision).bind(file).bind(Sha256::digest(b"test").to_vec()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    grant
}
async fn bind_project(f: &Fixture, grant: &WorkspaceGrant) {
    let company = f.scope.company_id();
    let community = f.scope.community_id().unwrap();
    let project = grant.project_id;
    let channel = Uuid::new_v4();
    let issuer = Keys::generate().public_key().to_hex();
    sqlx::query(
        "INSERT INTO channels(community_id,id,name,created_by) VALUES($1,$2,'Workspace probe',$3)",
    )
    .bind(community)
    .bind(channel)
    .bind(hex::decode(&issuer).unwrap())
    .execute(f.control.pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO project_api_bindings(company_id,project_id,community_id,channel_id,created_by) VALUES($1,$2,$3,$4,$5)")
        .bind(company).bind(project).bind(community).bind(channel).bind(issuer).execute(f.control.pool()).await.unwrap();
}

async fn current_registry(f: &Fixture) -> WorkspaceGrant {
    f.bridge.workspace_capable.store(true, Ordering::SeqCst);
    let grant = retained_registry(f).await;
    bind_project(f, &grant).await;
    grant
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL55432 with proposal74"]
async fn workspace_gate_loss_after_probe_crash_still_contains_and_accounts_for_old_child() {
    for capability_loss in [false, true] {
        let f = Fixture::new_with_workspace(true).await;
        let grant = current_registry(&f).await;
        let mut running = f.start();
        f.wait_started(&mut running).await;
        running.abort();
        let _ = running.await;
        let error = if capability_loss {
            f.bridge.workspace_capable.store(false, Ordering::SeqCst);
            "workspace_capability_unavailable"
        } else {
            ortak_runtime::postgres::workspace_tools::revoke(&f.control, &f.scope, grant.revision)
                .await
                .unwrap();
            "workspace_registry_unavailable"
        };
        f.bridge.stop_allowed.store(false, Ordering::SeqCst);
        assert_eq!(f.start().await.unwrap(), Err("probe_containment_pending"));
        assert_eq!(
            f.control
                .provisioning_runtime_probe(&f.scope, f.operation)
                .await
                .unwrap()
                .unwrap()
                .state(),
            "running"
        );
        f.bridge.stop_allowed.store(true, Ordering::SeqCst);
        assert_eq!(f.start().await.unwrap(), Err(error));
        let state: (String, String) = sqlx::query_as(
            "SELECT state,error_code FROM provisioning_runtime_probes WHERE company_id=$1 AND operation_id=$2",
        )
        .bind(f.scope.company_id())
        .bind(f.operation)
        .fetch_one(f.control.pool())
        .await
        .unwrap();
        assert_eq!(state, ("failed".into(), "probe_authority_changed".into()));
        assert_eq!(f.bridge.starts.load(Ordering::SeqCst), 1);
        assert_eq!(f.bridge.stops.load(Ordering::SeqCst), 1);
        assert_eq!(f.start().await.unwrap(), Err(error));
        assert_eq!(f.bridge.stops.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL55432 with proposal74"]
async fn workspace_withdrawal_during_profile_probe_cannot_persist_fresh_readiness() {
    let f = Fixture::new_with_workspace(true).await;
    let grant = current_registry(&f).await;
    let mut running = f.start();
    f.wait_started(&mut running).await;
    ortak_runtime::postgres::workspace_tools::revoke(&f.control, &f.scope, grant.revision)
        .await
        .unwrap();
    f.bridge.known.lock().unwrap().as_mut().unwrap().1 = "completed".into();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), running)
            .await
            .unwrap()
            .unwrap(),
        Err("workspace_registry_unavailable")
    );
    assert_eq!(f.bridge.starts.load(Ordering::SeqCst), 1);
    assert_eq!(f.bridge.stops.load(Ordering::SeqCst), 1);
    let state: String = sqlx::query_scalar(
        "SELECT state FROM provisioning_runtime_probes WHERE company_id=$1 AND operation_id=$2",
    )
    .bind(f.scope.company_id())
    .bind(f.operation)
    .fetch_one(f.control.pool())
    .await
    .unwrap();
    assert_eq!(state, "failed");
}

#[path = "workspace_activation_test.rs"]
mod activation;
