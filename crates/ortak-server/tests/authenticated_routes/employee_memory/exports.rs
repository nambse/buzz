use super::*;
use ortak_server::employee_memory_exports as exports;
#[path = "export_remote.rs"]
pub(crate) mod remote;
use remote::Remote;

async fn approved(x: &MemoryFixture, kind: &str) -> Uuid {
    let command = x.command(&x.preview(&x.f.operator, kind).await);
    let (status, value) = post(&x.app, &x.f.operator, PATH, &command).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    Uuid::parse_str(value["fact"]["id"].as_str().unwrap()).unwrap()
}
fn path(fact: Uuid) -> String {
    format!("{PATH}/{fact}/export")
}
async fn enqueue(x: &MemoryFixture, fact: Uuid) -> Value {
    let command = json!({"operation_id":Uuid::new_v4(),"expected_version":1});
    let (status, value) = post(&x.app, &x.f.operator, &path(fact), &command).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["created"], true);
    command
}

#[tokio::test]
#[ignore = "requires explicit disposable55432 plus employee storage/source/protocol candidate assembly and local HTTP sockets"]
async fn employee_exports_signed_registration_publish_stop_and_original_cleanup() {
    let x = MemoryFixture::new_owned().await;
    let remote = Remote::new(&x).await;
    let (target, until) = remote.register(&x).await;
    assert_eq!(remote.diagnostic_count(), 3);
    let registered:Value=sqlx::query_scalar("SELECT registration_receipt FROM employee_reviewed_memory_targets WHERE company_id=$1 AND id=$2")
        .bind(x.f.company).bind(target).fetch_one(&x.f.pool).await.unwrap();
    let fact = approved(&x, "relationship").await;
    let command = enqueue(&x, fact).await;
    let worker = exports::HonchoEmployeeExportAdapter::new(&remote.service);
    assert!(
        exports::schedule_one(&x.f.control, &remote.scope, &worker)
            .await
            .unwrap()
    );
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM employee_reviewed_memory_exports WHERE company_id=$1),
        (SELECT count(*) FROM employee_reviewed_memory_export_receipts WHERE company_id=$1)",
    )
    .bind(x.f.company)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1));
    let (status, replay) = post(&x.app, &x.f.operator, &path(fact), &command).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["created"], false);
    assert!(
        exports::refresh_target(
            &x.f.control,
            &remote.scope,
            &remote.service,
            &remote.namespace,
            target,
            true,
            until
        )
        .await
        .unwrap()
    );
    assert!(
        !exports::refresh_target(
            &x.f.control,
            &remote.scope,
            &remote.service,
            &remote.namespace,
            target,
            true,
            until + chrono::Duration::days(1)
        )
        .await
        .unwrap(),
        "refresh cannot extend expiry"
    );
    let same:Value=sqlx::query_scalar("SELECT registration_receipt FROM employee_reviewed_memory_targets WHERE company_id=$1 AND id=$2")
        .bind(x.f.company).bind(target).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(same, registered);
    sqlx::query("UPDATE events SET deleted_at=clock_timestamp() WHERE community_id=$1 AND id=$2")
        .bind(x.f.community)
        .bind(hex::decode(&x.source).unwrap())
        .execute(&x.f.pool)
        .await
        .unwrap();
    let recovery = app(
        &x.f,
        &x.f.operator,
        false,
        Role::Reader,
        vec![x.f.channel],
        vec!["cem"],
    );
    let stop = json!({"operation_id":Uuid::new_v4(),"expected_version":1});
    let (status, value) = post(
        &recovery,
        &x.f.operator,
        &format!("{PATH}/{fact}/stop"),
        &stop,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert!(
        exports::schedule_one(&x.f.control, &remote.scope, &worker)
            .await
            .unwrap()
    );
    let (status, value) = get(&recovery, &x.f.operator, &path(fact)).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert!(
        value["export"]["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| j["acknowledged"] == true)
    );
    assert!(!value.to_string().contains("The human edited"));
    let last = remote.state.lock().unwrap().calls.last().unwrap().clone();
    assert!(last.0.ends_with("/withdraw"));
    assert!(last.1.get("content").is_none() && last.1.get("provenance").is_none());
    assert_eq!(
        remote.diagnostic_count(),
        3,
        "ordinary refresh/export/cleanup must not write a new diagnostic"
    );
    let (status, replay) = post(&recovery, &x.f.operator, &path(fact), &command).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["created"], false);
    let denied = app(
        &x.f,
        &x.f.operator,
        true,
        Role::Operator,
        vec![x.f.channel, x.f.hidden],
        vec!["other"],
    );
    assert_eq!(
        get(&denied, &x.f.operator, &path(fact)).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable55432 plus employee storage/source/protocol candidate assembly and local HTTP sockets"]
async fn employee_exports_signed_failed_receipt_retry_and_live_lease_cas() {
    let x = MemoryFixture::new_owned().await;
    let remote = Remote::new(&x).await;
    remote.register(&x).await;
    let fact = approved(&x, "experience").await;
    let command = enqueue(&x, fact).await;
    let foreign = path(fact).replace("/cem/", "/other/");
    assert_eq!(
        post(&x.app, &x.f.operator, &foreign, &command).await.0,
        StatusCode::FORBIDDEN
    );
    let lease = exports::claim(&x.f.control, &remote.scope)
        .await
        .unwrap()
        .unwrap();
    assert!(
        exports::claim(&x.f.control, &remote.scope)
            .await
            .unwrap()
            .is_none(),
        "live publication lease cannot be stolen"
    );
    let request = exports::prepare(&x.f.control, &remote.scope, &lease)
        .await
        .unwrap()
        .unwrap();
    let adapter = exports::HonchoEmployeeExportAdapter::new(&remote.service);
    use exports::EmployeeExportAdapter;
    remote.state.lock().unwrap().forge_ack = true;
    assert!(
        adapter.write(&request).await.is_err(),
        "untrusted receipt cannot become a database ACK"
    );
    assert!(
        exports::fail(&x.f.control, &remote.scope, &lease, "service_refused", true)
            .await
            .unwrap()
    );
    assert!(
        !exports::fail(&x.f.control, &remote.scope, &lease, "service_retry", false)
            .await
            .unwrap(),
        "the cleared old lease cannot change the failed row"
    );
    let none: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM employee_reviewed_memory_export_receipts WHERE company_id=$1",
    )
    .bind(x.f.company)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(none, 0);
    let retry = json!({"operation_id":Uuid::new_v4(),"expected_version":0});
    let retry_path = format!("{}/retry/publish", path(fact));
    let (status, receipt) = post(&x.app, &x.f.operator, &retry_path, &retry).await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    assert_eq!(receipt["result_version"], 1);
    let (status, replayed) = post(&x.app, &x.f.operator, &retry_path, &retry).await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(replayed["created"], false);
    let mut changed = retry.clone();
    changed["expected_version"] = json!(1);
    assert_eq!(
        post(&x.app, &x.f.operator, &retry_path, &changed).await.0,
        StatusCode::CONFLICT
    );
    remote.state.lock().unwrap().forge_ack = false;
    assert!(
        exports::schedule_one(&x.f.control, &remote.scope, &adapter)
            .await
            .unwrap()
    );
    let state: (String, i32, i32) = sqlx::query_as(
        "SELECT state,retry_version,total_attempts FROM employee_reviewed_memory_export_jobs
        WHERE company_id=$1 AND fact_id=$2 AND action='publish'",
    )
    .bind(x.f.company)
    .bind(fact)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(state, ("acknowledged".into(), 1, 2));
    let hashes: Vec<String> = remote
        .state
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter(|(p, _)| p.ends_with("/publish"))
        .map(|(_, b)| b["idempotency_key"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(hashes.len(), 2);
    assert_eq!(hashes[0], hashes[1]);
    assert_eq!(remote.diagnostic_count(), 3);
}
