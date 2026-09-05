//! Production worker delivery deadlines must preserve retries and stop progress.
use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};

use ortak_domain::CredentialRef;
use ortak_office::event::FrozenSignedEvent;
use ortak_office::fakes::{FakeOfficePublisher, FakeOfficeSigner};
use ortak_office::publisher::{OfficePublishError, OfficePublisher, PublishReceipt};
use ortak_office::{DeliveryConfig, OfficeDeliveryService};
use ortak_runtime::cancellation::{CancellationReason, RuntimeCancellationRepository};
use ortak_runtime::office_delivery::deliver_one_office_output;
use ortak_runtime::office_output::schedule_office_outputs;
use ortak_runtime::reconciliation::reconcile_runtime;

const SIGNER: &str = "credential://office/delivery-contention";

async fn output_fixture() -> (Fixture, FakeOfficeSigner, Uuid, Uuid, Uuid) {
    let signer = FakeOfficeSigner::new().with_generated_signer(SIGNER);
    let mut employee = fixture_employee();
    employee.office.public_key = signer.public_key(SIGNER).expect("fresh signer").to_hex();
    employee.office.signer_ref = CredentialRef::parse(SIGNER).expect("opaque signer ref");
    let fixture = Fixture::new_for_employee(employee).await;
    let (run, reference, _) = fixture.started().await;
    super::office_output::complete(
        &fixture,
        run,
        &reference,
        DeliveryIntentKind::Reply,
        vec![BoundedText::raw("A bounded, actually signed answer.")],
    )
    .await;
    let scheduled = schedule_office_outputs(&fixture.control, &fixture.scope, 1)
        .await
        .expect("schedule output");
    assert_eq!(scheduled.enqueued, 1);
    let outbox: Uuid = sqlx::query_scalar(
        "SELECT id FROM outbox WHERE company_id=$1 AND run_id=$2 AND kind='office_publish'",
    )
    .bind(fixture.scope.company_id())
    .bind(run)
    .fetch_one(&fixture.pool)
    .await
    .expect("delivery row");
    let (stop, _, _) = fixture.started().await;
    fixture
        .control
        .enqueue_cancellation(&fixture.scope, stop, CancellationReason::HumanRequested)
        .await
        .expect("durable cancellation waiting after delivery");
    (fixture, signer, run, outbox, stop)
}

async fn cancellation_progresses(fixture: &Fixture, run: Uuid) {
    let report = tokio::time::timeout(
        Duration::from_secs(3),
        reconcile_runtime(
            &fixture.control,
            &fixture.adapter,
            &fixture.scope,
            &fixture.config(),
            64,
        ),
    )
    .await
    .expect("delivery must release control to cancellation")
    .expect("cancellation pass");
    assert_eq!(report.stop_attempts, 1);
    assert_eq!(fixture.run(run).await.status, "cancelled");
}

async fn frozen_bytes(fixture: &Fixture, outbox: Uuid) -> Vec<u8> {
    sqlx::query_scalar("SELECT signed_event_bytes FROM outbox WHERE company_id=$1 AND id=$2")
        .bind(fixture.scope.company_id())
        .bind(outbox)
        .fetch_one(&fixture.pool)
        .await
        .expect("frozen event bytes")
}

struct ActivePublish<'a>(&'a AtomicUsize);
impl Drop for ActivePublish<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct SlowPublisher {
    slow: AtomicBool,
    active: AtomicUsize,
    attempts: Mutex<Vec<Vec<u8>>>,
    office: FakeOfficePublisher,
}

