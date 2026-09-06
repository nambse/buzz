//! Actual approval/export → selected Work input → cancellation/output boundaries.
use super::authority::NamedMemory;
use super::*;
use crate::work::execution::fixture::{complete, queue};
use ortak_control::fakes::{FakeMemoryAdapter, FakeRuntimeAdapter};
use ortak_control::run_event::BoundedText;
use ortak_control::run_event::RedactionPolicy;
use ortak_runtime::memory_context::{
    FreezeSnapshotOutcome, FrozenRunSnapshot, RunContextRepository, RunMemory,
};
use ortak_runtime::memory_context::{ReviewedMemoryContext, ReviewedMemoryRecord};
use ortak_runtime::reviewed_memory::{
    ReviewedMemorySelection, ReviewedRunAdapter, ReviewedRunMemory,
};
use ortak_runtime::{
    DispatchAuthority, DispatchOutcome, DispatchRefusal, RunSupervisor, SupervisorConfig,
};
use ortak_runtime::{DispatchAuthorization, RunDispatchRepository};
#[path = "runtime_json_compat.rs"]
mod json_compat;
#[path = "runtime_revision.rs"]
mod revision;

impl ReviewedRunAdapter for NamedMemory {
    fn reviewed_enabled(&self, _: &DispatchAuthority) -> Result<bool, DispatchRefusal> {
        Ok(true)
    }
    async fn recall_selected(
        &self,
        selection: &ReviewedMemorySelection,
        _: &str,
    ) -> Result<ReviewedMemoryContext, DispatchRefusal> {
        // Controlled remote boundary: selected IDs and hashes still come from
        // production PG. Actual Honcho selected transport has separate socket/PG tests.
        Ok(ReviewedMemoryContext {
            records: selection
                .pins
                .iter()
                .take(8)
                .map(|pin| ReviewedMemoryRecord {
                    pin: pin.clone(),
                    content: "Reviewed deployment fact".into(),
                })
                .collect(),
            truncated: false,
        })
    }
}

pub(super) async fn prepared(expiry: Duration) -> (ExportFixture, Value) {
    prepared_with_adapter(expiry, &ObservedAdapter::default()).await
}

pub(super) async fn prepared_with_adapter(
    expiry: Duration,
    remote: &ObservedAdapter,
) -> (ExportFixture, Value) {
    let mut x = ExportFixture::new(expiry, false).await;
    x.target.runtime_consumption_enabled = true;
    x.advertise().await;
    x.publish().await;
    assert!(schedule_one(&x.f.control, &x.scope, remote).await.unwrap());
    let mut body = item_body("Deployment plan");
    body["source_message_id"] = json!(x.source);
    let created = post(
        &x.app,
        &x.f.operator,
        &format!("/api/v1/projects/{}/promotions", x.project),
        &body,
    )
    .await;
    assert_eq!(created.0, StatusCode::CREATED, "{created:?}");
    let item = created.1["work_item"].clone();
    let assigned=post(&x.app,&x.f.operator,&format!("/api/v1/work-items/{}/assignments",id(&item)),
        &json!({"operation_id":Uuid::new_v4(),"expected_version":version(&item),"employee_id":"cem","role":"owner"})).await;
    assert_eq!(assigned.0, StatusCode::OK, "{assigned:?}");
    let ready = transition(&x.f, &x.app, assigned.1["work_item"].clone(), "ready").await;
    (x, ready)
}

