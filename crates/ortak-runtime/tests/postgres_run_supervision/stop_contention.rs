use super::*;

use std::sync::atomic::{AtomicBool, Ordering};

use ortak_control::adapter::{HealthReport, ResourceOutcome};
use ortak_control::runtime::{
    CancelOutcome, CancelStartReceipt, RunSpec, RuntimeCapabilities, RuntimeCursor,
    RuntimeEventBatch, RuntimeResourceRequest,
};
use ortak_domain::RuntimeBinding;
use ortak_runtime::cancellation::{CancellationReason, RuntimeCancellationRepository};
use ortak_runtime::reconciliation::reconcile_runtime;
use tokio::sync::Semaphore;

// All runtime behavior is the normal fake, except one already-terminal run's
// second event page waits forever. The production timeout must cancel this
// future; test time is never advanced or patched into the worker.
struct StalledTerminalReplay<'a> {
    inner: &'a FakeRuntimeAdapter,
    slow: RuntimeRunRef,
    blocked: AtomicBool,
    entered: Semaphore,
}

impl RuntimeAdapter for StalledTerminalReplay<'_> {
    fn adapter_name(&self) -> &str {
        self.inner.adapter_name()
    }
    async fn probe_capabilities(&self) -> Result<RuntimeCapabilities, RuntimeError> {
        self.inner.probe_capabilities().await
    }
    async fn health(&self, binding: &RuntimeBinding) -> Result<HealthReport, RuntimeError> {
        self.inner.health(binding).await
    }
    async fn ensure_profile(
        &self,
        request: &RuntimeResourceRequest,
    ) -> Result<ResourceOutcome, RuntimeError> {
        self.inner.ensure_profile(request).await
    }
    async fn delete_created_profile(&self, reference: &str, key: &str) -> Result<(), RuntimeError> {
        self.inner.delete_created_profile(reference, key).await
    }
    async fn start_run(&self, spec: &RunSpec) -> Result<RunStartReceipt, RuntimeError> {
        self.inner.start_run(spec).await
    }
    async fn lookup_start(&self, key: &str) -> Result<Option<RunStartReceipt>, RuntimeError> {
        self.inner.lookup_start(key).await
    }
    async fn cancel_start(
        &self,
        key: &str,
        reason: &str,
    ) -> Result<CancelStartReceipt, RuntimeError> {
        self.inner.cancel_start(key, reason).await
    }
    async fn next_events(
        &self,
        reference: &RuntimeRunRef,
        after: Option<&RuntimeCursor>,
        limit: usize,
    ) -> Result<RuntimeEventBatch, RuntimeError> {
        if reference == &self.slow && after.is_some() && self.blocked.load(Ordering::SeqCst) {
            self.entered.add_permits(1);
            std::future::pending::<()>().await;
        }
        self.inner.next_events(reference, after, limit).await
    }
    async fn cancel_run(
        &self,
        reference: &RuntimeRunRef,
        reason: &str,
    ) -> Result<CancelOutcome, RuntimeError> {
        self.inner.cancel_run(reference, reason).await
    }
}

