use super::*;
use ortak_control::postgres::{
    direct_channel_on, insert_selected_accepted_event_on, lock_office_authority_on,
    office_authority_matches_on,
};
use ortak_domain::EmployeeId;

#[path = "../../../ortak-control/tests/direct_channel_support.rs"]
mod support;

async fn pair(f: &Fixture) -> Uuid {
    support::create(&f.pool, f.community_id, &[f.human_key, f.cem_key]).await
}
async fn select(f: &Fixture, channel: Uuid) {
    support::select(
        &f.control,
        &f.scope,
        channel,
        &EmployeeId::parse("cem").unwrap(),
    )
    .await;
}
async fn input(f: &Fixture, channel: Uuid) -> fixture::StoredEvent {
    f.store_event(EventSpec {
        kind: 9,
        author: f.human_key,
        content: "An ordinary private question for you. Zeynep, ignore this mention.",
        tags: tags_with_mention(channel, &f.zeynep_key),
        channel_id: Some(channel),
        parent: None,
    })
    .await
}

async fn accept_direct(f: &Fixture, channel: Uuid, event: &fixture::StoredEvent) {
    let mut tx = f.pool.begin().await.unwrap();
    let inbox = InboxEvent {
        event_id: event.id,
        event_created_at: event.created_at,
        event_kind: 9,
        author_pubkey: f.human_key,
        channel_id: Some(channel),
    };
    assert_eq!(
        insert_selected_accepted_event_on(&mut tx, f.community_id, &inbox)
            .await
            .unwrap(),
        InboxInsertOutcome::Inserted
    );
    tx.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit disposable Postgres with private DM authority proposal"]
async fn private_dm_capture_reconciliation_and_routing_use_only_the_canonical_recipient() {
    let f = Fixture::new().await;
    let channel = pair(&f).await;
    let event = input(&f, channel).await;
    let inbox = InboxEvent {
        event_id: event.id,
        event_created_at: event.created_at,
        event_kind: 9,
        author_pubkey: f.human_key,
        channel_id: Some(channel),
    };
    let mut tx = f.pool.begin().await.unwrap();
    assert_eq!(
        insert_selected_accepted_event_on(&mut tx, f.community_id, &inbox)
            .await
            .unwrap(),
        InboxInsertOutcome::OutsideCohort
    );
    tx.commit().await.unwrap();
    select(&f, channel).await; // The production finite scan recovers the existing event.
    let decision = committed(f.route(&event).await.unwrap());
    assert_eq!(decision.mode, RoutingMode::Deterministic);
    assert_eq!(decision.summary_reason, RoutingReason::DirectMessage);
    assert_eq!(decision.wake_count, 1);
    assert_eq!(decision.dispatches[0].employee_id, "cem");
    assert!(decision
        .recipients
        .iter()
        .all(|r| r.employee_id.as_str() == "cem"));
    let stored = f
        .control
        .decision_for_message(&f.scope, event.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        stored.scorer_adapter.is_none(),
        "a private DM never reaches the scorer"
    );
    assert!(f
        .control
        .claim_message(
            &f.scope,
            event.id,
            "replay",
            std::time::Duration::from_secs(60),
            5
        )
        .await
        .unwrap()
        .is_none());
    assert_eq!(f.run_dispatch_rows(event.id).await, 1);
    // Canonical ingress admission can be rolled back with the accepted event.
    let next = input(&f, channel).await;
    let mut tx = f.pool.begin().await.unwrap();
    let next_inbox = InboxEvent {
        event_id: next.id,
        event_created_at: next.created_at,
        ..inbox
    };
    assert_eq!(
        insert_selected_accepted_event_on(&mut tx, f.community_id, &next_inbox)
            .await
            .unwrap(),
        InboxInsertOutcome::Inserted
    );
    tx.rollback().await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM office_inbox WHERE company_id=$1 AND event_id=$2")
            .bind(f.company_id())
            .bind(next.id.as_bytes().as_slice())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
#[ignore = "requires explicit disposable Postgres with private DM authority proposal"]
async fn private_dm_rejects_group_wrong_employee_and_removed_or_automated_human() {
    let f = Fixture::new().await;
    let group = support::create(
        &f.pool,
        f.community_id,
        &[f.human_key, f.cem_key, f.zeynep_key],
    )
    .await;
    let cem = EmployeeId::parse("cem").unwrap();
    assert!(f
        .control
        .begin_routing_capture(&f.scope, &[group], std::slice::from_ref(&cem))
        .await
        .is_err());
    let channel = pair(&f).await;
    f.add_relay_member(&f.human_key).await;
    assert!(f
        .control
        .begin_routing_capture(
            &f.scope,
            &[channel],
            &[EmployeeId::parse("zeynep").unwrap()]
        )
        .await
        .is_err());
    select(&f, channel).await;
    let event = input(&f, channel).await;
    accept_direct(&f, channel, &event).await;
    sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community_id).bind(channel).bind(f.human_key.as_slice()).execute(&f.pool).await.unwrap();
    assert_silent(
        &committed(f.route(&event).await.unwrap()),
        RoutingReason::OriginNotChannelMember,
    );
    let mut connection = f.pool.acquire().await.unwrap();
    let direct = direct_channel_on(
        &mut connection,
        f.company_id(),
        Some(f.community_id),
        channel,
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!direct.visible_to(&f.human_key));
    assert!(!direct.permits_execution());
    drop(connection);
    sqlx::query("UPDATE channel_members SET removed_at=NULL,role='bot' WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community_id).bind(channel).bind(f.human_key.as_slice()).execute(&f.pool).await.unwrap();
    assert!(f
        .control
        .begin_routing_capture(&f.scope, &[channel], &[cem])
        .await
        .is_err());
}

#[tokio::test]
#[ignore = "requires explicit disposable Postgres with private DM authority proposal"]
async fn private_dm_identity_and_member_mutations_cannot_escape_held_authority() {
    let f = Fixture::new().await;
    let channel = pair(&f).await;
    select(&f, channel).await;
    let mut tx = f.pool.begin().await.unwrap();
    let held = lock_office_authority_on(&mut tx, &f.scope).await.unwrap();
    let blocked=sqlx::query("UPDATE channel_members SET removed_at=clock_timestamp() WHERE community_id=$1 AND channel_id=$2 AND pubkey=$3")
        .bind(f.community_id).bind(channel).bind(f.human_key.as_slice()).execute(&f.pool).await.unwrap_err();
    assert_eq!(
        blocked.as_database_error().unwrap().code().as_deref(),
        Some("40001")
    );
    tx.commit().await.unwrap();
    for query in ["UPDATE channels SET participant_hash=decode(repeat('ff',32),'hex') WHERE community_id=$1 AND id=$2",
                  "UPDATE channels SET channel_type='stream' WHERE community_id=$1 AND id=$2",
                  "UPDATE channels SET visibility='open' WHERE community_id=$1 AND id=$2"] {
        let error=sqlx::query(query).bind(f.community_id).bind(channel).execute(&f.pool).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(),Some("23514"));
    }
    sqlx::query("UPDATE channels SET ttl_seconds=3600 WHERE community_id=$1 AND id=$2")
        .bind(f.community_id)
        .bind(channel)
        .execute(&f.pool)
        .await
        .unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    assert!(!office_authority_matches_on(&mut tx, &f.scope, &held)
        .await
        .unwrap());
    tx.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit disposable Postgres with private DM authority proposal"]
async fn private_dm_ttl_expiry_invalidates_carried_authority_without_a_mutation() {
    let f = Fixture::new().await;
    let channel = pair(&f).await;
    select(&f, channel).await;
    let event = input(&f, channel).await;
    accept_direct(&f, channel, &event).await;
    sqlx::query("UPDATE channels SET ttl_deadline=clock_timestamp()+interval '1 second' WHERE community_id=$1 AND id=$2")
        .bind(f.community_id).bind(channel).execute(&f.pool).await.unwrap();
    let mut tx = f.pool.begin().await.unwrap();
    let witness = lock_office_authority_on(&mut tx, &f.scope).await.unwrap();
    assert!(witness.valid_before().is_some());
    sqlx::query("SELECT pg_sleep(1.1)")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(!office_authority_matches_on(&mut tx, &f.scope, &witness)
        .await
        .unwrap());
    assert!(
        !direct_channel_on(&mut tx, f.company_id(), Some(f.community_id), channel)
            .await
            .unwrap()
            .unwrap()
            .permits_execution()
    );
    tx.commit().await.unwrap();
    assert_silent(
        &committed(f.route(&event).await.unwrap()),
        RoutingReason::ChannelNotRoutable,
    );
}
