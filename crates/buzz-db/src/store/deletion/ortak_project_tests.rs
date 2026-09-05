//! Actual deletion lifecycle regressions for the narrowly controlled E1 detach.
//! Requires an explicitly named, disposable localhost:55432 deletion database.

use super::postgres_tests::{empty_storage_manifest, store};
use super::*;

struct Scope {
    community: CommunityId,
    host: String,
    company: Uuid,
    project: Uuid,
    item: Uuid,
}

async fn isolated_store() -> (Db, DeletionStore) {
    let url = std::env::var("BUZZ_TEST_DATABASE_URL")
        .expect("set BUZZ_TEST_DATABASE_URL to an explicit disposable deletion database");
    let name = url
        .strip_prefix("postgres://ortak:ortak@127.0.0.1:55432/")
        .or_else(|| url.strip_prefix("postgres://ortak:ortak@localhost:55432/"))
        .expect("use the exact local PostgreSQL test credentials and port 55432");
    assert!(
        name.len() <= 63
            && (name.starts_with("ortak_deletion_") || name.starts_with("buzz_deletion_"))
            && name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'),
        "use a fresh deletion database name without URL parameters, paths, or fragments"
    );
    store().await
}

async fn seed(db: &Db) -> Scope {
    let host = format!("ortak-project-delete-{}.example", Uuid::new_v4().simple());
    let community = db
        .ensure_configured_community(&host)
        .await
        .expect("seed community")
        .id;
    let company = Uuid::new_v4();
    let project = Uuid::new_v4();
    let item = Uuid::new_v4();
    let channel = Uuid::new_v4();
    let actor = "a".repeat(64);
    let mut tx = db.pool.begin().await.expect("seed transaction");
    sqlx::query("INSERT INTO companies(id,slug,display_name) VALUES($1,$2,'Deletion fixture')")
        .bind(company)
        .bind(format!("delete_{}", company.simple()))
        .execute(&mut *tx)
        .await
        .expect("company");
    sqlx::query("INSERT INTO office_company_bindings(company_id,community_id) VALUES($1,$2)")
        .bind(company)
        .bind(community.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("Office binding");
    sqlx::query(
        "INSERT INTO channels(community_id,id,name,created_by) VALUES($1,$2,'Fixture channel',$3)",
    )
    .bind(community.as_uuid())
    .bind(channel)
    .bind(vec![0xaa_u8; 32])
    .execute(&mut *tx)
    .await
    .expect("channel");
    sqlx::query("INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig) VALUES($1,$2,$3,now(),1,'[]','Deletion fixture',$4)")
        .bind(community.as_uuid()).bind(vec![0x55_u8;32]).bind(vec![0xaa_u8;32]).bind(vec![0x66_u8;64])
        .execute(&mut *tx).await.expect("canonical Office event");
    sqlx::query("INSERT INTO projects(company_id,id,slug,name,created_by_type,created_by_id) VALUES($1,$2,'fixture','Preserved project','human',$3)")
        .bind(company).bind(project).bind(&actor).execute(&mut *tx).await.expect("project");
    // Match the tagged ProjectEvent/WorkEvent JSON persisted by ortak-work.
    sqlx::query("INSERT INTO project_history(company_id,project_id,sequence,event_type,actor_type,actor_id,payload) VALUES($1,$2,0,'project.created','human',$3,$4)")
        .bind(company).bind(project).bind(&actor)
        .bind(serde_json::json!({"event":"created","slug":"fixture"}))
        .execute(&mut *tx).await.expect("project history");
    sqlx::query("INSERT INTO project_api_bindings(company_id,project_id,community_id,channel_id,created_by) VALUES($1,$2,$3,$4,$5)")
        .bind(company).bind(project).bind(community.as_uuid()).bind(channel).bind(&actor)
        .execute(&mut *tx).await.expect("API binding");
    sqlx::query("INSERT INTO project_access_grants(company_id,project_id,actor_pubkey,role,granted_by) VALUES($1,$2,$3,'owner',$3)")
        .bind(company).bind(project).bind(&actor).execute(&mut *tx).await.expect("durable grant");
    sqlx::query("INSERT INTO work_items(company_id,id,project_id,title,created_by_type,created_by_id) VALUES($1,$2,$3,'Preserved work','human',$4)")
        .bind(company).bind(item).bind(project).bind(&actor).execute(&mut *tx).await.expect("work item");
    sqlx::query("INSERT INTO work_item_history(company_id,work_item_id,sequence,version,event_type,actor_type,actor_id,payload) VALUES($1,$2,0,1,'work.created','human',$3,$4)")
        .bind(company).bind(item).bind(&actor)
        .bind(serde_json::json!({"event":"created","title":"Preserved work","source_message_id":null}))
        .execute(&mut *tx).await.expect("work history");
    sqlx::query("INSERT INTO work_api_operations(company_id,actor_pubkey,operation_id,action,request_hash,project_id,work_item_id,result_version,auth_event_id) VALUES($1,$2,$3,'create_work_item',$4,$5,$6,1,$4)")
        .bind(company).bind(&actor).bind(Uuid::new_v4()).bind(vec![0x77_u8;32]).bind(project).bind(item)
        .execute(&mut *tx).await.expect("immutable receipt");
    tx.commit().await.expect("seed commit");
    Scope {
        community,
        host,
        company,
        project,
        item,
    }
}

async fn preserved(db: &Db, scope: &Scope) -> serde_json::Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(\
         'company',(SELECT to_jsonb(c) FROM companies c WHERE c.id=$1),\
         'project',(SELECT to_jsonb(p) FROM projects p WHERE p.company_id=$1 AND p.id=$2),\
         'project_history',(SELECT jsonb_agg(h ORDER BY sequence) FROM project_history h WHERE company_id=$1 AND project_id=$2),\
         'grants',(SELECT jsonb_agg(g ORDER BY actor_pubkey) FROM project_access_grants g WHERE company_id=$1 AND project_id=$2),\
         'work',(SELECT to_jsonb(w) FROM work_items w WHERE w.company_id=$1 AND w.id=$3),\
         'history',(SELECT jsonb_agg(h ORDER BY sequence) FROM work_item_history h WHERE company_id=$1 AND work_item_id=$3),\
         'receipts',(SELECT jsonb_agg(o ORDER BY operation_id) FROM work_api_operations o WHERE company_id=$1 AND project_id=$2))",
    ).bind(scope.company).bind(scope.project).bind(scope.item).fetch_one(&db.pool).await.expect("preserved company record snapshot")
}

