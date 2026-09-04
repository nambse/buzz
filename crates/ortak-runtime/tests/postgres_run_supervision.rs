//! Production-seam Postgres tests for run dispatch and supervision.
//!
//! Run with a disposable local database that can receive the embedded
//! migrations:
//! `ORTAK_TEST_DATABASE_URL=postgres://... cargo test -p ortak-runtime -- --ignored`

use std::time::Duration;

use chrono::Utc;
use ortak_control::fakes::FakeRuntimeAdapter;
use ortak_control::inbox::InboxEvent;
use ortak_control::outbox::{OutboxFailOutcome, OutboxKind, OutboxLease};
use ortak_control::ports::{
    CompanyDirectory, InboxRepository, OutboxRepository, RoutingRepository,
};
use ortak_control::routing::{
    CandidateRevision, RosterScope, RoutingCommitOutcome, RoutingProposal,
};
use ortak_control::run_event::{
    BoundedText, DeliveryIntentKind, RedactionPolicy, RunEvent, RunEventPayload,
};
use ortak_control::runtime::{RunStartReceipt, RuntimeAdapter, RuntimeError, RuntimeRunRef};
use ortak_control::{CompanyScope, MessageId, PgControlPlane};
use ortak_domain::{
    Employee, EmployeeId, EmployeeManifest, EmployeeStatus, MessageOrigin, RecipientAction,
    RecipientDecision, RoutingDecision, RoutingMode, RoutingPolicy, RoutingReason,
};
use ortak_runtime::{
    AppendOutcome, CancellationOutcome, CorrelationOutcome, DispatchAuthorization, DispatchOutcome,
    DispatchRefusal, PrepareOutcome, PumpOutcome, RunDispatchRepository, RunStatus,
    RunSupervisionError, RunSupervisor, SupervisorConfig,
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const DEFAULT_DATABASE_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1 -- local test-only credentials
const PROFILE_REF: &str = "fake://profiles/cem";

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

/// The Cem fixture rebound to the fake runtime; secrets remain references.
fn fixture_employee() -> Employee {
    let yaml = std::fs::read_to_string(format!(
        "{}/../../config/employees/cem.yaml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read fixture");
    let manifest: EmployeeManifest = serde_yaml::from_str(&yaml).expect("parse fixture");
    let mut employee = manifest.employee;
    employee.status = EmployeeStatus::Active;
    employee.runtime.adapter = "fake-runtime".to_owned();
    employee.runtime.profile_ref = Some(PROFILE_REF.to_owned());
    employee
}

struct RunRow {
    status: String,
    runtime_run_ref: Option<String>,
    employee_revision_id: Uuid,
    message_id: Vec<u8>,
    delivery_intent: Option<String>,
    cancel_reason: Option<String>,
    error_code: Option<String>,
    finished_at: Option<chrono::DateTime<Utc>>,
}

struct OutboxRow {
    state: String,
    attempt_count: i32,
    lease_token: Option<Uuid>,
    run_id: Option<Uuid>,
    last_error: Option<String>,
}

struct Fixture {
    pool: PgPool,
    control: PgControlPlane,
    community_id: Uuid,
    scope: CompanyScope,
    revision_id: Uuid,
    policy: RoutingPolicy,
    adapter: FakeRuntimeAdapter,
}

impl Fixture {
    async fn new() -> Self {
        let pool = setup_pool().await;
        let control = PgControlPlane::new(pool.clone());
        let policy = RoutingPolicy::default();
        let (community_id, company_id) = create_company(&pool, &policy).await;
        let revision_id = activate_employee(&pool, company_id, &fixture_employee(), true).await;
        let scope = control
            .resolve_company_for_community(community_id)
            .await
            .expect("resolve scope");
        Self {
            pool,
            control,
            community_id,
            scope,
            revision_id,
            policy,
            adapter: FakeRuntimeAdapter::new().with_existing_profile(PROFILE_REF, true),
        }
    }

    fn supervisor(
        &self,
        config: SupervisorConfig,
    ) -> RunSupervisor<PgControlPlane, &FakeRuntimeAdapter> {
        RunSupervisor::new(self.control.clone(), &self.adapter, config)
    }

    fn config(&self) -> SupervisorConfig {
        SupervisorConfig {
            retry_backoff: Duration::ZERO,
            ..SupervisorConfig::default()
        }
    }

    /// Stores a signed Office event plus its inbox row, routes it to Cem
    /// through the production routing commit, and returns the decision id.
    async fn route(&self, content: &str) -> Uuid {
        let id = message_id();
        let created_at = Utc::now();
        sqlx::query(
            "INSERT INTO events (community_id, id, pubkey, created_at, kind, tags, content, sig)
             VALUES ($1, $2, $3, $4, 1, '[]'::jsonb, $5, $6)",
        )
        .bind(self.community_id)
        .bind(id.as_bytes().as_slice())
        .bind([7u8; 32].as_slice())
        .bind(created_at)
        .bind(content)
        .bind([9u8; 64].as_slice())
        .execute(&self.pool)
        .await
        .expect("insert event");
        self.control
            .insert_accepted_event(
                self.community_id,
                &InboxEvent {
                    event_id: id,
                    event_created_at: created_at,
                    event_kind: 1,
                    author_pubkey: [7; 32],
                    channel_id: Some(Uuid::new_v4()),
                },
            )
            .await
            .expect("insert inbox row");
        let claim = self
            .control
            .claim_message(&self.scope, id, "router", Duration::from_secs(60), 5)
            .await
            .expect("claim")
            .expect("claimable");
        let proposal = RoutingProposal {
            company_id: self.scope.company_id(),
            message_id: id,
            root_message_id: id,
            claim_generation: claim.claim_generation,
            origin: MessageOrigin::Human("sefa".to_owned()),
            input_hash: [3; 32],
            candidates: vec![CandidateRevision {
                employee_id: employee_id("cem"),
                revision_id: self.revision_id,
            }],
            roster_scope: RosterScope::Targets,
            decision: RoutingDecision {
                message_id: id.to_hex(),
                mode: RoutingMode::Deterministic,
                summary_reason: RoutingReason::StructuredDispatch,
                policy_version: self.policy.version.clone(),
                policy_fingerprint: self.policy.fingerprint(),
                recipients: vec![RecipientDecision {
                    employee_id: employee_id("cem"),
                    action: RecipientAction::Wake,
                    reason: RoutingReason::StructuredDispatch,
                    score: None,
                    evidence: Vec::new(),
                }],
            },
            scorer: None,
        };
        match self
            .control
            .commit_routing(&self.scope, &proposal)
            .await
            .expect("commit routing")
        {
            RoutingCommitOutcome::Committed(decision) => {
                assert_eq!(decision.dispatches.len(), 1);
                decision.decision_id
            }
            other => panic!("expected a committed decision, got {other:?}"),
        }
    }

    async fn lease(&self, lease: Duration) -> OutboxLease {
        let mut leases = self
            .control
            .claim_due(
                &self.scope,
                Some(OutboxKind::RunDispatch),
                "dispatcher",
                lease,
                10,
            )
            .await
            .expect("claim due");
        assert_eq!(leases.len(), 1, "expected exactly one due run_dispatch row");
        leases.remove(0)
    }

    async fn run_rows(&self) -> i64 {
        sqlx::query("SELECT count(*) FROM runs WHERE company_id = $1")
            .bind(self.scope.company_id())
            .fetch_one(&self.pool)
            .await
            .expect("count")
            .try_get(0)
            .expect("count column")
    }

    async fn run(&self, run_id: Uuid) -> RunRow {
        let row = sqlx::query(
            "SELECT status, runtime_run_ref, employee_revision_id, message_id, delivery_intent,
                    cancel_reason, error_code, finished_at
               FROM runs WHERE company_id = $1 AND id = $2",
        )
        .bind(self.scope.company_id())
        .bind(run_id)
        .fetch_one(&self.pool)
        .await
        .expect("run row");
        RunRow {
            status: row.try_get("status").expect("status"),
            runtime_run_ref: row.try_get("runtime_run_ref").expect("ref"),
            employee_revision_id: row.try_get("employee_revision_id").expect("revision"),
            message_id: row.try_get("message_id").expect("message"),
            delivery_intent: row.try_get("delivery_intent").expect("intent"),
            cancel_reason: row.try_get("cancel_reason").expect("cancel reason"),
            error_code: row.try_get("error_code").expect("error code"),
            finished_at: row.try_get("finished_at").expect("finished"),
        }
    }

    async fn outbox(&self, outbox_id: Uuid) -> OutboxRow {
        let row = sqlx::query(
            "SELECT state, attempt_count, lease_token, run_id, last_error
               FROM outbox WHERE company_id = $1 AND id = $2",
        )
        .bind(self.scope.company_id())
        .bind(outbox_id)
        .fetch_one(&self.pool)
        .await
        .expect("outbox row");
        OutboxRow {
            state: row.try_get("state").expect("state"),
            attempt_count: row.try_get("attempt_count").expect("attempts"),
            lease_token: row.try_get("lease_token").expect("token"),
            run_id: row.try_get("run_id").expect("run id"),
            last_error: row.try_get("last_error").expect("error"),
        }
    }

    /// `(sequence, event_type, runtime_cursor, payload)` in order.
    async fn events(&self, run_id: Uuid) -> Vec<(i64, String, Option<String>, serde_json::Value)> {
        sqlx::query(
            "SELECT sequence, event_type, runtime_cursor, payload FROM run_events
              WHERE company_id = $1 AND run_id = $2 ORDER BY sequence",
        )
        .bind(self.scope.company_id())
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .expect("events")
        .iter()
        .map(|row| {
            (
                row.try_get("sequence").expect("sequence"),
                row.try_get("event_type").expect("type"),
                row.try_get("runtime_cursor").expect("cursor"),
                row.try_get("payload").expect("payload"),
            )
        })
        .collect()
    }

    async fn started(&self) -> (Uuid, RuntimeRunRef, Uuid) {
        self.route("Cem, selam nasılsın?").await;
        let lease = self.lease(Duration::from_secs(60)).await;
        match self
            .supervisor(self.config())
            .dispatch(&self.scope, &lease)
            .await
            .expect("dispatch")
        {
            DispatchOutcome::Started {
                run_id,
                runtime_run_ref,
            } => (run_id, runtime_run_ref, lease.id),
            other => panic!("expected a started run, got {other:?}"),
        }
    }
}

async fn create_company(pool: &PgPool, policy: &RoutingPolicy) -> (Uuid, Uuid) {
    let community_id = Uuid::new_v4();
    sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
        .bind(community_id)
        .bind(format!("ortak-runtime-{}.example", community_id.simple()))
        .execute(pool)
        .await
        .expect("insert community");
    let company_id: Uuid = sqlx::query(
        "INSERT INTO companies (slug, display_name, routing_policy)
         VALUES ($1, 'Ortak runtime test', $2) RETURNING id",
    )
    .bind(format!("co-{}", Uuid::new_v4().simple()))
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

/// Employee, immutable revision, validated runtime binding row, activation:
/// the shape the provisioning saga leaves behind.
async fn activate_employee(
    pool: &PgPool,
    company_id: Uuid,
    employee: &Employee,
    validated: bool,
) -> Uuid {
    sqlx::query(
        "INSERT INTO employees (company_id, id, status) VALUES ($1, $2, 'draft')
         ON CONFLICT DO NOTHING",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .execute(pool)
    .await
    .expect("insert employee");
    let manifest = serde_json::to_value(employee).expect("manifest json");
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
    .bind(&manifest)
    .bind(Sha256::digest(manifest.to_string().as_bytes()).to_vec())
    .fetch_one(pool)
    .await
    .expect("insert revision")
    .try_get("id")
    .expect("revision id");
    sqlx::query(
        "INSERT INTO employee_runtime_bindings
             (company_id, revision_id, employee_id, adapter, provisioning_mode, profile_ref,
              model, workspace_ref, credential_refs, options, validated_at)
         VALUES ($1, $2, $3, $4, 'adopt', $5, $6, $7, $8, $9, $10)",
    )
    .bind(company_id)
    .bind(revision_id)
    .bind(employee.id.as_str())
    .bind(&employee.runtime.adapter)
    .bind(employee.runtime.profile_ref.as_deref())
    .bind(&employee.runtime.model)
    .bind(&employee.runtime.workspace_ref)
    .bind(serde_json::to_value(&employee.runtime.credential_refs).expect("refs"))
    .bind(serde_json::to_value(&employee.runtime.options).expect("options"))
    .bind(validated.then(Utc::now))
    .execute(pool)
    .await
    .expect("insert runtime binding");
    sqlx::query(
        "UPDATE employees SET active_revision_id = $3, status = 'active', updated_at = now()
          WHERE company_id = $1 AND id = $2",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .bind(revision_id)
    .execute(pool)
    .await
    .expect("activate");
    revision_id
}

fn authorized(authorization: DispatchAuthorization) -> ortak_runtime::DispatchAuthority {
    match authorization {
        DispatchAuthorization::Authorized(authority) => *authority,
        other => panic!("expected an authorized dispatch, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn dispatch_derives_authority_from_durable_rows_and_never_from_the_lease() {
    let fixture = Fixture::new().await;
    let decision_id = fixture.route("Cem, selam nasılsın?").await;
    let supervisor = fixture.supervisor(fixture.config());

    // The payload is a hint: point it at a foreign revision, message, and
    // employee and the run is still pinned to the durable recipient rows.
    let mut lease = fixture.lease(Duration::from_secs(60)).await;
    lease.payload = serde_json::json!({
        "routing_decision_id": Uuid::new_v4(),
        "message_id": message_id().to_hex(),
        "employee_id": "zeynep",
        "employee_revision_id": Uuid::new_v4(),
    });
    let (run_id, runtime_run_ref) = match supervisor
        .dispatch(&fixture.scope, &lease)
        .await
        .expect("dispatch")
    {
        DispatchOutcome::Started {
            run_id,
            runtime_run_ref,
        } => (run_id, runtime_run_ref),
        other => panic!("expected a started run, got {other:?}"),
    };
    let run = fixture.run(run_id).await;
    assert_eq!(run.status, "running");
    assert_eq!(
        run.runtime_run_ref.as_deref(),
        Some(runtime_run_ref.0.as_str())
    );
    assert_eq!(run.employee_revision_id, fixture.revision_id);
    let decided: Vec<u8> =
        sqlx::query("SELECT message_id FROM routing_decisions WHERE company_id = $1 AND id = $2")
            .bind(fixture.scope.company_id())
            .bind(decision_id)
            .fetch_one(&fixture.pool)
            .await
            .expect("decision")
            .try_get("message_id")
            .expect("message id");
    assert_eq!(run.message_id, decided);
    let outbox = fixture.outbox(lease.id).await;
    assert_eq!(outbox.state, "delivered");
    assert_eq!(outbox.run_id, Some(run_id));
    assert!(outbox.lease_token.is_none());
    let events = fixture.events(run_id).await;
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].0, events[0].1.as_str()), (0, "run.queued"));
    assert_eq!(fixture.run_rows().await, 1);

    // Lease hints that disagree with the durable row are rejected without
    // any write, and a lease presented under another company finds no row.
    fixture.route("Cem, ikinci mesaj").await;
    let honest = fixture.lease(Duration::from_secs(60)).await;
    let mut forged = honest.clone();
    forged.employee_id = Some("zeynep".to_owned());
    assert!(matches!(
        supervisor.dispatch(&fixture.scope, &forged).await,
        Err(RunSupervisionError::LeaseInconsistent { outbox_id }) if outbox_id == honest.id
    ));
    let (other_community, _) = create_company(&fixture.pool, &fixture.policy).await;
    let other_scope = fixture
        .control
        .resolve_company_for_community(other_community)
        .await
        .expect("other scope");
    assert!(matches!(
        supervisor.dispatch(&other_scope, &honest).await,
        Err(RunSupervisionError::UnknownOutboxRow { outbox_id }) if outbox_id == honest.id
    ));
    let untouched = fixture.outbox(honest.id).await;
    assert_eq!(untouched.state, "pending");
    assert_eq!(untouched.lease_token, Some(honest.lease_token));
    assert!(untouched.run_id.is_none());
    assert_eq!(fixture.run_rows().await, 1);

    assert!(matches!(
        supervisor
            .dispatch(&fixture.scope, &honest)
            .await
            .expect("honest dispatch"),
        DispatchOutcome::Started { .. }
    ));
    assert_eq!(fixture.run_rows().await, 2);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn lifecycle_and_binding_refusals_record_bounded_retry_before_any_runtime_call() {
    let fixture = Fixture::new().await;
    fixture.route("Cem, selam").await;
    let supervisor = fixture.supervisor(fixture.config());

    sqlx::query("UPDATE employees SET status = 'disabled' WHERE company_id = $1 AND id = 'cem'")
        .bind(fixture.scope.company_id())
        .execute(&fixture.pool)
        .await
        .expect("disable");
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let outcome = supervisor
        .dispatch(&fixture.scope, &lease)
        .await
        .expect("dispatch");
    assert_eq!(
        outcome,
        DispatchOutcome::Refused {
            refusal: DispatchRefusal::EmployeeNotActive {
                status: EmployeeStatus::Disabled
            },
            retry: OutboxFailOutcome::Retrying,
        }
    );
    assert_eq!(fixture.run_rows().await, 0);
    assert!(matches!(
        fixture
            .adapter
            .next_events(&RuntimeRunRef("fake-run-1".to_owned()), None, 1)
            .await,
        Err(RuntimeError::UnknownRun { .. })
    ));
    let row = fixture.outbox(lease.id).await;
    assert_eq!((row.state.as_str(), row.attempt_count), ("pending", 1));
    assert!(row.lease_token.is_none());
    assert!(row
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("not active")));

    // Re-enabled but with an unvalidated runtime binding: still refused.
    sqlx::query("UPDATE employees SET status = 'active' WHERE company_id = $1 AND id = 'cem'")
        .bind(fixture.scope.company_id())
        .execute(&fixture.pool)
        .await
        .expect("enable");
    sqlx::query(
        "UPDATE employee_runtime_bindings SET validated_at = NULL
          WHERE company_id = $1 AND revision_id = $2",
    )
    .bind(fixture.scope.company_id())
    .bind(fixture.revision_id)
    .execute(&fixture.pool)
    .await
    .expect("unvalidate");
    let lease = fixture.lease(Duration::from_secs(60)).await;
    assert_eq!(lease.attempt_count, 2);
    assert_eq!(
        supervisor
            .dispatch(&fixture.scope, &lease)
            .await
            .expect("dispatch"),
        DispatchOutcome::Refused {
            refusal: DispatchRefusal::RuntimeBindingUnvalidated,
            retry: OutboxFailOutcome::Retrying,
        }
    );
    assert_eq!(fixture.run_rows().await, 0);

    // A later revision does not matter: the decision pins the revision it
    // routed against, so re-validating that binding lets the dispatch through.
    sqlx::query(
        "UPDATE employee_runtime_bindings SET validated_at = now()
          WHERE company_id = $1 AND revision_id = $2",
    )
    .bind(fixture.scope.company_id())
    .bind(fixture.revision_id)
    .execute(&fixture.pool)
    .await
    .expect("revalidate");
    let mut newer = fixture_employee();
    newer.title = "Co-Founder (updated)".to_owned();
    let newer_revision =
        activate_employee(&fixture.pool, fixture.scope.company_id(), &newer, true).await;
    assert_ne!(newer_revision, fixture.revision_id);
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let run_id = match supervisor
        .dispatch(&fixture.scope, &lease)
        .await
        .expect("dispatch")
    {
        DispatchOutcome::Started { run_id, .. } => run_id,
        other => panic!("expected a started run, got {other:?}"),
    };
    assert_eq!(
        fixture.run(run_id).await.employee_revision_id,
        fixture.revision_id
    );
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn retry_after_lost_acknowledgement_converges_and_a_stale_lease_cannot_overwrite() {
    let fixture = Fixture::new().await;
    fixture.route("Cem, selam").await;
    let supervisor = fixture.supervisor(fixture.config());

    // Worker A: authorize, create the run, start it at the runtime, then
    // crash before the acknowledgement is recorded.
    let lease_a = fixture.lease(Duration::from_millis(50)).await;
    let authority = authorized(
        fixture
            .control
            .authorize_dispatch(&fixture.scope, &lease_a)
            .await
            .expect("authorize"),
    );
    let run_id = match fixture
        .control
        .prepare_run(&fixture.scope, &authority)
        .await
        .expect("prepare")
    {
        PrepareOutcome::Prepared(prepared) => {
            assert!(prepared.created);
            assert_eq!(prepared.status, RunStatus::Queued);
            prepared.run_id
        }
        PrepareOutcome::StaleLease => panic!("lease is live"),
    };
    let spec = authority.run_spec(run_id).expect("spec");
    let receipt_a = fixture
        .adapter
        .start_run(&spec)
        .await
        .expect("external start");
    assert_eq!(fixture.run(run_id).await.status, "queued");

    // Worker B reclaims after the lease expires and retries with the same
    // durable run and idempotency key: the runtime returns the same run.
    tokio::time::sleep(Duration::from_millis(120)).await;
    let lease_b = fixture.lease(Duration::from_secs(60)).await;
    assert_ne!(lease_b.lease_token, lease_a.lease_token);
    assert_eq!(lease_b.attempt_count, 2);
    assert_eq!(
        supervisor
            .dispatch(&fixture.scope, &lease_b)
            .await
            .expect("dispatch"),
        DispatchOutcome::Started {
            run_id,
            runtime_run_ref: receipt_a.runtime_run_ref.clone(),
        }
    );
    assert!(matches!(
        fixture
            .adapter
            .next_events(&RuntimeRunRef("fake-run-2".to_owned()), None, 1)
            .await,
        Err(RuntimeError::UnknownRun { .. })
    ));
    assert_eq!(fixture.run_rows().await, 1);
    assert_eq!(fixture.outbox(lease_a.id).await.state, "delivered");

    // Worker A wakes up: its lease is stale at every step and nothing changes.
    assert_eq!(
        supervisor
            .dispatch(&fixture.scope, &lease_a)
            .await
            .expect("stale dispatch"),
        DispatchOutcome::StaleLease
    );
    assert_eq!(
        fixture
            .control
            .prepare_run(&fixture.scope, &authority)
            .await
            .expect("stale prepare"),
        PrepareOutcome::StaleLease
    );
    // Even a direct correlation attempt with another runtime reference is
    // refused by the compare-and-set.
    let forged = RunStartReceipt {
        runtime_run_ref: RuntimeRunRef("fake-run-forged".to_owned()),
        started_at: Utc::now(),
    };
    assert_eq!(
        fixture
            .control
            .correlate_run(&fixture.scope, &authority, run_id, &forged)
            .await
            .expect("correlate"),
        CorrelationOutcome::RefConflict {
            durable: receipt_a.runtime_run_ref.clone()
        }
    );
    let run = fixture.run(run_id).await;
    assert_eq!(run.status, "running");
    assert_eq!(run.runtime_run_ref, Some(receipt_a.runtime_run_ref.0));
    assert_eq!(fixture.run_rows().await, 1);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn event_pump_resumes_from_the_durable_cursor_and_commits_terminal_state_atomically() {
    let fixture = Fixture::new().await;
    let (run_id, runtime_run_ref, _) = fixture.started().await;
    let supervisor = fixture.supervisor(SupervisorConfig {
        event_batch_limit: 2,
        ..fixture.config()
    });

    let secret = "sk-live-abcdef1234567890";
    for payload in [
        RunEventPayload::AssistantDelta {
            turn: 0,
            delta: BoundedText::raw(format!("thinking with {secret}")),
        },
        RunEventPayload::ToolCallStarted {
            call_id: "call-1".to_owned(),
            tool: "files".to_owned(),
            arguments: BoundedText::raw("{\"path\": \"README.md\"}"),
        },
        RunEventPayload::RunWaiting {
            reason: "approval".to_owned(),
            detail: BoundedText::raw("external publish requires approval"),
        },
        RunEventPayload::ToolCallCompleted {
            call_id: "call-1".to_owned(),
            result: BoundedText::raw("ok"),
        },
        RunEventPayload::DeliveryIntent {
            intent: DeliveryIntentKind::Reply,
            target_ref: None,
        },
        RunEventPayload::RunCompleted {
            delivery_intent: DeliveryIntentKind::Reply,
        },
        RunEventPayload::AssistantDelta {
            turn: 1,
            delta: BoundedText::raw("after the end"),
        },
    ] {
        fixture.adapter.push_event(&runtime_run_ref, payload);
    }

    // Batches of two, resuming from the stored cursor each time; the
    // runtime's own run.started is the first streamed event.
    let mut statuses = Vec::new();
    loop {
        match supervisor.pump(&fixture.scope, run_id).await.expect("pump") {
            PumpOutcome::Appended { status, .. } => statuses.push(status),
            PumpOutcome::Terminal { status } => {
                statuses.push(status);
                break;
            }
            other => panic!("unexpected pump outcome {other:?}"),
        }
        if statuses.len() > 8 {
            panic!("pump did not terminate: {statuses:?}");
        }
    }
    assert_eq!(
        statuses,
        vec![
            RunStatus::Running,
            RunStatus::Waiting,
            RunStatus::Running,
            RunStatus::Completed,
            RunStatus::Completed,
        ]
    );

    let run = fixture.run(run_id).await;
    assert_eq!(run.status, "completed");
    assert_eq!(run.delivery_intent.as_deref(), Some("reply"));
    assert!(run.finished_at.is_some());
    let events = fixture.events(run_id).await;
    let types = events
        .iter()
        .map(|(sequence, event_type, _, _)| (*sequence, event_type.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        types,
        vec![
            (0, "run.queued"),
            (1, "run.started"),
            (2, "assistant.delta"),
            (3, "tool_call.started"),
            (4, "run.waiting"),
            (5, "tool_call.completed"),
            (6, "delivery.intent"),
            (7, "run.completed"),
        ]
    );
    let stored = events
        .iter()
        .map(|(_, _, _, payload)| payload.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!stored.contains(secret), "secret leaked into run_events");
    assert!(stored.contains("[redacted]"));
    let cursors = events
        .iter()
        .filter_map(|(_, _, cursor, _)| cursor.clone())
        .collect::<Vec<_>>();
    assert_eq!(cursors.len(), 7);
    assert_eq!(
        cursors
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        7
    );

    // A replayed cursor is deduplicated rather than appended, and a
    // terminal run accepts nothing more.
    let replay = RunEvent::normalize(
        run_id,
        Utc::now(),
        Some(cursors[1].clone()),
        &RunEventPayload::AssistantDelta {
            turn: 0,
            delta: BoundedText::raw("replayed"),
        },
        &RedactionPolicy::new(),
    )
    .expect("normalize");
    assert_eq!(
        fixture
            .control
            .append_supervised_events(&fixture.scope, run_id, &runtime_run_ref, &[replay])
            .await
            .expect("append"),
        AppendOutcome::RunTerminal {
            status: RunStatus::Completed
        }
    );
    assert_eq!(fixture.events(run_id).await.len(), 8);

    // Mid-stream replay on a second run: the duplicate cursor is skipped
    // and the sequence stays dense.
    let (second, second_ref, _) = fixture.started().await;
    assert!(matches!(
        supervisor.pump(&fixture.scope, second).await.expect("pump"),
        PumpOutcome::Appended { appended: 1, .. }
    ));
    let first_cursor = fixture.events(second).await[1].2.clone().expect("cursor");
    let replay = RunEvent::normalize(
        second,
        Utc::now(),
        Some(first_cursor.clone()),
        &RunEventPayload::RunStarted {
            runtime_run_ref: second_ref.0.clone(),
        },
        &RedactionPolicy::new(),
    )
    .expect("normalize");
    assert_eq!(
        fixture
            .control
            .append_supervised_events(&fixture.scope, second, &second_ref, &[replay])
            .await
            .expect("append"),
        AppendOutcome::Appended {
            sequences: Vec::new(),
            duplicate_cursors: vec![first_cursor],
            status: RunStatus::Running,
        }
    );
    assert_eq!(fixture.events(second).await.len(), 2);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn cancellation_is_supervised_idempotent_and_requires_a_correlated_run() {
    let fixture = Fixture::new().await;
    let (run_id, runtime_run_ref, _) = fixture.started().await;
    let supervisor = fixture.supervisor(fixture.config());

    let outcome = supervisor
        .cancel(
            &fixture.scope,
            run_id,
            "operator stop; token Bearer abc123456789",
        )
        .await
        .expect("cancel");
    assert_eq!(outcome, CancellationOutcome::Cancelled { run_id });
    let run = fixture.run(run_id).await;
    assert_eq!(run.status, "cancelled");
    assert!(run.finished_at.is_some());
    assert!(run.error_code.is_none());
    let reason = run.cancel_reason.expect("reason recorded");
    assert!(reason.contains("operator stop"));
    assert!(!reason.contains("abc123456789"));
    let events = fixture.events(run_id).await;
    let last = events.last().expect("terminal event");
    assert_eq!(last.1, "run.cancelled");
    assert!(
        last.2.is_none(),
        "synthesized cancellation carries no runtime cursor"
    );
    assert!(!last.3.to_string().contains("abc123456789"));

    // Replays settle without touching the run again.
    assert_eq!(
        supervisor
            .cancel(&fixture.scope, run_id, "again")
            .await
            .expect("replay cancel"),
        CancellationOutcome::AlreadyTerminal {
            run_id,
            status: RunStatus::Cancelled
        }
    );
    assert_eq!(
        supervisor.pump(&fixture.scope, run_id).await.expect("pump"),
        PumpOutcome::Terminal {
            status: RunStatus::Cancelled
        }
    );
    assert_eq!(fixture.events(run_id).await.len(), events.len());
    // The runtime's own cancellation event stays behind the durable one.
    assert!(matches!(
        fixture.adapter.cancel_run(&runtime_run_ref, "again").await,
        Ok(ortak_control::runtime::CancelOutcome::AlreadyTerminal)
    ));

    // A run that was never correlated cannot be addressed at the runtime.
    fixture.route("Cem, bir daha").await;
    let lease = fixture.lease(Duration::from_secs(60)).await;
    let authority = authorized(
        fixture
            .control
            .authorize_dispatch(&fixture.scope, &lease)
            .await
            .expect("authorize"),
    );
    let queued = match fixture
        .control
        .prepare_run(&fixture.scope, &authority)
        .await
        .expect("prepare")
    {
        PrepareOutcome::Prepared(prepared) => prepared.run_id,
        PrepareOutcome::StaleLease => panic!("lease is live"),
    };
    assert!(matches!(
        supervisor.cancel(&fixture.scope, queued, "too early").await,
        Err(RunSupervisionError::NotCorrelated { run_id, status: RunStatus::Queued }) if run_id == queued
    ));
    assert_eq!(fixture.run(queued).await.status, "queued");
}
