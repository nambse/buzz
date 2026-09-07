//! Canonical capture and finite replay through the production transaction seams.

use super::*;
use chrono::{DateTime, Duration as ChronoDuration};
use ortak_control::cohort::RoutingCohort;
use ortak_control::postgres::insert_selected_accepted_event_on;

struct CaptureFixture {
    pool: PgPool,
    control: PgControlPlane,
    scope: CompanyScope,
    community: Uuid,
    channel: Uuid,
    other_channel: Uuid,
}

impl CaptureFixture {
    async fn new() -> Self {
        std::env::var("ORTAK_TEST_DATABASE_URL").expect("explicit disposable database required");
        let pool = setup_pool().await;
        let control = PgControlPlane::new(pool.clone());
        let (community, company) = create_company(&pool, &RoutingPolicy::default()).await;
        let mut channels = Vec::new();
        for label in ["selected", "outside"] {
            channels.push(sqlx::query_scalar::<_, Uuid>(
                "INSERT INTO channels(community_id,name,created_by) VALUES ($1,$2,$3) RETURNING id",
            ).bind(community).bind(label).bind([7u8;32].as_slice())
                .fetch_one(&pool).await.expect("channel"));
        }
        sqlx::query("INSERT INTO employees(company_id,id,status) VALUES ($1,'cem','draft')")
            .bind(company)
            .execute(&pool)
            .await
            .expect("draft employee");
        let scope = control
            .resolve_company_for_community(community)
            .await
            .expect("scope");
        Self {
            pool,
            control,
            scope,
            community,
            channel: channels[0],
            other_channel: channels[1],
        }
    }

    async fn capture(&self) -> RoutingCohort {
        self.control
            .begin_routing_capture(&self.scope, &[self.channel], &[employee_id("cem")])
            .await
            .expect("begin capture")
    }

    fn event(&self, channel: Uuid, at: DateTime<Utc>, kind: i32) -> InboxEvent {
        InboxEvent {
            event_id: message_id(),
            event_created_at: at,
            event_kind: kind,
            author_pubkey: [7; 32],
            channel_id: Some(channel),
        }
    }

    async fn store(&self, event: &InboxEvent, capture: bool) -> Option<InboxInsertOutcome> {
        let mut tx = self.pool.begin().await.expect("event transaction");
        sqlx::query(
            "INSERT INTO events(community_id,id,pubkey,created_at,kind,tags,content,sig,channel_id)
                     VALUES ($1,$2,$3,$4,$5,'[]','historical fixture',$6,$7)",
        )
        .bind(self.community)
        .bind(event.event_id.as_bytes().as_slice())
        .bind(event.author_pubkey.as_slice())
        .bind(event.event_created_at)
        .bind(event.event_kind)
        .bind([9u8; 64].as_slice())
        .bind(event.channel_id)
        .execute(&mut *tx)
        .await
        .expect("event");
        let result = if capture {
            Some(
                insert_selected_accepted_event_on(&mut tx, self.community, event)
                    .await
                    .expect("capture"),
            )
        } else {
            None
        };
        tx.commit().await.expect("atomic event commit");
        result
    }