async fn community_snapshot(db: &Db, scope: &Scope) -> serde_json::Value {
    sqlx::query_scalar("SELECT jsonb_build_object(        'community',(SELECT to_jsonb(c) FROM communities c WHERE id=$1),        'office',(SELECT jsonb_agg(o) FROM office_company_bindings o WHERE community_id=$1),        'bindings',(SELECT jsonb_agg(b ORDER BY project_id) FROM project_api_bindings b WHERE community_id=$1),        'channels',(SELECT jsonb_agg(c ORDER BY id) FROM channels c WHERE community_id=$1),        'events',(SELECT jsonb_agg(e ORDER BY id) FROM events e WHERE community_id=$1))")
        .bind(scope.community.as_uuid()).fetch_one(&db.pool).await.expect("unrelated community snapshot")
}

async fn scoped_counts(db: &Db, scope: &Scope) -> (i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM project_api_bindings WHERE community_id=$1), (SELECT count(*) FROM office_company_bindings WHERE community_id=$1), (SELECT count(*) FROM channels WHERE community_id=$1), (SELECT count(*) FROM events WHERE community_id=$1)")
        .bind(scope.community.as_uuid()).fetch_one(&db.pool).await.expect("community counts")
}

async fn approved(db: &Db, store: &DeletionStore, scope: &Scope) -> LeaseToken {
    let submitted = store
        .submit(&scope.host, "fixture-operator", Some("fresh fixture only"))
        .await
        .expect("submit");
    let inventory = FrozenInventory {
        schema: store
            .inventory_schema(scope.community)
            .await
            .expect("inventory seeded tables"),
        storage: empty_storage_manifest(scope.community),
    };
    store
        .freeze_inventory(submitted.id, &inventory)
        .await
        .expect("freeze inventory");
    store
        .approve(submitted.id, "fixture-approver", None)
        .await
        .expect("approve exact digest");
    let claim = store
        .claim_specific(submitted.id, "fixture-executor", DEFAULT_LEASE_DURATION)
        .await
        .expect("claim")
        .expect("won claim");
    store.begin_quiescing(&claim.lease).await.expect("quiesce");
    let fence = store.fence(&claim.lease).await.expect("fence");
    let token = LeaseToken {
        fence_generation: Some(fence),
        ..claim.lease
    };
    store
        .freeze_destructive_storage_manifest(&token, &inventory.storage)
        .await
        .expect("freeze empty external manifest");
    store.mark_drained(&token).await.expect("drain");
    store
        .mark_bindings_removed(&token, serde_json::json!({"keys":0}))
        .await
        .expect("external bindings removed");
    assert_eq!(scoped_counts(db, scope).await, (1, 1, 1, 1));
    token
}

