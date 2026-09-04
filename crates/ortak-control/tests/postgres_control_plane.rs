//! Production-seam Postgres tests for the Milestone 1 control plane.
//!
//! Run with a local database that can receive the embedded migrations:
//! `ORTAK_TEST_DATABASE_URL=postgres://... cargo test -p ortak-control -- --ignored`

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use ortak_control::inbox::{InboxEvent, InboxInsertOutcome, InboxState};
use ortak_control::outbox::{OutboxFailOutcome, OutboxKind};
use ortak_control::ports::{
    CompanyDirectory, InboxRepository, MessageNormalizer, NormalizedMessage, OutboxRepository,
    RoutingRepository, ScoringOutcome, SemanticScorer,
};
use ortak_control::routing::{
    CandidateRevision, RevalidationFailure, RosterScope, RoutingCommitOutcome, RoutingProposal,
    ScorerMetadata,
};
use ortak_control::service::routing_input_hash;
use ortak_control::{
    ClaimGeneration, CompanyScope, ControlError, InboxRoutingService, MessageId, PgControlPlane,
    RoutingWorkerConfig, ServiceOutcome,
};
use ortak_domain::{
    Employee, EmployeeId, EmployeeManifest, EmployeeStatus, EvidenceLabel, MessageEnvelope,
    MessageOrigin, RecipientAction, RecipientDecision, RoutingDecision, RoutingMode, RoutingPolicy,
    RoutingReason, SemanticScore,
};
use ortak_router::SemanticRoutingRequest;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

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
    buzz_db::migration::run_migrations(&pool)
        .await
        .expect("apply migrations");
    pool
}

fn message_id() -> MessageId {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    MessageId::from_bytes(bytes)
}

fn employee_id(value: &str) -> EmployeeId {
    EmployeeId::parse(value).expect("valid employee id")
}

