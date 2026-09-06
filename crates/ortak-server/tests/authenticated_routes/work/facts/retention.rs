//! Real signed promotion survives approved Office purge without retaining API access.
use super::*;
use buzz_db::{
    deletion::{FrozenInventory, KeyStreamDigest, LeaseToken, PrefixManifest, StorageManifest},
    Db, DbConfig,
};
use std::time::Duration;

async fn retained(f: &Fixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object(
        'facts',(SELECT jsonb_agg(x ORDER BY id) FROM reviewed_memory_facts x WHERE company_id=$1),
        'receipts',(SELECT jsonb_agg(x ORDER BY operation_id) FROM reviewed_memory_operations x WHERE company_id=$1),
        'projects',(SELECT jsonb_agg(x ORDER BY id) FROM projects x WHERE company_id=$1))")
        .bind(f.company).fetch_one(&f.pool).await.unwrap()
}

#[tokio::test]
#[ignore = "requires explicit disposable port55432 Postgres"]
async fn reviewed_fact_retention_survives_approved_purge_and_closes_current_reads() {
    let f = Fixture::new().await;
    fixture::employee(&f).await;
    let app = work_app(&f, true, Role::Operator, vec![f.channel]);
    let project = project(&f, &app, f.channel).await;
    let source = boundaries::source_message(&f, f.channel).await;
    let path = memory_path(project);
    let body = approval(&source);
    let saved = post(&app, &f.operator, &path, &body).await;
    assert_eq!(saved.0, StatusCode::OK);
    let fact = id(&saved.1["fact"]);
    // Preserve both the original approval and its explicit, retained stop-use receipt.
    let stop = json!({"operation_id":Uuid::new_v4(),"expected_version":1,"reason":"Retain reviewed evidence"});
    assert_eq!(
        post(&app, &f.operator, &format!("{path}/{fact}/stop"), &stop)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(count(&f).await, (1, 2));
    let before = retained(&f).await;
    let employee = EmployeeId::parse("cem").unwrap();
    let cached = ortak_work::AuthorizedWork::new(
        f.control.clone(),
        f.control
            .resolve_company_for_community(f.community)
            .await
            .unwrap(),
        ortak_work::ApiWorkPrincipal::new(
            f.community,
            f.operator.public_key().to_hex(),
            [0x65; 32],
            true,
            true,
            [f.channel].into_iter().collect(),
            [employee.clone()].into_iter().collect(),
        )
        .unwrap(),
    );
    assert_eq!(
        cached
            .reviewed_facts(project, employee.clone(), None)
            .await
            .unwrap()
            .facts
            .len(),
        1
    );

    let other = Fixture::new().await;
    fixture::employee(&other).await;
    let other_app = work_app(&other, true, Role::Operator, vec![other.channel]);
    let other_project = super::project(&other, &other_app, other.channel).await;
    let other_source = boundaries::source_message(&other, other.channel).await;
    let other_path = memory_path(other_project);
    assert_eq!(
        post(
            &other_app,
            &other.operator,
            &other_path,
            &approval(&other_source)
        )
        .await
        .0,
        StatusCode::OK
    );
    let other_before = retained(&other).await;

    // Fixture::new has already required explicit disposable port 55432. Do not run
    // migrations again while this approved destructive transaction is in progress.
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
        .submit(&host, "fixture", Some("Disposable retention regression"))
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
    assert_eq!(inventory.schema.row_counts["reviewed_memory_facts"], 1);
    assert_eq!(inventory.schema.row_counts["reviewed_memory_operations"], 2);
    for table in ["reviewed_memory_facts", "reviewed_memory_operations"] {
        assert!(inventory
            .schema
            .retained_tables
            .iter()
            .any(|name| name == table));
    }
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
    let purged = store
        .purge_postgres(&token)
        .await
        .expect("retained facts must not reference transient bindings");
    assert_eq!(purged.get("project_api_bindings"), Some(&1));
    assert_eq!(purged.get("office_company_bindings"), Some(&1));
    assert!(!purged.contains_key("reviewed_memory_facts"));
    assert!(!purged.contains_key("reviewed_memory_operations"));
    assert_eq!(retained(&f).await, before);
    assert_eq!(retained(&other).await, other_before);
    store
        .mark_cache_purged(&token, json!({"keys":0}))
        .await
        .unwrap();
    store
        .verify_postgres_logically_deleted(&token)
        .await
        .unwrap();

    // Bind the core authority seam too: even a previously resolved company and
    // principal must recheck current Office scope before retained evidence reads.
    assert!(matches!(
        cached.reviewed_facts(project, employee.clone(), None).await,
        Err(ortak_work::WorkError::AccessDenied)
    ));
    assert!(matches!(
        cached
            .recall_reviewed_facts(project, employee, "deployment".into())
            .await,
        Err(ortak_work::WorkError::AccessDenied)
    ));

    // A cached router/current signed caller cannot use retained provenance as a grant.
    for denied in [
        get(&app, &f.operator, &format!("{path}?employee_id=cem")).await,
        post(
            &app,
            &f.operator,
            &format!("{path}/recall"),
            &json!({"employee_id":"cem","query":"deployment"}),
        )
        .await,
        post(&app, &f.operator, &path, &body).await,
        post(&app, &f.operator, &format!("{path}/{fact}/stop"), &stop).await,
    ] {
        assert!(!denied.0.is_success());
        assert!(!denied.1.to_string().contains("Reviewed deployment fact"));
    }
    assert_eq!(
        get(
            &other_app,
            &other.operator,
            &format!("{other_path}?employee_id=cem")
        )
        .await
        .0,
        StatusCode::OK
    );
    // Tombstone fences remain authoritative even for writes to retained tables.
    for sql in [
        "INSERT INTO reviewed_memory_facts SELECT * FROM reviewed_memory_facts WHERE company_id=$1",
        "INSERT INTO reviewed_memory_operations SELECT * FROM reviewed_memory_operations WHERE company_id=$1",
    ] {
        let error = sqlx::query(sql).bind(f.company).execute(&f.pool).await.unwrap_err();
        assert_eq!(error.as_database_error().and_then(|e| e.code()).as_deref(), Some("55000"));
    }
    assert_eq!(retained(&f).await, before);
}