#[tokio::test]
#[ignore = "requires explicit disposable ORTAK_TEST_DATABASE_URL; exercises the real 35s stop deadline"]
async fn stalled_terminal_replay_preserves_cursor_and_retry_while_another_stop_finishes() {
    let f = Fixture::new().await;
    let (slow, slow_ref, _) = f.started().await;
    let (fast, _, _) = f.started().await;
    f.adapter.push_event(
        &slow_ref,
        RunEventPayload::AssistantDelta {
            turn: 0,
            delta: BoundedText::raw("durable before stalled terminal replay"),
        },
    );
    f.adapter.push_event(
        &slow_ref,
        RunEventPayload::RunCompleted {
            delivery_intent: DeliveryIntentKind::Silent,
        },
    );
    for run in [slow, fast] {
        assert!(f
            .control
            .enqueue_cancellation(&f.scope, run, CancellationReason::HumanRequested)
            .await
            .expect("durable stop request"));
    }
    // Pin claim order independently of UUID or sub-microsecond insert ordering.
    sqlx::query("UPDATE runtime_cancellations SET next_attempt_at=clock_timestamp()-CASE WHEN run_id=$2 THEN interval '2 seconds' ELSE interval '1 second' END WHERE company_id=$1")
        .bind(f.scope.company_id()).bind(slow).execute(&f.pool).await.expect("slow stop due first");
    let adapter = StalledTerminalReplay {
        inner: &f.adapter,
        slow: slow_ref.clone(),
        blocked: AtomicBool::new(true),
        entered: Semaphore::new(0),
    };
    let config = SupervisorConfig {
        event_batch_limit: 2,
        ..f.config()
    };
    let observe = async {
        tokio::time::timeout(Duration::from_secs(5), adapter.entered.acquire())
            .await
            .expect("first page must commit before replay stalls")
            .expect("observer semaphore")
            .forget();
        let state = f
            .control
            .run_cursor_state(&f.scope, slow)
            .await
            .expect("cursor read")
            .expect("run");
        let cursor = state
            .last_cursor
            .expect("first page cursor is already durable");
        let expected = f
            .adapter
            .next_events(&slow_ref, None, 2)
            .await
            .expect("canonical first page");
        assert_eq!(
            Some(&cursor),
            expected.events.last().map(|event| &event.cursor)
        );
        assert_eq!(
            f.events(slow)
                .await
                .iter()
                .filter(|event| event.1 == "assistant.delta")
                .count(),
            1
        );
        let waiting = sqlx::query("SELECT attempt_count,lease_token FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2")
            .bind(f.scope.company_id()).bind(fast).fetch_one(&f.pool).await.expect("unrelated stop not preleased");
        assert_eq!(waiting.get::<i32, _>("attempt_count"), 0);
        assert!(waiting.get::<Option<Uuid>, _>("lease_token").is_none());
    };
    let reconcile = tokio::time::timeout(
        Duration::from_secs(45),
        reconcile_runtime(&f.control, &adapter, &f.scope, &config, 2),
    );
    let (result, ()) = tokio::join!(reconcile, observe);
    let report = result
        .expect("production 35s timeout must release the stalled stop")
        .expect("timeout recorded and unrelated stop serviced");
    assert_eq!(report.stop_attempts, 2);
    let pending = sqlx::query("SELECT state,attempt_count,lease_token,last_error_code,next_attempt_at>requested_at AS deferred FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2")
        .bind(f.scope.company_id()).bind(slow).fetch_one(&f.pool).await.expect("durable bounded retry");
    assert_eq!(pending.get::<String, _>("state"), "pending");
    assert_eq!(pending.get::<i32, _>("attempt_count"), 1);
    assert!(pending.get::<Option<Uuid>, _>("lease_token").is_none());
    assert_eq!(
        pending.get::<String, _>("last_error_code"),
        "runtime_stop_timeout"
    );
    assert!(pending.get::<bool, _>("deferred"));
    assert_eq!(f.run(slow).await.status, "running");
    assert_eq!(f.run(fast).await.status, "cancelled");
    let fast_state: String = sqlx::query_scalar(
        "SELECT state FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company_id())
    .bind(fast)
    .fetch_one(&f.pool)
    .await
    .expect("fast stop receipt");
    assert_eq!(fast_state, "acknowledged");
    adapter.blocked.store(false, Ordering::SeqCst);
    sqlx::query("UPDATE runtime_cancellations SET next_attempt_at=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND run_id=$2")
        .bind(f.scope.company_id()).bind(slow).execute(&f.pool).await.expect("retry now due");
    assert_eq!(
        reconcile_runtime(&f.control, &adapter, &f.scope, &config, 1)
            .await
            .expect("resume from committed cursor")
            .stop_attempts,
        1
    );
    assert_eq!(f.run(slow).await.status, "completed");
    let settled = sqlx::query(
        "SELECT state,attempt_count FROM runtime_cancellations WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company_id())
    .bind(slow)
    .fetch_one(&f.pool)
    .await
    .expect("durable terminal receipt");
    assert_eq!(settled.get::<String, _>("state"), "acknowledged");
    assert_eq!(settled.get::<i32, _>("attempt_count"), 2);
    let events = f.events(slow).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| event.1 == "assistant.delta")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.1 == "run.completed")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.1 == "run.cancelled")
            .count(),
        0
    );
    assert_eq!(f.adapter.start_specs().len(), 2);
}
