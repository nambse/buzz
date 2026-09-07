//! Canonical purge uses actual leased workspace execution and retained bytes.
use super::*;
use buzz_db::{
    deletion::{FrozenInventory, KeyStreamDigest, LeaseToken, PrefixManifest, StorageManifest},
    Db, DbConfig, DbError,
};
use ortak_control::workspace::WorkspaceExecutionObserver;
use ortak_runtime::cancellation::{CancellationReason, RuntimeCancellationRepository};

const TABLES: [&str; 6] = [
    "workspace_bindings",
    "workspace_files",
    "run_workspace_uses",
    "workspace_tool_actions",
    "workspace_tool_receipts",
    "workspace_reader_executions",
];

async fn retained(x: &Selected) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object(
        'workspace_bindings',(SELECT jsonb_agg(t ORDER BY id) FROM workspace_bindings t WHERE company_id=$1),
        'workspace_files',(SELECT jsonb_agg(t ORDER BY workspace_id,ordinal) FROM workspace_files t WHERE company_id=$1),
        'run_workspace_uses',(SELECT jsonb_agg(t ORDER BY run_id) FROM run_workspace_uses t WHERE company_id=$1),
        'workspace_tool_actions',(SELECT jsonb_agg(t ORDER BY run_id,call_id) FROM workspace_tool_actions t WHERE company_id=$1),
        'workspace_tool_receipts',(SELECT jsonb_agg(t ORDER BY run_id,call_id) FROM workspace_tool_receipts t WHERE company_id=$1),
        'workspace_reader_executions',(SELECT jsonb_agg(t ORDER BY id) FROM workspace_reader_executions t WHERE company_id=$1))")
        .bind(x.f.company).fetch_one(&x.f.pool).await.unwrap()
}

async fn refused(
    x: &Selected,
    store: &buzz_db::deletion::DeletionStore,
    lease: &LeaseToken,
    code: &str,
) {
    let result = store.begin_quiescing(lease).await;
    assert!(
        matches!(&result, Err(DbError::DeletionSafety(found)) if found == code),
        "expected {code}, got {result:?}"
    );
    let active: (String, bool) =
        sqlx::query_as("SELECT deletion_state,archived_at IS NULL FROM communities WHERE id=$1")
            .bind(x.f.community)
            .fetch_one(&x.f.pool)
            .await
            .unwrap();
    assert_eq!(active, ("active".into(), true));
}

#[derive(Clone, Default)]
struct FailedPrepare {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
fn missing() -> RuntimeError {
    RuntimeError::Unavailable {
        detail: ortak_control::adapter::Detail::new("controlled missing input"),
    }
}
impl WorkspaceAdapter for FailedPrepare {
    async fn verify(&self, grant: &WorkspaceGrant) -> Result<(), RuntimeError> {
        grant.validate()
    }
    async fn prepare(
        &self,
        _: &WorkspaceGrant,
        _: Uuid,
    ) -> Result<PreparedWorkspace, RuntimeError> {
        Err(missing())
    }
    async fn prepare_observed<O: WorkspaceExecutionObserver>(
        &self,
        _: &WorkspaceGrant,
        _: Uuid,
        observer: &O,
    ) -> Result<PreparedWorkspace, RuntimeError> {
        observer.started(None).await?;
        self.entered.notify_one();
        self.release.notified().await;
        // This fixture has no child or detached I/O. The failed operation has
        // returned before its explicit stopped accounting, just like the port.
        observer.stopped().await?;
        Err(missing())
    }
    async fn read(
        &self,
        _: &WorkspaceGrant,
        _: &PreparedWorkspace,
        _: &WorkspaceToolRequest,
    ) -> Result<WorkspaceResult, RuntimeError> {
        panic!("failed preparation cannot reach a file read")
    }
}

async fn stopped_failed_prepare(
    x: &Selected,
    store: &buzz_db::deletion::DeletionStore,
    deletion: &LeaseToken,
) {
    let (project, item) = ready(&x.f, &x.app).await;
    let mut grant = x.grant.clone();
    grant.project_id = project;
    grant.revision = Uuid::new_v4();
    grant.manifest_hash = grant.compute_hash().unwrap();
    let adapter = FailedPrepare::default();
    let workspace =
        ConfiguredRunWorkspace::new(x.f.control.clone(), adapter.clone(), &x.scope, vec![grant])
            .unwrap();
    workspace
        .register(&x.scope, chrono::Utc::now() + chrono::Duration::days(1))
        .await
        .unwrap();
    let (run, _) = queue(&x.f, &x.app, &item).await;
    let lease =
        x.f.control
            .claim_runtime_dispatches(
                &x.scope,
                "fake-runtime",
                "failed-prepare",
                Duration::from_secs(1),
                1,
            )
            .await
            .unwrap()
            .remove(0);
    let supervisor =
        RunSupervisor::new(x.f.control.clone(), &x.runtime, SupervisorConfig::default())
            .with_memory(&x.memory)
            .with_workspace(workspace);
    let dispatch = supervisor.dispatch(&x.scope, &lease);
    tokio::pin!(dispatch);
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::select! {
            result=&mut dispatch => panic!("prepare did not reach its reader: {result:?}"),
            _=adapter.entered.notified() => (),
        }
    })
    .await
    .unwrap();
    refused(x, store, deletion, "workspace_readers_not_contained").await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let expired: bool=sqlx::query_scalar("SELECT owner_deadline<=clock_timestamp() FROM workspace_reader_executions WHERE company_id=$1 AND run_id=$2")
        .bind(x.f.company).bind(run).fetch_one(&x.f.pool).await.unwrap();
    assert!(expired);
    refused(x, store, deletion, "workspace_readers_not_contained").await;
    adapter.release.notify_one();
    assert!(matches!(
        dispatch.await.unwrap(),
        DispatchOutcome::StaleLease | DispatchOutcome::RuntimeFailed { .. }
    ));
    let history:(String,String,i64)=sqlx::query_as("SELECT state,stop_proof,(SELECT count(*) FROM run_workspace_uses WHERE company_id=$1 AND run_id=$2) FROM workspace_reader_executions WHERE company_id=$1 AND run_id=$2")
        .bind(x.f.company).bind(run).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(history, ("stopped".into(), "in_process_returned".into(), 0));
    refused(x, store, deletion, "workspace_runs_not_terminal").await;
    assert!(x
        .runtime
        .lookup_start(&ortak_runtime::run_idempotency_key(x.f.company, run))
        .await
        .unwrap()
        .is_none());
    x.f.control
        .enqueue_cancellation(&x.scope, run, CancellationReason::HumanRequested)
        .await
        .unwrap();
    let stop =
        x.f.control
            .claim_cancellations(&x.scope, "fake-runtime", Duration::from_secs(60), 1)
            .await
            .unwrap()
            .remove(0);
    assert_eq!(stop.run_id, run);
    let receipt = x
        .runtime
        .cancel_start(
            &ortak_runtime::run_idempotency_key(x.f.company, run),
            "controlled failed preparation",
        )
        .await
        .unwrap();
    assert!(matches!(
        x.f.control
            .acknowledge_cancellation(&x.scope, &stop, receipt.runtime_run_ref.as_ref())
            .await
            .unwrap(),
        ortak_runtime::cancellation::CancellationAckOutcome::Acknowledged { .. }
    ));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with immutable74"]
