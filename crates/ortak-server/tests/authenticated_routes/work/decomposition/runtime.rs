use super::super::execution::fixture;
use super::*;
use ortak_runtime::{
    DispatchAuthorization, DispatchRefusal, PrepareOutcome, RunDispatchRepository,
};
use std::time::Duration;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with decomposition schema"]
async fn decomposition_revokes_held_and_live_parent_execution_without_dispatching_the_child() {
    for live in [false, true] {
        let f = Fixture::new().await;
        let employee = fixture::employee(&f).await;
        let app = work_app(&f, true, Role::Operator, vec![f.channel]);
        let (_, parent) = fixture::ready(&f, &app).await;
        let (run, _) = fixture::queue(&f, &app, &parent).await;
        let scope = f
            .control
            .resolve_company_for_community(f.community)
            .await
            .unwrap();
        let current = get(
            &app,
            &f.operator,
            &format!("/api/v1/work-items/{}", id(&parent)),
        )
        .await
        .1["work_item"]
            .clone();
        if live {
            let (adapter, _, _) = fixture::start(&f, &employee, run).await;
            let created = create(&f, &app, &current).await;
            let revoked = ortak_runtime::reconciliation::reconcile_runtime(
                &f.control,
                &adapter,
                &scope,
                &ortak_runtime::SupervisorConfig::default(),
                64,
            )
            .await
            .unwrap();
            assert_eq!((revoked.revocations, revoked.stop_attempts), (1, 1));
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM runs WHERE company_id=$1 AND work_item_id=$2",
            )
            .bind(f.company)
            .bind(id(&created["child"]))
            .fetch_one(&f.pool)
            .await
            .unwrap();
            assert_eq!(count, 0);
            assert_eq!(adapter.start_specs().len(), 1);
        } else {
            let leases = f
                .control
                .claim_runtime_dispatches(
                    &scope,
                    "fake-runtime",
                    "held-decomposition",
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
                panic!("initial parent is executable")
            };
            create(&f, &app, &current).await;
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
            let state: (String, Option<String>) = sqlx::query_as(
                "SELECT status,runtime_run_ref FROM runs WHERE company_id=$1 AND id=$2",
            )
            .bind(f.company)
            .bind(run)
            .fetch_one(&f.pool)
            .await
            .unwrap();
            assert_eq!(state, ("queued".into(), None));
        }
    }
}
