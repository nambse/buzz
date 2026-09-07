//! The production worker pool must keep routing contention recoverable.

use std::time::Duration;

use chrono::Utc;
use ortak_control::{
    inbox::{InboxEvent, InboxRow, InboxState},
    ports::{
        CompanyDirectory, InboxRepository, MessageNormalizer, Normalization, NormalizationRefusal,
    },
    scorer::DisabledSemanticScorer,
    CompanyScope, ControlError, InboxRoutingService, MessageId, PgControlPlane,
    RoutingWorkerConfig, ServiceOutcome,
};
use ortak_domain::{MessageOrigin, RoutingReason};
use ortak_server::connect_worker_database;
use sqlx::PgPool;
use uuid::Uuid;

// Normalization is independent of the timeout under test. Use a deterministic
// refusal so this fixture reaches the real routing commit without provisioning
// an employee or contacting a scorer/runtime/Office service.
struct RefusalNormalizer;
impl MessageNormalizer for RefusalNormalizer {
    async fn normalize(
        &self,
        _: &CompanyScope,
        _: &InboxRow,
    ) -> ortak_control::Result<Normalization> {
        Ok(Normalization::Refused(NormalizationRefusal {
            reason: RoutingReason::NoEligibleEmployee,
            origin: MessageOrigin::System,
        }))
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL on localhost:55432"]
async fn held_inbox_routing_commit_is_bounded_and_recovers_after_release() {
    let url = std::env::var("ORTAK_TEST_DATABASE_URL").expect("explicit disposable database URL");
    let options: sqlx::postgres::PgConnectOptions = url.parse().expect("database URL");
    assert!(matches!(options.get_host(), "localhost" | "127.0.0.1") && options.get_port() == 55432);
    // Setup and the deliberately blocking connection are independent of the
    // production pool's timeouts. No existing database is reset or removed.
    let setup = PgPool::connect(&url).await.expect("disposable database");
    buzz_db::migration::run_migrations(&setup)
        .await
        .expect("migration52");
    let pool = connect_worker_database(&url)
        .await
        .expect("production worker pool");
    for (setting, expected) in [
        ("statement_timeout", "5s"),
        ("lock_timeout", "500ms"),
        ("idle_in_transaction_session_timeout", "10s"),
    ] {
        let actual: String = sqlx::query_scalar("SELECT current_setting($1)")
            .bind(setting)
            .fetch_one(&pool)
            .await
            .expect("live session setting");
        assert_eq!(actual, expected);
    }
    let community = Uuid::new_v4();
    let company = Uuid::new_v4();
    sqlx::query("INSERT INTO communities(id,host) VALUES ($1,$2)")
        .bind(community)
        .bind(format!("worker-db-{}.example", community.simple()))
        .execute(&setup)
        .await
        .expect("community");
    sqlx::query("INSERT INTO companies(id,slug,display_name) VALUES ($1,$2,'Worker lock test')")
        .bind(company)
        .bind(format!("worker-db-{}", company.simple()))
        .execute(&setup)
        .await
        .expect("company");
    sqlx::query("INSERT INTO office_company_bindings(company_id,community_id) VALUES ($1,$2)")
        .bind(company)
        .bind(community)
        .execute(&setup)
        .await
        .expect("Office scope");
    let control = PgControlPlane::new(pool.clone());
    let scope = control
        .resolve_company_for_community(community)
        .await
        .expect("scope");
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    let message = MessageId::from_bytes(bytes);
    control
        .insert_accepted_event(
            community,
            &InboxEvent {
                event_id: message,
                event_created_at: Utc::now(),
                event_kind: 9,
                author_pubkey: [7; 32],
                channel_id: Some(Uuid::new_v4()),
            },
        )
        .await
        .expect("accepted inbox");
    let claim = control
        .claim_message(
            &scope,
            message,
            "bounded-worker",
            Duration::from_secs(60),
            5,
        )
        .await
        .expect("claim")
        .expect("new inbox");
    let mut blocker = setup.begin().await.expect("held transaction");
    sqlx::query("SELECT event_id FROM office_inbox WHERE company_id=$1 AND event_id=$2 FOR UPDATE")
        .bind(company)
        .bind(message.as_bytes().as_slice())
        .fetch_one(&mut *blocker)
        .await
        .expect("hold canonical inbox row");
    let router = InboxRoutingService::new(
        control.clone(),
        RefusalNormalizer,
        DisabledSemanticScorer::new(),
        RoutingWorkerConfig::default(),
    );
    let result = tokio::time::timeout(Duration::from_secs(3), router.route_claim(&scope, &claim))
        .await
        .expect("production DB timeout must beat the test deadline");
    let error = result.expect_err("held routing commit must refuse promptly");
    let ControlError::Database(error) = error else {
        panic!("expected database lock timeout")
    };
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("55P03")
    );
    let inbox = control
        .inbox_row(&scope, message)
        .await
        .expect("inbox read")
        .expect("durable inbox");
    assert_eq!(inbox.state, InboxState::Claimed);
    let decisions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM routing_decisions WHERE company_id=$1")
            .bind(company)
            .fetch_one(&pool)
            .await
            .expect("no partial decision");
    assert_eq!(decisions, 0);
    blocker.rollback().await.expect("release held row");
    assert!(matches!(
        router
            .route_claim(&scope, &claim)
            .await
            .expect("retry same durable claim"),
        ServiceOutcome::Committed(_)
    ));
    let inbox = control
        .inbox_row(&scope, message)
        .await
        .expect("inbox read")
        .expect("durable decision");
    assert_eq!(inbox.state, InboxState::Decided);
    pool.close().await;
    setup.close().await;
}