async fn workspace_canonical_purge_requires_stop_and_preserves_all_six_tables() {
    let x = selected().await;
    let other = selected().await;
    assert_eq!(
        other
            .workspace
            .step(&other.scope, &other.port)
            .await
            .unwrap(),
        WorkspaceStep::Settled
    );
    let other_before = retained(&other).await;
    let db = Db::new(&DbConfig {
        database_url: std::env::var("ORTAK_TEST_DATABASE_URL").unwrap(),
        max_connections: 4,
        min_connections: 0,
        ..DbConfig::default()
    })
    .await
    .unwrap();
    let store = db.deletion_store();
    let host: String = sqlx::query_scalar("SELECT host FROM communities WHERE id=$1")
        .bind(x.f.community)
        .fetch_one(&x.f.pool)
        .await
        .unwrap();
    let request = store
        .submit(
            &host,
            "fixture",
            Some("Controlled workspace retention regression"),
        )
        .await
        .unwrap();
    let community = request.community_id;
    let inventory = FrozenInventory {
        schema: store.inventory_schema(community).await.unwrap(),
        storage: StorageManifest {
            version: 4,
            prefixes: ["_meta", "_uploads", "repos"]
                .into_iter()
                .map(|prefix| PrefixManifest {
                    prefix: format!("{prefix}/{community}/"),
                    object_count: 0,
                    total_bytes: 0,
                    keys_digest: KeyStreamDigest::new().finish().0,
                })
                .collect(),
        },
    };
    for table in TABLES {
        assert!(inventory
            .schema
            .retained_tables
            .iter()
            .any(|name| name == table));
        assert!(inventory
            .schema
            .fenced_tables
            .iter()
            .any(|name| name == table));
        assert!(inventory.schema.row_counts.contains_key(table));
    }
    // A genuine old frozen retention policy cannot acquire current approval.
    let other_host: String = sqlx::query_scalar("SELECT host FROM communities WHERE id=$1")
        .bind(other.f.community)
        .fetch_one(&other.f.pool)
        .await
        .unwrap();
    let old_request = store.submit(&other_host, "fixture", None).await.unwrap();
    let mut old = inventory.clone();
    old.schema = store
        .inventory_schema(old_request.community_id)
        .await
        .unwrap();
    old.schema
        .retained_tables
        .retain(|name| !TABLES.contains(&name.as_str()));
    for prefix in &mut old.storage.prefixes {
        prefix.prefix = prefix.prefix.replace(
            &community.to_string(),
            &old_request.community_id.to_string(),
        );
    }
    store.freeze_inventory(old_request.id, &old).await.unwrap();
    assert!(store
        .approve(old_request.id, "fixture", None)
        .await
        .is_err());
    store
        .freeze_inventory(request.id, &inventory)
        .await
        .unwrap();
    store.approve(request.id, "fixture", None).await.unwrap();
    let claim = store
        .claim_specific(request.id, "fixture", Duration::from_secs(120))
        .await
        .unwrap()
        .unwrap();
    refused(&x, &store, &claim.lease, "workspace_runs_not_terminal").await;
    sqlx::query("INSERT INTO workspace_tool_actions(company_id,community_id,run_id,call_id,file_id,arguments_hash,ordinal) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(x.f.company).bind(x.f.community).bind(x.run).bind(&x.port.request.call_id).bind(x.port.request.file_id)
        .bind(hex::decode(&x.port.request.arguments_hash).unwrap()).bind(x.port.request.ordinal as i32).execute(&x.f.pool).await.unwrap();
    refused(&x, &store, &claim.lease, "workspace_actions_not_settled").await;
    x.port.fail_first.store(true, Ordering::SeqCst);
    assert_eq!(
        x.workspace.step(&x.scope, &x.port).await.unwrap(),
        WorkspaceStep::Retry
    );
    refused(&x, &store, &claim.lease, "workspace_actions_not_settled").await;
    let result: Vec<u8> = sqlx::query_scalar(
        "SELECT result_bytes FROM workspace_tool_receipts WHERE company_id=$1 AND run_id=$2",
    )
    .bind(x.f.company)
    .bind(x.run)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        x.workspace.step(&x.scope, &x.port).await.unwrap(),
        WorkspaceStep::Settled
    );
    assert_eq!(x.inputs.reads.load(Ordering::SeqCst), 1);
    assert_eq!(x.port.deliveries.load(Ordering::SeqCst), 1);
    let receipt: Vec<u8> = sqlx::query_scalar(
        "SELECT result_bytes FROM workspace_tool_receipts WHERE company_id=$1 AND run_id=$2",
    )
    .bind(x.f.company)
    .bind(x.run)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(result, receipt);
    refused(&x, &store, &claim.lease, "workspace_runs_not_terminal").await;
    complete(
        &x.f,
        &x.runtime.inner,
        &x.memory,
        x.run,
        &x.reference,
        ortak_control::run_event::BoundedText::raw("Workspace retention result"),
    )
    .await;
    assert_eq!(
        ortak_work::schedule_work_outputs(&x.f.control, &x.scope, 8)
            .await
            .unwrap()
            .materialized,
        1
    );
    stopped_failed_prepare(&x, &store, &claim.lease).await;
    let before = retained(&x).await;
    for table in TABLES {
        assert!(!before[table].as_array().unwrap().is_empty(), "{table}");
    }
    assert_eq!(
        before["workspace_reader_executions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(before["run_workspace_uses"].as_array().unwrap().len(), 1);
    store.begin_quiescing(&claim.lease).await.unwrap();
    let generation = store.fence(&claim.lease).await.unwrap();
    let token = LeaseToken {
        fence_generation: Some(generation),
        ..claim.lease
    };
    store
        .freeze_destructive_storage_manifest(&token, &inventory.storage)
        .await
        .unwrap();
    store.mark_drained(&token).await.unwrap();
    store
        .mark_bindings_removed(&token, json!({"keys":0}))
        .await
        .unwrap();
    let purged = store.purge_postgres(&token).await.unwrap();
    assert_eq!(purged.get("office_company_bindings"), Some(&1));
    assert_eq!(retained(&x).await, before);
    assert_eq!(retained(&other).await, other_before);
    store
        .mark_cache_purged(&token, json!({"keys":0}))
        .await
        .unwrap();
    store
        .verify_postgres_logically_deleted(&token)
        .await
        .unwrap();
    assert!(!get(
        &x.app,
        &x.f.operator,
        &format!("/api/v1/work-items/{}", id(&x.item))
    )
    .await
    .0
    .is_success());
    assert!(get(
        &other.app,
        &other.f.operator,
        &format!("/api/v1/work-items/{}", id(&other.item))
    )
    .await
    .0
    .is_success());
    for query in [
        "INSERT INTO workspace_bindings SELECT * FROM workspace_bindings WHERE company_id=$1",
        "INSERT INTO workspace_files SELECT * FROM workspace_files WHERE company_id=$1",
        "INSERT INTO run_workspace_uses SELECT * FROM run_workspace_uses WHERE company_id=$1",
        "INSERT INTO workspace_tool_actions SELECT * FROM workspace_tool_actions WHERE company_id=$1",
        "INSERT INTO workspace_tool_receipts SELECT * FROM workspace_tool_receipts WHERE company_id=$1",
        "INSERT INTO workspace_reader_executions SELECT * FROM workspace_reader_executions WHERE company_id=$1",
    ] {
        let error=sqlx::query(query).bind(x.f.company).execute(&x.f.pool).await.unwrap_err();
        assert_eq!(error.as_database_error().and_then(|e|e.code()).as_deref(),Some("55000"));
    }
    assert_eq!(retained(&x).await, before);
    assert_eq!(retained(&other).await, other_before);
}
