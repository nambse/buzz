//! Mutations after canonical normalization must not authorize stale dispatch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ortak_control::inbox::InboxRow;
use ortak_control::ports::{InboxRepository, MessageNormalizer, Normalization};
use ortak_control::{
    CompanyScope, DisabledSemanticScorer, InboxRoutingService, RoutingWorkerConfig,
};
use ortak_office::PgChannelNormalizer;
use sqlx::PgPool;
use uuid::Uuid;

use super::committed;
use super::fixture::{EventSpec, Fixture, KIND_STREAM_MESSAGE};

struct MutatingNormalizer {
    inner: PgChannelNormalizer,
    pool: PgPool,
    company: Uuid,
    community: Uuid,
    channel: Uuid,
    key: [u8; 32],
    sql: &'static str,
    changed: AtomicBool,
}

impl MessageNormalizer for MutatingNormalizer {
    async fn normalize(
        &self,
        scope: &CompanyScope,
        inbox: &InboxRow,
    ) -> ortak_control::Result<Normalization> {
        let normalized = self.inner.normalize(scope, inbox).await?;
        if !self.changed.swap(true, Ordering::SeqCst) {
            sqlx::query(self.sql)
                .bind(self.company)
                .bind(self.community)
                .bind(self.channel)
                .bind(self.key.as_slice())
                .execute(&self.pool)
                .await?;
        }
        Ok(normalized)
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn mutation_after_normalization_cannot_reserve_a_visit_or_dispatch() {
    for (sql, mutate_author) in [
        ("UPDATE channel_members SET removed_at = now() WHERE $1::uuid IS NOT NULL AND community_id = $2 AND channel_id = $3 AND pubkey = $4", false),
        ("UPDATE channel_members SET removed_at = now() WHERE $1::uuid IS NOT NULL AND community_id = $2 AND channel_id = $3 AND pubkey = $4", true),
        ("UPDATE employee_office_bindings SET valid_until = now() WHERE company_id = $1 AND $2::uuid IS NOT NULL AND $3::uuid IS NOT NULL AND public_key = $4", false),
        ("UPDATE channels SET archived_at = now() WHERE $1::uuid IS NOT NULL AND community_id = $2 AND id = $3 AND $4::bytea IS NOT NULL", false),
    ] {
        let fixture = Fixture::new().await;
        fixture.set_visibility("private").await;
        let event = fixture.store_event(EventSpec {
            kind: KIND_STREAM_MESSAGE,
            author: fixture.human_key,
            content: "Cem, selam",
            tags: serde_json::json!([["h", fixture.channel_id.to_string()]]),
            channel_id: Some(fixture.channel_id),
            parent: None,
        }).await;
        fixture.accept(&event, KIND_STREAM_MESSAGE, fixture.human_key).await;
        let claim = fixture.control.claim_message(&fixture.scope, event.id, "fence-test", Duration::from_secs(60), 5)
            .await.expect("claim").expect("claimable");
        let normalizer = MutatingNormalizer {
            inner: PgChannelNormalizer::new(fixture.pool.clone()),
            pool: fixture.pool.clone(),
            company: fixture.company_id(), community: fixture.community_id,
            channel: fixture.channel_id,
            key: if mutate_author { fixture.human_key } else { fixture.cem_key },
            sql, changed: AtomicBool::new(false),
        };
        let service = InboxRoutingService::new(fixture.control.clone(), normalizer,
            DisabledSemanticScorer::new(), RoutingWorkerConfig::default());
        let decision = committed(service.route_claim(&fixture.scope, &claim).await.expect("fresh refusal"));
        assert_eq!(decision.wake_count, 0, "mutation: {sql}");
        assert_eq!(fixture.run_dispatch_rows(event.id).await, 0);
        assert_eq!(fixture.count("SELECT count(*) FROM delivery_chain_visits WHERE company_id = $1 AND root_message_id = $2", event.id).await, 0);
    }
}