fn fixture_employee(name: &str) -> Employee {
    let yaml = std::fs::read_to_string(format!(
        "{}/../../config/employees/{name}.yaml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read fixture");
    let manifest: EmployeeManifest = serde_yaml::from_str(&yaml).expect("parse fixture");
    manifest.employee
}

fn third_employee() -> Employee {
    let mut employee = fixture_employee("zeynep");
    employee.id = employee_id("ada");
    employee.name = "Ada".to_owned();
    employee.title = "Platform Lead".to_owned();
    employee.aliases = vec!["platform".to_owned()];
    employee.office.public_key = "ab".repeat(32);
    employee
}

struct Fixture {
    pool: PgPool,
    control: PgControlPlane,
    community_id: Uuid,
    scope: CompanyScope,
    revisions: HashMap<String, Uuid>,
}

impl Fixture {
    async fn new(policy: RoutingPolicy, employees: Vec<Employee>) -> Self {
        let pool = setup_pool().await;
        let control = PgControlPlane::new(pool.clone());
        let (community_id, company_id) = create_company(&pool, &policy).await;
        let mut revisions = HashMap::new();
        for employee in employees {
            let revision = activate_employee(&pool, company_id, &employee).await;
            revisions.insert(employee.id.to_string(), revision);
        }
        let scope = control
            .resolve_company_for_community(community_id)
            .await
            .expect("resolve company through the binding");
        assert_eq!(scope.company_id(), company_id);
        Self {
            pool,
            control,
            community_id,
            scope,
            revisions,
        }
    }

    fn revision(&self, employee: &str) -> Uuid {
        self.revisions[employee]
    }

    async fn accept(&self, id: MessageId) -> InboxInsertOutcome {
        self.control
            .insert_accepted_event(self.community_id, &inbox_event(id))
            .await
            .expect("insert inbox row")
    }

    async fn claim(&self, id: MessageId, worker: &str) -> ClaimGeneration {
        self.control
            .claim_message(&self.scope, id, worker, Duration::from_secs(60), 5)
            .await
            .expect("claim")
            .expect("row is claimable")
            .claim_generation
    }

    fn proposal(
        &self,
        id: MessageId,
        root: MessageId,
        generation: ClaimGeneration,
        origin: MessageOrigin,
        targets: &[&str],
        policy: &RoutingPolicy,
    ) -> RoutingProposal {
        let recipients = targets
            .iter()
            .map(|target| RecipientDecision {
                employee_id: employee_id(target),
                action: RecipientAction::Wake,
                reason: RoutingReason::StructuredDispatch,
                score: None,
                evidence: Vec::new(),
            })
            .collect::<Vec<_>>();
        let candidates = targets
            .iter()
            .filter_map(|target| {
                self.revisions
                    .get(*target)
                    .map(|revision| CandidateRevision {
                        employee_id: employee_id(target),
                        revision_id: *revision,
                    })
            })
            .collect::<Vec<_>>();
        RoutingProposal {
            company_id: self.scope.company_id(),
            message_id: id,
            root_message_id: root,
            claim_generation: generation,
            origin,
            input_hash: [9; 32],
            candidates,
            roster_scope: RosterScope::Targets,
            decision: RoutingDecision {
                message_id: id.to_hex(),
                mode: RoutingMode::Deterministic,
                summary_reason: RoutingReason::StructuredDispatch,
                policy_version: policy.version.clone(),
                policy_fingerprint: policy.fingerprint(),
                recipients,
            },
            scorer: None,
        }
    }

    async fn count(&self, sql: &'static str, root: Option<MessageId>) -> i64 {
        let query = sqlx::query(sql).bind(self.scope.company_id());
        let query = match root {
            Some(root) => query.bind(root.as_bytes().to_vec()),
            None => query.bind(Option::<Vec<u8>>::None),
        };
        query
            .fetch_one(&self.pool)
            .await
            .expect("count")
            .try_get::<i64, _>(0)
            .expect("count column")
    }
}

fn inbox_event(id: MessageId) -> InboxEvent {
    InboxEvent {
        event_id: id,
        event_created_at: Utc::now(),
        event_kind: 1,
        author_pubkey: [7; 32],
        channel_id: Some(Uuid::new_v4()),
    }
}

async fn create_company(pool: &PgPool, policy: &RoutingPolicy) -> (Uuid, Uuid) {
    let community_id = Uuid::new_v4();
    sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
        .bind(community_id)
        .bind(format!("ortak-control-{}.example", community_id.simple()))
        .execute(pool)
        .await
        .expect("insert community");
    let company_id: Uuid = sqlx::query(
        "INSERT INTO companies (slug, display_name, routing_policy)
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(format!("co-{}", Uuid::new_v4().simple()))
    .bind("Ortak test company")
    .bind(serde_json::to_value(policy).expect("policy json"))
    .fetch_one(pool)
    .await
    .expect("insert company")
    .try_get("id")
    .expect("company id");
    sqlx::query("INSERT INTO office_company_bindings (community_id, company_id) VALUES ($1, $2)")
        .bind(community_id)
        .bind(company_id)
        .execute(pool)
        .await
        .expect("insert binding");
    (community_id, company_id)
}

async fn activate_employee(pool: &PgPool, company_id: Uuid, employee: &Employee) -> Uuid {
    sqlx::query(
        "INSERT INTO employees (company_id, id, status) VALUES ($1, $2, 'draft')
         ON CONFLICT DO NOTHING",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .execute(pool)
    .await
    .expect("insert employee");
    activate_revision(pool, company_id, employee).await
}

async fn activate_revision(pool: &PgPool, company_id: Uuid, employee: &Employee) -> Uuid {
    let mut manifest = employee.clone();
    manifest.status = EmployeeStatus::Active;
    let manifest = serde_json::to_value(&manifest).expect("manifest json");
    let fingerprint = Sha256::digest(manifest.to_string().as_bytes()).to_vec();
    let revision_id: Uuid = sqlx::query(
        "INSERT INTO employee_revisions
             (company_id, employee_id, revision_number, manifest, manifest_fingerprint, provisioning_mode)
         VALUES ($1, $2,
                 (SELECT coalesce(max(revision_number), 0) + 1 FROM employee_revisions
                   WHERE company_id = $1 AND employee_id = $2),
                 $3, $4, 'adopt')
         RETURNING id",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .bind(manifest)
    .bind(fingerprint)
    .fetch_one(pool)
    .await
    .expect("insert revision")
    .try_get("id")
    .expect("revision id");
    sqlx::query(
        "UPDATE employees SET active_revision_id = $3, status = 'active', updated_at = now()
          WHERE company_id = $1 AND id = $2",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .bind(revision_id)
    .execute(pool)
    .await
    .expect("activate revision");
    revision_id
}

fn committed(outcome: RoutingCommitOutcome) -> ortak_control::routing::CommittedDecision {
    match outcome {
        RoutingCommitOutcome::Committed(decision) => decision,
        other => panic!("expected a committed decision, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn replayed_message_creates_one_decision_and_one_dispatch() {
    let policy = RoutingPolicy::default();
    let fixture = Fixture::new(policy.clone(), vec![fixture_employee("cem")]).await;
    let id = message_id();

    assert_eq!(fixture.accept(id).await, InboxInsertOutcome::Inserted);
    assert_eq!(fixture.accept(id).await, InboxInsertOutcome::AlreadyPresent);

    let generation = fixture.claim(id, "worker-a").await;
    let proposal = fixture.proposal(
        id,
        id,
        generation,
        MessageOrigin::Human("sefa".to_owned()),
        &["cem"],
        &policy,
    );
    let first = committed(
        fixture
            .control
            .commit_routing(&fixture.scope, &proposal)
            .await
            .expect("commit"),
    );
    assert_eq!(first.wake_count, 1);
    assert!(first.hop_consumed);
    assert_eq!(first.chain.hop_count, 1);
    assert_eq!(first.dispatches.len(), 1);

    // Replay after the decision: the inbox row is terminal, so it cannot be
    // reclaimed, and a late duplicate commit with the old claim writes nothing.
    assert_eq!(fixture.accept(id).await, InboxInsertOutcome::AlreadyPresent);
    let reclaim = fixture
        .control
        .claim_message(&fixture.scope, id, "worker-b", Duration::from_secs(60), 5)
        .await
        .expect("claim attempt");
    assert!(reclaim.is_none(), "decided rows must not be reclaimable");
    let duplicate = fixture
        .control
        .commit_routing(&fixture.scope, &proposal)
        .await
        .expect("duplicate commit attempt");
    assert_eq!(
        duplicate,
        RoutingCommitOutcome::AlreadyDecided {
            decision_id: first.decision_id
        }
    );

    let inbox = fixture
        .control
        .inbox_row(&fixture.scope, id)
        .await
        .expect("read inbox")
        .expect("inbox row");
    assert_eq!(inbox.state, InboxState::Decided);
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM routing_decisions WHERE company_id = $1 AND message_id = $2",
                Some(id)
            )
            .await,
        1
    );
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM outbox WHERE company_id = $1 AND kind = 'run_dispatch' \
                 AND routing_decision_id = (SELECT id FROM routing_decisions \
                 WHERE company_id = $1 AND message_id = $2)",
                Some(id)
            )
            .await,
        1
    );

    let stored = fixture
        .control
        .decision_for_message(&fixture.scope, id)
        .await
        .expect("read decision")
        .expect("decision exists");
    assert_eq!(stored.id, first.decision_id);
    assert_eq!(stored.inbox_claim_generation, generation);
    assert_eq!(stored.candidate_revision_ids, vec![fixture.revision("cem")]);
    assert_eq!(stored.recipients.len(), 1);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn stale_claim_generation_cannot_finalize_or_dispatch() {
    let policy = RoutingPolicy::default();
    let fixture = Fixture::new(policy.clone(), vec![fixture_employee("cem")]).await;
    let id = message_id();
    fixture.accept(id).await;

    // Worker A takes a zero-length lease, so worker B can reclaim immediately.
    let stale = fixture
        .control
        .claim_message(&fixture.scope, id, "worker-a", Duration::ZERO, 5)
        .await
        .expect("claim")
        .expect("claimable")
        .claim_generation;
    let current = fixture.claim(id, "worker-b").await;
    assert!(current > stale);

    let late = fixture.proposal(
        id,
        id,
        stale,
        MessageOrigin::Human("sefa".to_owned()),
        &["cem"],
        &policy,
    );
    let outcome = fixture
        .control
        .commit_routing(&fixture.scope, &late)
        .await
        .expect("late commit");
    assert_eq!(
        outcome,
        RoutingCommitOutcome::StaleClaim {
            observed_state: InboxState::Claimed,
            observed_generation: current,
        }
    );
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM outbox WHERE company_id = $1 AND $2::bytea IS NOT NULL",
                Some(id)
            )
            .await,
        0
    );
    assert!(fixture
        .control
        .chain_state(&fixture.scope, id)
        .await
        .expect("chain read")
        .is_none());

    let fresh = fixture.proposal(
        id,
        id,
        current,
        MessageOrigin::Human("sefa".to_owned()),
        &["cem"],
        &policy,
    );
    let decision = committed(
        fixture
            .control
            .commit_routing(&fixture.scope, &fresh)
            .await
            .expect("current commit"),
    );
    assert_eq!(decision.wake_count, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Postgres"]
async fn sibling_branches_cannot_both_spend_the_final_hop() {
    let policy = RoutingPolicy::default();
    assert_eq!(policy.max_hops, 2);
    let fixture = Fixture::new(
        policy.clone(),
        vec![
            fixture_employee("cem"),
            fixture_employee("zeynep"),
            third_employee(),
        ],
    )
    .await;

    let root = message_id();
    fixture.accept(root).await;
    let generation = fixture.claim(root, "worker-root").await;
    let initial = committed(
        fixture
            .control
            .commit_routing(
                &fixture.scope,
                &fixture.proposal(
                    root,
                    root,
                    generation,
                    MessageOrigin::Human("sefa".to_owned()),
                    &["cem"],
                    &policy,
                ),
            )
            .await
            .expect("root commit"),
    );
    assert_eq!(initial.chain.hop_count, 1);

    // Two sibling delegations from Cem race for the single remaining hop.
    let left = message_id();
    let right = message_id();
    fixture.accept(left).await;
    fixture.accept(right).await;
    let left_generation = fixture.claim(left, "worker-left").await;
    let right_generation = fixture.claim(right, "worker-right").await;
    let cem = MessageOrigin::Employee(employee_id("cem"));
    let left_proposal = fixture.proposal(
        left,
        root,
        left_generation,
        cem.clone(),
        &["zeynep"],
        &policy,
    );
    let right_proposal = fixture.proposal(right, root, right_generation, cem, &["ada"], &policy);

    let (left_outcome, right_outcome) = tokio::join!(
        fixture
            .control
            .commit_routing(&fixture.scope, &left_proposal),
        fixture
            .control
            .commit_routing(&fixture.scope, &right_proposal),
    );
    let left_outcome = committed(left_outcome.expect("left commit"));
    let right_outcome = committed(right_outcome.expect("right commit"));

    let (winner, loser) = if left_outcome.wake_count == 1 {
        (left_outcome, right_outcome)
    } else {
        (right_outcome, left_outcome)
    };
    assert_eq!(winner.wake_count, 1);
    assert!(winner.hop_consumed);
    assert_eq!(winner.chain.hop_count, 2);
    assert_eq!(winner.dispatches.len(), 1);

    assert_eq!(loser.wake_count, 0);
    assert!(!loser.hop_consumed);
    assert_eq!(loser.mode, RoutingMode::Silent);
    assert_eq!(loser.summary_reason, RoutingReason::HopLimitReached);
    assert!(loser.refreshed);
    assert!(loser.dispatches.is_empty());
    assert_eq!(loser.recipients[0].action, RecipientAction::Drop);
    assert_eq!(loser.recipients[0].reason, RoutingReason::HopLimitReached);

    let chain = fixture
        .control
        .chain_state(&fixture.scope, root)
        .await
        .expect("chain read")
        .expect("chain exists");
    assert_eq!(chain.hop_count, 2);
    assert_eq!(chain.wake_count, 2);
    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM outbox o JOIN routing_decisions d \
                   ON d.company_id = o.company_id AND d.id = o.routing_decision_id \
                 WHERE o.company_id = $1 AND d.root_message_id = $2",
                Some(root)
            )
            .await,
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Postgres"]
async fn same_employee_is_reserved_once_per_root_chain() {
    let policy = RoutingPolicy {
        max_hops: 3,
        max_chain_wakes: 4,
        ..RoutingPolicy::default()
    };
    let fixture = Fixture::new(
        policy.clone(),
        vec![fixture_employee("cem"), fixture_employee("zeynep")],
    )
    .await;

    let root = message_id();
    fixture.accept(root).await;
    let generation = fixture.claim(root, "worker-root").await;
    committed(
        fixture
            .control
            .commit_routing(
                &fixture.scope,
                &fixture.proposal(
                    root,
                    root,
                    generation,
                    MessageOrigin::Human("sefa".to_owned()),
                    &["cem"],
                    &policy,
                ),
            )
            .await
            .expect("root commit"),
    );

    let left = message_id();
    let right = message_id();
    fixture.accept(left).await;
    fixture.accept(right).await;
    let left_generation = fixture.claim(left, "worker-left").await;
    let right_generation = fixture.claim(right, "worker-right").await;
    let cem = MessageOrigin::Employee(employee_id("cem"));
    let left_proposal = fixture.proposal(
        left,
        root,
        left_generation,
        cem.clone(),
        &["zeynep"],
        &policy,
    );
    let right_proposal = fixture.proposal(
        right,
        root,
        right_generation,
        cem.clone(),
        &["zeynep"],
        &policy,
    );

    let (left_outcome, right_outcome) = tokio::join!(
        fixture
            .control
            .commit_routing(&fixture.scope, &left_proposal),
        fixture
            .control
            .commit_routing(&fixture.scope, &right_proposal),
    );
    let mut outcomes = [
        committed(left_outcome.expect("left commit")),
        committed(right_outcome.expect("right commit")),
    ];
    outcomes.sort_by_key(|outcome| outcome.wake_count);
    assert_eq!(outcomes[0].wake_count, 0);
    assert_eq!(
        outcomes[0].summary_reason,
        RoutingReason::AlreadyVisited,
        "losing sibling must explain the refreshed reservation"
    );
    assert!(outcomes[0].dispatches.is_empty());
    assert_eq!(outcomes[1].wake_count, 1);

    // A later branch cannot re-reserve the originally woken employee either.
    let back = message_id();
    fixture.accept(back).await;
    let back_generation = fixture.claim(back, "worker-back").await;
    let back_outcome = committed(
        fixture
            .control
            .commit_routing(
                &fixture.scope,
                &fixture.proposal(
                    back,
                    root,
                    back_generation,
                    MessageOrigin::Employee(employee_id("zeynep")),
                    &["cem"],
                    &policy,
                ),
            )
            .await
            .expect("back commit"),
    );
    assert_eq!(back_outcome.wake_count, 0);
    assert_eq!(back_outcome.summary_reason, RoutingReason::AlreadyVisited);

    assert_eq!(
        fixture
            .count(
                "SELECT count(*) FROM delivery_chain_visits \
                 WHERE company_id = $1 AND root_message_id = $2",
                Some(root)
            )
            .await,
        2
    );
    let chain = fixture
        .control
        .chain_state(&fixture.scope, root)
        .await
        .expect("chain read")
        .expect("chain exists");
    assert_eq!(chain.hop_count, 2);
    assert_eq!(chain.wake_count, 2);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn company_scope_is_derived_from_bindings_and_isolates_companies() {
    let policy = RoutingPolicy::default();
    let company_a = Fixture::new(policy.clone(), vec![fixture_employee("cem")]).await;
    let company_b = Fixture::new(policy.clone(), vec![fixture_employee("cem")]).await;
    assert_ne!(company_a.scope, company_b.scope);

    let unknown = company_a
        .control
        .resolve_company_for_community(Uuid::new_v4())
        .await;
    assert!(matches!(
        unknown,
        Err(ControlError::UnknownCompanyBinding { .. })
    ));
    let unbound_insert = company_a
        .control
        .insert_accepted_event(Uuid::new_v4(), &inbox_event(message_id()))
        .await;
    assert!(matches!(
        unbound_insert,
        Err(ControlError::UnknownCompanyBinding { .. })
    ));

    // The same signed event id accepted in two communities lands in two
    // company inboxes, and each scope can only see and claim its own row.
    let shared = message_id();
    assert_eq!(company_a.accept(shared).await, InboxInsertOutcome::Inserted);
    assert_eq!(company_b.accept(shared).await, InboxInsertOutcome::Inserted);
    let generation_a = company_a.claim(shared, "worker-a").await;
    let generation_b = company_b.claim(shared, "worker-b").await;

    let decision_a = committed(
        company_a
            .control
            .commit_routing(
                &company_a.scope,
                &company_a.proposal(
                    shared,
                    shared,
                    generation_a,
                    MessageOrigin::Human("sefa".to_owned()),
                    &["cem"],
                    &policy,
                ),
            )
            .await
            .expect("commit in company A"),
    );
    assert_eq!(decision_a.wake_count, 1);

    // Company B's Cem is a different employee with its own chain budget.
    let decision_b = committed(
        company_b
            .control
            .commit_routing(
                &company_b.scope,
                &company_b.proposal(
                    shared,
                    shared,
                    generation_b,
                    MessageOrigin::Human("sefa".to_owned()),
                    &["cem"],
                    &policy,
                ),
            )
            .await
            .expect("commit in company B"),
    );
    assert_eq!(decision_b.wake_count, 1);
    assert_ne!(decision_a.decision_id, decision_b.decision_id);

    let from_b = company_b
        .control
        .decision_for_message(&company_b.scope, shared)
        .await
        .expect("read")
        .expect("decision in B");
    assert_eq!(from_b.id, decision_b.decision_id);
    let visits_a = company_a
        .count(
            "SELECT count(*) FROM delivery_chain_visits WHERE company_id = $1 AND root_message_id = $2",
            Some(shared),
        )
        .await;
    assert_eq!(visits_a, 1);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn proposal_and_claim_from_one_company_cannot_commit_under_another_scope() {
    let policy = RoutingPolicy::default();
    let company_a = Fixture::new(policy.clone(), vec![fixture_employee("cem")]).await;
    let company_b = Fixture::new(policy.clone(), vec![fixture_employee("cem")]).await;

    // Both companies accept the same signed event id and each claims it once,
    // so message id and claim generation coincide across the two inboxes.
    let shared = message_id();
    assert_eq!(company_a.accept(shared).await, InboxInsertOutcome::Inserted);
    assert_eq!(company_b.accept(shared).await, InboxInsertOutcome::Inserted);
    let claim_a = company_a
        .control
        .claim_message(
            &company_a.scope,
            shared,
            "worker-a",
            Duration::from_secs(60),
            5,
        )
        .await
        .expect("claim in A")
        .expect("row in A is claimable");
    let generation_b = company_b.claim(shared, "worker-b").await;
    assert_eq!(claim_a.claim_generation, generation_b);
    assert_eq!(claim_a.company_id, company_a.scope.company_id());

    let proposal_a = company_a.proposal(
        shared,
        shared,
        claim_a.claim_generation,
        MessageOrigin::Human("sefa".to_owned()),
        &["cem"],
        &policy,
    );
    assert_eq!(proposal_a.company_id, company_a.scope.company_id());

    // The repository refuses the cross-company pairing before any write.
    let confused = company_b
        .control
        .commit_routing(&company_b.scope, &proposal_a)
        .await;
    assert!(
        matches!(confused, Err(ControlError::InvalidProposal(_))),
        "expected InvalidProposal, got {confused:?}"
    );

    // The service refuses a foreign claim before normalizing or scoring, and
    // does not release company B's coincident claim on the way out.
    let service = InboxRoutingService::new(
        company_b.control.clone(),
        FakeNormalizer {
            messages: HashMap::new(),
        },
        RevisionChangingScorer {
            pool: company_b.pool.clone(),
            company_id: company_b.scope.company_id(),
            calls: Arc::new(AtomicU32::new(0)),
        },
        RoutingWorkerConfig {
            worker_id: "worker-b".to_owned(),
            ..RoutingWorkerConfig::default()
        },
    );
    let confused_service = service.route_claim(&company_b.scope, &claim_a).await;
    assert!(
        matches!(confused_service, Err(ControlError::InvalidProposal(_))),
        "expected InvalidProposal, got {confused_service:?}"
    );

    // Company B has no decision, visit, chain counter, or outbox row.
    assert_eq!(
        company_b
            .count(
                "SELECT count(*) FROM routing_decisions WHERE company_id = $1 AND message_id = $2",
                Some(shared),
            )
            .await,
        0
    );
    assert_eq!(
        company_b
            .count(
                "SELECT count(*) FROM delivery_chain_visits WHERE company_id = $1 AND root_message_id = $2",
                Some(shared),
            )
            .await,
        0
    );
    assert_eq!(
        company_b
            .count(
                "SELECT count(*) FROM delivery_chains WHERE company_id = $1 AND root_message_id = $2",
                Some(shared),
            )
            .await,
        0
    );
    assert_eq!(
        company_b
            .count(
                "SELECT count(*) FROM outbox WHERE company_id = $1 AND $2::bytea IS NOT NULL",
                Some(shared),
            )
            .await,
        0
    );
    let inbox_b = company_b
        .control
        .inbox_row(&company_b.scope, shared)
        .await
        .expect("read inbox row in B")
        .expect("inbox row in B");
    assert_eq!(inbox_b.state, InboxState::Claimed);
    assert_eq!(inbox_b.claim_generation, generation_b);
    assert_eq!(inbox_b.claimed_by.as_deref(), Some("worker-b"));
    assert_eq!(inbox_b.last_error, None);

    // Nothing was written in A either; A's own scope still commits normally.
    assert_eq!(
        company_a
            .count(
                "SELECT count(*) FROM routing_decisions WHERE company_id = $1 AND message_id = $2",
                Some(shared),
            )
            .await,
        0
    );
    let decision_a = committed(
        company_a
            .control
            .commit_routing(&company_a.scope, &proposal_a)
            .await
            .expect("commit in company A"),
    );
    assert_eq!(decision_a.wake_count, 1);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn changed_inputs_roll_back_and_outbox_leases_are_fenced() {
    let policy = RoutingPolicy::default();
    let fixture = Fixture::new(
        policy.clone(),
        vec![fixture_employee("cem"), fixture_employee("zeynep")],
    )
    .await;
    let id = message_id();
    fixture.accept(id).await;
    let generation = fixture.claim(id, "worker-a").await;

    // A stale candidate revision invalidates the proposal without touching state.
    let mut stale_revision = fixture.proposal(
        id,
        id,
        generation,
        MessageOrigin::Human("sefa".to_owned()),
        &["cem"],
        &policy,
    );
    stale_revision.candidates[0].revision_id = Uuid::new_v4();
    let outcome = fixture
        .control
        .commit_routing(&fixture.scope, &stale_revision)
        .await
        .expect("commit attempt");
    assert!(matches!(
        outcome,
        RoutingCommitOutcome::InputsChanged(RevalidationFailure::CandidateRevisionChanged { .. })
    ));

    // A changed policy fingerprint under the same version does too.
    let mut stale_policy = fixture.proposal(
        id,
        id,
        generation,
        MessageOrigin::Human("sefa".to_owned()),
        &["cem"],
        &policy,
    );
    stale_policy.decision.policy_fingerprint = RoutingPolicy {
        semantic_threshold: 0.5,
        ..policy.clone()
    }
    .fingerprint();
    let outcome = fixture
        .control
        .commit_routing(&fixture.scope, &stale_policy)
        .await
        .expect("commit attempt");
    assert!(matches!(
        outcome,
        RoutingCommitOutcome::InputsChanged(RevalidationFailure::PolicyChanged { .. })
    ));
    let inbox = fixture
        .control
        .inbox_row(&fixture.scope, id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(inbox.state, InboxState::Claimed);
    assert_eq!(inbox.claim_generation, generation);
    assert!(fixture
        .control
        .chain_state(&fixture.scope, id)
        .await
        .expect("chain read")
        .is_none());

    let decision = committed(
        fixture
            .control
            .commit_routing(
                &fixture.scope,
                &fixture.proposal(
                    id,
                    id,
                    generation,
                    MessageOrigin::Human("sefa".to_owned()),
                    &["cem"],
                    &policy,
                ),
            )
            .await
            .expect("commit"),
    );
    assert_eq!(decision.dispatches.len(), 1);

    // Outbox: lease, fail with retry, re-lease, stale completion rejected.
    let first = fixture
        .control
        .claim_due(
            &fixture.scope,
            Some(OutboxKind::RunDispatch),
            "dispatcher-a",
            Duration::from_secs(30),
            10,
        )
        .await
        .expect("lease");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].id, decision.dispatches[0].outbox_id);
    assert_eq!(first[0].employee_id.as_deref(), Some("cem"));
    assert!(fixture
        .control
        .claim_due(
            &fixture.scope,
            None,
            "dispatcher-b",
            Duration::from_secs(30),
            10
        )
        .await
        .expect("lease")
        .is_empty());

    assert_eq!(
        fixture
            .control
            .fail(&fixture.scope, &first[0], "runtime unavailable", Utc::now())
            .await
            .expect("fail"),
        OutboxFailOutcome::Retrying
    );
    let second = fixture
        .control
        .claim_due(
            &fixture.scope,
            None,
            "dispatcher-b",
            Duration::from_secs(30),
            10,
        )
        .await
        .expect("lease");
    assert_eq!(second.len(), 1);
    assert_ne!(second[0].lease_token, first[0].lease_token);
    assert_eq!(second[0].attempt_count, 2);

    assert!(!fixture
        .control
        .complete(&fixture.scope, &first[0])
        .await
        .expect("stale complete"));
    assert!(fixture
        .control
        .complete(&fixture.scope, &second[0])
        .await
        .expect("complete"));
    assert!(fixture
        .control
        .claim_due(
            &fixture.scope,
            None,
            "dispatcher-c",
            Duration::from_secs(30),
            10
        )
        .await
        .expect("lease")
        .is_empty());
}

struct FakeNormalizer {
    messages: HashMap<MessageId, NormalizedMessage>,
}

impl MessageNormalizer for FakeNormalizer {
    async fn normalize(
        &self,
        _scope: &CompanyScope,
        inbox: &ortak_control::inbox::InboxRow,
    ) -> ortak_control::Result<Option<NormalizedMessage>> {
        Ok(self.messages.get(&inbox.event.event_id).cloned())
    }
}

/// Scorer that mutates Zeynep's active revision during its first call, the
/// way a concurrent provisioning activation would while scoring is in flight.
struct RevisionChangingScorer {
    pool: PgPool,
    company_id: Uuid,
    calls: Arc<AtomicU32>,
}

impl SemanticScorer for RevisionChangingScorer {
    async fn score(&self, request: &SemanticRoutingRequest) -> ScoringOutcome {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            activate_revision(&self.pool, self.company_id, &fixture_employee("zeynep")).await;
        }
        let scores = request
            .candidates()
            .iter()
            .map(|candidate| SemanticScore {
                employee_id: candidate.employee_id().clone(),
                score: if candidate.employee_id().as_str() == "zeynep" {
                    0.95
                } else {
                    0.1
                },
                evidence: vec![EvidenceLabel::parse("domain:mobile").expect("label")],
            })
            .collect();
        ScoringOutcome {
            result: Ok(scores),
            metadata: ScorerMetadata {
                adapter: "fake".to_owned(),
                model: Some("fake-model".to_owned()),
                prompt_version: Some("p0".to_owned()),
                version: "v0".to_owned(),
                latency_ms: Some(1),
                usage: None,
            },
        }
    }
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn service_rescores_when_a_candidate_revision_changes_during_scoring() {
    let policy = RoutingPolicy::default();
    let fixture = Fixture::new(
        policy.clone(),
        vec![fixture_employee("cem"), fixture_employee("zeynep")],
    )
    .await;
    let id = message_id();
    fixture.accept(id).await;

    let envelope = MessageEnvelope::human_channel(
        id.to_hex(),
        "sefa",
        "office",
        "Mobil uygulama fikrini kim değerlendirebilir?",
    );
    let normalizer = FakeNormalizer {
        messages: HashMap::from([(
            id,
            NormalizedMessage {
                envelope: envelope.clone(),
                root_message_id: id,
            },
        )]),
    };
    let calls = Arc::new(AtomicU32::new(0));
    let scorer = RevisionChangingScorer {
        pool: fixture.pool.clone(),
        company_id: fixture.scope.company_id(),
        calls: calls.clone(),
    };
    let service = InboxRoutingService::new(
        fixture.control.clone(),
        normalizer,
        scorer,
        RoutingWorkerConfig {
            worker_id: "service-worker".to_owned(),
            ..RoutingWorkerConfig::default()
        },
    );

    let outcome = service
        .claim_and_route(&fixture.scope)
        .await
        .expect("route")
        .expect("a claim was due");
    let ServiceOutcome::Committed(decision) = outcome else {
        panic!("expected a committed decision, got {outcome:?}");
    };
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "first score was invalidated"
    );
    assert_eq!(decision.mode, RoutingMode::Semantic);
    assert_eq!(decision.wake_count, 1);
    assert_eq!(decision.dispatches[0].employee_id, "zeynep");

    let stored = fixture
        .control
        .decision_for_message(&fixture.scope, id)
        .await
        .expect("read")
        .expect("stored");
    assert_eq!(stored.scorer_adapter.as_deref(), Some("fake"));
    let current_zeynep: Uuid = sqlx::query(
        "SELECT active_revision_id FROM employees WHERE company_id = $1 AND id = 'zeynep'",
    )
    .bind(fixture.scope.company_id())
    .fetch_one(&fixture.pool)
    .await
    .expect("read")
    .try_get("active_revision_id")
    .expect("column");
    assert_ne!(current_zeynep, fixture.revision("zeynep"));
    assert!(stored.candidate_revision_ids.contains(&current_zeynep));
    assert!(!stored
        .candidate_revision_ids
        .contains(&fixture.revision("zeynep")));
    let expected_hash = routing_input_hash(
        &envelope,
        &[
            CandidateRevision {
                employee_id: employee_id("cem"),
                revision_id: fixture.revision("cem"),
            },
            CandidateRevision {
                employee_id: employee_id("zeynep"),
                revision_id: current_zeynep,
            },
        ],
        &policy,
    );
    assert_eq!(stored.input_hash, expected_hash);

    // Nothing is due any more and the row is terminal.
    assert!(service
        .claim_and_route(&fixture.scope)
        .await
        .expect("route")
        .is_none());
}
