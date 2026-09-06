//! Real Work admission/materialization across sealed employee lifecycle cycles.
use super::*;
use ortak_runtime::{
    DispatchAuthorization, DispatchRefusal, PrepareOutcome, RunDispatchRepository,
};
#[path = "../../../../../ortak-control/tests/lifecycle_support.rs"]
mod lifecycle_support;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with lifecycle schema"]
async fn lifecycle_old_work_dispatch_refuses_and_new_work_pins_the_fresh_epoch() {
    let f = Fixture::new().await;
    let employee = employee(&f).await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let (_, item) = ready(&f, &app).await;
    let (old, _) = queue(&f, &app, &item).await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let leases = f
        .control
        .claim_runtime_dispatches(
            &scope,
            "fake-runtime",
            "epoch-held",
            Duration::from_secs(60),
            8,
        )
        .await
        .unwrap();
    assert_eq!(leases.len(), 1);
    let held = f
        .control
        .authorize_dispatch(&scope, &leases[0])
        .await
        .unwrap();
    let DispatchAuthorization::Authorized(held) = held else {
        panic!("current Work input must authorize")
    };
    lifecycle_support::cycle(&f.pool, &f.control, &scope, &employee).await;
    assert!(matches!(
        f.control
            .authorize_dispatch(&scope, &leases[0])
            .await
            .unwrap(),
        DispatchAuthorization::Refused(DispatchRefusal::WorkAuthorityChanged)
    ));
    assert!(matches!(
        f.control.prepare_run(&scope, &held).await.unwrap(),
        PrepareOutcome::Refused(DispatchRefusal::WorkAuthorityChanged)
    ));
    let status:(String,Option<String>,i64)=sqlx::query_as("SELECT status,runtime_run_ref,employee_lifecycle_epoch FROM runs WHERE company_id=$1 AND id=$2").bind(f.company).bind(old).fetch_one(&f.pool).await.unwrap();
    assert_eq!(status, ("queued".into(), None, 0));
    let (_, fresh_item) = ready(&f, &app).await;
    let (fresh, _) = queue(&f, &app, &fresh_item).await;
    let (adapter, _, _) = start(&f, &employee, fresh).await;
    assert_eq!(adapter.start_specs().len(), 1);
    let pin: i64 = sqlx::query_scalar(
        "SELECT employee_lifecycle_epoch FROM runs WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company)
    .bind(fresh)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(pin, 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with lifecycle schema"]
async fn lifecycle_old_terminal_work_output_cannot_materialize_artifact_or_open_review() {
    let f = Fixture::new().await;
    let employee = employee(&f).await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let (_, item) = ready(&f, &app).await;
    let (run, _) = queue(&f, &app, &item).await;
    let (adapter, memory, reference) = start(&f, &employee, run).await;
    complete(
        &f,
        &adapter,
        &memory,
        run,
        &reference,
        BoundedText::raw("Old epoch deliverable"),
    )
    .await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    lifecycle_support::cycle(&f.pool, &f.control, &scope, &employee).await;
    let report = schedule_work_outputs(&f.control, &scope, 8).await.unwrap();
    assert_eq!((report.attempted, report.materialized), (1, 0));
    let output: String = sqlx::query_scalar(
        "SELECT state FROM runtime_work_outputs WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.company)
    .bind(run)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(output, "failed");
    let (_, saved) = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}", id(&item)),
    )
    .await;
    assert_eq!(saved["work_item"]["state"], "in_progress");
    assert_eq!(saved["work_item"]["version"], version(&item) + 1);
    let artifacts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM artifacts WHERE company_id=$1 AND run_id=$2")
            .bind(f.company)
            .bind(run)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(artifacts, 0);
    assert_eq!(saved["work_item"]["approvals"], item["approvals"]);
}