pub(super) async fn start(
    x: &ExportFixture,
    item: &Value,
) -> (
    Uuid,
    FakeRuntimeAdapter,
    NamedMemory,
    ortak_control::runtime::RuntimeRunRef,
) {
    let (run, _) = queue(&x.f, &x.app, item).await;
    let adapter = FakeRuntimeAdapter::new().with_existing_profile("fake://work-profile", true);
    let memory = NamedMemory(
        FakeMemoryAdapter::new().with_existing_binding(x.employee.memory.as_ref().unwrap()),
    );
    let lease =
        x.f.control
            .claim_runtime_dispatches(
                &x.scope,
                "fake-runtime",
                "reviewed-test",
                Duration::from_secs(60),
                1,
            )
            .await
            .unwrap()
            .remove(0);
    let supervisor = RunSupervisor::new(x.f.control.clone(), &adapter, SupervisorConfig::default())
        .with_run_memory(ReviewedRunMemory::new(
            &memory,
            x.f.control.clone(),
            x.scope.clone(),
        ));
    let result = supervisor.dispatch(&x.scope, &lease).await.unwrap();
    let DispatchOutcome::Started {
        runtime_run_ref, ..
    } = result
    else {
        panic!("{result:?}")
    };
    assert_eq!(adapter.start_specs().len(), 1);
    (run, adapter, memory, runtime_run_ref)
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal71"]
async fn reviewed_runtime_freezes_exact_approval_and_materializes_only_current_context() {
    let (x, item) = prepared(Duration::from_secs(86400)).await;
    let (run, adapter, memory, reference) = start(&x, &item).await;
    let spec = &adapter.start_specs()[0];
    assert_eq!(spec.context.memory_context.len(), 1);
    let record: Value = serde_json::from_str(&spec.context.memory_context[0]).unwrap();
    assert_eq!(record["type"], "reviewed_project_memory");
    assert_eq!(record["record"]["pin"]["fact_id"], json!(x.fact));
    assert_eq!(record["record"]["content"], "Reviewed deployment fact");
    let bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT spec_bytes FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
    )
    .bind(x.f.company)
    .bind(run)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["version"],
        3
    );
    let uses:i64=sqlx::query_scalar("SELECT count(*) FROM run_reviewed_memory_uses WHERE company_id=$1 AND run_id=$2 AND fact_id=$3")
        .bind(x.f.company).bind(run).bind(x.fact).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(uses, 1);
    complete(
        &x.f,
        &adapter,
        &memory.0,
        run,
        &reference,
        BoundedText::raw("Reviewed deployment result"),
    )
    .await;
    assert_eq!(
        ortak_work::schedule_work_outputs(&x.f.control, &x.scope, 1)
            .await
            .unwrap()
            .materialized,
        1
    );
    let (_, result) = get(
        &x.app,
        &x.f.operator,
        &format!("/api/v1/work-items/{}", id(&item)),
    )
    .await;
    assert_eq!(result["work_item"]["state"], "review");
    assert_eq!(result["work_item"]["criteria"], item["criteria"]);
    let detail = get(&x.app, &x.f.operator, &format!("/api/v1/runs/{run}")).await;
    assert_eq!(detail.0, StatusCode::OK, "{detail:?}");
    assert_eq!(
        detail.1["memory"]["reviewed"][0]["content"]["text"],
        "Reviewed deployment fact"
    );
    x.stop().await;
    let detail = get(&x.app, &x.f.operator, &format!("/api/v1/runs/{run}")).await;
    assert_eq!(detail.0, StatusCode::OK, "{detail:?}");
    assert_eq!(detail.1["memory"]["reviewed"][0]["fact_id"], json!(x.fact));
    assert_eq!(detail.1["memory"]["reviewed"][0]["current"], false);
    assert!(detail.1["memory"]["reviewed"][0]["content"].is_null());
    assert_eq!(
        sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT spec_bytes FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2"
        )
        .bind(x.f.company)
        .bind(run)
        .fetch_one(&x.f.pool)
        .await
        .unwrap(),
        bytes
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal71"]
async fn reviewed_runtime_stop_is_durable_and_late_terminal_output_is_rejected() {
    let (x, item) = prepared(Duration::from_secs(86400)).await;
    let (run, adapter, memory, reference) = start(&x, &item).await;
    x.stop().await;
    let report = ortak_runtime::reconciliation::reconcile_office_runs(&x.f.control, &x.scope, 64)
        .await
        .unwrap();
    assert_eq!(report.revocations, 1);
    assert_eq!(
        ortak_runtime::reconciliation::reconcile_office_runs(&x.f.control, &x.scope, 64)
            .await
            .unwrap()
            .revocations,
        0
    );
    complete(
        &x.f,
        &adapter,
        &memory.0,
        run,
        &reference,
        BoundedText::raw("Late unauthorized result"),
    )
    .await;
    assert_eq!(
        ortak_work::schedule_work_outputs(&x.f.control, &x.scope, 1)
            .await
            .unwrap()
            .materialized,
        0
    );
    let code: String = sqlx::query_scalar(
        "SELECT last_error_code FROM runtime_work_outputs WHERE company_id=$1 AND run_id=$2",
    )
    .bind(x.f.company)
    .bind(run)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(code, "work_output_authority_changed");
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal71"]
async fn reviewed_runtime_held_admission_blocks_opt_out_and_reenable_never_revives_old_epoch() {
    let (x, item) = prepared(Duration::from_secs(86400)).await;
    let (run, _, _, _) = start(&x, &item).await;
    let mut tx = x.f.pool.begin().await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT ortak_lock_run_reviewed_memory($1,$2)")
            .bind(x.f.company)
            .bind(run)
            .fetch_one(&mut *tx)
            .await
            .unwrap()
    );
    let mut target = x.target.clone();
    target.runtime_consumption_enabled = false;
    let targets = [target];
    let disable = exports::advertise_targets(&x.f.control, &x.scope, &targets);
    tokio::pin!(disable);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut disable)
            .await
            .is_err(),
        "target opt-out must wait for admitted use lock"
    );
    tx.commit().await.unwrap();
    assert_eq!(disable.await.unwrap(), 1);
    x.advertise().await;
    let current: bool = sqlx::query_scalar("SELECT ortak_run_reviewed_memory_current($1,$2)")
        .bind(x.f.company)
        .bind(run)
        .fetch_one(&x.f.pool)
        .await
        .unwrap();
    assert!(!current);
    assert_eq!(
        ortak_runtime::reconciliation::reconcile_office_runs(&x.f.control, &x.scope, 64)
            .await
            .unwrap()
            .revocations,
        1
    );
}

