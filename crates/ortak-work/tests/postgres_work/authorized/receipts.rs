//! Deferred database guards must remain effective without the facade precheck.
use super::*;

#[tokio::test]
#[ignore = "requires disposable Postgres"]
async fn receipt_commit_refuses_elapsed_authority_and_cross_project_item() {
    let f = ApiFixture::new().await;
    let project = f.project().await;
    let other = f.project().await;
    let item = f
        .api
        .create_work_item(Uuid::new_v4(), f.item_input(project))
        .await
        .unwrap()
        .item;
    for (receipt_project, expired, expected) in [(project, true, "40001"), (other, false, "23503")]
    {
        let op = Uuid::new_v4();
        let mut tx = f.company.pool.begin().await.unwrap();
        sqlx::query("INSERT INTO work_api_operations(company_id,actor_pubkey,operation_id,action,request_hash,project_id,work_item_id,result_version,auth_event_id,valid_before)
 VALUES($1,$2,$3,'mutate_work_item',$4,$5,$6,2,$4,CASE WHEN $7 THEN clock_timestamp()-interval '1 millisecond' ELSE NULL END)")
            .bind(f.company.scope.company_id()).bind(f.key.to_hex()).bind(op).bind([0_u8;32].as_slice())
            .bind(receipt_project).bind(item.item.id).bind(expired).execute(&mut *tx).await.unwrap();
        let error = tx
            .commit()
            .await
            .expect_err("the actual deferred production guard must reject this receipt");
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some(expected)
        );
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM work_api_operations WHERE company_id=$1 AND operation_id=$2",
        )
        .bind(f.company.scope.company_id())
        .bind(op)
        .fetch_one(&f.company.pool)
        .await
        .unwrap();
        assert_eq!(count, 0);
    }
}
