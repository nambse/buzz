//! The real inbox service must own deadlines and stale-claim/revision fences.
use super::*;
use std::sync::Mutex;

use ortak_control::inbox::InboxClaim;
use ortak_control::DisabledSemanticScorer;

struct ActiveCall(Arc<AtomicU32>);
impl Drop for ActiveCall {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
struct HangingScorer {
    calls: Arc<AtomicU32>,
    active: Arc<AtomicU32>,
}
fn metadata() -> ScorerMetadata {
    ScorerMetadata {
        adapter: "semantic-contract-fixture".to_owned(),
        model: Some("fixture-model-snapshot".to_owned()),
        prompt_version: Some("fixture-prompt-v1".to_owned()),
        version: "fixture-scorer-v1".to_owned(),
        latency_ms: None,
        usage: None,
    }
}
impl SemanticScorer for HangingScorer {
    fn metadata(&self) -> ScorerMetadata {
        metadata()
    }
    async fn score(
        &self,
        _: &SemanticScoringInput,
        _budget: ortak_control::ScoringBudget,
    ) -> ScoringOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.active.fetch_add(1, Ordering::SeqCst);
        let _held = ActiveCall(self.active.clone());
        std::future::pending().await
    }
}
fn normalizer(id: MessageId) -> FakeNormalizer {
    FakeNormalizer {
        messages: HashMap::from([(
            id,
            NormalizedMessage {
                envelope: MessageEnvelope::human_channel(
                    id.to_hex(),
                    "sefa",
                    "office",
                    "A general question for the team",
                ),
                root_message_id: id,
                eligible_employee_ids: [employee_id("cem"), employee_id("zeynep")]
                    .into_iter()
                    .collect(),
            },
        )]),
    }
}
fn scores(input: &SemanticScoringInput) -> ScoringOutcome {
    ScoringOutcome {
        result: Ok(input
            .request()
            .candidates()
            .iter()
            .map(|candidate| SemanticScore {
                employee_id: candidate.employee_id().clone(),
                score: 0.95,
                evidence: vec![EvidenceLabel::parse("role_match").expect("label")],
            })
            .collect()),
        metadata: metadata(),
    }
}
async fn no_dispatch(fixture: &Fixture) {
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM outbox WHERE company_id=$1 AND $2::bytea IS NULL",
                None
            )
            .await,
        0
    );
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM delivery_chain_visits WHERE company_id=$1 AND $2::bytea IS NULL",
                None
            )
            .await,
        0
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn timed_out_scorer_is_dropped_and_commits_one_silent_decision_with_configured_pins() {
    let fixture = Fixture::new(
        RoutingPolicy::default(),
        vec![fixture_employee("cem"), fixture_employee("zeynep")],
    )
    .await;
    let id = message_id();
    fixture.accept(id).await;
    let calls = Arc::new(AtomicU32::new(0));
    let active = Arc::new(AtomicU32::new(0));
    let service = InboxRoutingService::new(
        fixture.control.clone(),
        normalizer(id),
        HangingScorer {
            calls: calls.clone(),
            active: active.clone(),
        },
        RoutingWorkerConfig {
            semantic_timeout: Duration::from_millis(25),
            ..RoutingWorkerConfig::default()
        },
    );
    let outcome = tokio::time::timeout(
        Duration::from_secs(3),
        service.claim_and_route(&fixture.scope),
    )
    .await
    .expect("service deadline must bound a pending scorer")
    .expect("route")
    .expect("claimed");
    let ServiceOutcome::Committed(decision) = outcome else {
        panic!("expected silent decision")
    };
    assert_eq!(decision.mode, RoutingMode::Silent);
    assert_eq!(decision.wake_count, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        active.load(Ordering::SeqCst),
        0,
        "no scorer future survives timeout"
    );
    let row = sqlx::query("SELECT summary_reason,scorer_adapter,scorer_model,scorer_prompt_version,scorer_version FROM routing_decisions WHERE company_id=$1 AND message_id=$2")
        .bind(fixture.scope.company_id()).bind(id.as_bytes().as_slice()).fetch_one(&fixture.pool).await.expect("decision");
    assert_eq!(
        row.get::<String, _>("summary_reason"),
        "semantic_scorer_timed_out"
    );
    assert_eq!(
        row.get::<String, _>("scorer_adapter"),
        "semantic-contract-fixture"
    );
    assert_eq!(
        row.get::<String, _>("scorer_model"),
        "fixture-model-snapshot"
    );
    assert_eq!(
        row.get::<String, _>("scorer_prompt_version"),
        "fixture-prompt-v1"
    );
    assert_eq!(row.get::<String, _>("scorer_version"), "fixture-scorer-v1");
    assert!(service
        .claim_and_route(&fixture.scope)
        .await
        .expect("retry")
        .is_none());
    no_dispatch(&fixture).await;
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM routing_decisions WHERE company_id=$1 AND $2::bytea IS NULL",
                None
            )
            .await,
        1
    );
}

