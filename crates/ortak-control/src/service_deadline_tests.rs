use super::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::routing::ScorerMetadata;
use crate::semantic::tests::Fixture;

struct PendingScorer {
    calls: AtomicU32,
    drops: Arc<AtomicU32>,
}
struct PendingCall(Arc<AtomicU32>);
impl Drop for PendingCall {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl SemanticScorer for PendingScorer {
    fn metadata(&self) -> ScorerMetadata {
        ScorerMetadata {
            adapter: "deadline-fixture".to_owned(),
            model: Some("pinned-model".to_owned()),
            prompt_version: Some("pinned-prompt".to_owned()),
            version: "pinned-scorer".to_owned(),
            latency_ms: None,
            usage: None,
        }
    }
    async fn score(&self, _: &SemanticScoringInput) -> ScoringOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _held = PendingCall(self.drops.clone());
        std::future::pending().await
    }
}

#[tokio::test]
async fn one_deadline_drops_a_pending_call_and_never_polls_a_later_retry() {
    let fixture = Fixture::new(MessageId::from_bytes([20; 32]), "General routing question");
    let input = fixture.input();
    let scorer = PendingScorer {
        calls: AtomicU32::new(0),
        drops: Arc::new(AtomicU32::new(0)),
    };
    let deadline = Instant::now() + Duration::from_millis(15);
    let first = score_before_deadline(&scorer, &input, deadline).await;
    assert_eq!(first.result, Err(SemanticScoringFailure::TimedOut));
    assert_eq!(scorer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        scorer.drops.load(Ordering::SeqCst),
        1,
        "timeout must drop the actual scorer future"
    );
    assert_eq!(first.metadata.model, Some("pinned-model".to_owned()));
    assert_eq!(
        first.metadata.prompt_version,
        Some("pinned-prompt".to_owned())
    );
    assert_eq!(first.metadata.version, "pinned-scorer");
    let retry = score_before_deadline(&scorer, &input, deadline).await;
    assert_eq!(retry.result, Err(SemanticScoringFailure::TimedOut));
    assert_eq!(
        scorer.calls.load(Ordering::SeqCst),
        1,
        "an expired shared budget cannot start another call"
    );
}

#[tokio::test]
async fn expired_claim_never_starts_external_scoring() {
    let mut fixture = Fixture::new(MessageId::from_bytes([21; 32]), "General routing question");
    fixture.snapshot.inbox.claim_expires_at = Some(Utc::now() - chrono::Duration::seconds(1));
    let remaining = remaining_claim_time(&fixture.claim, &fixture.snapshot);
    let scorer = PendingScorer {
        calls: AtomicU32::new(0),
        drops: Arc::new(AtomicU32::new(0)),
    };
    let outcome =
        score_before_deadline(&scorer, &fixture.input(), Instant::now() + remaining).await;
    assert_eq!(outcome.result, Err(SemanticScoringFailure::TimedOut));
    assert_eq!(scorer.calls.load(Ordering::SeqCst), 0);
}

struct BlockingScorer;
impl SemanticScorer for BlockingScorer {
    fn metadata(&self) -> ScorerMetadata {
        PendingScorer {
            calls: AtomicU32::new(0),
            drops: Arc::new(AtomicU32::new(0)),
        }
        .metadata()
    }

    async fn score(&self, input: &SemanticScoringInput) -> ScoringOutcome {
        // Intentionally block one poll: Tokio cannot preempt a misbehaving
        // adapter, so its completed result must be checked against the deadline.
        std::thread::sleep(Duration::from_millis(20));
        ScoringOutcome {
            result: Ok(input
                .request()
                .candidates()
                .iter()
                .map(|candidate| ortak_domain::SemanticScore {
                    employee_id: candidate.employee_id().clone(),
                    score: 0.99,
                    evidence: Vec::new(),
                })
                .collect()),
            metadata: self.metadata(),
        }
    }
}

#[tokio::test]
async fn a_completed_score_returned_after_blocking_one_poll_is_still_too_late() {
    let fixture = Fixture::new(MessageId::from_bytes([22; 32]), "General routing question");
    let outcome = score_before_deadline(
        &BlockingScorer,
        &fixture.input(),
        Instant::now() + Duration::from_millis(1),
    )
    .await;
    assert_eq!(
        outcome.result,
        Err(SemanticScoringFailure::TimedOut),
        "removing the post-poll deadline check would accept late high scores"
    );
    assert_eq!(outcome.metadata.model, Some("pinned-model".to_owned()));
}
