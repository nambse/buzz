//! Exercise the exact production ACK statement, production transition guard and
//! real generation takeover. Every change is rolled back before WS publication.
use super::*;

const ACK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/postgres/confidential/reply/ack.sql"
));

pub(super) async fn prove(x: &EncryptedFixture, run: Uuid) {
    let company = x.f.scope.company_id();
    let community = x.f.scope.community_id().unwrap();
    let token = Uuid::new_v4();
    let replacement = Uuid::new_v4();
    let mut tx = x.f.pool.begin().await.unwrap();
    sqlx::query("SET LOCAL statement_timeout='2s'")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    let current: bool = sqlx::query_scalar("SELECT ortak_lock_confidential_dm($1,$2)")
        .bind(company)
        .bind(run)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(current);
    let deadline: chrono::DateTime<Utc> = sqlx::query_scalar(
        "SELECT execution_deadline FROM confidential_runs WHERE company_id=$1 AND run_id=$2",
    )
    .bind(company)
    .bind(run)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    // This is a fresh legitimate short claim, accepted by the actual guard;
    // neither a prior lease nor the immutable run deadline is rewritten.
    let expires: chrono::DateTime<Utc> = sqlx::query_scalar(
        "UPDATE confidential_reply_outbox SET generation=generation+1,attempts=attempts+1,
            lease_token=$3,lease_expires_at=clock_timestamp()+interval '100 milliseconds'
         WHERE company_id=$1 AND run_id=$2 AND copy=0 AND state='pending' AND attempts=0
         RETURNING lease_expires_at",
    )
    .bind(company)
    .bind(run)
    .bind(token)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(5200)).await;
    sqlx::query("SAVEPOINT before_takeover")
        .execute(&mut *tx)
        .await
        .unwrap();
    let generation: i64 = sqlx::query_scalar(
        "UPDATE confidential_reply_outbox SET generation=generation+1,attempts=attempts+1,
            lease_token=$3,lease_expires_at=clock_timestamp()+interval '20 seconds'
         WHERE company_id=$1 AND run_id=$2 AND copy=0 RETURNING generation",
    )
    .bind(company)
    .bind(run)
    .bind(replacement)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(generation, 2);
    for (old_generation, old_token) in [(1_i64, token), (1, replacement), (2, token)] {
        let changed = sqlx::query(ACK)
            .bind(company)
            .bind(community)
            .bind(run)
            .bind(0_i32)
            .bind(old_generation)
            .bind(old_token)
            .execute(&mut *tx)
            .await
            .unwrap()
            .rows_affected();
        assert_eq!(
            changed, 0,
            "an older or mismatched owner cannot record an ACK"
        );
    }
    sqlx::query("ROLLBACK TO SAVEPOINT before_takeover")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SAVEPOINT before_expired_retry")
        .execute(&mut *tx)
        .await
        .unwrap();
    let denied = sqlx::query(
        "UPDATE confidential_reply_outbox SET lease_token=NULL,lease_expires_at=NULL,
            next_attempt_at=clock_timestamp()+interval '5 seconds',error_code='unavailable'
         WHERE company_id=$1 AND run_id=$2 AND copy=0 AND generation=1 AND lease_token=$3",
    )
    .bind(company)
    .bind(run)
    .bind(token)
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert_eq!(
        denied.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("23514")
    );
    sqlx::query("ROLLBACK TO SAVEPOINT before_expired_retry")
        .execute(&mut *tx)
        .await
        .unwrap();
    let changed = sqlx::query(ACK)
        .bind(company)
        .bind(community)
        .bind(run)
        .bind(0_i32)
        .bind(1_i64)
        .bind(token)
        .execute(&mut *tx)
        .await
        .unwrap()
        .rows_affected();
    assert_eq!(
        changed, 1,
        "known ACK survives expiry of the same unchanged claim"
    );
    let receipt: (String, chrono::DateTime<Utc>, Option<Uuid>) = sqlx::query_as(
        "SELECT state,acknowledged_at,lease_token FROM confidential_reply_outbox
         WHERE company_id=$1 AND run_id=$2 AND copy=0",
    )
    .bind(company)
    .bind(run)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(receipt.0, "acked");
    assert!(receipt.1 > expires);
    assert!(receipt.2.is_none());
    let retained: chrono::DateTime<Utc> = sqlx::query_scalar(
        "SELECT execution_deadline FROM confidential_runs WHERE company_id=$1 AND run_id=$2",
    )
    .bind(company)
    .bind(run)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(retained, deadline);
    tx.rollback().await.unwrap();
    let copies: Vec<(i32,String,i32)> = sqlx::query_as(
        "SELECT copy,state,attempts FROM confidential_reply_outbox WHERE company_id=$1 AND run_id=$2 ORDER BY copy"
    ).bind(company).bind(run).fetch_all(&x.f.pool).await.unwrap();
    assert_eq!(
        copies,
        vec![(0, "pending".into(), 0), (1, "pending".into(), 0)]
    );
}
