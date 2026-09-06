//! Removing and re-adding an edge cannot revive the original dispatch witness.
use super::*;
use ortak_runtime::{
    DispatchAuthorization, DispatchRefusal, PrepareOutcome, RunDispatchRepository,
};

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with dependency schema"]
async fn dependency_remove_readd_refuses_held_prepare_with_the_same_final_graph() {
    let f = Fixture::new().await;
    employee(&f).await;
    let app = work_app(&f, true, Role::Reader, vec![f.channel]);
    let (project, source) = ready(&f, &app).await;
    let target = item(&f, &app, project).await;
    let target = transition(&f, &app, target, "cancelled").await;
    let path = format!("/api/v1/work-items/{}/dependencies", id(&source));
    let added=post(&app,&f.operator,&path,&json!({"operation_id":Uuid::new_v4(),"expected_version":version(&source),"depends_on":id(&target)})).await;
    assert_eq!(added.0, StatusCode::OK);
    let source = added.1["work_item"].clone();
    let edge = id(&get(&app, &f.operator, &path).await.1["dependencies"][0]);
    let (run, _) = queue(&f, &app, &source).await;
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
            "held-dependency",
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
        panic!("initial graph is executable")
    };
    let remove=post(&app,&f.operator,&format!("{path}/{edge}/remove"),&json!({"operation_id":Uuid::new_v4(),"expected_version":version(&source)+1,"reason":"Correct graph"})).await;
    assert_eq!(remove.0, StatusCode::OK);
    let readd=post(&app,&f.operator,&path,&json!({"operation_id":Uuid::new_v4(),"expected_version":version(&remove.1["work_item"]),"depends_on":id(&target)})).await;
    assert_eq!(readd.0, StatusCode::OK);
    assert_eq!(
        get(&app, &f.operator, &path).await.1["dependencies"][0]["id"],
        edge.to_string()
    );
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
    let state: (String, Option<String>) =
        sqlx::query_as("SELECT status,runtime_run_ref FROM runs WHERE company_id=$1 AND id=$2")
            .bind(f.company)
            .bind(run)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(state, ("queued".into(), None));
}
