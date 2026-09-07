//! Recovery must remain available when external credentials are missing.

use super::*;
use ortak_control::{
    ports::ProvisioningRepository,
    provisioning::{OperationStatus, OperationUpdate},
};

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432"]
async fn adopted_compensation_does_not_require_revoked_external_credentials() {
    let (pool, scope, value) = fixture().await;
    assert!(provision_once(pool.clone(), &value.to_string(), false)
        .await
        .is_err());
    let id: Uuid = sqlx::query_scalar("SELECT id FROM provisioning_operations WHERE company_id=$1")
        .bind(scope.company_id())
        .fetch_one(&pool)
        .await
        .unwrap();
    PgControlPlane::new(pool.clone())
        .update_operation(
            &scope,
            id,
            &OperationUpdate {
                status: OperationStatus::Failed,
                current_step: None,
                error_message: Some("selected credential unavailable".into()),
            },
        )
        .await
        .unwrap();
    for _ in 0..2 {
        let result = provision_once(pool.clone(), &value.to_string(), true)
            .await
            .unwrap();
        assert_eq!(result.operation_id, id);
        assert_eq!(result.status, "compensated");
        assert_eq!(result.revision_id, None);
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM office_identity_profiles WHERE company_id=$1")
            .bind(scope.company_id())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432"]
async fn different_operation_keys_cannot_provision_one_employee_concurrently() {
    let (pool, scope, mut value) = fixture().await;
    let mut lock = pool.acquire().await.unwrap();
    lock.close_on_drop();
    let key = format!(
        "ortak-provision-employee:{}:prepared-fixture",
        scope.company_id()
    );
    sqlx::query("SELECT pg_advisory_lock(hashtextextended($1,0))")
        .bind(key)
        .execute(&mut *lock)
        .await
        .unwrap();
    for _ in 0..2 {
        value["operation_key"] = json!(Uuid::new_v4().to_string());
        assert_eq!(
            provision_once(pool.clone(), &value.to_string(), false)
                .await
                .err(),
            Some("selected provisioning operation is already running")
        );
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM provisioning_operations WHERE company_id=$1")
            .bind(scope.company_id())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    drop(lock);
    pool.close().await;
}
