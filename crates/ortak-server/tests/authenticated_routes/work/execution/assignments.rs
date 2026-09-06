//! Assignment commands fence queued, active and late terminal production execution.
use super::*;
use ortak_runtime::{
    DispatchAuthorization, DispatchRefusal, PrepareOutcome, RunDispatchRepository,
};

async fn change(f: &Fixture, app: &Router, item: Uuid, release: bool) -> Value {
    let current = get(app, &f.operator, &format!("/api/v1/work-items/{item}")).await;
    assert_eq!(current.0, StatusCode::OK);
    let mut body = json!({"operation_id":Uuid::new_v4(),"expected_version":version(&current.1["work_item"]),"reason":"Assignment changed by human"});
    let action = if release {
        "release"
    } else {
        body["replacement_employee_id"] = json!("cem");
        body["role"] = json!("contributor");
        "reassign"
    };
    let changed = post(
        app,
        &f.operator,
        &format!("/api/v1/work-items/{item}/assignments/cem/{action}"),
        &body,
    )
    .await;
    assert_eq!(changed.0, StatusCode::OK);
    changed.1["work_item"].clone()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn assignment_change_rejects_held_prepare_even_when_employee_remains_assigned() {
    let f = Fixture::new().await;
    employee(&f).await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let (_, item) = ready(&f, &app).await;
    let (run, _) = queue(&f, &app, &item).await;
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
            "held-assignment",
            Duration::from_secs(60),
            8,
        )
        .await
        .unwrap();
    assert_eq!(leases.len(), 1);
    let DispatchAuthorization::Authorized(held) = f
        .control
        .authorize_dispatch(&scope, &leases[0])
        .await
        .unwrap()
    else {
        panic!("initial dispatch must authorize")
    };
    let changed = change(&f, &app, id(&item), false).await;
    assert_eq!(changed["assignments"][0]["status"], "active");
    assert_eq!(changed["assignments"][0]["role"], "contributor");
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
    let saved: (String, Option<String>) =
        sqlx::query_as("SELECT status,runtime_run_ref FROM runs WHERE company_id=$1 AND id=$2")
            .bind(f.company)
            .bind(run)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(saved, ("queued".into(), None));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn assignment_release_stops_active_runtime_through_durable_work_revocation() {
    let f = Fixture::new().await;
    let employee = employee(&f).await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let (_, item) = ready(&f, &app).await;
    let (run, _) = queue(&f, &app, &item).await;
    let (adapter, _, _) = start(&f, &employee, run).await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    change(&f, &app, id(&item), true).await;
    let report = ortak_runtime::reconciliation::reconcile_runtime(
        &f.control,
        &adapter,
        &scope,
        &SupervisorConfig::default(),
        64,
    )
    .await
    .unwrap();
    assert_eq!((report.revocations, report.stop_attempts), (1, 1));
    let saved:(String,String,String) = sqlx::query_as("SELECT r.status,c.state,c.reason FROM runs r JOIN runtime_cancellations c ON c.company_id=r.company_id AND c.run_id=r.id WHERE r.company_id=$1 AND r.id=$2")
        .bind(f.company).bind(run).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        saved,
        (
            "cancelled".into(),
            "acknowledged".into(),
            "work_revoked".into()
        )
    );
    assert_eq!(
        ortak_runtime::reconciliation::reconcile_runtime(
            &f.control,
            &adapter,
            &scope,
            &SupervisorConfig::default(),
            64
        )
        .await
        .unwrap()
        .stop_attempts,
        0
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn assignment_change_withholds_old_terminal_artifact_and_preserves_human_acceptance() {
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
        BoundedText::raw("Old assignment deliverable"),
    )
    .await;
    let changed = change(&f, &app, id(&item), false).await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let report = schedule_work_outputs(&f.control, &scope, 8).await.unwrap();
    assert_eq!((report.attempted, report.materialized), (1, 0));
    let artifacts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM artifacts WHERE company_id=$1 AND run_id=$2")
            .bind(f.company)
            .bind(run)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(artifacts, 0);
    let saved = get(
        &app,
        &f.operator,
        &format!("/api/v1/work-items/{}", id(&item)),
    )
    .await;
    assert_eq!(saved.1["work_item"], changed);
    assert_eq!(changed["state"], "in_progress");
    assert_eq!(changed["criteria"], item["criteria"]);
    assert_eq!(changed["approvals"], item["approvals"]);
}
