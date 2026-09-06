//! A real protected admission must block canonical deletion before quiescence.
use super::*;
use buzz_db::{
    deletion::{FrozenInventory, KeyStreamDigest, PrefixManifest, StorageManifest},
    Db, DbConfig, DbError,
};

async fn retained(x: &EncryptedFixture) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
          'selections',(SELECT jsonb_agg(t ORDER BY selection_id) FROM encrypted_dm_selections t WHERE company_id=$1),
          'jobs',(SELECT jsonb_agg(t ORDER BY source_id) FROM encrypted_dm_decrypt_jobs t WHERE company_id=$1),
          'runs',(SELECT jsonb_agg(t ORDER BY id) FROM runs t WHERE company_id=$1),
          'protected',(SELECT jsonb_agg(t ORDER BY run_id) FROM confidential_runs t WHERE company_id=$1),
          'payloads',(SELECT jsonb_agg(t ORDER BY run_id,purpose,ordinal) FROM confidential_run_payloads t WHERE company_id=$1),
          'receipts',(SELECT jsonb_agg(t ORDER BY source_id) FROM confidential_dm_receipts t WHERE company_id=$1),
          'dispatches',(SELECT jsonb_agg(t ORDER BY run_id) FROM confidential_run_dispatches t WHERE company_id=$1),
          'execution',(SELECT jsonb_agg(t ORDER BY run_id) FROM confidential_execution_leases t WHERE company_id=$1),
          'events',(SELECT jsonb_agg(t ORDER BY run_id,ordinal) FROM confidential_event_receipts t WHERE company_id=$1),
          'bundles',(SELECT jsonb_agg(t ORDER BY run_id) FROM confidential_reply_bundles t WHERE company_id=$1),
          'outbox',(SELECT jsonb_agg(t ORDER BY run_id,copy) FROM confidential_reply_outbox t WHERE company_id=$1))",
    )
    .bind(x.f.scope.company_id())
    .fetch_one(&x.f.pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires disposable77 and explicit synthetic key env; no runtime/provider start"]
async fn confidential_pending_dispatch_blocks_canonical_deletion_and_preserves_ciphertext() {
    let x = EncryptedFixture::new().await;
    // Production strict decode, key-purpose selection, prepare/protect and
    // atomic admission create the pending dispatch. No bridge is constructed.
    let run = admitted(&x).await;
    let before = retained(&x).await;
    for key in ["selections", "jobs", "runs", "protected", "payloads", "receipts", "dispatches"] {
        assert_eq!(before[key].as_array().unwrap().len(), 1, "{key}");
    }
    assert_eq!(before["dispatches"][0]["state"], "pending");
    assert_eq!(before["dispatches"][0]["attempts"], 0);
    assert_eq!(before["dispatches"][0]["run_id"], run.to_string());
    assert_eq!(before["payloads"][0]["purpose"], "snapshot");
    assert!(before["payloads"][0]["envelope_bytes"].as_str().is_some());
    assert!(before["protected"][0]["wrapped_key"].as_str().is_some());
    for key in ["execution", "events", "bundles", "outbox"] {
        assert!(before[key].is_null(), "no started execution: {key}");
    }

    // Reuse the existing deletion executor's actual approved-inventory path;
    // connect to the database already guarded by EncryptedFixture, without
    // migrations, fabricated lease rows or disabled retention triggers.
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
        .bind(x.f.community_id)
        .fetch_one(&x.f.pool)
        .await
        .unwrap();
    let request = store
        .submit(&host, "fixture", Some("Protected admission retention regression"))
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
    store.freeze_inventory(request.id, &inventory).await.unwrap();
    store.approve(request.id, "fixture", None).await.unwrap();
    let claim = store
        .claim_specific(request.id, "fixture", Duration::from_secs(120))
        .await
        .unwrap()
        .unwrap();

    let result = store.begin_quiescing(&claim.lease).await;
    assert!(
        matches!(&result, Err(DbError::DeletionSafety(code)) if code == "confidential_execution_not_settled"),
        "expected protected settlement refusal, got {result:?}"
    );
    let active: (String, bool, bool) = sqlx::query_as(
        "SELECT deletion_state,archived_at IS NULL,deleted_at IS NULL FROM communities WHERE id=$1",
    )
    .bind(x.f.community_id)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(active, ("active".into(), true, true));
    assert!(store.is_serving_active(community).await.unwrap());
    assert!(
        retained(&x).await == before,
        "refused deletion must preserve every protected byte, job, receipt and dispatch row"
    );
    let unchanged_request: (String, bool) = sqlx::query_as(
        "SELECT stage,quiescing_started_at IS NULL FROM community_deletion_requests WHERE id=$1",
    )
    .bind(request.id)
    .fetch_one(&x.f.pool)
    .await
    .unwrap();
    assert_eq!(unchanged_request, ("approved".into(), true));
}
