use super::fixture::{Fixture, Reply};
use super::*;
#[path = "direct.rs"]
mod direct;

#[tokio::test]
#[ignore = "requires disposable localhost:55432 PostgreSQL"]
async fn lost_ack_and_adapter_restart_reuse_committed_signed_bytes() {
    let mut fixture = Fixture::new().await;
    let server = fixture.serve(vec![Reply::LostAck, Reply::Accepted]);
    let adapter = fixture.adapter();
    assert!(matches!(
        fixture.publish(&adapter).await,
        Err(OfficeIdentityError::Unavailable { .. })
    ));
    let (first, ack) = fixture.journal().await;
    assert!(!ack);
    drop(adapter);
    let restarted = fixture.adapter();
    let result = fixture.publish(&restarted).await.unwrap();
    let (second, ack) = fixture.journal().await;
    assert_eq!(first, second);
    assert!(ack);
    assert_eq!(
        result.receipt_ref,
        nostr::Event::from_json(&first).unwrap().id.to_hex()
    );
    let bodies = server.await.unwrap();
    assert_eq!(bodies, vec![first.clone(), first.clone()]);
    // A durable acknowledged replay performs only current authority/head reads.
    assert_eq!(fixture.publish(&restarted).await.unwrap(), result);
    assert_eq!(fixture.journal_count().await, 1);
    assert_eq!(fixture.journal().await.0, first);
}

#[tokio::test]
#[ignore = "requires disposable localhost:55432 PostgreSQL"]
async fn current_channel_membership_and_relay_host_are_rechecked_after_ack() {
    let mut fixture = Fixture::new().await;
    let server = fixture.serve(vec![Reply::Accepted]);
    let adapter = fixture.adapter();
    fixture.publish(&adapter).await.unwrap();
    server.await.unwrap();
    let public = OfficePublicKey::parse_hex(&fixture.entry().office.public_key).unwrap();
    assert!(adapter
        .membership_health(&public)
        .await
        .unwrap()
        .is_healthy());
    sqlx::query(
        "UPDATE channel_members SET removed_at=now() WHERE community_id=$1 AND channel_id=$2",
    )
    .bind(fixture.config.community_id)
    .bind(fixture.entry().channels[0])
    .execute(&fixture.pool)
    .await
    .unwrap();
    assert!(adapter.membership_health(&public).await.is_err());
    assert!(fixture.publish(&adapter).await.is_err());
    sqlx::query(
        "UPDATE channel_members SET removed_at=NULL WHERE community_id=$1 AND channel_id=$2",
    )
    .bind(fixture.config.community_id)
    .bind(fixture.entry().channels[0])
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE communities SET host=$2 WHERE id=$1")
        .bind(fixture.config.community_id)
        .bind(format!("changed-{}.example", Uuid::new_v4()))
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(adapter.membership_health(&public).await.is_err());
    assert!(fixture.publish(&adapter).await.is_err());
    assert_eq!(fixture.journal_count().await, 1);
}

#[tokio::test]
#[ignore = "requires disposable localhost:55432 PostgreSQL"]
async fn deactivation_archival_and_channel_expiry_are_live_membership_refusals() {
    let fixture = Fixture::new().await;
    let adapter = fixture.adapter();
    let public = OfficePublicKey::parse_hex(&fixture.entry().office.public_key).unwrap();
    for (invalidate, restore) in [
        (
            "UPDATE users SET deactivated_at=now() WHERE community_id=$1",
            "UPDATE users SET deactivated_at=NULL WHERE community_id=$1",
        ),
        (
            "UPDATE channels SET archived_at=now() WHERE community_id=$1",
            "UPDATE channels SET archived_at=NULL WHERE community_id=$1",
        ),
        (
            "UPDATE channels SET ttl_deadline=now()-interval '1 second' WHERE community_id=$1",
            "UPDATE channels SET ttl_deadline=NULL WHERE community_id=$1",
        ),
    ] {
        assert!(adapter
            .membership_health(&public)
            .await
            .unwrap()
            .is_healthy());
        sqlx::query(invalidate)
            .bind(fixture.config.community_id)
            .execute(&fixture.pool)
            .await
            .unwrap();
        assert!(adapter.membership_health(&public).await.is_err());
        assert!(fixture.publish(&adapter).await.is_err());
        sqlx::query(restore)
            .bind(fixture.config.community_id)
            .execute(&fixture.pool)
            .await
            .unwrap();
    }
    assert!(adapter
        .membership_health(&public)
        .await
        .unwrap()
        .is_healthy());
    assert_eq!(fixture.journal_count().await, 0);
}

