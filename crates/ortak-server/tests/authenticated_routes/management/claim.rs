//! Force a real candidate-read → policy-lock → claim race through the executor.
use super::*;
use std::time::Duration;

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with management schema"]
async fn management_claim_rechecks_retry_deadline_after_policy_contention() {
    let (f, _, app, catalog, _) = setup().await;
    let (id, _) = admitted(&f, &app, catalog).await;
    let mut held = f.pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    sqlx::query("SELECT public_key FROM employee_management_policies WHERE company_id=$1 AND public_key=$2 FOR UPDATE")
        .bind(f.company).bind(f.operator.public_key().to_hex()).execute(&mut *held).await.unwrap();
    let control = f.control.clone();
    let community = f.community;
    let executor = tokio::spawn(async move { execute_next(&control, community).await });
    // This lock is reached only after execute_next selected its due candidate.
    let mut blocked = false;
    for _ in 0..100 {
        blocked = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
        )
        .bind(pid)
        .fetch_one(&f.pool)
        .await
        .unwrap();
        if blocked {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    if !blocked {
        held.rollback().await.unwrap();
        executor.abort();
        panic!("executor did not reach the production policy lock");
    }
    // Model the other executor's just-committed transient retry record. The
    // stale candidate must not consume its next attempt before this deadline.
    sqlx::query("UPDATE employee_management_commands SET next_attempt_at=clock_timestamp()+interval '30 seconds' WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(id).execute(&mut *held).await.unwrap();
    held.commit().await.unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), executor)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        ortak_server::management::ExecutionOutcome::Idle
    ));
    let saved:(String,i32,Option<Uuid>)=sqlx::query_as("SELECT status,attempts,operation_id FROM employee_management_commands WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(
        saved,
        ("pending".into(), 0, None),
        "removing the locked due predicate consumes an attempt and starts the real runner"
    );
}