struct NoSecondRecall;
impl RunMemory for NoSecondRecall {
    async fn check(&self, _: &DispatchAuthority) -> Result<(), DispatchRefusal> {
        Ok(())
    }
    async fn snapshot(
        &self,
        _: &DispatchAuthority,
        _: Uuid,
        _: &RedactionPolicy,
    ) -> Result<FrozenRunSnapshot, DispatchRefusal> {
        panic!("a retry must load the committed reviewed snapshot, never recall again")
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal71"]
async fn reviewed_runtime_frozen_start_retry_reuses_exact_bytes_and_stale_recall_cannot_commit() {
    for revoke in [false, true] {
        let (x, item) = prepared(Duration::from_secs(86400)).await;
        let (run, _) = queue(&x.f, &x.app, &item).await;
        let lease =
            x.f.control
                .claim_runtime_dispatches(
                    &x.scope,
                    "fake-runtime",
                    "reviewed-freeze",
                    Duration::from_secs(60),
                    1,
                )
                .await
                .unwrap()
                .remove(0);
        let DispatchAuthorization::Authorized(authority) =
            x.f.control
                .authorize_dispatch(&x.scope, &lease)
                .await
                .unwrap()
        else {
            panic!("authority")
        };
        let memory = NamedMemory(
            FakeMemoryAdapter::new().with_existing_binding(x.employee.memory.as_ref().unwrap()),
        );
        let candidate = ReviewedRunMemory::new(&memory, x.f.control.clone(), x.scope.clone())
            .snapshot(&authority, run, &RedactionPolicy::new())
            .await
            .unwrap();
        assert_eq!(candidate.reviewed().unwrap().records.len(), 1);
        if revoke {
            x.stop().await;
        }
        let frozen =
            x.f.control
                .freeze_run_snapshot(&x.scope, &lease, &authority, run, &candidate)
                .await
                .unwrap();
        if revoke {
            assert!(matches!(frozen, FreezeSnapshotOutcome::Refused(_)));
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2",
            )
            .bind(x.f.company)
            .bind(run)
            .fetch_one(&x.f.pool)
            .await
            .unwrap();
            assert_eq!(count, 0);
        } else {
            let FreezeSnapshotOutcome::Ready(winner) = frozen else {
                panic!("freeze")
            };
            assert_eq!(winner.encode().unwrap(), candidate.encode().unwrap());
            let mut changed: Value = serde_json::from_slice(&candidate.encode().unwrap()).unwrap();
            changed["reviewed"]["truncated"] = json!(true);
            let changed =
                FrozenRunSnapshot::decode(&serde_json::to_vec(&changed).unwrap(), &authority, run)
                    .unwrap();
            assert!(matches!(
                x.f.control
                    .freeze_run_snapshot(&x.scope, &lease, &authority, run, &changed)
                    .await
                    .unwrap(),
                FreezeSnapshotOutcome::Refused(DispatchRefusal::MemoryContextRejected)
            ));
            let adapter =
                FakeRuntimeAdapter::new().with_existing_profile("fake://work-profile", true);
            let outcome =
                RunSupervisor::new(x.f.control.clone(), &adapter, SupervisorConfig::default())
                    .with_run_memory(NoSecondRecall)
                    .dispatch(&x.scope, &lease)
                    .await
                    .unwrap();
            assert!(matches!(outcome, DispatchOutcome::Started { .. }));
            assert_eq!(adapter.start_specs(), vec![winner.spec().clone()]);
        }
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal71"]
async fn reviewed_runtime_expiry_alone_schedules_one_durable_stop_without_remote_cleanup() {
    let (x, item) = prepared(Duration::from_secs(3)).await;
    let (run, _, _, _) = start(&x, &item).await;
    let remaining:f64=sqlx::query_scalar("SELECT greatest(extract(epoch FROM expires_at-clock_timestamp())::float8,0) FROM reviewed_memory_facts WHERE company_id=$1 AND id=$2")
        .bind(x.f.company).bind(x.fact).fetch_one(&x.f.pool).await.unwrap();
    assert!(remaining <= 3.0);
    tokio::time::sleep(Duration::from_secs_f64(remaining) + Duration::from_millis(30)).await;
    assert_eq!(
        ortak_runtime::reconciliation::reconcile_office_runs(&x.f.control, &x.scope, 64)
            .await
            .unwrap()
            .revocations,
        1
    );
    let cleanup:String=sqlx::query_scalar("SELECT state FROM reviewed_memory_export_jobs WHERE company_id=$1 AND fact_id=$2 AND action='withdraw'")
        .bind(x.f.company).bind(x.fact).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(cleanup, "pending");
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT ortak_run_reviewed_memory_current($1,$2)")
            .bind(x.f.company)
            .bind(run)
            .fetch_one(&x.f.pool)
            .await
            .unwrap()
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal71"]
async fn reviewed_runtime_database_refuses_forged_or_orphan_frozen_use_records() {
    let (x, item) = prepared(Duration::from_secs(86400)).await;
    let (run, _) = queue(&x.f, &x.app, &item).await;
    let lease =
        x.f.control
            .claim_runtime_dispatches(
                &x.scope,
                "fake-runtime",
                "reviewed-forgery",
                Duration::from_secs(60),
                1,
            )
            .await
            .unwrap()
            .remove(0);
    let DispatchAuthorization::Authorized(authority) =
        x.f.control
            .authorize_dispatch(&x.scope, &lease)
            .await
            .unwrap()
    else {
        panic!("authority")
    };
    let memory = NamedMemory(
        FakeMemoryAdapter::new().with_existing_binding(x.employee.memory.as_ref().unwrap()),
    );
    let candidate = ReviewedRunMemory::new(&memory, x.f.control.clone(), x.scope.clone())
        .snapshot(&authority, run, &RedactionPolicy::new())
        .await
        .unwrap();
    for mutation in ["missing_use", "rendered_text", "provenance", "version_type"] {
        let mut wire: Value = serde_json::from_slice(&candidate.encode().unwrap()).unwrap();
        match mutation {
            "rendered_text" => {
                wire["spec"]["context"]["memory_context"][0] = json!("unapproved replacement")
            }
            "provenance" => {
                wire["reviewed"]["records"][0]["pin"]["approved_by"] = json!("ff".repeat(32))
            }
            "version_type" => wire["version"] = json!("3"),
            _ => {}
        }
        let bytes = serde_json::to_vec(&wire).unwrap();
        let mut tx = x.f.pool.begin().await.unwrap();
        sqlx::query("INSERT INTO run_context_snapshots(company_id,run_id,spec_bytes,spec_hash) VALUES($1,$2,$3,$4)")
            .bind(x.f.company).bind(run).bind(&bytes).bind(Sha256::digest(&bytes).to_vec()).execute(&mut *tx).await.unwrap();
        if mutation != "missing_use" {
            sqlx::query("INSERT INTO run_reviewed_memory_uses(company_id,community_id,run_id,ordinal,fact_id,target_id,fact_version,
                consumption_epoch,content_hash,source_hash,binding_hash,approval_id,approved_by,expires_at)
                SELECT f.company_id,f.community_id,$2,0,f.id,t.id,f.version,t.consumption_epoch,x.content_hash,x.source_hash,t.binding_hash,
                f.promotion_operation_id,f.approved_by,f.expires_at FROM reviewed_memory_facts f
                JOIN reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
                JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
                WHERE f.company_id=$1 AND f.id=$3")
                .bind(x.f.company).bind(run).bind(x.fact).execute(&mut *tx).await.unwrap();
        }
        assert!(tx.commit().await.is_err(), "{mutation} must not commit");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM run_context_snapshots WHERE company_id=$1 AND run_id=$2"
            )
            .bind(x.f.company)
            .bind(run)
            .fetch_one(&x.f.pool)
            .await
            .unwrap(),
            0
        );
    }
    assert!(matches!(
        x.f.control
            .freeze_run_snapshot(&x.scope, &lease, &authority, run, &candidate)
            .await
            .unwrap(),
        FreezeSnapshotOutcome::Ready(_)
    ));
}
