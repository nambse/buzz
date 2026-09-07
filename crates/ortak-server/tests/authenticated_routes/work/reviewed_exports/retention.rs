//! Canonical purge must drain external text, while backup obligations stay distinct.
use super::*;
use buzz_db::{
    deletion::{FrozenInventory, KeyStreamDigest, LeaseToken, PrefixManifest, StorageManifest},
    Db, DbConfig, DbError,
};

async fn retained(x: &ExportFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object(
        'targets',(SELECT jsonb_agg(t ORDER BY id) FROM reviewed_memory_targets t WHERE company_id=$1),
        'exports',(SELECT jsonb_agg(t ORDER BY fact_id) FROM reviewed_memory_exports t WHERE company_id=$1),
        'jobs',(SELECT jsonb_agg(t ORDER BY fact_id,action) FROM reviewed_memory_export_jobs t WHERE company_id=$1),
        'commands',(SELECT jsonb_agg(t ORDER BY operation_id) FROM reviewed_memory_export_commands t WHERE company_id=$1),
        'receipts',(SELECT jsonb_agg(t ORDER BY fact_id,action) FROM reviewed_memory_export_receipts t WHERE company_id=$1),
        'uses',(SELECT jsonb_agg(t ORDER BY run_id,ordinal) FROM run_reviewed_memory_uses t WHERE company_id=$1))")
        .bind(x.f.company).fetch_one(&x.f.pool).await.unwrap()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with proposal71"]
async fn reviewed_export_canonical_purge_waits_for_cleanup_and_retains_exact_receipts() {
    canonical_purge(false).await;
    canonical_purge(true).await;
}

async fn canonical_purge(with_run_use: bool) {
    // This controlled remote retains the publication header across cleanup,
    // just as Honcho's store does. A fresh fake cannot prove its content hash.
    let published_remote = ObservedAdapter::default();
    let x = if with_run_use {
        let (x, item) =
            super::runtime::prepared_with_adapter(Duration::from_secs(86400), &published_remote)
                .await;
        let (run, adapter, memory, reference) = super::runtime::start(&x, &item).await;
        crate::work::execution::fixture::complete(
            &x.f,
            &adapter,
            &memory.0,
            run,
            &reference,
            ortak_control::run_event::BoundedText::raw("Reviewed retention result"),
        )
        .await;
        assert_eq!(
            ortak_work::schedule_work_outputs(&x.f.control, &x.scope, 1)
                .await
                .unwrap()
                .materialized,
            1
        );
        assert_eq!(retained(&x).await["uses"].as_array().unwrap().len(), 1);
        x
    } else {
        let x = ExportFixture::new(Duration::from_secs(86400), true).await;
        x.publish().await;
        x
    };
    let other = ExportFixture::new(Duration::from_secs(86400), true).await;
    other.publish().await;
    let other_before = retained(&other).await;
    let db = Db::new(&DbConfig {
        database_url: std::env::var("ORTAK_TEST_DATABASE_URL").unwrap(),
        max_connections: 4,
        min_connections: 0,
        ..DbConfig::default()
    })
    .await
    .unwrap();
    let store = db.deletion_store();
    let host: String = sqlx::query_scalar("SELECT host FROM communities WHERE id=$1")
        .bind(x.f.community)
        .fetch_one(&x.f.pool)
        .await
        .unwrap();
    let request = store
        .submit(
            &host,
            "fixture",
            Some("Disposable reviewed-export retention regression"),
        )
        .await
        .unwrap();
    let community = request.community_id;
    let inventory = FrozenInventory {
        schema: store.inventory_schema(community).await.unwrap(),
        storage: StorageManifest {
            version: 4,
            prefixes: ["_meta", "_uploads", "repos"]
                .into_iter()
                .map(|prefix| PrefixManifest {
                    prefix: format!("{prefix}/{community}/"),
                    object_count: 0,
                    total_bytes: 0,
                    keys_digest: KeyStreamDigest::new().finish().0,
                })
                .collect(),
        },
    };
    for table in [
        "reviewed_memory_targets",
        "reviewed_memory_exports",
        "reviewed_memory_export_jobs",
        "reviewed_memory_export_commands",
        "reviewed_memory_export_receipts",
        "run_reviewed_memory_uses",
    ] {
        assert!(inventory.schema.retained_tables.iter().any(|v| v == table));
        assert!(inventory.schema.row_counts.contains_key(table));
    }
    if with_run_use {
        assert_eq!(
            inventory.schema.row_counts.get("run_reviewed_memory_uses"),
            Some(&1)
        );
    }
    store
        .freeze_inventory(request.id, &inventory)
        .await
        .unwrap();
    store.approve(request.id, "fixture", None).await.unwrap();
    let claim = store
        .claim_specific(request.id, "fixture", Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        store.begin_quiescing(&claim.lease).await,
        Err(DbError::ReviewedMemoryExportsNotDrained {
            unacknowledged_exports: 1,
            leased_publications: 0,
            ..
        })
    ));
    let active: (String, bool) =
        sqlx::query_as("SELECT deletion_state,archived_at IS NULL FROM communities WHERE id=$1")
            .bind(community.as_uuid())
            .fetch_one(&x.f.pool)
            .await
            .unwrap();
    assert_eq!(active, ("active".into(), true));
    if with_run_use {
        x.stop().await;
        // Local withdrawal alone cannot claim that remote text was erased.
        assert!(matches!(
            store.begin_quiescing(&claim.lease).await,
            Err(DbError::ReviewedMemoryExportsNotDrained {
                unacknowledged_exports: 1,
                ..
            })
        ));
        assert!(schedule_one(&x.f.control, &x.scope, &published_remote)
            .await
            .unwrap());
    } else {
        let lease = exports::claim(&x.f.control, &x.scope)
            .await
            .unwrap()
            .unwrap();
        let prepared = exports::prepare(&x.f.control, &x.scope, &lease)
            .await
            .unwrap()
            .unwrap();
        let adapter = ObservedAdapter::default();
        let remote_ack = adapter.write(&prepared).await.unwrap(); // external commit; local ACK deliberately held
        x.stop().await;
        assert!(schedule_one(&x.f.control, &x.scope, &adapter)
            .await
            .unwrap());
        assert!(
            matches!(
                store.begin_quiescing(&claim.lease).await,
                Err(DbError::ReviewedMemoryExportsNotDrained {
                    unacknowledged_exports: 0,
                    leased_publications: 1,
                    ..
                })
            ),
            "a removal proof cannot discharge an uncertain publication lease"
        );
        // The late observed publication receipt cannot re-enable the fact or replace its cleanup receipt.
        assert!(
            exports::acknowledge(&x.f.control, &x.scope, &lease, &remote_ack)
                .await
                .unwrap()
        );
    }
    assert_eq!(x.page().await["facts"][0]["status"], "revoked");
    assert_eq!(
        x.page().await["facts"][0]["export"]["erased_from_reviewed_store"],
        true
    );
    let before = retained(&x).await;
    if with_run_use {
        assert_eq!(before["uses"].as_array().unwrap().len(), 1);
    }
    store.begin_quiescing(&claim.lease).await.unwrap();
    let generation = store.fence(&claim.lease).await.unwrap();
    let token = LeaseToken {
        fence_generation: Some(generation),
        ..claim.lease
    };
    store
        .freeze_destructive_storage_manifest(&token, &inventory.storage)
        .await
        .unwrap();
    store.mark_drained(&token).await.unwrap();
    store
        .mark_bindings_removed(&token, json!({"keys":0}))
        .await
        .unwrap();
    let purged = store.purge_postgres(&token).await.unwrap();
    assert_eq!(purged.get("office_company_bindings"), Some(&1));
    assert_eq!(retained(&x).await, before);
    assert_eq!(retained(&other).await, other_before);
    store
        .mark_cache_purged(&token, json!({"keys":0}))
        .await
        .unwrap();
    store
        .verify_postgres_logically_deleted(&token)
        .await
        .unwrap();
    assert!(!get(
        &x.app,
        &x.f.operator,
        &format!(
            "/api/v1/projects/{}/reviewed-memory?employee_id=cem",
            x.project
        )
    )
    .await
    .0
    .is_success());
    for statement in [
        "INSERT INTO reviewed_memory_targets SELECT * FROM reviewed_memory_targets WHERE company_id=$1",
        "INSERT INTO reviewed_memory_exports SELECT * FROM reviewed_memory_exports WHERE company_id=$1",
        "INSERT INTO reviewed_memory_export_jobs SELECT * FROM reviewed_memory_export_jobs WHERE company_id=$1",
        "INSERT INTO reviewed_memory_export_commands SELECT * FROM reviewed_memory_export_commands WHERE company_id=$1",
        "INSERT INTO reviewed_memory_export_receipts SELECT * FROM reviewed_memory_export_receipts WHERE company_id=$1"] {
        let error=sqlx::query(statement).bind(x.f.company).execute(&x.f.pool).await.unwrap_err();
        assert_eq!(error.as_database_error().and_then(|v|v.code()).as_deref(),Some("55000"));
    }
    if with_run_use {
        let error = sqlx::query("INSERT INTO run_reviewed_memory_uses SELECT * FROM run_reviewed_memory_uses WHERE company_id=$1")
            .bind(x.f.company).execute(&x.f.pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|v| v.code()).as_deref(),
            Some("55000")
        );
    }
    assert_eq!(retained(&x).await, before);
    assert_eq!(retained(&other).await, other_before);
    assert_eq!(other.page().await["facts"].as_array().unwrap().len(), 1);
}
