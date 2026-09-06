use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal69"]
async fn reviewed_export_failure_is_durable_bounded_and_does_not_hot_loop() {
    let x = ExportFixture::new(Duration::from_secs(86400), true).await;
    x.publish().await;
    let adapter = ObservedAdapter {
        unavailable: true,
        ..Default::default()
    };
    assert!(!schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    assert!(!schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    assert_eq!(
        adapter.calls.lock().unwrap().len(),
        1,
        "backoff survives the next worker pass"
    );
    let state:(String,i32,bool,bool,String)=sqlx::query_as("SELECT state,attempt_count,next_attempt_at>clock_timestamp(),lease_token IS NULL,last_error_code
        FROM reviewed_memory_export_jobs WHERE company_id=$1 AND fact_id=$2 AND action='publish'")
        .bind(x.f.company).bind(x.fact).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(
        state,
        ("pending".into(), 1, true, true, "service_retry".into())
    );
    assert!(sqlx::query("UPDATE reviewed_memory_export_jobs SET state='acknowledged' WHERE company_id=$1 AND fact_id=$2 AND action='publish'")
        .bind(x.f.company).bind(x.fact).execute(&x.f.pool).await.is_err());
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal69"]
async fn reviewed_export_same_job_retry_and_atomic_receipt_fence_old_owners() {
    let x = ExportFixture::new(Duration::from_secs(86400), true).await;
    x.publish().await;
    let old = exports::claim(&x.f.control, &x.scope)
        .await
        .unwrap()
        .unwrap();
    let request = exports::prepare(&x.f.control, &x.scope, &old)
        .await
        .unwrap()
        .unwrap();
    let receipt = acknowledgement(&request);
    // Deliberately wrong binding receipt cannot commit the preceding job ACK.
    let mut wrong = acknowledgement(&request);
    wrong.binding_hash = vec![0; 32];
    assert!(exports::acknowledge(&x.f.control, &x.scope, &old, &wrong)
        .await
        .is_err());
    let pending:String=sqlx::query_scalar("SELECT state FROM reviewed_memory_export_jobs WHERE company_id=$1 AND fact_id=$2 AND action='publish'")
        .bind(x.f.company).bind(x.fact).fetch_one(&x.f.pool).await.unwrap();
    assert_eq!(pending, "pending");
    assert!(
        exports::fail(&x.f.control, &x.scope, &old, "service_refused", true)
            .await
            .unwrap()
    );
    // Neither a missing human receipt nor an unversioned failed→pending edit can reopen the job.
    for statement in ["UPDATE reviewed_memory_export_jobs SET state='pending' WHERE company_id=$1 AND fact_id=$2 AND action='publish'",
        "UPDATE reviewed_memory_export_jobs SET state='pending',attempt_count=0,retry_version=1,last_error_code=NULL,next_attempt_at=clock_timestamp() WHERE company_id=$1 AND fact_id=$2 AND action='publish'",
        "DELETE FROM reviewed_memory_export_jobs WHERE company_id=$1 AND fact_id=$2"] {
        assert!(sqlx::query(statement).bind(x.f.company).bind(x.fact).execute(&x.f.pool).await.is_err());
    }
    let retry = json!({"operation_id":Uuid::new_v4(),"retry_version":0});
    let path = format!("{}/exports/publish/retry", x.path());
    for _ in 0..2 {
        let result = post(&x.app, &x.f.operator, &path, &retry).await;
        assert_eq!(result.0, StatusCode::OK, "{result:?}");
    }
    let new = exports::claim(&x.f.control, &x.scope)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(old.token, new.token);
    assert_eq!(new.total_attempts, 2);
    let next = exports::prepare(&x.f.control, &x.scope, &new)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(request.idempotency_key, next.idempotency_key);
    assert_eq!(request.request_hash, next.request_hash);
    assert_eq!(request.content, next.content);
    assert!(
        !exports::acknowledge(&x.f.control, &x.scope, &old, &receipt)
            .await
            .unwrap()
    );
    assert!(exports::acknowledge(&x.f.control, &x.scope, &new, &receipt)
        .await
        .unwrap());
    assert!(
        !exports::acknowledge(&x.f.control, &x.scope, &new, &receipt)
            .await
            .unwrap()
    );
    assert!(
        !exports::fail(&x.f.control, &x.scope, &new, "deadline", false)
            .await
            .unwrap()
    );
    assert_eq!(x.counts().await, (1, 2, 2));
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal69"]
async fn reviewed_export_expiry_uses_the_original_scheduled_withdrawal() {
    let x = ExportFixture::new(Duration::from_secs(3), true).await;
    x.publish().await;
    let adapter = ObservedAdapter::default();
    assert!(schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    assert!(!schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    let key:String=sqlx::query_scalar("SELECT idempotency_key FROM reviewed_memory_export_jobs WHERE company_id=$1 AND fact_id=$2 AND action='withdraw'")
        .bind(x.f.company).bind(x.fact).fetch_one(&x.f.pool).await.unwrap();
    tokio::time::sleep(Duration::from_millis(3100)).await;
    assert!(schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    assert!(!schedule_one(&x.f.control, &x.scope, &adapter)
        .await
        .unwrap());
    assert_eq!(adapter.calls.lock().unwrap()[1].1, key);
    assert_eq!(
        x.page().await["facts"][0]["export"]["erased_from_reviewed_store"],
        true
    );
    assert_eq!(x.counts().await, (1, 2, 1));
}
