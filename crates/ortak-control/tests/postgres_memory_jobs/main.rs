//! Explicit disposable PostgreSQL tests for durable memory scheduling.
use std::time::Duration;

use ortak_control::memory::{MemoryScope, MemoryWriteReceipt};
use ortak_control::memory_jobs::{MemoryWriteJobOutcome, MemoryWriteJobRepository};
use ortak_control::outbox::{OutboxKind, OutboxLease};
use ortak_control::ports::{CompanyDirectory, OutboxRepository};
use ortak_control::postgres::{lock_office_authority_on, prepare_memory_write_on};
use ortak_control::{CompanyScope, PgControlPlane};
use sqlx::PgPool;
use uuid::Uuid;

mod fixture;
use fixture::Fixture;

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn unicode_whitespace_fact_boundaries_prepare_and_acknowledge_exact_bytes() {
    let content = "ab".to_owned() + &" ".repeat(16380) + "\u{3000}" + &" ".repeat(16381) + "c";
    assert_eq!(content.len(), 32767);
    let f = Fixture::with_content("completed", "reply", &content).await;
    f.control
        .complete(&f.scope, &f.outbox)
        .await
        .expect("Office acknowledged");
    let lease = f.claim().await;
    let request = f.prepare(&lease).await;
    request
        .validate()
        .expect("every fact is bounded and nonblank");
    assert_eq!(
        request
            .facts
            .iter()
            .map(|fact| fact.content.as_str())
            .collect::<String>(),
        content
    );
    assert_eq!(
        request
            .facts
            .iter()
            .map(|fact| fact.content.len())
            .collect::<Vec<_>>(),
        [1, 16384, 16382]
    );
    assert!(f
        .control
        .acknowledge_memory_write(
            &f.scope,
            &lease,
            &MemoryWriteReceipt {
                receipt_ref: "unicode-three-fact-receipt".to_owned(),
                written: 3,
            }
        )
        .await
        .expect("all three immutable facts acknowledged"));
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn trailing_newline_across_fact_boundary_preserves_every_published_byte() {
    let content = "a".repeat(16384) + "\n";
    let f = Fixture::with_content("completed", "reply", &content).await;
    f.control.complete(&f.scope, &f.outbox).await.expect("ack");
    let lease = f.claim().await;
    let request = f.prepare(&lease).await;
    assert_eq!(request.facts.len(), 2);
    assert_eq!(
        request
            .facts
            .iter()
            .map(|f| f.content.as_str())
            .collect::<String>(),
        content
    );
    assert!(request
        .facts
        .iter()
        .all(|f| !f.content.trim().is_empty() && f.content.len() <= 16384));
    assert!(f
        .control
        .acknowledge_memory_write(
            &f.scope,
            &lease,
            &MemoryWriteReceipt {
                receipt_ref: "two-fact-receipt".to_owned(),
                written: 2,
            }
        )
        .await
        .expect("both facts acknowledged"));
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn memory_lease_expiry_is_checked_at_commit_even_without_an_office_deadline() {
    let f = Fixture::new("completed", "reply").await;
    f.control.complete(&f.scope, &f.outbox).await.expect("ack");
    let lease = f.claim().await;
    sqlx::query("UPDATE runtime_memory_writes SET lease_expires_at=clock_timestamp()+interval '1 second' WHERE company_id=$1")
        .bind(f.scope.company_id()).execute(&f.pool).await.expect("near-expired lease");
    let mut tx = f.pool.begin().await.expect("prepare");
    let witness = lock_office_authority_on(&mut tx, &f.scope)
        .await
        .expect("no upcoming Office boundary");
    assert_eq!(witness.valid_before(), None);
    prepare_memory_write_on(&mut tx, &f.scope, &lease, &witness)
        .await
        .expect("prepare while lease live")
        .expect("live");
    sqlx::query("SELECT pg_sleep(greatest(0,extract(epoch FROM admission_valid_before-clock_timestamp()))::double precision+0.02) FROM runtime_memory_writes WHERE company_id=$1")
        .bind(f.scope.company_id()).execute(&mut *tx).await.expect("cross exact admission deadline");
    let error = tx
        .commit()
        .await
        .expect_err("deferred witness includes authoritative lease deadline");
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("40001")
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn changed_office_verification_refuses_old_and_refreshed_witnesses() {
    let f = Fixture::new("completed", "reply").await;
    f.control.complete(&f.scope, &f.outbox).await.expect("ack");
    let lease = f.claim().await;
    let mut tx = f.pool.begin().await.expect("capture");
    let old = lock_office_authority_on(&mut tx, &f.scope)
        .await
        .expect("original witness");
    tx.commit().await.expect("release");
    sqlx::query("UPDATE employee_office_bindings SET verified_at=NULL WHERE company_id=$1")
        .bind(f.scope.company_id())
        .execute(&f.pool)
        .await
        .expect("Office signer verification revoked");
    let mut tx = f.pool.begin().await.expect("prepare");
    let fresh = lock_office_authority_on(&mut tx, &f.scope)
        .await
        .expect("new witness");
    assert!(fresh.generation() > old.generation());
    assert!(
        prepare_memory_write_on(&mut tx, &f.scope, &lease, &old)
            .await
            .is_err(),
        "old generation fails"
    );
    assert!(
        prepare_memory_write_on(&mut tx, &f.scope, &lease, &fresh)
            .await
            .is_err(),
        "fresh generation cannot bless an unverified Office identity"
    );
    tx.rollback().await.expect("release");
    assert_eq!(
        f.control
            .fail_memory_write(&f.scope, &lease, "memory_office_revoked", true)
            .await
            .expect("visible failure"),
        MemoryWriteJobOutcome::Failed
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn repeated_admission_with_same_witness_still_checks_expiry_at_commit() {
    let f = Fixture::new("completed", "reply").await;
    f.control.complete(&f.scope, &f.outbox).await.expect("ack");
    let lease = f.claim().await;
    sqlx::query("UPDATE employee_office_bindings SET valid_until=clock_timestamp()+interval '1 second' WHERE company_id=$1")
        .bind(f.scope.company_id()).execute(&f.pool).await.expect("time boundary");
    f.prepare(&lease).await;
    let first: Uuid =
        sqlx::query_scalar("SELECT admission_token FROM runtime_memory_writes WHERE company_id=$1")
            .bind(f.scope.company_id())
            .fetch_one(&f.pool)
            .await
            .expect("first committed token");
    let mut tx = f.pool.begin().await.expect("repeated admission");
    let witness = lock_office_authority_on(&mut tx, &f.scope)
        .await
        .expect("unchanged generation and deadline");
    prepare_memory_write_on(&mut tx, &f.scope, &lease, &witness)
        .await
        .expect("prepare before expiry")
        .expect("live lease");
    let second: Uuid =
        sqlx::query_scalar("SELECT admission_token FROM runtime_memory_writes WHERE company_id=$1")
            .bind(f.scope.company_id())
            .fetch_one(&mut *tx)
            .await
            .expect("fresh token");
    assert_ne!(first, second);
    // Wait for the exact database-owned deadline, bounded by the 1s fixture
    // boundary and production statement timeout; do not guess wall-clock drift.
    sqlx::query("SELECT pg_sleep(greatest(0,extract(epoch FROM $1::timestamptz-clock_timestamp()))::double precision+0.02)")
        .bind(witness.valid_before()).execute(&mut *tx).await.expect("cross bound deadline");
    let error = tx
        .commit()
        .await
        .expect_err("deferred witness trigger rejects expired repeated admission");
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("40001")
    );
    let persisted: Uuid =
        sqlx::query_scalar("SELECT admission_token FROM runtime_memory_writes WHERE company_id=$1")
            .bind(f.scope.company_id())
            .fetch_one(&f.pool)
            .await
            .expect("rollback retains only original admission");
    assert_eq!(persisted, first);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn delivery_parent_contention_refuses_promptly_without_losing_the_job_or_lease() {
    let f = Fixture::new("completed", "reply").await;
    let mut cancellation = f
        .pool
        .begin()
        .await
        .expect("cancellation starts with run lock");
    sqlx::query("SELECT id FROM runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.scope.company_id())
        .bind(f.run_id)
        .fetch_one(&mut *cancellation)
        .await
        .expect("held run");
    let attempt = tokio::time::timeout(
        Duration::from_secs(2),
        f.control.complete(&f.scope, &f.outbox),
    )
    .await
    .expect("ack must not wait in outbox to run order")
    .expect_err("NOWAIT parent check");
    match attempt {
        ortak_control::ControlError::Database(error) => assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("55P03")
        ),
        other => panic!("expected lock refusal: {other}"),
    }
    assert_eq!(f.count().await, 0);
    cancellation.rollback().await.expect("release");
    assert!(f
        .control
        .complete(&f.scope, &f.outbox)
        .await
        .expect("same live delivery lease retries"));
    assert_eq!(f.count().await, 1);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn acknowledgement_atomically_schedules_one_immutable_request_and_receipt() {
    let f = Fixture::new("completed", "reply").await;
    assert_eq!(
        f.count().await,
        0,
        "completion and signing are not delivery"
    );
    let mut tx = f.pool.begin().await.expect("transaction");
    sqlx::query("UPDATE outbox SET state='delivered',delivered_at=clock_timestamp(),lease_token=NULL,lease_expires_at=NULL WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company_id()).bind(f.outbox.id).execute(&mut *tx).await.expect("ack in transaction");
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM runtime_memory_writes WHERE company_id=$1")
            .bind(f.scope.company_id())
            .fetch_one(&mut *tx)
            .await
            .expect("job in same transaction");
    assert_eq!(count, 1);
    tx.rollback().await.expect("crash before commit");
    assert_eq!(f.count().await, 0);
    assert!(f
        .control
        .complete(&f.scope, &f.outbox)
        .await
        .expect("actual acknowledgement"));
    assert_eq!(f.count().await, 1);
    assert!(f
        .control
        .claim_memory_write(&f.scope, "other-adapter", Duration::from_secs(60))
        .await
        .expect("adapter filtering")
        .is_none());
    let lease = f.claim().await;
    assert_eq!(lease.attempt_count, 1);
    assert!(f
        .control
        .claim_memory_write(&f.scope, "honcho", Duration::from_secs(60))
        .await
        .expect("one live owner")
        .is_none());
    let receipt = MemoryWriteReceipt {
        receipt_ref: "server:receipt".to_owned(),
        written: 1,
    };
    assert!(!f
        .control
        .acknowledge_memory_write(&f.scope, &lease, &receipt)
        .await
        .expect("unadmitted cannot ack"));
    let request = f.prepare(&lease).await;
    assert_eq!(request.scope, MemoryScope::RunScratch { run_id: f.run_id });
    assert_eq!(
        request.idempotency_key,
        format!("office-output:{}", f.run_id)
    );
    assert_eq!(request.facts[0].content, "published final reply");
    assert_eq!(request.facts[0].provenance.run_id, Some(f.run_id));
    assert_eq!(
        request.facts[0].provenance.source,
        format!("office:{}", hex::encode([5; 32]))
    );
    assert_eq!(
        f.control
            .fail_memory_write(&f.scope, &lease, "memory_network", false)
            .await
            .expect("durable retry"),
        MemoryWriteJobOutcome::Retrying
    );
    f.make_due().await;
    let retry = f.claim().await;
    assert_ne!(retry.lease_token, lease.lease_token);
    assert_eq!(
        f.prepare(&retry).await,
        request,
        "lost acknowledgement retry preserves every fact and timestamp"
    );
    assert!(!f
        .control
        .acknowledge_memory_write(&f.scope, &lease, &receipt)
        .await
        .expect("old lease fenced"));
    let wrong = MemoryWriteReceipt {
        written: 2,
        ..receipt.clone()
    };
    assert!(f
        .control
        .acknowledge_memory_write(&f.scope, &retry, &wrong)
        .await
        .is_err());
    assert!(f
        .control
        .acknowledge_memory_write(&f.scope, &retry, &receipt)
        .await
        .expect("verified adapter receipt"));
    assert!(!f
        .control
        .acknowledge_memory_write(&f.scope, &retry, &receipt)
        .await
        .expect("duplicate ack"));
    assert!(sqlx::query(
        "UPDATE runtime_memory_writes SET content='replacement' WHERE company_id=$1"
    )
    .bind(f.scope.company_id())
    .execute(&f.pool)
    .await
    .is_err());
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn failed_cancelled_silent_and_cancellation_requested_outputs_never_schedule_memory() {
    for (status, intent) in [
        ("failed", "reply"),
        ("cancelled", "reply"),
        ("completed", "silent"),
    ] {
        let f = Fixture::new(status, intent).await;
        assert!(f
            .control
            .complete(&f.scope, &f.outbox)
            .await
            .expect("delivery receipt"));
        assert_eq!(f.count().await, 0);
    }
    let f = Fixture::new("completed", "reply").await;
    sqlx::query("INSERT INTO runtime_cancellations(company_id,run_id,reason) VALUES ($1,$2,'office_revoked')")
        .bind(f.scope.company_id()).bind(f.run_id).execute(&f.pool).await.expect("stop request");
    assert!(f
        .control
        .complete(&f.scope, &f.outbox)
        .await
        .expect("remote receipt still recorded"));
    assert_eq!(f.count().await, 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn memory_mutations_are_fenced_and_refused_before_write_even_with_fresh_witness() {
    let f = Fixture::new("completed", "reply").await;
    f.control.complete(&f.scope, &f.outbox).await.expect("ack");
    let lease = f.claim().await;
    let mut held = f.pool.begin().await.expect("hold authority");
    let old = lock_office_authority_on(&mut held, &f.scope)
        .await
        .expect("witness");
    let error =
        sqlx::query("UPDATE employee_memory_bindings SET workspace='foreign' WHERE company_id=$1")
            .bind(f.scope.company_id())
            .execute(&f.pool)
            .await
            .expect_err("mutation fails closed under shared fence");
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("40001")
    );
    held.commit().await.expect("release");
    sqlx::query("UPDATE employee_memory_bindings SET workspace='foreign' WHERE company_id=$1")
        .bind(f.scope.company_id())
        .execute(&f.pool)
        .await
        .expect("later mutation succeeds");
    let mut tx = f.pool.begin().await.expect("prepare");
    let fresh = lock_office_authority_on(&mut tx, &f.scope)
        .await
        .expect("new witness");
    assert!(fresh.generation() > old.generation());
    assert!(
        prepare_memory_write_on(&mut tx, &f.scope, &lease, &fresh)
            .await
            .is_err(),
        "fresh generation cannot bless changed binding"
    );
    tx.rollback().await.expect("release");
    assert_eq!(
        f.control
            .fail_memory_write(&f.scope, &lease, "memory_authority_changed", true)
            .await
            .expect("visible refusal"),
        MemoryWriteJobOutcome::Failed
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn suspended_company_refuses_and_expired_twentieth_claim_is_terminal() {
    let f = Fixture::new("completed", "reply").await;
    f.control.complete(&f.scope, &f.outbox).await.expect("ack");
    let lease = f.claim().await;
    sqlx::query("UPDATE companies SET status='suspended' WHERE id=$1")
        .bind(f.scope.company_id())
        .execute(&f.pool)
        .await
        .expect("suspend");
    let mut tx = f.pool.begin().await.expect("prepare");
    let witness = lock_office_authority_on(&mut tx, &f.scope)
        .await
        .expect("witness");
    assert!(prepare_memory_write_on(&mut tx, &f.scope, &lease, &witness)
        .await
        .is_err());
    tx.rollback().await.expect("release");
    sqlx::query("UPDATE runtime_memory_writes SET attempt_count=20,lease_expires_at=clock_timestamp()-interval '1 second' WHERE company_id=$1")
        .bind(f.scope.company_id()).execute(&f.pool).await.expect("final attempt crashed");
    assert!(f
        .control
        .claim_memory_write(&f.scope, "honcho", Duration::from_secs(60))
        .await
        .expect("bounded maintenance")
        .is_none());
    let state: String =
        sqlx::query_scalar("SELECT state FROM runtime_memory_writes WHERE company_id=$1")
            .bind(f.scope.company_id())
            .fetch_one(&f.pool)
            .await
            .expect("state");
    assert_eq!(state, "failed");
}