    async fn count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM office_inbox WHERE company_id=$1")
            .bind(self.scope.company_id())
            .fetch_one(&self.pool)
            .await
            .expect("inbox count")
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn capture_is_default_off_and_bounded_replay_covers_future_and_late_backdated_events() {
    let f = CaptureFixture::new().await;
    let now = Utc::now();
    let old = f.event(f.channel, now - ChronoDuration::hours(2), 9);
    assert_eq!(
        f.store(&old, true).await,
        Some(InboxInsertOutcome::OutsideCohort)
    );
    let duplicate = f.event(f.channel, now, 40002);
    f.store(&duplicate, false).await;
    f.control
        .insert_accepted_event(f.community, &duplicate)
        .await
        .expect("existing inbox");
    let future = f.event(f.channel, now + ChronoDuration::days(1), 9);
    f.store(&future, false).await;
    f.store(&f.event(f.other_channel, now, 9), false).await;
    f.store(&f.event(f.channel, now, 1), false).await;
    let capture = f.capture().await;
    assert!(f
        .control
        .enable_routing_cohort(&f.scope, capture.capture_id)
        .await
        .is_err());
    assert!(f
        .control
        .claim_next(&f.scope, "capture-worker", Duration::from_secs(30), 5)
        .await
        .expect("claim")
        .is_none());
    let start = f
        .control
        .start_inbox_reconciliation(&f.scope, capture.capture_id, f.channel)
        .await
        .expect("pin");
    assert_eq!(
        (start.scanned, start.inserted, start.completed),
        (0, 0, false)
    );
    for limit in [0, 257] {
        assert!(f
            .control
            .reconcile_inbox_batch(&f.scope, capture.capture_id, f.channel, limit)
            .await
            .is_err());
    }
    let first = f
        .control
        .reconcile_inbox_batch(&f.scope, capture.capture_id, f.channel, 1)
        .await
        .expect("first page");
    assert_eq!(
        (first.scanned, first.inserted, first.completed),
        (1, 1, false)
    );
    // Accepted after the cursor passed this key: only the atomic ingress hook
    // can protect it. A wall-clock or received_at cutoff would lose it.
    let late = f.event(f.channel, now - ChronoDuration::days(1), 9);
    assert_eq!(
        f.store(&late, true).await,
        Some(InboxInsertOutcome::Inserted)
    );
    let outside = f.event(f.other_channel, now, 9);
    assert_eq!(
        f.store(&outside, true).await,
        Some(InboxInsertOutcome::OutsideCohort)
    );
    let restarted = PgControlPlane::new(f.pool.clone());
    assert_eq!(
        restarted
            .start_inbox_reconciliation(&f.scope, capture.capture_id, f.channel)
            .await
            .expect("resume pin"),
        first
    );
    let final_page = restarted
        .reconcile_inbox_batch(&f.scope, capture.capture_id, f.channel, 2)
        .await
        .expect("resume page");
    assert_eq!(
        (
            final_page.scanned,
            final_page.inserted,
            final_page.completed
        ),
        (3, 2, true)
    );
    assert_eq!(
        restarted
            .reconcile_inbox_batch(&f.scope, capture.capture_id, f.channel, 1)
            .await
            .expect("retry completed"),
        final_page
    );
    assert_eq!(f.count().await, 4);
    restarted
        .enable_routing_cohort(&f.scope, capture.capture_id)
        .await
        .expect("enable completed capture");
    assert!(f
        .control
        .claim_next(&f.scope, "enabled-worker", Duration::from_secs(30), 5)
        .await
        .expect("claim")
        .is_some());
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn reconciliation_evidence_cannot_be_forged_replaced_deleted_or_reused_for_new_selection() {
    let f = CaptureFixture::new().await;
    let event = f.event(f.channel, Utc::now(), 9);
    f.store(&event, false).await;
    let capture = f.capture().await;
    f.control
        .start_inbox_reconciliation(&f.scope, capture.capture_id, f.channel)
        .await
        .expect("pin");
    for sql in [
        "UPDATE office_inbox_reconciliations SET completed_at=clock_timestamp() WHERE company_id=$1",
        "UPDATE office_inbox_reconciliations SET upper_created_at=upper_created_at+interval '1 day' WHERE company_id=$1",
        "UPDATE office_inbox_reconciliations SET cursor_created_at=upper_created_at,cursor_event_id=upper_event_id,scanned=1,inserted=1,completed_at=clock_timestamp() WHERE company_id=$1",
    ] {
        assert!(sqlx::query(sql).bind(f.scope.company_id()).execute(&f.pool).await.is_err(),"{sql}");
    }
    assert_eq!(f.count().await, 0);
    let completed = f
        .control
        .reconcile_inbox_batch(&f.scope, capture.capture_id, f.channel, 1)
        .await
        .expect("real page");
    assert!(completed.completed);
    for sql in [
        "UPDATE office_inbox_reconciliations SET inserted=0 WHERE company_id=$1",
        "UPDATE office_inbox_reconciliations SET completed_at=NULL WHERE company_id=$1",
        "DELETE FROM office_inbox_reconciliations WHERE company_id=$1",
    ] {
        assert!(sqlx::query(sql)
            .bind(f.scope.company_id())
            .execute(&f.pool)
            .await
            .is_err());
    }
    f.control
        .enable_routing_cohort(&f.scope, capture.capture_id)
        .await
        .expect("enable");
    sqlx::query("DELETE FROM office_routing_employees WHERE company_id=$1")
        .bind(f.scope.company_id())
        .execute(&f.pool)
        .await
        .expect("explicit cohort removal");
    let changed = f
        .control
        .routing_cohort(&f.scope)
        .await
        .expect("cohort")
        .expect("exists");
    assert_eq!(changed.state, "capture");
    assert_ne!(changed.capture_id, capture.capture_id);
    assert!(f
        .control
        .enable_routing_cohort(&f.scope, capture.capture_id)
        .await
        .is_err());
    assert!(f
        .control
        .reconcile_inbox_batch(&f.scope, capture.capture_id, f.channel, 1)
        .await
        .is_err());
    assert!(f
        .control
        .claim_next(&f.scope, "removed-worker", Duration::from_secs(30), 5)
        .await
        .expect("claim")
        .is_none());
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn canonical_inbox_conflict_rolls_back_replay_cursor_and_can_be_retried() {
    let f = CaptureFixture::new().await;
    let event = f.event(f.channel, Utc::now(), 9);
    f.store(&event, false).await;
    let capture = f.capture().await;
    let initial = f
        .control
        .start_inbox_reconciliation(&f.scope, capture.capture_id, f.channel)
        .await
        .expect("pin");
    let mut wrong = event.clone();
    wrong.event_kind = 40002;
    f.control
        .insert_accepted_event(f.community, &wrong)
        .await
        .expect("legacy malformed fixture");
    assert!(f
        .control
        .reconcile_inbox_batch(&f.scope, capture.capture_id, f.channel, 1)
        .await
        .is_err());
    assert_eq!(
        f.control
            .start_inbox_reconciliation(&f.scope, capture.capture_id, f.channel)
            .await
            .expect("same cursor"),
        initial
    );
    sqlx::query("UPDATE office_inbox SET event_kind=$2 WHERE company_id=$1 AND event_id=$3")
        .bind(f.scope.company_id())
        .bind(event.event_kind)
        .bind(event.event_id.as_bytes().as_slice())
        .execute(&f.pool)
        .await
        .expect("repair disposable malformed fixture");
    let repaired = f
        .control
        .reconcile_inbox_batch(&f.scope, capture.capture_id, f.channel, 1)
        .await
        .expect("retry");
    assert_eq!(
        (repaired.scanned, repaired.inserted, repaired.completed),
        (1, 0, true)
    );
}
