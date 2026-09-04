//! Production-seam Postgres tests for the provisioning saga repository and the
//! run-event store (migration 0045 tables).
//!
//! Run with a local database that can receive the embedded migrations:
//! `ORTAK_TEST_DATABASE_URL=postgres://... cargo test -p ortak-control --test postgres_provisioning -- --ignored`

use chrono::Utc;
use ortak_control::fakes::{
    FakeCredentialResolver, FakeMemoryAdapter, FakeOfficeIdentityAdapter, FakeRuntimeAdapter,
};
use ortak_control::ports::{CompanyDirectory, ProvisioningRepository, RunEventRepository};
use ortak_control::provisioning::{
    OperationMode, OperationStatus, OperationUpdate, ProvisioningError, ProvisioningOperation,
    ProvisioningRequest, ProvisioningSaga, ProvisioningStep, RevisionActivation, SagaConfig,
    SagaOutcome, StepRecord, StepState,
};
use ortak_control::run_event::{
    BoundedText, RedactionPolicy, RunEvent, RunEventError, RunEventPayload, TerminalStream,
};
use ortak_control::{CompanyScope, ControlError, PgControlPlane};
use ortak_domain::{
    CredentialRef, EmployeeId, EmployeeManifest, EmployeeStatus, ProvisioningMode, RoutingPolicy,
};
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

