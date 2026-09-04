//! Production-seam Postgres tests for the Ortak Milestone 2 Office-ingress
//! adapter (`buzz_relay::handlers::office_ingress`).
//!
//! Run against the local scratch database that carries the embedded
//! migrations, for example:
//! `ORTAK_TEST_DATABASE_URL=postgres://ortak:ortak@localhost:55432/ortak \
//!   cargo test -p buzz-relay --test office_ingress_postgres -- --ignored`

use buzz_core::kind::KIND_STREAM_MESSAGE;
use buzz_core::CommunityId;
use buzz_db::Db;
use buzz_relay::handlers::office_ingress::{persist_with_office_inbox, OfficeIngressError};
use nostr::{EventBuilder, Keys, Kind, Tag};
use ortak_control::inbox::InboxInsertOutcome;
use sqlx::{PgPool, Row};
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

const DEFAULT_DATABASE_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1 -- local test-only credentials

fn database_url() -> String {
    std::env::var("ORTAK_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("BUZZ_TEST_DATABASE_URL"))
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned())
}

async fn setup_pool() -> PgPool {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to test database");
    MIGRATOR.run(&pool).await.expect("apply migrations");
    pool
}

/// Inserts a Buzz community. Returns its id; no company binding is created.
async fn create_community(pool: &PgPool) -> CommunityId {
    let community_id = Uuid::new_v4();
    sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
        .bind(community_id)
        .bind(format!("office-ingress-{}.example", community_id.simple()))
        .execute(pool)
        .await
        .expect("insert community");
    CommunityId::from_uuid(community_id)
}

/// Registers an Ortak company and binds the community to it. Returns the
/// company id.
async fn bind_company(pool: &PgPool, community: CommunityId) -> Uuid {
    let company_id: Uuid =
        sqlx::query("INSERT INTO companies (slug, display_name) VALUES ($1, $2) RETURNING id")
            .bind(format!("co-{}", Uuid::new_v4().simple()))
            .bind("Office ingress test company")
            .fetch_one(pool)
            .await
            .expect("insert company")
            .try_get("id")
            .expect("company id");
    sqlx::query("INSERT INTO office_company_bindings (community_id, company_id) VALUES ($1, $2)")
        .bind(community.as_uuid())
        .bind(company_id)
        .execute(pool)
        .await
        .expect("insert binding");
    company_id
}

fn signed_channel_message(channel_id: Uuid) -> nostr::Event {
    EventBuilder::new(
        Kind::Custom(KIND_STREAM_MESSAGE as u16),
        "Cem, selam nasılsın?",
    )
    .tags([Tag::parse(["h", &channel_id.to_string()]).expect("h tag")])
    .sign_with_keys(&Keys::generate())
    .expect("sign event")
}

async fn event_rows(pool: &PgPool, community: CommunityId, event: &nostr::Event) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM events WHERE community_id = $1 AND id = $2")
        .bind(community.as_uuid())
        .bind(event.id.as_bytes().as_slice())
        .fetch_one(pool)
        .await
        .expect("count events")
}

/// Counts inbox rows for the event id across every company, so a leaked
/// row under the wrong scope would also be detected.
async fn inbox_rows(pool: &PgPool, event: &nostr::Event) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM office_inbox WHERE event_id = $1")
        .bind(event.id.as_bytes().as_slice())
        .fetch_one(pool)
        .await
        .expect("count inbox rows")
}

#[tokio::test]
#[ignore = "requires a local PostgreSQL scratch database"]
async fn disabled_path_keeps_inherited_persistence_without_inbox_row() {
    let pool = setup_pool().await;
    let db = Db::from_pool(pool.clone());
    let community = create_community(&pool).await;
    // A binding exists, so only the path selection can explain the absence
    // of an inbox row.
    bind_company(&pool, community).await;
    let channel_id = Uuid::new_v4();
    let event = signed_channel_message(channel_id);

    let (stored, was_inserted) = db
        .insert_event_with_thread_metadata(community, &event, Some(channel_id), None)
        .await
        .expect("inherited insert");

    assert!(was_inserted);
    assert_eq!(stored.event.id, event.id);
    assert_eq!(event_rows(&pool, community, &event).await, 1);
    assert_eq!(
        inbox_rows(&pool, &event).await,
        0,
        "the inherited path must never write an office_inbox row"
    );
}