async fn executor_gucs(tx: &mut Transaction<'_, Postgres>, token: &LeaseToken, full: bool) {
    sqlx::query("SELECT set_config('buzz.deletion_executor_community',$1,true),set_config('buzz.deletion_fence_generation',$2,true)")
        .bind(token.community_id.to_string()).bind(token.fence_generation.expect("fenced").to_string())
        .execute(&mut **tx).await.expect("bare executor settings");
    if full {
        sqlx::query("SELECT set_config('buzz.deletion_request_id',$1,true),set_config('buzz.deletion_lease_owner',$2,true),set_config('buzz.deletion_lease_generation',$3,true)")
            .bind(token.request_id.to_string()).bind(&token.owner).bind(token.generation.to_string())
            .execute(&mut **tx).await.expect("complete executor settings");
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable BUZZ_TEST_DATABASE_URL and migration 0055"]
async fn approved_community_purge_detaches_only_its_api_bindings_and_preserves_work() {
    let (db, store) = isolated_store().await;
    let selected = seed(&db).await;
    let other = seed(&db).await;
    let before = preserved(&db, &selected).await;
    let other_before = preserved(&db, &other).await;
    let other_office_before = community_snapshot(&db, &other).await;
    for sql in [
        "DELETE FROM project_api_bindings WHERE company_id=$1",
        "UPDATE project_api_bindings SET channel_id=channel_id WHERE company_id=$1",
    ] {
        assert!(
            sqlx::query(sql)
                .bind(selected.company)
                .execute(&db.pool)
                .await
                .is_err(),
            "ordinary binding mutation is forbidden"
        );
    }
    let token = approved(&db, &store, &selected).await;
    let mut forged = db.pool.begin().await.expect("forged transaction");
    executor_gucs(&mut forged, &token, false).await;
    assert!(
        sqlx::query("DELETE FROM project_api_bindings WHERE company_id=$1")
            .bind(selected.company)
            .execute(&mut *forged)
            .await
            .is_err(),
        "bare serving-fence GUCs are not deletion approval"
    );
    forged.rollback().await.expect("rollback forged attempt");
    let purged = store
        .purge_postgres(&token)
        .await
        .expect("real approved purge");
    assert_eq!(purged.get("project_api_bindings"), Some(&1));
    assert_eq!(scoped_counts(&db, &selected).await, (0, 0, 0, 0));
    assert_eq!(scoped_counts(&db, &other).await, (1, 1, 1, 1));
    assert_eq!(preserved(&db, &selected).await, before);
    assert_eq!(preserved(&db, &other).await, other_before);
    assert_eq!(community_snapshot(&db, &other).await, other_office_before);
    assert_eq!(
        store.get(token.request_id).await.expect("request").stage,
        DeletionStage::PostgresPurged
    );
    let state: String = sqlx::query_scalar("SELECT deletion_state FROM communities WHERE id=$1")
        .bind(selected.community.as_uuid())
        .fetch_one(&db.pool)
        .await
        .expect("tombstone");
    assert_eq!(state, "tombstone");
    assert!(store
        .is_serving_active(other.community)
        .await
        .expect("other still serves"));
}

#[tokio::test]
#[ignore = "requires explicit disposable BUZZ_TEST_DATABASE_URL and migration 0055"]
async fn stale_expired_or_incomplete_executor_cannot_detach_project_authority() {
    let (db, store) = isolated_store().await;
    let scope = seed(&db).await;
    let token = approved(&db, &store, &scope).await;
    let mut stale = token.clone();
    stale.generation += 1;
    assert!(store.purge_postgres(&stale).await.is_err());
    for candidate in [
        stale,
        LeaseToken {
            owner: "wrong-owner".into(),
            ..token.clone()
        },
    ] {
        let mut tx = db.pool.begin().await.expect("stale SQL transaction");
        executor_gucs(&mut tx, &candidate, true).await;
        assert!(
            sqlx::query("DELETE FROM project_api_bindings WHERE company_id=$1")
                .bind(scope.company)
                .execute(&mut *tx)
                .await
                .is_err()
        );
        tx.rollback().await.expect("rollback stale SQL");
    }
    let mut early = db.pool.begin().await.expect("early constraint transaction");
    executor_gucs(&mut early, &token, true).await;
    sqlx::query("DELETE FROM project_api_bindings WHERE company_id=$1")
        .bind(scope.company)
        .execute(&mut *early)
        .await
        .expect("immediate lease permits pending detach");
    assert!(
        sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
            .execute(&mut *early)
            .await
            .is_err(),
        "detachment cannot commit before postgres_purged plus tombstone"
    );
    early.rollback().await.expect("rollback early detach");
    sqlx::query("UPDATE community_deletion_requests SET lease_until=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(token.request_id).execute(&db.pool).await.expect("expire fixture lease");
    assert!(
        store.purge_postgres(&token).await.is_err(),
        "expired durable lease refuses purge"
    );
    assert_eq!(scoped_counts(&db, &scope).await, (1, 1, 1, 1));
    assert_eq!(
        store
            .get(token.request_id)
            .await
            .expect("unchanged stage")
            .stage,
        DeletionStage::BindingsRemoved
    );
}

#[tokio::test]
#[ignore = "requires explicit disposable BUZZ_TEST_DATABASE_URL and migration 0055"]
async fn expiry_after_real_purge_stage_write_rolls_back_at_deferred_commit() {
    let (db, store) = isolated_store().await;
    let scope = seed(&db).await;
    let before = preserved(&db, &scope).await;
    let token = approved(&db, &store, &scope).await;
    let suffix = Uuid::new_v4().simple().to_string();
    let name = format!("ortak_detach_barrier_{suffix}");
    let key = i64::from(Uuid::new_v4().as_fields().0 & 0x3fff_ffff);
    // UUID-only identifiers and a locally generated integer; no external SQL input.
    sqlx::raw_sql(AssertSqlSafe(format!(
        "CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock({key}); RETURN NEW; END $$; \
         CREATE TRIGGER {name} AFTER UPDATE ON community_deletion_requests FOR EACH ROW \
         WHEN (NEW.id='{}'::uuid AND NEW.stage='postgres_purged' AND OLD.stage IS DISTINCT FROM NEW.stage) EXECUTE FUNCTION {name}()", token.request_id)))
        .execute(&db.pool).await.expect("install request-scoped final-stage barrier");
    let mut barrier = db.pool.acquire().await.expect("barrier connection");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(key)
        .execute(&mut *barrier)
        .await
        .expect("hold barrier");
    sqlx::query("UPDATE community_deletion_requests SET lease_until=clock_timestamp()+interval '900 milliseconds' WHERE id=$1")
        .bind(token.request_id).execute(&db.pool).await.expect("bounded fixture deadline");
    let worker = store.clone();
    let owned = token.clone();
    let mut task = tokio::spawn(async move { worker.purge_postgres(&owned).await });
    let reached = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND classid=0 AND objid=$1::bigint::oid AND NOT granted)")
                .bind(key).fetch_one(&db.pool).await.expect("observe exact final-stage barrier");
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.is_ok();
    let expired = if reached {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let elapsed: bool = sqlx::query_scalar("SELECT lease_until<clock_timestamp() FROM community_deletion_requests WHERE id=$1")
                    .bind(token.request_id).fetch_one(&db.pool).await.expect("observe database lease expiry");
                if elapsed { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.is_ok()
    } else {
        false
    };
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(key)
        .execute(&mut *barrier)
        .await
        .expect("release barrier");
    let result = match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
        Ok(joined) => Some(joined.expect("purge task")),
        Err(_) => {
            task.abort();
            let _ = task.await;
            None
        }
    };
    sqlx::raw_sql(AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON community_deletion_requests; DROP FUNCTION {name}()"
    )))
    .execute(&db.pool)
    .await
    .expect("remove only the fixture barrier");
    assert!(
        reached && expired,
        "must exercise expiry after the production postgres_purged write"
    );
    let error = result
        .expect("purge settles after releasing its barrier")
        .expect_err("deferred commit refuses an expired deletion lease");
    assert!(
        matches!(&error, DbError::Sqlx(sqlx::Error::Database(database))
        if database.code().as_deref() == Some("40001")
            && database.message().contains("project binding deletion authority is not current")),
        "must fail at the new deferred project binding guard: {error}"
    );
    assert_eq!(scoped_counts(&db, &scope).await, (1, 1, 1, 1));
    assert_eq!(preserved(&db, &scope).await, before);
    assert_eq!(
        store
            .get(token.request_id)
            .await
            .expect("rolled-back request")
            .stage,
        DeletionStage::BindingsRemoved
    );
    let state: String = sqlx::query_scalar("SELECT deletion_state FROM communities WHERE id=$1")
        .bind(scope.community.as_uuid())
        .fetch_one(&db.pool)
        .await
        .expect("not tombstoned");
    assert_eq!(state, "fenced");
}
