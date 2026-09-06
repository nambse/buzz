use super::*;
use buzz_db::{
    deletion::{FrozenInventory, KeyStreamDigest, LeaseToken, PrefixManifest, StorageManifest},
    Db, DbConfig,
};
use std::time::Duration;

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres with decomposition schema"]
async fn decomposition_retention_survives_canonical_purge_without_reusing_retained_links_as_authority(
) {
    let f = Fixture::new().await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let parent = item(&f, &app, project).await;
    let created = create(&f, &app, &parent).await;
    let routed = crate::routing_read::fixture::record(&f, f.channel, false).await;
    assert_eq!(
        crate::routing_read::fixture::read(&f.app, &f.operator, f.channel, routed)
            .await
            .0,
        StatusCode::OK
    );
    let before = snapshot(&f).await;
    let scope = f
        .control
        .resolve_company_for_community(f.community)
        .await
        .unwrap();
    let cached = ortak_work::AuthorizedWork::new(
        f.control.clone(),
        scope,
        ortak_work::ApiWorkPrincipal::new(
            f.community,
            f.operator.public_key().to_hex(),
            [0x70; 32],
            true,
            true,
            [f.channel].into_iter().collect(),
            Default::default(),
        )
        .unwrap(),
    );
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
        .bind(f.community)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let request = store
        .submit(
            &host,
            "fixture",
            Some("Disposable decomposition retention regression"),
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
    assert!(!inventory
        .schema
        .scoped_tables
        .iter()
        .any(|name| name == "work_decomposition"));
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
    assert_eq!(purged.get("project_api_bindings"), Some(&1));
    assert_eq!(purged.get("office_company_bindings"), Some(&1));
    assert!(!purged.contains_key("work_decomposition"));
    store
        .mark_cache_purged(&token, json!({"keys":0}))
        .await
        .unwrap();
    store
        .verify_postgres_logically_deleted(&token)
        .await
        .unwrap();
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(
        crate::routing_read::fixture::read(&f.app, &f.operator, f.channel, routed)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let retained_decisions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM routing_decisions WHERE company_id=$1 AND message_id=$2",
    )
    .bind(f.company)
    .bind(routed.to_bytes().as_slice())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(retained_decisions, 1);
    assert!(matches!(
        cached.decomposition(id(&parent)).await,
        Err(ortak_work::WorkError::AccessDenied)
    ));
    for item in [&created["work_item"], &created["child"]] {
        let response = get(
            &app,
            &f.operator,
            &format!("/api/v1/work-items/{}/decomposition", id(item)),
        )
        .await;
        assert_eq!(
            response.0,
            StatusCode::NOT_FOUND,
            "purged read status: {}",
            response.0
        );
        let response = post(&app, &f.operator, &path(item), &body(item)).await;
        assert_eq!(
            response.0,
            StatusCode::NOT_FOUND,
            "purged mutation status: {}",
            response.0
        );
    }
    assert_eq!(snapshot(&f).await, before);
}