#[tokio::test]
#[ignore = "requires a local PostgreSQL scratch database"]
async fn enabled_path_commits_event_and_inbox_row_together() {
    let pool = setup_pool().await;
    let db = Db::from_pool(pool.clone());
    let community = create_community(&pool).await;
    let company_id = bind_company(&pool, community).await;
    let channel_id = Uuid::new_v4();
    let event = signed_channel_message(channel_id);

    let outcome = persist_with_office_inbox(&db, community, &event, Some(channel_id), None)
        .await
        .expect("atomic handoff");

    assert!(outcome.was_inserted);
    assert_eq!(outcome.inbox, InboxInsertOutcome::Inserted);
    assert_eq!(outcome.stored_event.event.id, event.id);
    assert_eq!(event_rows(&pool, community, &event).await, 1);

    let row = sqlx::query(
        "SELECT i.company_id, i.event_kind, i.author_pubkey, i.channel_id, i.state,
                (i.event_created_at = e.created_at) AS partition_key_matches
           FROM office_inbox i
           JOIN events e ON e.community_id = $1 AND e.id = i.event_id
          WHERE i.event_id = $2",
    )
    .bind(community.as_uuid())
    .bind(event.id.as_bytes().as_slice())
    .fetch_one(&pool)
    .await
    .expect("inbox row joined to the stored event");
    let bound_company: Uuid = row.try_get("company_id").unwrap();
    let kind: i32 = row.try_get("event_kind").unwrap();
    let author: Vec<u8> = row.try_get("author_pubkey").unwrap();
    let inbox_channel: Option<Uuid> = row.try_get("channel_id").unwrap();
    let state: String = row.try_get("state").unwrap();
    let partition_key_matches: bool = row.try_get("partition_key_matches").unwrap();
    assert_eq!(
        bound_company, company_id,
        "company scope comes only from office_company_bindings"
    );
    assert_eq!(kind, KIND_STREAM_MESSAGE as i32);
    assert_eq!(author, event.pubkey.to_bytes().to_vec());
    assert_eq!(inbox_channel, Some(channel_id));
    assert_eq!(state, "pending");
    assert!(partition_key_matches);
}

#[tokio::test]
#[ignore = "requires a local PostgreSQL scratch database"]
async fn enabled_path_unknown_binding_fails_closed_and_rolls_back_event() {
    let pool = setup_pool().await;
    let db = Db::from_pool(pool.clone());
    let community = create_community(&pool).await;
    let channel_id = Uuid::new_v4();
    let event = signed_channel_message(channel_id);

    let error = persist_with_office_inbox(&db, community, &event, Some(channel_id), None)
        .await
        .expect_err("unbound community must fail closed");

    match error {
        OfficeIngressError::UnknownCompanyBinding { community_id } => {
            assert_eq!(community_id, *community.as_uuid());
        }
        other => panic!("expected UnknownCompanyBinding, got {other:?}"),
    }
    assert_eq!(
        event_rows(&pool, community, &event).await,
        0,
        "the signed event must roll back with the failed handoff"
    );
    assert_eq!(inbox_rows(&pool, &event).await, 0);
}

#[tokio::test]
#[ignore = "requires a local PostgreSQL scratch database"]
async fn enabled_path_replay_is_idempotent() {
    let pool = setup_pool().await;
    let db = Db::from_pool(pool.clone());
    let community = create_community(&pool).await;
    bind_company(&pool, community).await;
    let channel_id = Uuid::new_v4();
    let event = signed_channel_message(channel_id);

    let first = persist_with_office_inbox(&db, community, &event, Some(channel_id), None)
        .await
        .expect("first accept");
    let replay = persist_with_office_inbox(&db, community, &event, Some(channel_id), None)
        .await
        .expect("replay is accepted");

    assert!(first.was_inserted);
    assert_eq!(first.inbox, InboxInsertOutcome::Inserted);
    assert!(!replay.was_inserted, "replay must report the duplicate");
    assert_eq!(replay.inbox, InboxInsertOutcome::AlreadyPresent);
    assert_eq!(replay.stored_event.event.id, event.id);
    assert_eq!(event_rows(&pool, community, &event).await, 1);
    assert_eq!(
        inbox_rows(&pool, &event).await,
        1,
        "replay must not create a second inbox row"
    );
}