#[tokio::test]
#[ignore = "requires disposable localhost:55432 PostgreSQL"]
async fn cross_company_community_employee_and_channel_rows_cannot_supply_membership() {
    let fixture = Fixture::new().await;
    let adapter = fixture.adapter();
    let public = OfficePublicKey::parse_hex(&fixture.entry().office.public_key).unwrap();
    assert!(adapter
        .membership_health(&public)
        .await
        .unwrap()
        .is_healthy());
    let mut config = fixture.config.clone();
    config.community_id = Uuid::new_v4();
    let foreign = PgOfficeIdentityAdapter::new(
        PgControlPlane::new(fixture.pool.clone()),
        fixture.signer.clone(),
        config,
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(foreign.membership_health(&public).await.is_err());
    let mut config = fixture.config.clone();
    config.employees[0].channels = vec![Uuid::new_v4()];
    config.employees[0].office.home_channel_ref = None;
    let foreign = PgOfficeIdentityAdapter::new(
        PgControlPlane::new(fixture.pool.clone()),
        fixture.signer.clone(),
        config,
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(foreign.membership_health(&public).await.is_err());
    let other_community = Uuid::new_v4();
    sqlx::query("INSERT INTO communities(id,host) VALUES ($1,$2)")
        .bind(other_community)
        .bind(format!("foreign-{}.example", other_community.simple()))
        .execute(&fixture.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO relay_members(community_id,pubkey,role) VALUES ($1,$2,'member')")
        .bind(other_community)
        .bind(&fixture.entry().office.public_key)
        .execute(&fixture.pool)
        .await
        .unwrap();
    // The same public key in another live community cannot satisfy the
    // configured community's relay membership requirement.
    sqlx::query("DELETE FROM relay_members WHERE community_id=$1")
        .bind(fixture.config.community_id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(adapter.membership_health(&public).await.is_err());
    sqlx::query("DELETE FROM office_company_bindings WHERE company_id=$1")
        .bind(fixture.config.company_id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO office_company_bindings(company_id,community_id) VALUES ($1,$2)")
        .bind(fixture.config.company_id)
        .bind(other_community)
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(adapter.membership_health(&public).await.is_err());
    assert_eq!(fixture.journal_count().await, 0);
}

#[tokio::test]
#[ignore = "requires disposable localhost:55432 PostgreSQL"]
async fn profile_requires_exact_running_authorized_manifest_before_any_journal_write() {
    let fixture = Fixture::new().await;
    let adapter = fixture.adapter();
    assert!(adapter
        .publish_profile(
            &fixture.entry().employee_id,
            &fixture.entry().office,
            "Different",
            &fixture.key
        )
        .await
        .is_err());
    assert!(adapter
        .publish_profile(
            &fixture.entry().employee_id,
            &fixture.entry().office,
            "Ada",
            "unknown-step"
        )
        .await
        .is_err());
    sqlx::query("UPDATE provisioning_operations SET dry_run=true WHERE company_id=$1")
        .bind(fixture.config.company_id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(fixture.publish(&adapter).await.is_err());
    sqlx::query("UPDATE provisioning_operations SET dry_run=false WHERE company_id=$1")
        .bind(fixture.config.company_id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE provisioning_operation_steps SET state='pending' WHERE company_id=$1")
        .bind(fixture.config.company_id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    assert!(fixture.publish(&adapter).await.is_err());
    assert_eq!(fixture.journal_count().await, 0);
}

#[tokio::test]
#[ignore = "requires disposable localhost:55432 PostgreSQL"]
async fn ack_without_canonical_profile_remains_retryable_and_unacknowledged() {
    let mut fixture = Fixture::new().await;
    let server = fixture.serve(vec![Reply::AckOnly]);
    assert!(matches!(fixture.publish(&fixture.adapter()).await,
        Err(OfficeIdentityError::Unavailable{detail}) if detail.as_str()=="office_profile_canonical_receipt_missing"));
    server.await.unwrap();
    assert!(!fixture.journal().await.1);
}

#[tokio::test]
#[ignore = "requires disposable localhost:55432 PostgreSQL"]
async fn rejected_or_oversized_http_ack_never_settles_the_durable_profile() {
    for reply in [Reply::Rejected, Reply::Oversized] {
        let mut fixture = Fixture::new().await;
        let server = fixture.serve(vec![reply]);
        assert!(matches!(
            fixture.publish(&fixture.adapter()).await,
            Err(OfficeIdentityError::Rejected { .. })
        ));
        server.await.unwrap();
        assert!(!fixture.journal().await.1);
    }
}

#[tokio::test]
#[ignore = "requires disposable localhost:55432 PostgreSQL"]
async fn concurrent_preparation_freezes_one_event_and_receipt_cannot_be_rewritten() {
    let fixture = Fixture::new().await;
    let first = fixture.adapter();
    let second = fixture.adapter();
    let (a, b) = tokio::join!(
        first.freeze_profile(fixture.entry(), "Ada", &fixture.key),
        second.freeze_profile(fixture.entry(), "Ada", &fixture.key)
    );
    assert_eq!(a.unwrap().bytes, b.unwrap().bytes);
    assert_eq!(fixture.journal_count().await, 1);
    for statement in [
        "UPDATE office_identity_profiles SET signed_event_bytes=decode('00','hex') WHERE company_id=$1",
        "UPDATE office_identity_profiles SET event_id=decode(repeat('00',32),'hex') WHERE company_id=$1",
        "DELETE FROM office_identity_profiles WHERE company_id=$1",
    ] {
        assert!(sqlx::query(statement).bind(fixture.config.company_id).execute(&fixture.pool).await.is_err());
    }
    let mut changed = fixture.config.clone();
    // Same durable step key and Office binding, but a newly broadened cohort:
    // both memberships are valid; the frozen scope hash must still conflict.
    let extra = Uuid::new_v4();
    let public = OfficePublicKey::parse_hex(&fixture.entry().office.public_key).unwrap();
    sqlx::query("INSERT INTO channels(community_id,id,name,created_by) VALUES ($1,$2,'extra',$3)")
        .bind(fixture.config.community_id)
        .bind(extra)
        .bind(public.as_bytes().as_slice())
        .execute(&fixture.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO channel_members(community_id,channel_id,pubkey) VALUES ($1,$2,$3)")
        .bind(fixture.config.community_id)
        .bind(extra)
        .bind(public.as_bytes().as_slice())
        .execute(&fixture.pool)
        .await
        .unwrap();
    changed.employees[0].channels.push(extra);
    let changed = PgOfficeIdentityAdapter::new(
        PgControlPlane::new(fixture.pool.clone()),
        fixture.signer.clone(),
        changed,
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(
        matches!(changed.freeze_profile(&changed.config.employees[0],"Ada",&fixture.key).await,
        Err(OfficeIdentityError::Rejected{detail}) if detail.as_str()=="office_profile_idempotency_conflict")
    );
}
