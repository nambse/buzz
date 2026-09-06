//! Populated retained evidence through the real approved deletion transaction.
use super::ortak_project_tests::{approved, preserved, seed, Scope};
use super::postgres_tests::empty_storage_manifest;
use super::*;
use crate::DbConfig;

async fn database() -> (Db, DeletionStore) {
    let db = Db::new(&DbConfig {
        database_url: crate::test_support::database_url(),
        max_connections: 5,
        min_connections: 0,
        ..DbConfig::default()
    })
    .await
    .expect("connect isolated test database");
    if std::env::var("BUZZ_TEST_SCHEMA_MODE").as_deref() != Ok("desired") {
        db.migrate().await.expect("apply isolated test migrations");
    }
    let store = db.deletion_store();
    (db, store)
}

async fn seed_evidence(db: &Db, scope: &Scope) {
    let channel: Uuid = sqlx::query_scalar("SELECT id FROM channels WHERE community_id=$1")
        .bind(scope.community.as_uuid())
        .fetch_one(&db.pool)
        .await
        .expect("selected channel");
    let operation = Uuid::new_v4();
    let mut tx = db.pool.begin().await.expect("evidence seed");
    sqlx::query("INSERT INTO employees(company_id,id) VALUES($1,'retained')")
        .bind(scope.company)
        .execute(&mut *tx)
        .await
        .expect("stable employee");
    sqlx::query("INSERT INTO provisioning_operations(company_id,id,employee_id,mode,idempotency_key,manifest,manifest_fingerprint) VALUES($1,$2,'retained','adopt','retained-op','{}',$3)")
        .bind(scope.company).bind(operation).bind(vec![0x31_u8;32])
        .execute(&mut *tx).await.expect("retained adopt operation");
    sqlx::query("INSERT INTO provisioning_operation_steps(company_id,operation_id,step_index,step_name,idempotency_key,adopted_existing,result) VALUES($1,$2,0,'office_profile','retained-step',true,'{\"profile_ref\":\"existing-disposable-fixture\"}')")
        .bind(scope.company).bind(operation).execute(&mut *tx).await.expect("adopted ownership receipt");
    sqlx::query("INSERT INTO office_identity_profiles(company_id,idempotency_key,community_id,employee_id,request_hash,event_id,signed_event_bytes) VALUES($1,'retained-step',$2,'retained',$3,$3,$4)")
        .bind(scope.company).bind(scope.community.as_uuid()).bind(vec![0x32_u8;32])
        .bind(b"fixed-public-signed-profile-fixture".to_vec()).execute(&mut *tx).await.expect("profile byte snapshot");
    sqlx::query(
        "INSERT INTO office_routing_cohorts(company_id,community_id,state) VALUES($1,$2,'capture')",
    )
    .bind(scope.company)
    .bind(scope.community.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("capture selection");
    sqlx::query(
        "INSERT INTO office_routing_channels(company_id,community_id,channel_id) VALUES($1,$2,$3)",
    )
    .bind(scope.company)
    .bind(scope.community.as_uuid())
    .bind(channel)
    .execute(&mut *tx)
    .await
    .expect("selected channel");
    sqlx::query(
        "INSERT INTO office_routing_employees(company_id,employee_id) VALUES($1,'retained')",
    )
    .bind(scope.company)
    .execute(&mut *tx)
    .await
    .expect("selected employee");
    // The seeded event is kind 1, outside the canonical 9/40002 input window.
    sqlx::query("INSERT INTO office_inbox_reconciliations(company_id,capture_id,community_id,channel_id,completed_at) SELECT company_id,capture_id,community_id,$2,clock_timestamp() FROM office_routing_cohorts WHERE company_id=$1")
        .bind(scope.company).bind(channel).execute(&mut *tx).await.expect("real empty-window reconciliation");
    sqlx::query("UPDATE office_routing_cohorts SET state='enabled' WHERE company_id=$1")
        .bind(scope.company)
        .execute(&mut *tx)
        .await
        .expect("completed capture can enable");
    tx.commit().await.expect("evidence seed committed");
}

async fn evidence(db: &Db, company: Uuid) -> serde_json::Value {
    sqlx::query_scalar("SELECT jsonb_build_object(\
        'profiles',(SELECT jsonb_agg(p ORDER BY idempotency_key) FROM office_identity_profiles p WHERE company_id=$1),\
        'reconciliations',(SELECT jsonb_agg(r ORDER BY capture_id,channel_id) FROM office_inbox_reconciliations r WHERE company_id=$1),\
        'employees',(SELECT jsonb_agg(e ORDER BY id) FROM employees e WHERE company_id=$1),\
        'operations',(SELECT jsonb_agg(o ORDER BY id) FROM provisioning_operations o WHERE company_id=$1),\
        'steps',(SELECT jsonb_agg(s ORDER BY step_index) FROM provisioning_operation_steps s WHERE company_id=$1))")
        .bind(company).fetch_one(&db.pool).await.expect("all retained byte/count evidence")
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn populated_office_evidence_survives_purge_and_all_post_revocation_writes_fail() {
    let (db, store) = database().await;
    let selected = seed(&db).await;
    let other = seed(&db).await;
    seed_evidence(&db, &selected).await;
    seed_evidence(&db, &other).await;
    let before = evidence(&db, selected.company).await;
    let other_before = evidence(&db, other.company).await;
    let work_before = preserved(&db, &selected).await;
    let inventory = store
        .inventory_schema(selected.community)
        .await
        .expect("all fences present");
    assert_eq!(inventory.retained_tables, RETAINED_SCOPED_TABLES);
    assert_eq!(inventory.row_counts["office_identity_profiles"], 1);
    assert_eq!(inventory.row_counts["office_inbox_reconciliations"], 1);
    let mut drift = db
        .pool
        .begin()
        .await
        .expect("temporary missing retained fence");
    sqlx::query("ALTER TABLE office_identity_profiles DISABLE TRIGGER community_write_fence_office_identity_profiles")
        .execute(&mut *drift).await.expect("disable only fixture transaction fence");
    assert!(
        validate_catalog_on(&mut drift).await.is_err(),
        "retained-table fence is mandatory for deletion"
    );
    drift.rollback().await.expect("restore fixture fence");
    let token = approved(&db, &store, &selected).await;
    for sql in [
        "UPDATE office_identity_profiles SET acknowledged_at=clock_timestamp() WHERE company_id=$1",
        "UPDATE office_inbox_reconciliations SET completed_at=completed_at WHERE company_id=$1",
    ] {
        let error = sqlx::query(sql)
            .bind(selected.company)
            .execute(&db.pool)
            .await
            .expect_err("fenced retained write");
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("55000")
        );
    }
    let deleted = store
        .purge_postgres(&token)
        .await
        .expect("approved retained-evidence purge");
    assert!(!deleted.contains_key("office_identity_profiles"));
    assert!(!deleted.contains_key("office_inbox_reconciliations"));
    assert_eq!(evidence(&db, selected.company).await, before);
    assert_eq!(evidence(&db, other.company).await, other_before);
    assert_eq!(preserved(&db, &selected).await, work_before);
    let mutable: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM office_routing_cohorts WHERE company_id=$1),(SELECT count(*) FROM office_routing_channels WHERE company_id=$1),(SELECT count(*) FROM office_routing_employees WHERE company_id=$1)")
        .bind(selected.company).fetch_one(&db.pool).await.expect("mutable selection counts");
    assert_eq!(mutable, (0, 0, 0));
    for table in RETAINED_SCOPED_TABLES {
        let sql = format!("DELETE FROM {table} WHERE company_id=$1");
        assert!(sqlx::query(AssertSqlSafe(sql))
            .bind(selected.company)
            .execute(&db.pool)
            .await
            .is_err());
    }
    let rejected = sqlx::query("INSERT INTO office_identity_profiles SELECT company_id,idempotency_key,community_id,employee_id,request_hash,event_id,signed_event_bytes,created_at,acknowledged_at FROM office_identity_profiles WHERE company_id=$1")
        .bind(selected.company).execute(&db.pool).await.expect_err("tombstone rejects new snapshots");
    assert_eq!(
        rejected
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("55000"),
        "authority rejection must precede any duplicate-key error"
    );
    store
        .mark_cache_purged(&token, serde_json::json!({"keys":0}))
        .await
        .expect("cache checkpoint");
    store
        .verify_postgres_logically_deleted(&token)
        .await
        .expect("retained evidence is not mutable Office authority");
    assert_eq!(evidence(&db, selected.company).await, before);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn old_approved_inventory_cannot_adopt_new_retention_policy() {
    let (db, store) = database().await;
    let scope = seed(&db).await;
    let request = store
        .submit(&scope.host, "fixture", None)
        .await
        .expect("submit");
    let mut schema = store
        .inventory_schema(scope.community)
        .await
        .expect("current schema");
    schema.retained_tables.clear();
    let inventory = FrozenInventory {
        schema,
        storage: empty_storage_manifest(scope.community),
    };
    store
        .freeze_inventory(request.id, &inventory)
        .await
        .expect("freeze old policy fixture");
    assert!(
        store.approve(request.id, "fixture", None).await.is_err(),
        "new approval rejects old policy"
    );
    // Reproduce a persisted approval from the old executable, without disabling
    // immutability or rewriting the frozen manifest after approval.
    let mut tx = db
        .pool
        .begin()
        .await
        .expect("old executable approval fixture");
    sqlx::query("INSERT INTO community_deletion_approvals(request_id,community_id,inventory_digest,approved_by) SELECT id,community_id,inventory_digest,'old-executable-fixture' FROM community_deletion_requests WHERE id=$1")
        .bind(request.id).execute(&mut *tx).await.expect("old approval");
    sqlx::query("UPDATE community_deletion_requests SET stage='approved' WHERE id=$1")
        .bind(request.id)
        .execute(&mut *tx)
        .await
        .expect("old approved stage");
    tx.commit().await.expect("old approval persisted");
    let claim = store
        .claim_specific(request.id, "fixture", DEFAULT_LEASE_DURATION)
        .await
        .expect("claim query")
        .expect("claimed");
    assert!(
        store
            .verify_execution_token(&claim.lease, DeletionStage::Approved)
            .await
            .is_err(),
        "external-effect guard cannot accept old frozen retention semantics"
    );
    assert!(
        store.begin_quiescing(&claim.lease).await.is_err(),
        "old approved scope cannot revoke serving"
    );
    assert!(store
        .is_serving_active(scope.community)
        .await
        .expect("still active"));
}
