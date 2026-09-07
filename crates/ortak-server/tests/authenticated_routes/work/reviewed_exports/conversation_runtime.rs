//! V4 inputs originate in real routing/Work admission, never forged snapshot rows.
use super::*;
use ortak_control::fakes::{FakeMemoryAdapter, FakeRuntimeAdapter};
use ortak_control::memory::*;
use ortak_control::run_event::{BoundedText, DeliveryIntentKind};
use ortak_control::runtime::RuntimeRunRef;
use ortak_office::fakes::{FakeOfficePublisher, FakeOfficeSigner};
use ortak_office::{DeliveryConfig, OfficeDeliveryService};
use ortak_runtime::memory_context::*;
use ortak_runtime::reviewed_memory::*;
use ortak_runtime::{
    DispatchAuthority, DispatchOutcome, DispatchRefusal, RunSupervisor, SupervisorConfig,
};

#[path = "conversation_runtime/adapter.rs"]
mod adapter;
#[path = "conversation_runtime/current_work.rs"]
mod current_work;
#[path = "conversation_runtime/delivery.rs"]
mod delivery;
#[path = "conversation_runtime/employee.rs"]
mod employee;
#[path = "conversation_runtime/fixture.rs"]
mod fixture;
use fixture::*;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with conversation76"]
async fn conversation_runtime_office_selects_exact_thread_after_nonzero_opt_in_epoch() {
    let c = ConversationFixture::new().await;
    let wrong_source = boundaries::source_message(&c.x.f, c.x.f.channel).await;
    let wrong = c
        .approve_publish("thread", &wrong_source, "Deployment wrong-thread canary")
        .await;
    c.opt_out().await;
    super::conversation_publication::advertise(&c.x).await;
    let (run, reference) = c.start_office().await;
    let wire = c.wire(run).await;
    assert_eq!(wire["version"], 4);
    assert_eq!(wire["conversation"]["records"].as_array().unwrap().len(), 1);
    let pin = &wire["conversation"]["records"][0]["record"]["pin"];
    assert_eq!(pin["fact_id"], json!(c.fact));
    assert_eq!(pin["conversation_consumption_epoch"], 1);
    assert_eq!(pin["consumption_epoch"], 0);
    assert!(!wire.to_string().contains(&wrong.to_string()));
    assert_eq!(
        c.memory.selected.lock().unwrap().as_slice(),
        &[vec![c.fact]]
    );
    assert!(c.current(run).await);
    let (status, body) = c.read(run).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["memory"]["scope"],
        "run_scratch_and_reviewed_conversation"
    );
    assert_eq!(body["memory"]["reviewed"][0]["content"]["text"], FACT);
    assert_eq!(
        body["memory"]["reviewed"][0]["audience_kind"],
        "conversation"
    );
    assert_eq!(body["memory"]["reviewed"][0]["audience"]["kind"], "thread");
    assert_eq!(
        body["memory"]["reviewed"][0]["audience"]["channel_id"],
        json!(c.x.f.channel)
    );
    assert_eq!(
        body["memory"]["reviewed"][0]["audience"]["thread_root_event_id"],
        c.x.source
    );
    assert_eq!(
        body["memory"]["recall"]["records"][0]["content"]["text"],
        SCRATCH
    );
    c.complete(run, &reference).await;
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with conversation76"]
async fn conversation_runtime_promoted_work_freezes_v4_and_old_epoch_cannot_materialize() {
    let c = ConversationFixture::new().await;
    let item = c.ready_work().await;
    let (run, _) = super::super::execution::fixture::queue(&c.x.f, &c.x.app, &item).await;
    let reference = c.dispatch(run).await;
    let before = c.bytes(run).await;
    assert_eq!(c.wire(run).await["version"], 4);
    // An actual source edit, then restoration, must not revive a frozen use.
    c.edit_fact_source(true).await;
    c.edit_fact_source(false).await;
    assert!(!c.current(run).await);
    c.complete(run, &reference).await;
    let report = ortak_work::schedule_work_outputs(&c.x.f.control, &c.x.scope, 8)
        .await
        .unwrap();
    assert_eq!(report.materialized, 0);
    let artifacts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM artifacts WHERE company_id=$1 AND run_id=$2")
            .bind(c.x.f.company)
            .bind(run)
            .fetch_one(&c.x.f.pool)
            .await
            .unwrap();
    assert_eq!(artifacts, 0);
    assert_eq!(c.bytes(run).await, before);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 PostgreSQL with conversation76"]
async fn conversation_runtime_office_epoch_revocation_stops_without_renewing_admission() {
    let c = ConversationFixture::new().await;
    let (run, _) = c.start_office().await;
    let before = c.bytes(run).await;
    let token: Option<Uuid> =
        sqlx::query_scalar("SELECT office_admission_token FROM runs WHERE company_id=$1 AND id=$2")
            .bind(c.x.f.company)
            .bind(run)
            .fetch_one(&c.x.f.pool)
            .await
            .unwrap();
    c.opt_out().await;
    super::conversation_publication::advertise(&c.x).await;
    let report =
        ortak_runtime::reconciliation::reconcile_office_runs(&c.x.f.control, &c.x.scope, 8)
            .await
            .unwrap();
    assert_eq!((report.reviewed, report.revocations), (1, 1));
    let after: Option<Uuid> =
        sqlx::query_scalar("SELECT office_admission_token FROM runs WHERE company_id=$1 AND id=$2")
            .bind(c.x.f.company)
            .bind(run)
            .fetch_one(&c.x.f.pool)
            .await
            .unwrap();
    assert_eq!(token, after);
    let report = ortak_runtime::reconciliation::reconcile_runtime(
        &c.x.f.control,
        &c.runtime,
        &c.x.scope,
        &SupervisorConfig::default(),
        8,
    )
    .await
    .unwrap();
    assert_eq!(report.stop_attempts, 1);
    let acknowledged: bool = sqlx::query_scalar(
        "SELECT state='acknowledged' FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2",
    )
    .bind(c.x.f.company)
    .bind(run)
    .fetch_one(&c.x.f.pool)
    .await
    .unwrap();
    assert!(acknowledged);
    let (status, body) = c.read(run).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["memory"]["recall"]["withheld"], true);
    assert_eq!(body["memory"]["recall"]["records"], json!([]));
    assert_eq!(body["memory"]["reviewed"][0]["current"], false);
    assert!(body["memory"]["reviewed"][0]["content"].is_null());
    assert!(body["memory"]["reviewed"][0]["audience"].is_null());
    assert!(!body["memory"].to_string().contains(FACT));
    assert!(!body["memory"].to_string().contains(SCRATCH));
    assert_eq!(c.bytes(run).await, before);
}