async fn setup() -> (PgPool, PgControlPlane, CompanyScope) {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to test database");
    MIGRATOR.run(&pool).await.expect("apply migrations");
    let community_id = Uuid::new_v4();
    sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
        .bind(community_id)
        .bind(format!("ortak-prov-{}.example", community_id.simple()))
        .execute(&pool)
        .await
        .expect("insert community");
    let company_id: Uuid = sqlx::query(
        "INSERT INTO companies (slug, display_name, routing_policy)
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(format!("co-{}", Uuid::new_v4().simple()))
    .bind("Ortak provisioning test company")
    .bind(serde_json::to_value(RoutingPolicy::default()).expect("policy json"))
    .fetch_one(&pool)
    .await
    .expect("insert company")
    .try_get("id")
    .expect("company id");
    sqlx::query("INSERT INTO office_company_bindings (community_id, company_id) VALUES ($1, $2)")
        .bind(community_id)
        .bind(company_id)
        .execute(&pool)
        .await
        .expect("insert binding");
    let control = PgControlPlane::new(pool.clone());
    let scope = control
        .resolve_company_for_community(community_id)
        .await
        .expect("resolve scope");
    (pool, control, scope)
}

fn fixture(name: &str) -> EmployeeManifest {
    let yaml = std::fs::read_to_string(format!(
        "{}/../../config/employees/{name}.yaml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read fixture");
    serde_yaml::from_str(&yaml).expect("parse fixture")
}

/// Unique-per-test disposable `create` employee so parallel runs never
/// collide on employee ids, public keys, or aliases.
fn disposable() -> EmployeeManifest {
    let suffix = Uuid::new_v4().simple().to_string();
    let id = format!("ada-{}", &suffix[..12]);
    let mut manifest = fixture("zeynep");
    manifest.provisioning = ProvisioningMode::Create;
    let employee = &mut manifest.employee;
    employee.id = EmployeeId::parse(&id).expect("id");
    employee.name = format!("Ada {}", &suffix[..6]);
    employee.aliases = vec![format!("platform-{}", &suffix[..6])];
    employee.runtime.profile_ref = None;
    employee.runtime.credential_refs =
        vec![
            CredentialRef::parse(format!("credential://ortak-runtime/{id}/codex-oauth"))
                .expect("ref"),
        ];
    if let Some(memory) = &mut employee.memory {
        memory.workspace = format!("{id}-workspace");
        memory.employee_peer = id.clone();
    }
    let mut key = suffix.clone();
    key.push_str(&Uuid::new_v4().simple().to_string());
    employee.office.public_key = key;
    employee.office.signer_ref = CredentialRef::parse(format!(
        "credential://ortak-runtime/{id}/office-signing-key"
    ))
    .expect("ref");
    manifest
}

struct Fakes {
    runtime: FakeRuntimeAdapter,
    memory: FakeMemoryAdapter,
    office: FakeOfficeIdentityAdapter,
    credentials: FakeCredentialResolver,
}

impl Fakes {
    fn credentials_for(manifest: &EmployeeManifest) -> FakeCredentialResolver {
        let employee = &manifest.employee;
        FakeCredentialResolver::new().with_references(
            employee
                .runtime
                .credential_refs
                .iter()
                .map(|reference| reference.as_str().to_owned())
                .chain(std::iter::once(
                    employee.office.signer_ref.as_str().to_owned(),
                )),
        )
    }

    fn adoptable(manifest: &EmployeeManifest) -> Self {
        let employee = &manifest.employee;
        Self {
            runtime: FakeRuntimeAdapter::new().with_existing_profile(
                employee.runtime.profile_ref.as_deref().expect("profile"),
                true,
            ),
            memory: FakeMemoryAdapter::new()
                .with_existing_binding(employee.memory.as_ref().expect("memory")),
            office: FakeOfficeIdentityAdapter::new()
                .with_signer(
                    employee.office.signer_ref.as_str(),
                    &employee.office.public_key,
                )
                .with_existing_member(&employee.office.public_key),
            credentials: Self::credentials_for(manifest),
        }
    }

    fn creatable(manifest: &EmployeeManifest) -> Self {
        let employee = &manifest.employee;
        Self {
            runtime: FakeRuntimeAdapter::new(),
            memory: FakeMemoryAdapter::new(),
            office: FakeOfficeIdentityAdapter::new().with_signer(
                employee.office.signer_ref.as_str(),
                &employee.office.public_key,
            ),
            credentials: Self::credentials_for(manifest),
        }
    }

    fn saga<'a>(
        &'a self,
        control: &'a PgControlPlane,
    ) -> ProvisioningSaga<
        'a,
        PgControlPlane,
        FakeRuntimeAdapter,
        FakeMemoryAdapter,
        FakeOfficeIdentityAdapter,
        FakeCredentialResolver,
    > {
        ProvisioningSaga::new(
            control,
            &self.runtime,
            &self.memory,
            &self.office,
            &self.credentials,
            SagaConfig::default(),
        )
    }
}

fn request(manifest: &EmployeeManifest, mode: OperationMode, dry_run: bool) -> ProvisioningRequest {
    ProvisioningRequest {
        employee_id: manifest.employee.id.clone(),
        mode,
        dry_run,
        idempotency_key: format!("{}-{}", manifest.employee.id, Uuid::new_v4().simple()),
        manifest: manifest.clone(),
    }
}

/// The activation the saga would commit for `manifest` under `operation_id`,
/// built directly so tests can hit the repository's SQL seam.
fn activation_for(manifest: &EmployeeManifest, operation_id: Uuid) -> RevisionActivation {
    let now = Utc::now();
    let mut employee = manifest.employee.clone();
    employee.status = EmployeeStatus::Active;
    let mut activation_step = StepRecord::pending(operation_id, ProvisioningStep::ActivateRevision);
    activation_step.state = StepState::Succeeded;
    activation_step.attempt_count = 1;
    activation_step.started_at = Some(now);
    activation_step.finished_at = Some(now);
    RevisionActivation {
        employee,
        provisioning_mode: ProvisioningMode::Create,
        manifest_fingerprint: [0; 32],
        activation_step,
        runtime_validated_at: now,
        memory_validated_at: Some(now),
        office_verified_at: now,
    }
}

async fn revision_count(pool: &PgPool, scope: &CompanyScope, id: &str) -> i64 {
    sqlx::query(
        "SELECT count(*) FROM employee_revisions WHERE company_id = $1 AND employee_id = $2",
    )
    .bind(scope.company_id())
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("count")
    .try_get(0)
    .expect("count column")
}

/// `(valid_until is set, signer_ref)` for the binding of one public key.
async fn office_binding(
    pool: &PgPool,
    scope: &CompanyScope,
    public_key_hex: &str,
) -> Option<(bool, String)> {
    let key =
        ortak_control::office_identity::OfficePublicKey::parse_hex(public_key_hex).expect("key");
    sqlx::query(
        "SELECT valid_until IS NOT NULL AS retired, signer_ref FROM employee_office_bindings
          WHERE company_id = $1 AND public_key = $2",
    )
    .bind(scope.company_id())
    .bind(key.as_bytes().to_vec())
    .fetch_optional(pool)
    .await
    .expect("binding")
    .map(|row| {
        (
            row.try_get("retired").expect("retired"),
            row.try_get("signer_ref").expect("signer_ref"),
        )
    })
}

async fn employee_row(pool: &PgPool, scope: &CompanyScope, id: &str) -> (String, Option<Uuid>) {
    let row = sqlx::query(
        "SELECT status, active_revision_id FROM employees WHERE company_id = $1 AND id = $2",
    )
    .bind(scope.company_id())
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("employee row");
    (
        row.try_get("status").expect("status"),
        row.try_get("active_revision_id").expect("revision"),
    )
}

async fn step_states(
    pool: &PgPool,
    scope: &CompanyScope,
    operation: &ProvisioningOperation,
) -> Vec<(String, String)> {
    sqlx::query(
        "SELECT step_name, state FROM provisioning_operation_steps
          WHERE company_id = $1 AND operation_id = $2 ORDER BY step_index",
    )
    .bind(scope.company_id())
    .bind(operation.id)
    .fetch_all(pool)
    .await
    .expect("steps")
    .iter()
    .map(|row| {
        (
            row.try_get("step_name").expect("name"),
            row.try_get("state").expect("state"),
        )
    })
    .collect()
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn cem_adopt_dry_run_persists_steps_and_never_activates() {
    let (pool, control, scope) = setup().await;
    let manifest = fixture("cem");
    let fakes = Fakes::adoptable(&manifest);
    let saga = fakes.saga(&control);
    let request = request(&manifest, OperationMode::Adopt, true);
    let operation = saga.begin(&scope, &request).await.expect("begin");
    assert_eq!(operation.status, OperationStatus::Pending);
    assert_eq!(operation.steps.len(), ProvisioningStep::ALL.len());

    let outcome = saga.resume(&scope, operation.id).await.expect("resume");
    let SagaOutcome::Succeeded(done) = outcome else {
        panic!("expected success, got {outcome:?}");
    };
    assert_eq!(done.result_revision_id, None);

    let states = step_states(&pool, &scope, &done).await;
    assert_eq!(states.len(), 10);
    assert_eq!(
        states[3],
        ("ensure_runtime_profile".to_owned(), "succeeded".to_owned())
    );
    assert_eq!(
        states[7],
        ("publish_office_profile".to_owned(), "skipped".to_owned())
    );
    assert_eq!(
        states[9],
        ("activate_revision".to_owned(), "skipped".to_owned())
    );
    let adopted: bool = sqlx::query(
        "SELECT adopted_existing FROM provisioning_operation_steps
          WHERE company_id = $1 AND operation_id = $2 AND step_name = 'ensure_runtime_profile'",
    )
    .bind(scope.company_id())
    .bind(done.id)
    .fetch_one(&pool)
    .await
    .expect("row")
    .try_get("adopted_existing")
    .expect("column");
    assert!(adopted);

    let (status, revision) = employee_row(&pool, &scope, "cem").await;
    assert_eq!(status, "draft");
    assert_eq!(revision, None);
    let revisions: i64 = sqlx::query(
        "SELECT count(*) FROM employee_revisions WHERE company_id = $1 AND employee_id = 'cem'",
    )
    .bind(scope.company_id())
    .fetch_one(&pool)
    .await
    .expect("count")
    .try_get(0)
    .expect("count column");
    assert_eq!(revisions, 0);
    assert!(fakes.runtime.created_profiles().is_empty());
    assert!(fakes.office.published_profiles().is_empty());

    // Same key resumes the same row; a different manifest conflicts.
    let again = saga.begin(&scope, &request).await.expect("begin again");
    assert_eq!(again.id, operation.id);
    assert_eq!(again.status, OperationStatus::Succeeded);
    let mut changed = request.clone();
    changed.manifest.employee.biography = "changed".to_owned();
    assert!(matches!(
        saga.begin(&scope, &changed).await,
        Err(ControlError::Provisioning(
            ortak_control::provisioning::ProvisioningError::IdempotencyConflict { .. }
        ))
    ));
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn create_activates_atomically_and_resume_is_idempotent() {
    let (pool, control, scope) = setup().await;
    let manifest = disposable();
    let id = manifest.employee.id.to_string();
    let fakes = Fakes::creatable(&manifest);
    let saga = fakes.saga(&control);
    let operation = saga
        .begin(&scope, &request(&manifest, OperationMode::Create, false))
        .await
        .expect("begin");

    fakes.memory.set_unavailable(true);
    let outcome = saga.resume(&scope, operation.id).await.expect("resume");
    let SagaOutcome::Failed { step, .. } = outcome else {
        panic!("expected failure, got {outcome:?}");
    };
    assert_eq!(step, ProvisioningStep::EnsureMemoryResources);
    let reloaded = control
        .load_operation(&scope, operation.id)
        .await
        .expect("load")
        .expect("exists");
    assert_eq!(reloaded.status, OperationStatus::Failed);
    assert!(reloaded.finished_at.is_some());
    assert_eq!(
        reloaded
            .step(ProvisioningStep::EnsureMemoryResources)
            .expect("step")
            .state,
        StepState::Failed
    );
    let (status, _) = employee_row(&pool, &scope, &id).await;
    assert_eq!(status, "draft");

    fakes.memory.set_unavailable(false);
    let outcome = saga.resume(&scope, operation.id).await.expect("resume");
    let SagaOutcome::Succeeded(done) = outcome else {
        panic!("expected success, got {outcome:?}");
    };
    let revision_id = done.result_revision_id.expect("activated revision");
    assert_eq!(fakes.runtime.created_profiles().len(), 1);
    assert_eq!(
        done.step(ProvisioningStep::EnsureRuntimeProfile)
            .expect("step")
            .attempt_count,
        1
    );
    assert_eq!(
        done.step(ProvisioningStep::EnsureMemoryResources)
            .expect("step")
            .attempt_count,
        2
    );

    let (status, active) = employee_row(&pool, &scope, &id).await;
    assert_eq!(status, "active");
    assert_eq!(active, Some(revision_id));
    let validated: Option<chrono::DateTime<Utc>> = sqlx::query(
        "SELECT validated_at FROM employee_runtime_bindings WHERE company_id = $1 AND revision_id = $2",
    )
    .bind(scope.company_id())
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .expect("runtime binding")
    .try_get("validated_at")
    .expect("column");
    assert!(validated.is_some());
    let verified: Option<chrono::DateTime<Utc>> = sqlx::query(
        "SELECT verified_at FROM employee_office_bindings WHERE company_id = $1 AND revision_id = $2",
    )
    .bind(scope.company_id())
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .expect("office binding")
    .try_get("verified_at")
    .expect("column");
    assert!(verified.is_some());
    let aliases: i64 = sqlx::query(
        "SELECT count(*) FROM employee_aliases WHERE company_id = $1 AND employee_id = $2",
    )
    .bind(scope.company_id())
    .bind(&id)
    .fetch_one(&pool)
    .await
    .expect("aliases")
    .try_get(0)
    .expect("count");
    assert_eq!(aliases, 3);
    let states = step_states(&pool, &scope, &done).await;
    assert!(
        states.iter().all(|(_, state)| state == "succeeded"),
        "{states:?}"
    );

    let again = saga
        .resume(&scope, operation.id)
        .await
        .expect("resume again");
    assert!(matches!(again, SagaOutcome::AlreadyTerminal(_)));
    let revisions: i64 = sqlx::query(
        "SELECT count(*) FROM employee_revisions WHERE company_id = $1 AND employee_id = $2",
    )
    .bind(scope.company_id())
    .bind(&id)
    .fetch_one(&pool)
    .await
    .expect("count")
    .try_get(0)
    .expect("count column");
    assert_eq!(revisions, 1);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn run_events_append_densely_and_reject_replayed_cursors() {
    let (pool, control, scope) = setup().await;
    let manifest = disposable();
    let fakes = Fakes::creatable(&manifest);
    let saga = fakes.saga(&control);
    let operation = saga
        .begin(&scope, &request(&manifest, OperationMode::Create, false))
        .await
        .expect("begin");
    let SagaOutcome::Succeeded(done) = saga.resume(&scope, operation.id).await.expect("resume")
    else {
        panic!("expected success");
    };
    let revision_id = done.result_revision_id.expect("revision");
    let run_id: Uuid = sqlx::query(
        "INSERT INTO runs (company_id, employee_id, employee_revision_id, runtime_adapter, status, started_at)
         VALUES ($1, $2, $3, 'fake-runtime', 'running', now()) RETURNING id",
    )
    .bind(scope.company_id())
    .bind(manifest.employee.id.as_str())
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .expect("insert run")
    .try_get("id")
    .expect("run id");

    let policy = RedactionPolicy::new().with_literal_secrets(["fixture-literal-secret-value-0001"]);
    let payloads = [
        RunEventPayload::RunStarted {
            runtime_run_ref: "fake-run-1".to_owned(),
        },
        RunEventPayload::TerminalOutput {
            command_id: "cmd".to_owned(),
            stream: TerminalStream::Stdout,
            chunk: BoundedText::raw("token=fixture-literal-secret-value-0001 done"),
        },
        RunEventPayload::TerminalOutput {
            command_id: "cmd".to_owned(),
            stream: TerminalStream::Stdout,
            chunk: BoundedText::raw("x".repeat(50_000)),
        },
    ];
    let events = payloads
        .iter()
        .enumerate()
        .map(|(index, payload)| {
            RunEvent::normalize(
                run_id,
                Utc::now(),
                Some(format!("cursor-{index}")),
                payload,
                &policy,
            )
            .expect("normalize")
        })
        .collect::<Vec<_>>();

    let appended = control
        .append_run_events(&scope, run_id, &events)
        .await
        .expect("append");
    assert_eq!(appended.sequences, vec![0, 1, 2]);
    assert!(appended.duplicate_cursors.is_empty());

    let replay = control
        .append_run_events(&scope, run_id, &events[1..])
        .await
        .expect("replay");
    assert!(replay.sequences.is_empty());
    assert_eq!(
        replay.duplicate_cursors,
        vec!["cursor-1".to_owned(), "cursor-2".to_owned()]
    );

    let stored = control
        .run_events_after(&scope, run_id, -1, 100)
        .await
        .expect("read");
    assert_eq!(stored.len(), 3);
    assert_eq!(
        stored
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1), Some(2)]
    );
    let json = serde_json::to_string(&stored[1].payload).expect("json");
    assert!(!json.contains("fixture-literal-secret-value-0001"));
    assert!(json.contains("[redacted]"));
    let RunEventPayload::TerminalOutput { chunk, .. } = &stored[2].payload else {
        panic!("unexpected payload");
    };
    assert!(chunk.truncated);

    let count: i64 =
        sqlx::query("SELECT count(*) FROM run_events WHERE company_id = $1 AND run_id = $2")
            .bind(scope.company_id())
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .expect("count")
            .try_get(0)
            .expect("column");
    assert_eq!(count, 3);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn stale_writes_never_regress_a_committed_activation() {
    let (pool, control, scope) = setup().await;
    let manifest = disposable();
    let id = manifest.employee.id.to_string();
    let fakes = Fakes::creatable(&manifest);
    let saga = fakes.saga(&control);
    let operation = saga
        .begin(&scope, &request(&manifest, OperationMode::Create, false))
        .await
        .expect("begin");
    let SagaOutcome::Succeeded(done) = saga.resume(&scope, operation.id).await.expect("resume")
    else {
        panic!("expected success");
    };
    let revision_id = done.result_revision_id.expect("revision");

    // A replayed or concurrent worker with a stale view tries to re-run the
    // activation step and to flip the status; every write is refused.
    let mut stale = done
        .step(ProvisioningStep::ActivateRevision)
        .expect("step")
        .clone();
    stale.state = StepState::Running;
    stale.attempt_count += 1;
    let refused = control.record_step(&scope, operation.id, &stale).await;
    assert!(
        matches!(
            refused,
            Err(ControlError::Provisioning(
                ProvisioningError::Superseded { .. }
            ))
        ),
        "{refused:?}"
    );
    for status in [OperationStatus::Running, OperationStatus::Failed] {
        let refused = control
            .update_operation(
                &scope,
                operation.id,
                &OperationUpdate {
                    status,
                    current_step: Some(ProvisioningStep::ActivateRevision),
                    error_message: Some("stale".to_owned()),
                },
            )
            .await;
        assert!(
            matches!(
                refused,
                Err(ControlError::Provisioning(
                    ProvisioningError::Superseded { .. }
                ))
            ),
            "{status:?}: {refused:?}"
        );
    }
    // A stale worker replaying the activation itself gets the committed id.
    let replay = control
        .activate_revision(
            &scope,
            operation.id,
            &activation_for(&manifest, operation.id),
        )
        .await
        .expect("idempotent replay");
    assert_eq!(replay, revision_id);

    let stored = control
        .load_operation(&scope, operation.id)
        .await
        .expect("load")
        .expect("exists");
    assert_eq!(stored.status, OperationStatus::Succeeded);
    assert_eq!(stored.result_revision_id, Some(revision_id));
    assert_eq!(stored.error_message, None);
    assert!(stored.finished_at.is_some());
    let activation = stored
        .step(ProvisioningStep::ActivateRevision)
        .expect("step");
    assert_eq!(activation.state, StepState::Succeeded);
    assert_eq!(activation.attempt_count, 1);
    assert_eq!(revision_count(&pool, &scope, &id).await, 1);

    // Even if the status column were regressed behind the repository's back,
    // compensation refuses an operation that activated a revision.
    sqlx::query(
        "UPDATE provisioning_operations SET status = 'failed', current_step = 'activate_revision'
          WHERE company_id = $1 AND id = $2",
    )
    .bind(scope.company_id())
    .bind(operation.id)
    .execute(&pool)
    .await
    .expect("regress status");
    let refused = saga.compensate(&scope, operation.id).await;
    assert!(
        matches!(
            refused,
            Err(ControlError::Provisioning(
                ProvisioningError::InvalidTransition { .. }
            ))
        ),
        "{refused:?}"
    );
    assert!(matches!(
        saga.resume(&scope, operation.id).await.expect("resume"),
        SagaOutcome::AlreadyTerminal(_)
    ));
    assert!(fakes.runtime.deleted_profiles().is_empty());
    assert!(fakes.memory.deleted_resources().is_empty());
    assert!(fakes.office.removed_memberships().is_empty());
    let (status, active) = employee_row(&pool, &scope, &id).await;
    assert_eq!(status, "active");
    assert_eq!(active, Some(revision_id));
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn begin_conflicts_on_mode_or_dry_run_mismatch() {
    let (_pool, control, scope) = setup().await;
    let manifest = fixture("cem");
    let fakes = Fakes::adoptable(&manifest);
    let saga = fakes.saga(&control);
    let dry_run = request(&manifest, OperationMode::Adopt, true);
    let first = saga.begin(&scope, &dry_run).await.expect("begin");
    let same = saga.begin(&scope, &dry_run).await.expect("begin again");
    assert_eq!(first.id, same.id);

    let mut live = dry_run.clone();
    live.dry_run = false;
    assert!(matches!(
        saga.begin(&scope, &live).await,
        Err(ControlError::Provisioning(
            ProvisioningError::IdempotencyConflict { .. }
        ))
    ));
    let mut update = dry_run.clone();
    update.mode = OperationMode::Update;
    assert!(matches!(
        saga.begin(&scope, &update).await,
        Err(ControlError::Provisioning(
            ProvisioningError::IdempotencyConflict { .. }
        ))
    ));
    let stored = control
        .load_operation(&scope, first.id)
        .await
        .expect("load")
        .expect("exists");
    assert!(stored.dry_run);
    assert_eq!(stored.mode, OperationMode::Adopt);
    assert_eq!(stored.status, OperationStatus::Pending);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn activate_revision_rejects_a_compensating_operation() {
    let (pool, control, scope) = setup().await;
    let manifest = disposable();
    let id = manifest.employee.id.to_string();
    let fakes = Fakes::creatable(&manifest);
    let saga = fakes.saga(&control);
    let operation = saga
        .begin(&scope, &request(&manifest, OperationMode::Create, false))
        .await
        .expect("begin");
    sqlx::query(
        "UPDATE provisioning_operations SET status = 'compensating'
          WHERE company_id = $1 AND id = $2",
    )
    .bind(scope.company_id())
    .bind(operation.id)
    .execute(&pool)
    .await
    .expect("mark compensating");

    let refused = control
        .activate_revision(
            &scope,
            operation.id,
            &activation_for(&manifest, operation.id),
        )
        .await;
    assert!(
        matches!(
            refused,
            Err(ControlError::Provisioning(
                ProvisioningError::InvalidTransition {
                    status: OperationStatus::Compensating,
                    ..
                }
            ))
        ),
        "{refused:?}"
    );
    // Nor can a stale worker turn compensation back into a run.
    let refused = control
        .update_operation(
            &scope,
            operation.id,
            &OperationUpdate {
                status: OperationStatus::Running,
                current_step: Some(ProvisioningStep::ValidateManifest),
                error_message: None,
            },
        )
        .await;
    assert!(
        matches!(
            refused,
            Err(ControlError::Provisioning(
                ProvisioningError::Superseded { .. }
            ))
        ),
        "{refused:?}"
    );
    assert_eq!(revision_count(&pool, &scope, &id).await, 0);
    let (status, active) = employee_row(&pool, &scope, &id).await;
    assert_eq!(status, "draft");
    assert_eq!(active, None);
    let stored = control
        .load_operation(&scope, operation.id)
        .await
        .expect("load")
        .expect("exists");
    assert_eq!(stored.status, OperationStatus::Compensating);
    assert_eq!(stored.result_revision_id, None);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn retired_office_binding_is_never_reactivated() {
    let (pool, control, scope) = setup().await;
    let manifest = disposable();
    let id = manifest.employee.id.to_string();
    let first_key = manifest.employee.office.public_key.clone();
    let fakes = Fakes::creatable(&manifest);
    let saga = fakes.saga(&control);

    let first = saga
        .begin(&scope, &request(&manifest, OperationMode::Create, false))
        .await
        .expect("begin first");
    let first_revision = control
        .activate_revision(&scope, first.id, &activation_for(&manifest, first.id))
        .await
        .expect("activate first key");

    // Rotate to a second key: the first binding is retired, not deleted.
    let mut rotated = manifest.clone();
    let second_key = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    rotated.employee.office.public_key = second_key.clone();
    let second = saga
        .begin(&scope, &request(&rotated, OperationMode::Create, false))
        .await
        .expect("begin second");
    let second_revision = control
        .activate_revision(&scope, second.id, &activation_for(&rotated, second.id))
        .await
        .expect("activate second key");
    assert_ne!(first_revision, second_revision);
    assert_eq!(
        office_binding(&pool, &scope, &first_key).await,
        Some((
            true,
            manifest.employee.office.signer_ref.as_str().to_owned()
        ))
    );
    assert_eq!(
        office_binding(&pool, &scope, &second_key).await,
        Some((
            false,
            manifest.employee.office.signer_ref.as_str().to_owned()
        ))
    );

    // Going back to the retired key must fail closed and roll back everything.
    let third = saga
        .begin(&scope, &request(&manifest, OperationMode::Create, false))
        .await
        .expect("begin third");
    let refused = control
        .activate_revision(&scope, third.id, &activation_for(&manifest, third.id))
        .await;
    match refused {
        Err(ControlError::InvalidData(detail)) => {
            assert!(detail.contains("retired"), "{detail}");
        }
        other => panic!("expected InvalidData, got {other:?}"),
    }

    // Reusing the current key under a different signer reference is refused too.
    let mut resigned = rotated.clone();
    resigned.employee.office.signer_ref = CredentialRef::parse(format!(
        "credential://ortak-runtime/{id}/office-signing-key-2"
    ))
    .expect("ref");
    let fourth = saga
        .begin(&scope, &request(&resigned, OperationMode::Create, false))
        .await
        .expect("begin fourth");
    let refused = control
        .activate_revision(&scope, fourth.id, &activation_for(&resigned, fourth.id))
        .await;
    match refused {
        Err(ControlError::InvalidData(detail)) => {
            assert!(detail.contains("different signer reference"), "{detail}");
        }
        other => panic!("expected InvalidData, got {other:?}"),
    }

    assert_eq!(
        office_binding(&pool, &scope, &first_key).await,
        Some((
            true,
            manifest.employee.office.signer_ref.as_str().to_owned()
        )),
        "the retired binding stays retired"
    );
    assert_eq!(
        office_binding(&pool, &scope, &second_key).await,
        Some((
            false,
            manifest.employee.office.signer_ref.as_str().to_owned()
        )),
        "the current binding keeps its signer"
    );
    assert_eq!(revision_count(&pool, &scope, &id).await, 2);
    let (status, active) = employee_row(&pool, &scope, &id).await;
    assert_eq!(status, "active");
    assert_eq!(active, Some(second_revision));
    for operation_id in [third.id, fourth.id] {
        let stored = control
            .load_operation(&scope, operation_id)
            .await
            .expect("load")
            .expect("exists");
        assert_eq!(stored.status, OperationStatus::Pending);
        assert_eq!(stored.result_revision_id, None);
    }
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn run_events_store_text_with_nul_and_control_bytes() {
    let (pool, control, scope) = setup().await;
    let manifest = disposable();
    let fakes = Fakes::creatable(&manifest);
    let saga = fakes.saga(&control);
    let operation = saga
        .begin(&scope, &request(&manifest, OperationMode::Create, false))
        .await
        .expect("begin");
    let SagaOutcome::Succeeded(done) = saga.resume(&scope, operation.id).await.expect("resume")
    else {
        panic!("expected success");
    };
    let run_id: Uuid = sqlx::query(
        "INSERT INTO runs (company_id, employee_id, employee_revision_id, runtime_adapter, status, started_at)
         VALUES ($1, $2, $3, 'fake-runtime', 'running', now()) RETURNING id",
    )
    .bind(scope.company_id())
    .bind(manifest.employee.id.as_str())
    .bind(done.result_revision_id.expect("revision"))
    .fetch_one(&pool)
    .await
    .expect("insert run")
    .try_get("id")
    .expect("run id");

    let policy = RedactionPolicy::new();
    let raw = "line\0one\u{1b}[0m\tdone\n";
    let event = RunEvent::normalize(
        run_id,
        Utc::now(),
        Some("cursor-nul".to_owned()),
        &RunEventPayload::TerminalOutput {
            command_id: "cmd\0".to_owned(),
            stream: TerminalStream::Stderr,
            chunk: BoundedText::raw(raw),
        },
        &policy,
    )
    .expect("normalize");
    let appended = control
        .append_run_events(&scope, run_id, &[event])
        .await
        .expect("jsonb must accept the sanitized payload");
    assert_eq!(appended.sequences, vec![0]);

    let stored = control
        .run_events_after(&scope, run_id, -1, 10)
        .await
        .expect("read");
    let RunEventPayload::TerminalOutput {
        command_id, chunk, ..
    } = &stored[0].payload
    else {
        panic!("unexpected payload {:?}", stored[0].payload);
    };
    assert_eq!(command_id, "cmd");
    assert_eq!(chunk.text, "lineone[0m\tdone\n");

    // An event that skipped normalization is refused before any row or
    // sequence is touched, so a bad batch cannot replay-loop on the database.
    let unnormalized = RunEvent {
        run_id,
        sequence: None,
        occurred_at: Utc::now(),
        runtime_cursor: Some("cursor-raw".to_owned()),
        artifact_ref: None,
        payload: RunEventPayload::TerminalOutput {
            command_id: "cmd".to_owned(),
            stream: TerminalStream::Stdout,
            chunk: BoundedText::raw("raw\0nul"),
        },
    };
    let refused = control
        .append_run_events(&scope, run_id, &[unnormalized])
        .await;
    assert!(
        matches!(
            refused,
            Err(ControlError::RunEvent(RunEventError::NulInPayload))
        ),
        "{refused:?}"
    );
    let count: i64 =
        sqlx::query("SELECT count(*) FROM run_events WHERE company_id = $1 AND run_id = $2")
            .bind(scope.company_id())
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .expect("count")
            .try_get(0)
            .expect("column");
    assert_eq!(count, 1);
}

#[tokio::test]
#[ignore = "requires Postgres"]
async fn succeeded_steps_cannot_be_regressed_by_a_stale_worker() {
    let (pool, control, scope) = setup().await;
    let manifest = disposable();
    let fakes = Fakes::creatable(&manifest);
    let saga = fakes.saga(&control);
    fakes.memory.set_unavailable(true);
    let operation = saga
        .begin(&scope, &request(&manifest, OperationMode::Create, false))
        .await
        .expect("begin");
    let SagaOutcome::Failed { step, .. } = saga.resume(&scope, operation.id).await.expect("resume")
    else {
        panic!("expected failure");
    };
    assert_eq!(step, ProvisioningStep::EnsureMemoryResources);
    let failed = control
        .load_operation(&scope, operation.id)
        .await
        .expect("load")
        .expect("exists");

    let mut regress = failed
        .step(ProvisioningStep::EnsureRuntimeProfile)
        .expect("step")
        .clone();
    regress.state = StepState::Running;
    regress.attempt_count += 1;
    regress.result = serde_json::json!({});
    let refused = control.record_step(&scope, operation.id, &regress).await;
    assert!(
        matches!(
            refused,
            Err(ControlError::Provisioning(
                ProvisioningError::Superseded { .. }
            ))
        ),
        "{refused:?}"
    );
    let states = step_states(&pool, &scope, &failed).await;
    assert_eq!(
        states[3],
        ("ensure_runtime_profile".to_owned(), "succeeded".to_owned())
    );
    let (attempts, receipt): (i32, serde_json::Value) = sqlx::query(
        "SELECT attempt_count, result FROM provisioning_operation_steps
          WHERE company_id = $1 AND operation_id = $2 AND step_name = 'ensure_runtime_profile'",
    )
    .bind(scope.company_id())
    .bind(operation.id)
    .fetch_one(&pool)
    .await
    .map(|row| {
        (
            row.try_get("attempt_count").expect("attempts"),
            row.try_get("result").expect("result"),
        )
    })
    .expect("row");
    assert_eq!(attempts, 1);
    assert_eq!(receipt["ownership"], serde_json::json!("created"));

    // The failed step itself still accepts a retry and the saga completes.
    fakes.memory.set_unavailable(false);
    let SagaOutcome::Succeeded(done) = saga.resume(&scope, operation.id).await.expect("resume")
    else {
        panic!("expected success");
    };
    assert!(done.result_revision_id.is_some());
    assert_eq!(fakes.runtime.created_profiles().len(), 1);
}