struct BudgetRevisionScorer {
    pool: PgPool,
    company: Uuid,
    calls: AtomicU32,
    inputs: Arc<Mutex<Vec<SemanticScoringInput>>>,
}
impl SemanticScorer for BudgetRevisionScorer {
    fn metadata(&self) -> ScorerMetadata {
        metadata()
    }
    async fn score(
        &self,
        input: &SemanticScoringInput,
        _budget: ortak_control::ScoringBudget,
    ) -> ScoringOutcome {
        assert_eq!(input.company_id(), self.company);
        self.inputs.lock().expect("inputs").push(input.clone());
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            activate_revision(&self.pool, self.company, &fixture_employee("zeynep")).await;
        }
        // Each response fits the configured one-second budget independently.
        // Only retaining the first deadline makes the second response too late.
        tokio::time::sleep(Duration::from_millis(650)).await;
        scores(input)
    }
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn revision_refresh_reuses_total_budget_and_pins_the_fresh_candidate_set() {
    let fixture = Fixture::new(
        RoutingPolicy::default(),
        vec![fixture_employee("cem"), fixture_employee("zeynep")],
    )
    .await;
    let id = message_id();
    fixture.accept(id).await;
    let inputs = Arc::new(Mutex::new(Vec::new()));
    let service = InboxRoutingService::new(
        fixture.control.clone(),
        normalizer(id),
        BudgetRevisionScorer {
            pool: fixture.pool.clone(),
            company: fixture.scope.company_id(),
            calls: AtomicU32::new(0),
            inputs: inputs.clone(),
        },
        RoutingWorkerConfig {
            semantic_timeout: Duration::from_secs(1),
            ..RoutingWorkerConfig::default()
        },
    );
    let outcome = service
        .claim_and_route(&fixture.scope)
        .await
        .expect("route")
        .expect("claim");
    let ServiceOutcome::Committed(decision) = outcome else {
        panic!("expected committed timeout")
    };
    assert_eq!(
        decision.mode,
        RoutingMode::Silent,
        "resetting the deadline would accept the second high score"
    );
    assert_eq!(decision.wake_count, 0);
    let inputs = inputs.lock().expect("captured").clone();
    assert_eq!(
        inputs.len(),
        2,
        "the first response invalidates the pinned revision"
    );
    assert_ne!(inputs[0].input_hash(), inputs[1].input_hash());
    assert_ne!(inputs[0].candidates(), inputs[1].candidates());
    let stored = fixture
        .control
        .decision_for_message(&fixture.scope, id)
        .await
        .expect("read")
        .expect("decision");
    assert_eq!(stored.summary_reason, RoutingReason::SemanticScorerTimedOut);
    for candidate in inputs[1].candidates() {
        assert!(stored
            .candidate_revision_ids
            .contains(&candidate.revision_id));
    }
    no_dispatch(&fixture).await;
}

struct ReclaimingScorer {
    control: PgControlPlane,
    pool: PgPool,
    scope: CompanyScope,
    replacement: Arc<Mutex<Option<InboxClaim>>>,
}
impl SemanticScorer for ReclaimingScorer {
    fn metadata(&self) -> ScorerMetadata {
        metadata()
    }
    async fn score(
        &self,
        input: &SemanticScoringInput,
        _budget: ortak_control::ScoringBudget,
    ) -> ScoringOutcome {
        sqlx::query("UPDATE office_inbox SET claim_expires_at=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND event_id=$2")
            .bind(self.scope.company_id()).bind(input.message_id().as_bytes().as_slice())
            .execute(&self.pool).await.expect("expire held claim");
        let claim = self
            .control
            .claim_message(
                &self.scope,
                input.message_id(),
                "replacement",
                Duration::from_secs(60),
                5,
            )
            .await
            .expect("reclaim")
            .expect("expired claim is reclaimable");
        *self.replacement.lock().expect("replacement") = Some(claim);
        scores(input)
    }
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn scoring_result_from_reclaimed_inbox_cannot_dispatch_or_finalize_the_new_claim() {
    let fixture = Fixture::new(
        RoutingPolicy::default(),
        vec![fixture_employee("cem"), fixture_employee("zeynep")],
    )
    .await;
    let id = message_id();
    fixture.accept(id).await;
    let replacement = Arc::new(Mutex::new(None));
    let service = InboxRoutingService::new(
        fixture.control.clone(),
        normalizer(id),
        ReclaimingScorer {
            control: fixture.control.clone(),
            pool: fixture.pool.clone(),
            scope: fixture.scope.clone(),
            replacement: replacement.clone(),
        },
        RoutingWorkerConfig::default(),
    );
    assert_eq!(
        service
            .claim_and_route(&fixture.scope)
            .await
            .expect("route"),
        Some(ServiceOutcome::StaleClaim)
    );
    assert!(fixture
        .control
        .decision_for_message(&fixture.scope, id)
        .await
        .expect("read")
        .is_none());
    no_dispatch(&fixture).await;
    let claim = replacement
        .lock()
        .expect("replacement")
        .clone()
        .expect("new claim");
    let current = fixture
        .control
        .inbox_row(&fixture.scope, id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(current.state, InboxState::Claimed);
    assert_eq!(current.claim_generation, claim.claim_generation);
    let service = InboxRoutingService::new(
        fixture.control.clone(),
        normalizer(id),
        DisabledSemanticScorer::new(),
        RoutingWorkerConfig::default(),
    );
    assert!(matches!(
        service
            .route_claim(&fixture.scope, &claim)
            .await
            .expect("new worker route"),
        ServiceOutcome::Committed(_)
    ));
    no_dispatch(&fixture).await;
}
