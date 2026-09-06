//! Native read-state replacement must not retire an unrelated plaintext source.
use super::*;

#[tokio::test]
#[ignore = "requires explicit disposable55432 with immutable78; local HTTP only"]
async fn employee_runtime_v5_read_state_replacement_preserves_use_but_source_edit_revokes() {
    let x = EmployeeFixture::new("relationship", false).await;
    let c = &x.c;
    let db = buzz_db::Db::from_pool(c.x.f.pool.clone());
    let community = db
        .lookup_community_by_host(&format!("api-{}.example", c.x.f.community.simple()))
        .await
        .unwrap()
        .unwrap()
        .id;
    assert_eq!(community.as_uuid(), &c.x.f.community);
    let d_tag = format!("read-state:{}", Uuid::new_v4().simple());
    let tags = vec![
        Tag::parse(["d", d_tag.as_str()]).unwrap(),
        Tag::parse(["t", "read-state"]).unwrap(),
    ];
    let at = nostr::Timestamp::now().as_secs().saturating_sub(2);
    // Payload stays opaque to the production store, as native encrypted
    // read-state does. This tests persistence, not NIP-44 encryption/decryption.
    let old = EventBuilder::new(Kind::Custom(30078), "opaque old read-state fixture")
        .tags(tags.clone())
        .custom_created_at(nostr::Timestamp::from(at))
        .sign_with_keys(&c.x.f.operator)
        .unwrap();
    let replacement = EventBuilder::new(Kind::Custom(30078), "opaque new read-state fixture")
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from(at + 1))
        .sign_with_keys(&c.x.f.operator)
        .unwrap();
    assert!(db
        .replace_parameterized_event(community, &old, &d_tag, None)
        .await
        .unwrap()
        .1);

    let (run, _) = c.start_office_with(&x.memory()).await;
    RunSupervisor::new(c.x.f.control.clone(), &c.runtime, SupervisorConfig::default())
        .pump(&c.x.scope, run)
        .await
        .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM runs WHERE company_id=$1 AND id=$2")
        .bind(c.x.f.company)
        .bind(run)
        .fetch_one(&c.x.f.pool)
        .await
        .unwrap();
    assert_eq!(status, "running");
    let frozen = c.bytes(run).await;
    assert_eq!(c.wire(run).await["version"], 5);
    assert_eq!(x.uses(run).await, vec![(0, x.fact)]);
    assert!(c.current(run).await);
    let before = epoch(c).await;
    let generation: i64 = sqlx::query_scalar("SELECT generation FROM office_authority_generations WHERE company_id=$1")
        .bind(c.x.f.company).fetch_one(&c.x.f.pool).await.unwrap();

    // The actual production NIP-RS replacement hard-deletes the old NULL-
    // channel row; no synthetic trigger invocation or fabricated use receipt.
    assert!(db
        .replace_parameterized_event(community, &replacement, &d_tag, None)
        .await
        .unwrap()
        .1);
    let retained: Vec<Vec<u8>> = sqlx::query_scalar("SELECT id FROM events WHERE community_id=$1 AND kind=30078 AND pubkey=$2 AND d_tag=$3 ORDER BY id LIMIT 2")
        .bind(c.x.f.community).bind(c.x.f.operator.public_key().to_bytes().as_slice())
        .bind(&d_tag).fetch_all(&c.x.f.pool).await.unwrap();
    assert_eq!(retained, vec![replacement.id.to_bytes().to_vec()]);
    assert_eq!(epoch(c).await, before, "unrelated read-state deletion must not retire the selected employee source");
    let changed_generation: i64 = sqlx::query_scalar("SELECT generation FROM office_authority_generations WHERE company_id=$1")
        .bind(c.x.f.company).fetch_one(&c.x.f.pool).await.unwrap();
    assert!(changed_generation > generation, "the existing global Office mutation fence is still active");
    assert!(c.current(run).await);
    let report = ortak_runtime::reconciliation::reconcile_office_runs(
        &c.x.f.control, &c.x.scope, 8,
    ).await.unwrap();
    assert_eq!((report.reviewed, report.revocations), (1, 0));
    let cancelled: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2)")
        .bind(c.x.f.company).bind(run).fetch_one(&c.x.f.pool).await.unwrap();
    assert!(!cancelled);
    assert_eq!(c.bytes(run).await, frozen);

    // Either side may be the canonical event. Roll back these two deliberate
    // kind mutations so the subsequent real source edit has its own witness.
    let mut tx = c.x.f.pool.begin().await.unwrap();
    for (kind, expected_epoch) in [(30078, before + 1), (9, before + 2)] {
        let changed = sqlx::query("UPDATE events SET kind=$3 WHERE community_id=$1 AND id=$2")
            .bind(c.x.f.community).bind(hex::decode(&c.x.source).unwrap())
            .bind(kind).execute(&mut *tx).await.unwrap();
        assert_eq!(changed.rows_affected(), 1);
        let actual: i64 = sqlx::query_scalar("SELECT epoch FROM employee_memory_channel_authorities WHERE company_id=$1 AND employee_id='cem' AND channel_id=$2")
            .bind(c.x.f.company).bind(c.x.f.channel).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(actual, expected_epoch, "transition into or out of plaintext must retire the epoch");
    }
    tx.rollback().await.unwrap();
    assert_eq!(epoch(c).await, before);
    assert!(c.current(run).await);

    // This source is also the active request's actual parent/root. A canonical
    // source/ancestor edit must still retire its use; restoring bytes must not
    // revive the original epoch. No other runtime or provider run is started.
    c.edit_fact_source(true).await;
    assert!(epoch(c).await > before);
    assert!(!c.current(run).await);
    c.edit_fact_source(false).await;
    assert!(!c.current(run).await);
    let report = ortak_runtime::reconciliation::reconcile_runtime(
        &c.x.f.control, &c.runtime, &c.x.scope, &SupervisorConfig::default(), 8,
    ).await.unwrap();
    assert_eq!((report.revocations, report.stop_attempts), (1, 1));
    let cancelled: (String, String) = sqlx::query_as("SELECT r.status,c.state FROM runs r JOIN runtime_cancellations c ON c.company_id=r.company_id AND c.run_id=r.id WHERE r.company_id=$1 AND r.id=$2")
        .bind(c.x.f.company).bind(run).fetch_one(&c.x.f.pool).await.unwrap();
    assert_eq!(cancelled, ("cancelled".into(), "acknowledged".into()));
    assert_eq!(c.bytes(run).await, frozen);
    assert_eq!(x.uses(run).await, vec![(0, x.fact)]);
    assert_eq!(c.runtime.start_specs().len(), 1);
    x.prove_target_unchanged().await;
}

async fn epoch(c: &ConversationFixture) -> i64 {
    sqlx::query_scalar("SELECT epoch FROM employee_memory_channel_authorities WHERE company_id=$1 AND employee_id='cem' AND channel_id=$2")
        .bind(c.x.f.company).bind(c.x.f.channel).fetch_one(&c.x.f.pool).await.unwrap()
}