impl OfficePublisher for SlowPublisher {
    async fn publish(
        &self,
        scope: &CompanyScope,
        event: &FrozenSignedEvent,
    ) -> Result<PublishReceipt, OfficePublishError> {
        self.attempts
            .lock()
            .expect("attempt lock")
            .push(event.signed_bytes().to_vec());
        self.active.fetch_add(1, Ordering::SeqCst);
        let _active = ActivePublish(&self.active);
        if self.slow.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        self.office.publish(scope, event).await
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL; exercises actual eight-second delivery deadline"]
async fn slow_office_publisher_times_out_without_losing_retry_or_following_cancellation() {
    let (fixture, signer, _, outbox, stop) = output_fixture().await;
    let publisher = SlowPublisher {
        slow: AtomicBool::new(true),
        active: AtomicUsize::new(0),
        attempts: Mutex::new(Vec::new()),
        office: FakeOfficePublisher::new(),
    };
    let delivery = OfficeDeliveryService::new(
        fixture.control.clone(),
        &signer,
        &publisher,
        DeliveryConfig::default(),
    );
    let attempted = tokio::time::timeout(
        Duration::from_secs(11),
        deliver_one_office_output(
            &fixture.control,
            &fixture.scope,
            "slow-publisher",
            &delivery,
        ),
    )
    .await
    .expect("production eight-second timeout must finish before test deadline")
    .expect("timeout must be durably retryable");
    assert!(attempted);
    assert_eq!(
        publisher.attempts.lock().expect("attempts").len(),
        1,
        "actual publisher was reached"
    );
    assert_eq!(
        publisher.active.load(Ordering::SeqCst),
        0,
        "timed-out publish future was dropped"
    );
    let row = fixture.outbox(outbox).await;
    assert_eq!(row.state, "pending");
    assert_eq!(row.attempt_count, 1);
    assert!(row.lease_token.is_none());
    assert_eq!(row.last_error.as_deref(), Some("office_delivery_timeout"));
    let before = frozen_bytes(&fixture, outbox).await;
    assert_eq!(signer.sign_calls(), 1);
    cancellation_progresses(&fixture, stop).await;

    publisher.slow.store(false, Ordering::SeqCst);
    sqlx::query("UPDATE outbox SET retry_after=clock_timestamp() WHERE company_id=$1 AND id=$2")
        .bind(fixture.scope.company_id())
        .bind(outbox)
        .execute(&fixture.pool)
        .await
        .expect("make retry due");
    assert!(deliver_one_office_output(
        &fixture.control,
        &fixture.scope,
        "retry-publisher",
        &delivery
    )
    .await
    .expect("retry frozen event"));
    assert_eq!(fixture.outbox(outbox).await.state, "delivered");
    assert_eq!(signer.sign_calls(), 1, "retry never resigns");
    assert_eq!(before, frozen_bytes(&fixture, outbox).await);
    let attempts = publisher.attempts.lock().expect("attempts");
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0], attempts[1]);
}

struct GatedAcknowledgement {
    gate_once: AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    office: FakeOfficePublisher,
}

impl OfficePublisher for GatedAcknowledgement {
    async fn publish(
        &self,
        scope: &CompanyScope,
        event: &FrozenSignedEvent,
    ) -> Result<PublishReceipt, OfficePublishError> {
        let receipt = self.office.publish(scope, event).await?;
        if self.gate_once.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        Ok(receipt)
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL"]
async fn unchanged_locked_delivery_row_bounds_acknowledgement_and_preserves_frozen_retry() {
    let (fixture, signer, _, outbox, stop) = output_fixture().await;
    let publisher = GatedAcknowledgement {
        gate_once: AtomicBool::new(true),
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        office: FakeOfficePublisher::new(),
    };
    let delivery = OfficeDeliveryService::new(
        fixture.control.clone(),
        &signer,
        &publisher,
        DeliveryConfig::default(),
    );
    let mut attempt = Box::pin(deliver_one_office_output(
        &fixture.control,
        &fixture.scope,
        "locked-ack",
        &delivery,
    ));
    tokio::select! {
        result = &mut attempt => panic!("publisher must reach acknowledgement gate first: {result:?}"),
        _ = tokio::time::sleep(Duration::from_secs(3)) => panic!("publisher did not reach gate"),
        _ = publisher.entered.notified() => {}
    }
    let mut blocker = fixture.pool.begin().await.expect("outbox blocker");
    sqlx::query("SELECT id FROM outbox WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(fixture.scope.company_id())
        .bind(outbox)
        .fetch_one(&mut *blocker)
        .await
        .expect("hold unchanged outbox row");
    publisher.release.notify_one();
    let result = tokio::time::timeout(Duration::from_secs(4), &mut attempt)
        .await
        .expect("DB lock deadlines must precede the outer delivery timeout");
    assert!(result.is_err(), "failure to persist retry must propagate");
    let row = fixture.outbox(outbox).await;
    assert_eq!(row.state, "pending");
    assert_eq!(row.attempt_count, 1);
    assert!(
        row.lease_token.is_some(),
        "durable lease remains reclaimable after expiry"
    );
    let before = frozen_bytes(&fixture, outbox).await;
    assert_eq!(
        publisher.office.published().len(),
        1,
        "Office already accepted exactly one event"
    );
    cancellation_progresses(&fixture, stop).await;
    blocker
        .rollback()
        .await
        .expect("release unchanged outbox row");
    sqlx::query("UPDATE outbox SET lease_expires_at=clock_timestamp()-interval '1 second',retry_after=clock_timestamp() WHERE company_id=$1 AND id=$2")
        .bind(fixture.scope.company_id()).bind(outbox).execute(&fixture.pool).await.expect("simulate expired lease recovery");
    assert!(deliver_one_office_output(
        &fixture.control,
        &fixture.scope,
        "recovered-ack",
        &delivery
    )
    .await
    .expect("retry previously accepted event"));
    assert_eq!(fixture.outbox(outbox).await.state, "delivered");
    assert_eq!(before, frozen_bytes(&fixture, outbox).await);
    assert_eq!(signer.sign_calls(), 1);
    assert_eq!(publisher.office.publish_calls(), 2);
    assert_eq!(
        publisher.office.published().len(),
        1,
        "retry was AlreadyPresent"
    );
}
