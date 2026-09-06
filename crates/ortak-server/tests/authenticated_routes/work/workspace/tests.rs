use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL55432 with proposal74"]
async fn workspace_signed_work_freezes_input_reads_once_and_retries_exact_result() {
    let x = selected().await;
    x.port.fail_first.store(true, Ordering::SeqCst);
    assert_eq!(
        x.workspace.step(&x.scope, &x.port).await.unwrap(),
        WorkspaceStep::Retry
    );
    let row=sqlx::query("SELECT a.state,a.next_attempt_at>clock_timestamp() AS delayed,r.result_bytes FROM workspace_tool_actions a
        JOIN workspace_tool_receipts r USING(company_id,run_id,call_id) WHERE a.company_id=$1 AND a.run_id=$2")
        .bind(x.f.company).bind(x.run).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(row.get::<String, _>("state"), "result_ready");
    assert!(row.get::<bool, _>("delayed"));
    let bytes: Vec<u8> = row.get("result_bytes");
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["content"],
        "Selected brief"
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        x.workspace.step(&x.scope, &x.port).await.unwrap(),
        WorkspaceStep::Settled
    );
    assert_eq!(x.inputs.reads.load(Ordering::SeqCst), 1);
    assert_eq!(x.port.resolves.load(Ordering::SeqCst), 2);
    let after: Vec<u8> = sqlx::query_scalar(
        "SELECT result_bytes FROM workspace_tool_receipts WHERE company_id=$1 AND run_id=$2",
    )
    .bind(x.f.company)
    .bind(x.run)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(bytes, after);
    complete(
        &x.f,
        &x.runtime.inner,
        &x.memory,
        x.run,
        &x.reference,
        ortak_control::run_event::BoundedText::raw("Deliverable from selected brief"),
    )
    .await;
    let output = ortak_work::schedule_work_outputs(&x.f.control, &x.scope, 8)
        .await
        .unwrap();
    assert_eq!((output.attempted, output.materialized), (1, 1));
    assert_eq!(x.port.deliveries.load(Ordering::SeqCst), 1);
    let (_, detail) = get(
        &x.app,
        &x.f.operator,
        &format!("/api/v1/work-items/{}", id(&x.item)),
    )
    .await;
    assert_eq!(detail["work_item"]["state"], "review");
    assert_eq!(detail["work_item"]["criteria"], x.item["criteria"]);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL55432 with proposal74"]
async fn workspace_revocation_during_io_discards_result_and_preserves_stop_recovery() {
    let x = selected().await;
    x.inputs.revoke_on_read.store(true, Ordering::SeqCst);
    assert_eq!(
        x.workspace.step(&x.scope, &x.port).await.unwrap(),
        WorkspaceStep::RecoveryPending
    );
    assert_eq!(x.inputs.reads.load(Ordering::SeqCst), 1);
    assert_eq!(x.port.resolves.load(Ordering::SeqCst), 0);
    let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM workspace_tool_receipts WHERE company_id=$1 AND run_id=$2),
        (SELECT count(*) FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2 AND state='pending')")
        .bind(x.f.company).bind(x.run).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(counts, (0, 1));
    let changed = x.grant.clone();
    let mut crossed = changed.clone();
    crossed.company_id = Uuid::new_v4();
    crossed.manifest_hash = crossed.compute_hash().unwrap();
    assert!(ConfiguredRunWorkspace::new(
        x.f.control.clone(),
        x.inputs.clone(),
        &x.scope,
        vec![crossed]
    )
    .is_err());
    assert_eq!(x.inputs.verified.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL55432 with proposal74; bounded21s lease-expiry proof"]
async fn workspace_expired_lease_is_not_reader_containment_or_cancel_ack() {
    use ortak_runtime::cancellation::{CancellationReason, RuntimeCancellationRepository};
    let x = selected().await;
    x.inputs.block.store(true, Ordering::SeqCst);
    let workspace = x.workspace.clone();
    let scope = x.scope.clone();
    let port = x.port.clone();
    let task = tokio::spawn(async move { workspace.step(&scope, &port).await });
    x.inputs.entered.notified().await;
    x.f.control
        .enqueue_cancellation(&x.scope, x.run, CancellationReason::HumanRequested)
        .await
        .unwrap();
    let lease =
        x.f.control
            .claim_cancellations(&x.scope, "fake-runtime", Duration::from_secs(60), 1)
            .await
            .unwrap()
            .remove(0);
    let receipt = x
        .runtime
        .cancel_start(
            &ortak_runtime::run_idempotency_key(x.f.company, x.run),
            "fixture cancellation",
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(21)).await;
    let expired:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_reader_executions WHERE company_id=$1 AND run_id=$2 AND state='running' AND owner_deadline<clock_timestamp())")
        .bind(x.f.company).bind(x.run).fetch_one(&x.f.pool).await.unwrap();
    assert!(expired);
    assert!(x
        .f
        .control
        .acknowledge_cancellation(&x.scope, &lease, receipt.runtime_run_ref.as_ref())
        .await
        .is_err());
    x.inputs.release.notify_one();
    task.await.unwrap().unwrap();
    let ack =
        x.f.control
            .acknowledge_cancellation(&x.scope, &lease, receipt.runtime_run_ref.as_ref())
            .await
            .unwrap();
    assert!(matches!(
        ack,
        ortak_runtime::cancellation::CancellationAckOutcome::Acknowledged { .. }
    ));
    assert_eq!(x.port.resolves.load(Ordering::SeqCst), 0);
}

async fn initial_action(x: &Selected) {
    sqlx::query("INSERT INTO workspace_tool_actions(company_id,community_id,run_id,call_id,file_id,arguments_hash,ordinal) VALUES($1,$2,$3,$4,$5,$6,1)")
        .bind(x.f.company).bind(x.f.community).bind(x.run).bind(&x.port.request.call_id).bind(x.port.request.file_id)
        .bind(hex::decode(&x.port.request.arguments_hash).unwrap()).execute(&x.f.pool).await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with proposal74"]
async fn workspace_live_third_reader_lease_is_not_cancelled_by_competing_worker() {
    let x = selected().await;
    initial_action(&x).await;
    for _ in 0..2 {
        sqlx::query("UPDATE workspace_tool_actions SET lease_token=$3,lease_expires_at=clock_timestamp()+INTERVAL '1 millisecond',attempt_count=attempt_count+1,updated_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2")
            .bind(x.f.company).bind(x.run).bind(Uuid::new_v4()).execute(&x.f.pool).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    x.inputs.block.store(true, Ordering::SeqCst);
    let workspace = x.workspace.clone();
    let scope = x.scope.clone();
    let port = x.port.clone();
    let first = tokio::spawn(async move { workspace.step(&scope, &port).await });
    tokio::time::timeout(Duration::from_secs(3), x.inputs.entered.notified())
        .await
        .unwrap();
    assert_eq!(
        x.workspace.step(&x.scope, &x.port).await.unwrap(),
        WorkspaceStep::Idle
    );
    let held: (i32, bool, i64) = sqlx::query_as(
        "SELECT attempt_count,lease_expires_at>clock_timestamp(),
        (SELECT count(*) FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2)
        FROM workspace_tool_actions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(x.f.company)
    .bind(x.run)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(held, (3, true, 0));
    x.inputs.release.notify_one();
    assert_eq!(first.await.unwrap().unwrap(), WorkspaceStep::Settled);
    assert_eq!(x.inputs.reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with proposal74"]
async fn workspace_terminal_lost_ack_replays_only_retained_result_after_confirmed_stop_without_config(
) {
    use ortak_runtime::cancellation::{CancellationReason, RuntimeCancellationRepository};
    use ortak_runtime::postgres::workspace_tools::settle_workspace_receipts;
    let x = selected().await;
    x.port.fail_first.store(true, Ordering::SeqCst);
    assert_eq!(
        x.workspace.step(&x.scope, &x.port).await.unwrap(),
        WorkspaceStep::Retry
    );
    assert_eq!(x.port.deliveries.load(Ordering::SeqCst), 1);
    complete(
        &x.f,
        &x.runtime.inner,
        &x.memory,
        x.run,
        &x.reference,
        ortak_control::run_event::BoundedText::raw("Terminal output"),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(
        settle_workspace_receipts(&x.f.control, &x.scope, &x.port, &[])
            .await
            .unwrap()
    );
    assert_eq!(
        x.port.resolves.load(Ordering::SeqCst),
        1,
        "terminal status is not stop proof"
    );
    // Even an explicit repeated enqueue retains the original stop and key.
    assert!(!x
        .f
        .control
        .enqueue_cancellation(&x.scope, x.run, CancellationReason::WorkRevoked)
        .await
        .unwrap());
    let lease =
        x.f.control
            .claim_cancellations(&x.scope, "fake-runtime", Duration::from_secs(60), 1)
            .await
            .unwrap()
            .remove(0);
    let stopped = x
        .runtime
        .cancel_start(
            &ortak_runtime::run_idempotency_key(x.f.company, x.run),
            "fixture stop",
        )
        .await
        .unwrap();
    x.f.control
        .acknowledge_cancellation(&x.scope, &lease, stopped.runtime_run_ref.as_ref())
        .await
        .unwrap();
    assert!(
        settle_workspace_receipts(&x.f.control, &x.scope, &x.port, &[])
            .await
            .unwrap()
    );
    assert_eq!(x.inputs.reads.load(Ordering::SeqCst), 1);
    assert_eq!(x.port.resolves.load(Ordering::SeqCst), 2);
    assert_eq!(
        x.port.deliveries.load(Ordering::SeqCst),
        1,
        "identical terminal ACK cannot deliver twice"
    );
    let state: String = sqlx::query_scalar(
        "SELECT state FROM workspace_tool_actions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(x.f.company)
    .bind(x.run)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(state, "delivered");
    assert!(
        !settle_workspace_receipts(&x.f.control, &x.scope, &x.port, &[])
            .await
            .unwrap()
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with proposal74"]
async fn workspace_sql_refuses_forged_input_hash_result_without_reader_and_retained_mutation() {
    let x = selected().await;
    assert!(sqlx::query("INSERT INTO workspace_tool_actions(company_id,community_id,run_id,call_id,file_id,arguments_hash,ordinal) VALUES($1,$2,$3,'forged',$4,$5,1)")
        .bind(x.f.company).bind(x.f.community).bind(x.run).bind(x.port.request.file_id).bind(vec![0u8;32]).execute(&x.f.pool).await.is_err());
    initial_action(&x).await;
    let token = Uuid::new_v4();
    sqlx::query("UPDATE workspace_tool_actions SET lease_token=$3,lease_expires_at=clock_timestamp()+INTERVAL '20 seconds',attempt_count=1,updated_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2")
        .bind(x.f.company).bind(x.run).bind(token).execute(&x.f.pool).await.unwrap();
    let file = &x.grant.files[0];
    let result = WorkspaceResult::Completed {
        content: "Selected brief".into(),
        sha256: file.sha256.clone(),
        bytes: file.bytes,
        name: file.name.clone(),
    };
    let bytes = result.canonical_bytes().unwrap();
    let mut tx = x.f.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO workspace_tool_receipts(company_id,community_id,run_id,call_id,arguments_hash,lease_token,attempt_count,result_bytes,result_hash) VALUES($1,$2,$3,$4,$5,$6,1,$7,$8)")
        .bind(x.f.company).bind(x.f.community).bind(x.run).bind(&x.port.request.call_id).bind(hex::decode(&x.port.request.arguments_hash).unwrap())
        .bind(token).bind(&bytes).bind(Sha256::digest(&bytes).to_vec()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE workspace_tool_actions SET state='result_ready',updated_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2")
        .bind(x.f.company).bind(x.run).execute(&mut *tx).await.unwrap();
    assert!(
        tx.commit().await.is_err(),
        "a valid-looking result is not an actual stopped-reader witness"
    );
    assert!(sqlx::query(
        "UPDATE run_workspace_uses SET store_ref='forged' WHERE company_id=$1 AND run_id=$2"
    )
    .bind(x.f.company)
    .bind(x.run)
    .execute(&x.f.pool)
    .await
    .is_err());
    assert!(
        sqlx::query("DELETE FROM run_workspace_uses WHERE company_id=$1 AND run_id=$2")
            .bind(x.f.company)
            .bind(x.run)
            .execute(&x.f.pool)
            .await
            .is_err()
    );
    assert_eq!(x.inputs.reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with proposal74"]
async fn workspace_exhaustion_and_required_stop_are_one_atomic_persist() {
    let x = selected().await;
    initial_action(&x).await;
    for _ in 0..3 {
        sqlx::query("UPDATE workspace_tool_actions SET lease_token=$3,lease_expires_at=clock_timestamp()+INTERVAL '1 millisecond',attempt_count=attempt_count+1,updated_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2")
            .bind(x.f.company).bind(x.run).bind(Uuid::new_v4()).execute(&x.f.pool).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let name = format!("workspace_stop_failure_{}", Uuid::new_v4().simple());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION {name}() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'controlled stop persistence failure'; END $$;
        CREATE TRIGGER {name} BEFORE INSERT ON runtime_cancellations FOR EACH ROW WHEN(NEW.run_id='{}'::uuid) EXECUTE FUNCTION {name}();",x.run)))
        .execute(&x.f.pool).await.unwrap();
    let failed = x.workspace.step(&x.scope, &x.port).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON runtime_cancellations; DROP FUNCTION {name}();"
    )))
    .execute(&x.f.pool)
    .await
    .unwrap();
    assert!(failed.is_err());
    let state: String = sqlx::query_scalar(
        "SELECT state FROM workspace_tool_actions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(x.f.company)
    .bind(x.run)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(state, "pending");
    x.workspace.step(&x.scope, &x.port).await.unwrap();
    let row:(String,i64)=sqlx::query_as("SELECT state,(SELECT count(*) FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2 AND state='pending')
        FROM workspace_tool_actions WHERE company_id=$1 AND run_id=$2")
        .bind(x.f.company).bind(x.run).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(row, ("interrupted".into(), 1));
    assert_eq!(x.inputs.reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with proposal74 and matched reader binary; bounded8s watchdog"]
async fn workspace_restart_recovery_requires_exact_owned_process_absence_after_lease_expiry() {
    use ortak_server::worker_workspace_tools::recover_reader;
    let x = selected().await;
    initial_action(&x).await;
    let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_ortak-workspace-reader"));
    let hash = Sha256::digest(std::fs::read(&binary).unwrap()).to_vec();
    let token = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let deadline:chrono::DateTime<Utc>=sqlx::query_scalar("UPDATE workspace_tool_actions SET lease_token=$3,lease_expires_at=clock_timestamp()+INTERVAL '2 seconds',attempt_count=1,updated_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 RETURNING lease_expires_at")
        .bind(x.f.company).bind(x.run).bind(owner).fetch_one(&x.f.pool).await.unwrap();
    sqlx::query("INSERT INTO workspace_reader_executions(company_id,community_id,run_id,id,workspace_id,request_key,owner_lease,owner_deadline,executable,executable_hash,operating_uid)
        VALUES($1,$2,$3,$4,$5,'read:call_1',$6,$7,$8,$9,$10)")
        .bind(x.f.company).bind(x.f.community).bind(x.run).bind(token).bind(x.grant.revision).bind(owner).bind(deadline)
        .bind(binary.to_str().unwrap()).bind(hash).bind(i64::from(rustix::process::getuid().as_raw())).execute(&x.f.pool).await.unwrap();
    let mut child = tokio::process::Command::new(&binary)
        .arg(format!("--ortak-workspace-child={token}"))
        .env_clear()
        .current_dir("/")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let _stdin = child.stdin.take().unwrap();
    sqlx::query("UPDATE workspace_reader_executions SET state='running',pid=$3 WHERE company_id=$1 AND id=$2")
        .bind(x.f.company).bind(token).bind(i64::from(child.id().unwrap())).execute(&x.f.pool).await.unwrap();
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert!(
        !recover_reader(&x.f.control, &x.scope).await.unwrap(),
        "an expired lease cannot prove a live child absent"
    );
    let status = tokio::time::timeout(Duration::from_secs(9), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.code(), Some(124));
    assert!(recover_reader(&x.f.control, &x.scope).await.unwrap());
    let proof: String = sqlx::query_scalar(
        "SELECT stop_proof FROM workspace_reader_executions WHERE company_id=$1 AND id=$2",
    )
    .bind(x.f.company)
    .bind(token)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(proof, "confirmed_absence");
    assert!(!recover_reader(&x.f.control, &x.scope).await.unwrap());
    assert_eq!(x.inputs.reads.load(Ordering::SeqCst), 0);
}

#[path = "lost_start.rs"]
mod lost_start;
