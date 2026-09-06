//! Approved community purge retains probe ownership without retaining authority.
use super::*;
use buzz_db::{
    deletion::{FrozenInventory, KeyStreamDigest, LeaseToken, PrefixManifest, StorageManifest},
    Db, DbConfig,
};

async fn retained(f: &Fixture) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(p) FROM provisioning_runtime_probes p WHERE company_id=$1 AND operation_id=$2")
        .bind(f.scope.company_id()).bind(f.operation).fetch_one(f.control.pool()).await.unwrap()
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL 55432 with proposal68"]
async fn runtime_probe_retention_survives_approved_purge_and_allows_only_containment_accounting() {
    let f = Fixture::new().await;
    let mut run = f.start();
    f.wait_started(&mut run).await;
    run.abort();
    let _ = run.await;
    let issued = f
        .control
        .provisioning_runtime_probe(&f.scope, f.operation)
        .await
        .unwrap()
        .unwrap();
    let before = retained(&f).await;
    assert_eq!(before["state"], "running");

    // Use the same canonical DeletionStore workflow as the reviewed-fact
    // retention regression. This is a storage-boundary test: the external G
    // drain gate must first contain running children in an actual cutover.
    // A retained issued handle also needs to settle a delayed ACK after purge.
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
        .bind(f.scope.community_id())
        .fetch_one(f.control.pool())
        .await
        .unwrap();
    let request = store
        .submit(
            &host,
            "fixture",
            Some("Disposable runtime probe retention regression"),
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
    // This journal is company-owned, not a community-scoped purge target.
    assert!(!inventory
        .schema
        .scoped_tables
        .iter()
        .any(|t| t == "provisioning_runtime_probes"));
    store
        .freeze_inventory(request.id, &inventory)
        .await
        .unwrap();
    store.approve(request.id, "fixture", None).await.unwrap();
    let claim = store
        .claim_specific(request.id, "fixture", Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
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
    assert!(!purged.contains_key("provisioning_runtime_probes"));
    assert_eq!(retained(&f).await, before);
    store
        .mark_cache_purged(&token, json!({"keys":0}))
        .await
        .unwrap();
    store
        .verify_postgres_logically_deleted(&token)
        .await
        .unwrap();

    // Neither a previously resolved scope nor its issued probe revives access.
    assert!(f
        .control
        .provisioning_runtime_probe(&f.scope, f.operation)
        .await
        .is_err());
    assert!(f
        .control
        .admit_provisioning_runtime_probe(
            &f.scope,
            f.operation,
            issued.origin(),
            issued.token_environment(),
            Some(issued.id())
        )
        .await
        .is_err());
    assert!(f
        .control
        .settle_provisioning_runtime_probe(&f.scope, &issued, None)
        .await
        .is_err());
    assert_eq!(f.start().await.unwrap(), Err("probe_authority_changed"));
    assert_eq!(f.bridge.starts.load(Ordering::SeqCst), 1);
    assert_eq!(f.bridge.stops.load(Ordering::SeqCst), 0);
    assert_eq!(retained(&f).await, before);

    // The original transport contains the exact child; only the retained failed
    // cleanup receipt may commit after current authority has disappeared.
    contain(&f.runtime, &issued).await.unwrap();
    f.control
        .settle_provisioning_runtime_probe(&f.scope, &issued, Some("probe_authority_changed"))
        .await
        .unwrap();
    assert_eq!(f.bridge.stops.load(Ordering::SeqCst), 1);
    let mut after = retained(&f).await;
    assert_eq!(after["state"], "failed");
    assert_eq!(after["error_code"], "probe_authority_changed");
    assert!(after["contained_at"].is_string());
    for key in ["state", "error_code", "contained_at"] {
        after[key] = before[key].clone();
    }
    assert_eq!(
        after, before,
        "ownership and fixed deadline remain immutable"
    );
}
