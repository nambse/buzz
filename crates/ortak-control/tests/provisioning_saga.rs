//! Fake-backed tests for the provisioning saga: create vs adopt, compensation
//! safety for adopted resources, activation gating, and idempotent resume.

use std::collections::BTreeSet;
use std::sync::Mutex;

use ortak_control::adapter::{HealthReport, HealthState, ResourceOutcome};
use ortak_control::fakes::{
    all_runtime_capabilities, memory_resource_refs, FakeCredentialResolver, FakeMemoryAdapter,
    FakeOfficeIdentityAdapter, FakeRuntimeAdapter, InMemoryProvisioningRepository,
};
use ortak_control::memory::{MemoryCapabilities, MemoryHealthReport};
use ortak_control::office_identity::{OfficePublicKey, SignerVerification};
use ortak_control::ports::ProvisioningRepository;
use ortak_control::provisioning::{
    evaluate_activation_gates, GateEvidence, GateFailure, IdentityReservation, OperationMode,
    OperationStatus, OperationUpdate, ProvisioningError, ProvisioningOperation,
    ProvisioningRequest, ProvisioningSaga, ProvisioningStep, RevisionActivation, SagaConfig,
    SagaOutcome, StepRecord, StepState,
};
use ortak_control::runtime::{
    RuntimeAdapter, RuntimeCapabilities, RuntimeCapability, RuntimeResourceRequest,
};
use ortak_control::{CompanyScope, ControlError};
use ortak_domain::{CredentialRef, EmployeeId, EmployeeManifest, EmployeeStatus, ProvisioningMode};
use uuid::Uuid;

fn fixture(name: &str) -> EmployeeManifest {
    let yaml = std::fs::read_to_string(format!(
        "{}/../../config/employees/{name}.yaml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read fixture");
    serde_yaml::from_str(&yaml).expect("parse fixture")
}

/// A disposable `create` employee derived from the Zeynep fixture shape.
fn disposable() -> EmployeeManifest {
    let mut manifest = fixture("zeynep");
    manifest.provisioning = ProvisioningMode::Create;
    let employee = &mut manifest.employee;
    employee.id = EmployeeId::parse("ada").expect("id");
    employee.name = "Ada".to_owned();
    employee.title = "Platform Lead".to_owned();
    employee.aliases = vec!["platform".to_owned()];
    employee.runtime.profile_ref = None;
    employee.runtime.credential_refs =
        vec![CredentialRef::parse("credential://ortak-runtime/ada/codex-oauth").expect("ref")];
    if let Some(memory) = &mut employee.memory {
        memory.workspace = "ada-workspace".to_owned();
        memory.employee_peer = "ada".to_owned();
    }
    employee.office.public_key = "ab".repeat(32);
    employee.office.signer_ref =
        CredentialRef::parse("credential://ortak-runtime/ada/office-signing-key").expect("ref");
    manifest
}

/// A disposable `adopt` employee whose fake resources pre-exist, so adopt and
/// resume paths can activate without touching the Cem or Zeynep fixtures.
fn disposable_adopt() -> EmployeeManifest {
    let mut manifest = disposable();
    manifest.provisioning = ProvisioningMode::Adopt;
    manifest.employee.runtime.profile_ref = Some("fake://profiles/ada-adopted".to_owned());
    manifest
}

/// Complete, passing probe evidence for the fake adapters; `signer_matches`
/// false yields evidence the activation gate must refuse.
fn healthy_evidence(public_key_hex: &str, signer_matches: bool) -> GateEvidence {
    let key = OfficePublicKey::parse_hex(public_key_hex).expect("key");
    GateEvidence {
        runtime_capabilities: Some(RuntimeCapabilities {
            adapter: "fake-runtime".to_owned(),
            api_version: "fake/v0".to_owned(),
            capabilities: all_runtime_capabilities(),
        }),
        runtime_health: Some(HealthReport::healthy("ok")),
        memory_capabilities: Some(MemoryCapabilities {
            adapter: "fake-memory".to_owned(),
            api_version: "fake/v0".to_owned(),
            capabilities: ortak_control::fakes::all_memory_capabilities(),
        }),
        memory_health: Some(MemoryHealthReport {
            workspace: HealthReport::healthy("ok"),
            user_peer: HealthReport::healthy("ok"),
            employee_peer: HealthReport::healthy("ok"),
        }),
        office_membership: Some(HealthReport::healthy("member")),
        signer: Some(SignerVerification {
            produced_public_key: key,
            matches_expected: signer_matches,
        }),
    }
}

/// Marks every step before activation as succeeded with the given evidence,
/// leaving only `activate_revision` pending.
async fn mark_ready_for_activation(
    harness: &Harness,
    operation: &ProvisioningOperation,
    evidence: &GateEvidence,
) {
    for candidate in ProvisioningStep::ALL {
        if candidate == ProvisioningStep::ActivateRevision {
            continue;
        }
        let mut record = StepRecord::pending(operation.id, candidate);
        record.state = StepState::Succeeded;
        record.attempt_count = 1;
        record.finished_at = Some(chrono::Utc::now());
        if candidate == ProvisioningStep::ProbeHealth {
            record.result = serde_json::json!({ "evidence": evidence, "gates": "passed" });
        }
        harness
            .repo
            .record_step(&harness.repo.scope(), operation.id, &record)
            .await
            .expect("record step");
    }
}

/// The activation another worker would commit for `operation`.
fn committed_activation(manifest: &EmployeeManifest, operation_id: Uuid) -> RevisionActivation {
    let now = chrono::Utc::now();
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

struct Harness {
    repo: InMemoryProvisioningRepository,
    runtime: FakeRuntimeAdapter,
    memory: FakeMemoryAdapter,
    office: FakeOfficeIdentityAdapter,
    credentials: FakeCredentialResolver,
}

impl Harness {
    /// Fakes seeded so `manifest` can be adopted: profile, workspace/peers,
    /// signer, and membership all pre-exist and were not created by Ortak.
    fn adoptable(manifest: &EmployeeManifest) -> Self {
        let employee = &manifest.employee;
        let profile_ref = employee.runtime.profile_ref.clone().expect("adopt profile");
        let memory = employee.memory.clone().expect("memory binding");
        Self {
            repo: InMemoryProvisioningRepository::new(),
            runtime: FakeRuntimeAdapter::new().with_existing_profile(&profile_ref, true),
            memory: FakeMemoryAdapter::new().with_existing_binding(&memory),
            office: FakeOfficeIdentityAdapter::new()
                .with_signer(
                    employee.office.signer_ref.as_str(),
                    &employee.office.public_key,
                )
                .with_existing_member(&employee.office.public_key),
            credentials: Self::credentials_for(manifest),
        }
    }

    /// Fakes with nothing pre-existing except the signer and credentials.
    fn creatable(manifest: &EmployeeManifest) -> Self {
        let employee = &manifest.employee;
        Self {
            repo: InMemoryProvisioningRepository::new(),
            runtime: FakeRuntimeAdapter::new(),
            memory: FakeMemoryAdapter::new(),
            office: FakeOfficeIdentityAdapter::new().with_signer(
                employee.office.signer_ref.as_str(),
                &employee.office.public_key,
            ),
            credentials: Self::credentials_for(manifest),
        }
    }

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

    fn saga(
        &self,
    ) -> ProvisioningSaga<
        '_,
        InMemoryProvisioningRepository,
        FakeRuntimeAdapter,
        FakeMemoryAdapter,
        FakeOfficeIdentityAdapter,
        FakeCredentialResolver,
    > {
        ProvisioningSaga::new(
            &self.repo,
            &self.runtime,
            &self.memory,
            &self.office,
            &self.credentials,
            SagaConfig::default(),
        )
    }

    async fn begin(
        &self,
        manifest: &EmployeeManifest,
        mode: OperationMode,
        dry_run: bool,
        key: &str,
    ) -> ProvisioningOperation {
        self.saga()
            .begin(
                &self.repo.scope(),
                &ProvisioningRequest {
                    employee_id: manifest.employee.id.clone(),
                    mode,
                    dry_run,
                    idempotency_key: key.to_owned(),
                    manifest: manifest.clone(),
                },
            )
            .await
            .expect("begin operation")
    }

    async fn resume(&self, operation: &ProvisioningOperation) -> SagaOutcome {
        self.saga()
            .resume(&self.repo.scope(), operation.id)
            .await
            .expect("resume")
    }
}

fn step(operation: &ProvisioningOperation, step: ProvisioningStep) -> &StepRecord {
    operation.step(step).expect("step record")
}

fn failed(outcome: SagaOutcome) -> (ProvisioningOperation, ProvisioningStep, String) {
    match outcome {
        SagaOutcome::Failed {
            operation,
            step,
            error,
        } => (operation, step, error),
        other => panic!("expected failure, got {other:?}"),
    }
}

fn succeeded(outcome: SagaOutcome) -> ProvisioningOperation {
    match outcome {
        SagaOutcome::Succeeded(operation) => operation,
        other => panic!("expected success, got {other:?}"),
    }
}

#[tokio::test]
async fn cem_adopt_dry_run_verifies_everything_without_mutating_or_activating() {
    let manifest = fixture("cem");
    let harness = Harness::adoptable(&manifest);
    let operation = harness
        .begin(&manifest, OperationMode::Adopt, true, "cem-dry-run")
        .await;
    let operation = succeeded(harness.resume(&operation).await);

    assert_eq!(operation.status, OperationStatus::Succeeded);
    assert_eq!(operation.result_revision_id, None);
    let runtime = step(&operation, ProvisioningStep::EnsureRuntimeProfile);
    assert_eq!(runtime.state, StepState::Succeeded);
    assert!(
        runtime.adopted_existing,
        "adopt must attach to the existing profile"
    );
    assert_eq!(
        step(&operation, ProvisioningStep::EnsureMemoryResources).state,
        StepState::Succeeded
    );
    assert_eq!(
        step(&operation, ProvisioningStep::ProbeHealth).state,
        StepState::Succeeded
    );
    assert_eq!(
        step(&operation, ProvisioningStep::PublishOfficeProfile).state,
        StepState::Skipped
    );
    assert_eq!(
        step(&operation, ProvisioningStep::ActivateRevision).state,
        StepState::Skipped
    );

    // Nothing external was created or published, and Cem stays draft.
    assert!(harness.runtime.created_profiles().is_empty());
    assert!(harness.memory.created_resources().is_empty());
    assert!(harness.office.published_profiles().is_empty());
    assert_eq!(harness.repo.activations(), 0);
    assert_eq!(
        harness.repo.employee("cem").expect("row").status,
        EmployeeStatus::Draft
    );
}

#[tokio::test]
async fn adopt_never_creates_a_missing_profile() {
    let manifest = fixture("zeynep");
    let harness = Harness::adoptable(&manifest);
    // Replace the runtime with one that has no Zeynep profile.
    let harness = Harness {
        runtime: FakeRuntimeAdapter::new(),
        ..harness
    };
    let operation = harness
        .begin(&manifest, OperationMode::Adopt, false, "zeynep-missing")
        .await;
    let (operation, failed_step, error) = failed(harness.resume(&operation).await);

    assert_eq!(failed_step, ProvisioningStep::EnsureRuntimeProfile);
    assert!(error.contains("profile not found"), "{error}");
    assert_eq!(operation.status, OperationStatus::Failed);
    assert!(harness.runtime.created_profiles().is_empty());
    assert!(!harness.runtime.profile_exists("/opt/data/profiles/zeynep"));
    assert_eq!(harness.repo.activations(), 0);
}

#[tokio::test]
async fn signer_key_mismatch_fails_closed_and_resume_continues_from_that_step() {
    // Adopt/resume coverage on a disposable employee: the Zeynep fixture is an
    // adopted external resource and must never be activated by a test.
    let manifest = disposable_adopt();
    let profile_ref = manifest
        .employee
        .runtime
        .profile_ref
        .clone()
        .expect("adopt profile");
    let harness = Harness::adoptable(&manifest);
    let wrong_office = FakeOfficeIdentityAdapter::new()
        .with_signer(
            manifest.employee.office.signer_ref.as_str(),
            &"cd".repeat(32),
        )
        .with_existing_member(&manifest.employee.office.public_key);
    let harness = Harness {
        office: wrong_office,
        ..harness
    };
    let operation = harness
        .begin(&manifest, OperationMode::Adopt, false, "ada-signer")
        .await;
    let (operation, failed_step, error) = failed(harness.resume(&operation).await);
    assert_eq!(failed_step, ProvisioningStep::EnsureOfficeIdentity);
    assert!(error.contains("signer does not produce"), "{error}");
    assert_eq!(harness.repo.activations(), 0);

    // Fix the signer and resume: earlier steps are not re-executed.
    let harness = Harness {
        office: FakeOfficeIdentityAdapter::new()
            .with_signer(
                manifest.employee.office.signer_ref.as_str(),
                &manifest.employee.office.public_key,
            )
            .with_existing_member(&manifest.employee.office.public_key),
        ..harness
    };
    let operation = succeeded(harness.resume(&operation).await);
    assert_eq!(
        step(&operation, ProvisioningStep::EnsureRuntimeProfile).attempt_count,
        1
    );
    assert!(step(&operation, ProvisioningStep::EnsureRuntimeProfile).adopted_existing);
    assert_eq!(
        step(&operation, ProvisioningStep::EnsureOfficeIdentity).attempt_count,
        2
    );
    assert_eq!(operation.status, OperationStatus::Succeeded);
    assert!(operation.result_revision_id.is_some());
    assert_eq!(harness.repo.activations(), 1);
    assert_eq!(
        harness.repo.employee("ada").expect("row").status,
        EmployeeStatus::Active
    );
    // Adoption activated a revision but created nothing external.
    assert!(harness.runtime.created_profiles().is_empty());
    assert!(harness.runtime.profile_exists(&profile_ref));
    assert!(harness.memory.created_resources().is_empty());
}

#[tokio::test]
async fn create_provisions_a_disposable_employee_and_activates_it() {
    let manifest = disposable();
    let harness = Harness::creatable(&manifest);
    let operation = harness
        .begin(&manifest, OperationMode::Create, false, "ada-create")
        .await;
    let operation = succeeded(harness.resume(&operation).await);

    assert_eq!(
        harness.runtime.created_profiles(),
        vec!["fake://profiles/ada".to_owned()]
    );
    assert_eq!(harness.memory.created_resources().len(), 3);
    assert!(harness
        .office
        .is_member(&manifest.employee.office.public_key));
    assert_eq!(harness.office.published_profiles(), vec!["ada".to_owned()]);
    assert_eq!(harness.repo.activations(), 1);
    let row = harness.repo.employee("ada").expect("row");
    assert_eq!(row.status, EmployeeStatus::Active);
    assert_eq!(row.active_revision_id, operation.result_revision_id);
    let revision = harness
        .repo
        .revision(operation.result_revision_id.expect("revision"))
        .expect("stored revision");
    assert_eq!(revision.status, EmployeeStatus::Active);
    assert!(!step(&operation, ProvisioningStep::EnsureRuntimeProfile).adopted_existing);
}

#[tokio::test]
async fn resume_after_transient_failure_is_idempotent() {
    let manifest = disposable();
    let harness = Harness::creatable(&manifest);
    harness.memory.set_unavailable(true);
    let operation = harness
        .begin(&manifest, OperationMode::Create, false, "ada-resume")
        .await;
    let (operation, failed_step, _) = failed(harness.resume(&operation).await);
    assert_eq!(failed_step, ProvisioningStep::EnsureMemoryResources);
    assert_eq!(harness.runtime.created_profiles().len(), 1);

    harness.memory.set_unavailable(false);
    let operation = succeeded(harness.resume(&operation).await);
    assert_eq!(
        harness.runtime.created_profiles().len(),
        1,
        "the succeeded runtime step must not run again"
    );
    assert_eq!(
        step(&operation, ProvisioningStep::EnsureRuntimeProfile).attempt_count,
        1
    );
    assert_eq!(
        step(&operation, ProvisioningStep::EnsureMemoryResources).attempt_count,
        2
    );

    // A third resume is a no-op.
    let again = harness.resume(&operation).await;
    assert!(
        matches!(again, SagaOutcome::AlreadyTerminal(_)),
        "{again:?}"
    );
    assert_eq!(harness.repo.activations(), 1);
    assert_eq!(harness.office.published_profiles().len(), 1);
}

#[tokio::test]
async fn step_attempts_are_bounded() {
    let manifest = disposable();
    let harness = Harness::creatable(&manifest);
    harness.memory.set_unavailable(true);
    let operation = harness
        .begin(&manifest, OperationMode::Create, false, "ada-bounded")
        .await;
    for _ in 0..SagaConfig::default().max_step_attempts {
        let (_, failed_step, _) = failed(harness.resume(&operation).await);
        assert_eq!(failed_step, ProvisioningStep::EnsureMemoryResources);
    }
    let (operation, _, error) = failed(harness.resume(&operation).await);
    assert!(error.contains("exhausted"), "{error}");
    assert_eq!(
        step(&operation, ProvisioningStep::EnsureMemoryResources).attempt_count,
        SagaConfig::default().max_step_attempts
    );
}

#[tokio::test]
async fn begin_is_idempotent_by_key_and_conflicts_on_a_different_manifest() {
    let manifest = fixture("cem");
    let harness = Harness::adoptable(&manifest);
    let first = harness
        .begin(&manifest, OperationMode::Adopt, true, "cem-key")
        .await;
    let second = harness
        .begin(&manifest, OperationMode::Adopt, true, "cem-key")
        .await;
    assert_eq!(first.id, second.id);

    let mut changed = manifest.clone();
    changed.employee.biography = "changed".to_owned();
    let conflict = harness
        .saga()
        .begin(
            &harness.repo.scope(),
            &ProvisioningRequest {
                employee_id: changed.employee.id.clone(),
                mode: OperationMode::Adopt,
                dry_run: true,
                idempotency_key: "cem-key".to_owned(),
                manifest: changed,
            },
        )
        .await;
    assert!(
        matches!(
            conflict,
            Err(ControlError::Provisioning(
                ortak_control::provisioning::ProvisioningError::IdempotencyConflict { .. }
            ))
        ),
        "{conflict:?}"
    );
}

#[tokio::test]
async fn compensation_retains_every_adopted_resource_and_never_activates() {
    let manifest = fixture("cem");
    let harness = Harness::adoptable(&manifest);
    let memory = manifest.employee.memory.clone().expect("memory");
    let [workspace, user_peer, employee_peer] = memory_resource_refs(&memory);
    // Everything adopts, then the final probe finds the employee peer impaired.
    harness.memory.set_resource_health(&employee_peer, false);
    // Runtime health must still pass ValidateRuntimeProfile; memory health is
    // only evaluated by the ProbeHealth gate.
    let operation = harness
        .begin(&manifest, OperationMode::Adopt, false, "cem-compensate")
        .await;
    let (operation, failed_step, error) = failed(harness.resume(&operation).await);
    assert_eq!(failed_step, ProvisioningStep::ProbeHealth);
    assert!(error.contains("MemoryUnhealthy"), "{error}");
    assert_eq!(harness.repo.activations(), 0);

    let outcome = harness
        .saga()
        .compensate(&harness.repo.scope(), operation.id)
        .await
        .expect("compensate");
    let SagaOutcome::Compensated {
        operation,
        retained_adopted,
        deleted,
    } = outcome
    else {
        panic!("expected compensation, got {outcome:?}");
    };
    assert_eq!(operation.status, OperationStatus::Compensated);
    assert!(
        deleted.is_empty(),
        "adopted resources must never be deleted: {deleted:?}"
    );
    let retained: BTreeSet<String> = retained_adopted.into_iter().collect();
    let expected: BTreeSet<String> = [
        "/opt/data/profiles/cem".to_owned(),
        workspace,
        user_peer,
        employee_peer,
        format!("office-member://{}", manifest.employee.office.public_key),
    ]
    .into_iter()
    .collect();
    assert_eq!(retained, expected);

    // External state is untouched and Cem is still draft.
    assert!(harness.runtime.profile_exists("/opt/data/profiles/cem"));
    assert!(harness.runtime.deleted_profiles().is_empty());
    assert!(harness.memory.deleted_resources().is_empty());
    assert!(harness
        .office
        .is_member(&manifest.employee.office.public_key));
    assert!(harness.office.removed_memberships().is_empty());
    assert_eq!(harness.repo.activations(), 0);
    assert_eq!(
        harness.repo.employee("cem").expect("row").status,
        EmployeeStatus::Draft
    );
    assert_eq!(
        step(&operation, ProvisioningStep::EnsureRuntimeProfile).state,
        StepState::Compensated
    );
}

#[tokio::test]
async fn compensation_deletes_only_resources_created_by_the_operation() {
    let manifest = disposable();
    let harness = Harness::creatable(&manifest);
    // Signer unknown → EnsureOfficeIdentity fails after runtime and memory
    // resources were created.
    let harness = Harness {
        office: FakeOfficeIdentityAdapter::new(),
        ..harness
    };
    let operation = harness
        .begin(&manifest, OperationMode::Create, false, "ada-compensate")
        .await;
    let (operation, failed_step, _) = failed(harness.resume(&operation).await);
    assert_eq!(failed_step, ProvisioningStep::EnsureOfficeIdentity);
    assert!(harness.runtime.profile_exists("fake://profiles/ada"));

    let outcome = harness
        .saga()
        .compensate(&harness.repo.scope(), operation.id)
        .await
        .expect("compensate");
    let SagaOutcome::Compensated {
        retained_adopted,
        deleted,
        ..
    } = outcome
    else {
        panic!("expected compensation, got {outcome:?}");
    };
    assert!(retained_adopted.is_empty());
    assert_eq!(deleted.len(), 4, "{deleted:?}");
    assert!(!harness.runtime.profile_exists("fake://profiles/ada"));
    assert_eq!(harness.memory.deleted_resources().len(), 3);
    assert_eq!(harness.repo.activations(), 0);
}

#[tokio::test]
async fn activation_reevaluates_gates_from_durable_probe_evidence() {
    let manifest = disposable();
    let harness = Harness::creatable(&manifest);
    let operation = harness
        .begin(&manifest, OperationMode::Create, false, "ada-tampered")
        .await;
    // Mark every step before activation as done, but store probe evidence
    // whose signer proof does not match. The activation path must refuse.
    for candidate in ProvisioningStep::ALL {
        if candidate == ProvisioningStep::ActivateRevision {
            continue;
        }
        let mut record = StepRecord::pending(operation.id, candidate);
        record.state = StepState::Succeeded;
        record.attempt_count = 1;
        record.finished_at = Some(chrono::Utc::now());
        if candidate == ProvisioningStep::ProbeHealth {
            let key = OfficePublicKey::parse_hex(&"ab".repeat(32)).expect("key");
            let evidence = GateEvidence {
                runtime_capabilities: Some(RuntimeCapabilities {
                    adapter: "fake-runtime".to_owned(),
                    api_version: "fake/v0".to_owned(),
                    capabilities: all_runtime_capabilities(),
                }),
                runtime_health: Some(HealthReport::healthy("ok")),
                memory_capabilities: Some(MemoryCapabilities {
                    adapter: "fake-memory".to_owned(),
                    api_version: "fake/v0".to_owned(),
                    capabilities: ortak_control::fakes::all_memory_capabilities(),
                }),
                memory_health: Some(MemoryHealthReport {
                    workspace: HealthReport::healthy("ok"),
                    user_peer: HealthReport::healthy("ok"),
                    employee_peer: HealthReport::healthy("ok"),
                }),
                office_membership: Some(HealthReport::healthy("member")),
                signer: Some(SignerVerification {
                    produced_public_key: key,
                    matches_expected: false,
                }),
            };
            record.result = serde_json::json!({ "evidence": evidence, "gates": "passed" });
        }
        harness
            .repo
            .record_step(&harness.repo.scope(), operation.id, &record)
            .await
            .expect("record step");
    }

    let (operation, failed_step, error) = failed(harness.resume(&operation).await);
    assert_eq!(failed_step, ProvisioningStep::ActivateRevision);
    assert!(error.contains("SignerKeyMismatch"), "{error}");
    assert_eq!(harness.repo.activations(), 0);
    assert_eq!(operation.result_revision_id, None);
    assert_eq!(
        harness.repo.employee("ada").expect("row").status,
        EmployeeStatus::Draft
    );
}

/// A runtime that reports `created` for adopt requests: a port-contract
/// violation the saga must refuse rather than record as adopted.
struct LyingRuntime(FakeRuntimeAdapter);

impl RuntimeAdapter for LyingRuntime {
    fn adapter_name(&self) -> &str {
        self.0.adapter_name()
    }
    async fn probe_capabilities(
        &self,
    ) -> Result<RuntimeCapabilities, ortak_control::runtime::RuntimeError> {
        self.0.probe_capabilities().await
    }
    async fn health(
        &self,
        binding: &ortak_domain::RuntimeBinding,
    ) -> Result<HealthReport, ortak_control::runtime::RuntimeError> {
        self.0.health(binding).await
    }
    async fn ensure_profile(
        &self,
        request: &RuntimeResourceRequest,
    ) -> Result<ResourceOutcome, ortak_control::runtime::RuntimeError> {
        let outcome = self.0.ensure_profile(request).await?;
        Ok(ResourceOutcome::created(outcome.resource_ref))
    }
    async fn delete_created_profile(
        &self,
        resource_ref: &str,
        idempotency_key: &str,
    ) -> Result<(), ortak_control::runtime::RuntimeError> {
        self.0
            .delete_created_profile(resource_ref, idempotency_key)
            .await
    }
    async fn start_run(
        &self,
        spec: &ortak_control::runtime::RunSpec,
    ) -> Result<ortak_control::runtime::RunStartReceipt, ortak_control::runtime::RuntimeError> {
        self.0.start_run(spec).await
    }
    async fn next_events(
        &self,
        runtime_run_ref: &ortak_control::runtime::RuntimeRunRef,
        after: Option<&ortak_control::runtime::RuntimeCursor>,
        limit: usize,
    ) -> Result<ortak_control::runtime::RuntimeEventBatch, ortak_control::runtime::RuntimeError>
    {
        self.0.next_events(runtime_run_ref, after, limit).await
    }
    async fn cancel_run(
        &self,
        runtime_run_ref: &ortak_control::runtime::RuntimeRunRef,
        reason: &str,
    ) -> Result<ortak_control::runtime::CancelOutcome, ortak_control::runtime::RuntimeError> {
        self.0.cancel_run(runtime_run_ref, reason).await
    }
}

#[tokio::test]
async fn adopt_refuses_an_adapter_that_reports_a_created_resource() {
    let manifest = fixture("cem");
    let harness = Harness::adoptable(&manifest);
    let lying = LyingRuntime(
        FakeRuntimeAdapter::new().with_existing_profile("/opt/data/profiles/cem", true),
    );
    let saga = ProvisioningSaga::new(
        &harness.repo,
        &lying,
        &harness.memory,
        &harness.office,
        &harness.credentials,
        SagaConfig::default(),
    );
    let operation = harness
        .begin(&manifest, OperationMode::Adopt, false, "cem-lying")
        .await;
    let outcome = saga
        .resume(&harness.repo.scope(), operation.id)
        .await
        .expect("resume");
    let (operation, failed_step, error) = failed(outcome);
    assert_eq!(failed_step, ProvisioningStep::EnsureRuntimeProfile);
    assert!(error.contains("contract violation"), "{error}");
    assert_eq!(
        step(&operation, ProvisioningStep::EnsureRuntimeProfile).state,
        StepState::Failed
    );
    assert_eq!(harness.repo.activations(), 0);
}

#[test]
fn activation_gate_requires_every_gate() {
    let key = OfficePublicKey::parse_hex(&"ab".repeat(32)).expect("key");
    let healthy = GateEvidence {
        runtime_capabilities: Some(RuntimeCapabilities {
            adapter: "fake-runtime".to_owned(),
            api_version: "fake/v0".to_owned(),
            capabilities: all_runtime_capabilities(),
        }),
        runtime_health: Some(HealthReport::healthy("ok")),
        memory_capabilities: Some(MemoryCapabilities {
            adapter: "fake-memory".to_owned(),
            api_version: "fake/v0".to_owned(),
            capabilities: ortak_control::fakes::all_memory_capabilities(),
        }),
        memory_health: Some(MemoryHealthReport {
            workspace: HealthReport::healthy("ok"),
            user_peer: HealthReport::healthy("ok"),
            employee_peer: HealthReport::healthy("ok"),
        }),
        office_membership: Some(HealthReport::healthy("member")),
        signer: Some(SignerVerification {
            produced_public_key: key,
            matches_expected: true,
        }),
    };
    assert_eq!(evaluate_activation_gates(&healthy, true), Ok(()));

    let mut missing_cancel = healthy.clone();
    if let Some(capabilities) = &mut missing_cancel.runtime_capabilities {
        capabilities
            .capabilities
            .remove(&RuntimeCapability::RunCancel);
    }
    assert_eq!(
        evaluate_activation_gates(&missing_cancel, true),
        Err(vec![GateFailure::RuntimeMissingCapabilities {
            missing: vec![RuntimeCapability::RunCancel]
        }])
    );

    let mut degraded = healthy.clone();
    degraded.runtime_health = Some(HealthReport::degraded("config invalid"));
    assert_eq!(
        evaluate_activation_gates(&degraded, true),
        Err(vec![GateFailure::RuntimeUnhealthy {
            state: HealthState::Degraded
        }])
    );

    let mut peer_down = healthy.clone();
    if let Some(memory) = &mut peer_down.memory_health {
        memory.employee_peer = HealthReport::unhealthy("missing");
    }
    assert!(matches!(
        evaluate_activation_gates(&peer_down, true)
            .unwrap_err()
            .as_slice(),
        [GateFailure::MemoryUnhealthy { .. }]
    ));

    let mut not_member = healthy.clone();
    not_member.office_membership = Some(HealthReport::unhealthy("not a member"));
    assert!(matches!(
        evaluate_activation_gates(&not_member, true)
            .unwrap_err()
            .as_slice(),
        [GateFailure::OfficeMembershipUnhealthy { .. }]
    ));

    let mut mismatch = healthy.clone();
    mismatch.signer = Some(SignerVerification {
        produced_public_key: key,
        matches_expected: false,
    });
    assert_eq!(
        evaluate_activation_gates(&mismatch, true),
        Err(vec![GateFailure::SignerKeyMismatch])
    );

    let mut no_memory = healthy.clone();
    no_memory.memory_capabilities = None;
    no_memory.memory_health = None;
    assert_eq!(
        evaluate_activation_gates(&no_memory, true),
        Err(vec![GateFailure::MemoryNotProbed])
    );
    assert_eq!(evaluate_activation_gates(&no_memory, false), Ok(()));

    let empty = GateEvidence::default();
    let failures = evaluate_activation_gates(&empty, true).unwrap_err();
    assert_eq!(failures.len(), 4, "{failures:?}");
}

#[test]
fn fixtures_are_adopt_only_and_draft() {
    for name in ["cem", "zeynep"] {
        let manifest = fixture(name);
        assert_eq!(manifest.provisioning, ProvisioningMode::Adopt);
        assert_eq!(manifest.employee.status, EmployeeStatus::Draft);
        let request = ProvisioningRequest {
            employee_id: manifest.employee.id.clone(),
            mode: OperationMode::Create,
            dry_run: true,
            idempotency_key: format!("{name}-create"),
            manifest: manifest.clone(),
        };
        assert!(
            request.validate().is_err(),
            "an adopt fixture must not be accepted by a create operation"
        );
    }
}

#[test]
fn durable_transition_fences_never_regress_finished_state() {
    use OperationStatus as O;
    use StepState as S;
    for status in [O::Succeeded, O::Compensated] {
        for next in [
            O::Pending,
            O::Running,
            O::Failed,
            O::Compensating,
            O::Succeeded,
        ] {
            assert!(!status.can_transition_to(next), "{status:?} -> {next:?}");
        }
    }
    assert!(!O::Compensating.can_transition_to(O::Running));
    assert!(!O::Compensating.can_transition_to(O::Failed));
    assert!(O::Compensating.can_transition_to(O::Compensated));
    assert!(!O::Running.can_transition_to(O::Compensating));
    assert!(O::Failed.can_transition_to(O::Compensating));
    assert!(O::Failed.can_transition_to(O::Running));
    assert!(O::Running.can_transition_to(O::Succeeded));

    for done in [S::Succeeded, S::Skipped, S::Compensated] {
        for next in [S::Pending, S::Running, S::Failed] {
            assert!(!done.can_transition_to(next), "{done:?} -> {next:?}");
        }
    }
    assert!(S::Succeeded.can_transition_to(S::Compensating));
    assert!(!S::Compensating.can_transition_to(S::Succeeded));
    assert!(S::Compensating.can_transition_to(S::Compensated));
    assert!(!S::Compensated.can_transition_to(S::Compensating));
    assert!(S::Running.can_transition_to(S::Succeeded));
    assert!(S::Failed.can_transition_to(S::Running));
}

/// Repository wrapper that lets another worker commit the activation between
/// this worker's load and its first durable write.
struct RacingRepository<'a> {
    inner: &'a InMemoryProvisioningRepository,
    pending_activation: Mutex<Option<RevisionActivation>>,
}

impl ProvisioningRepository for RacingRepository<'_> {
    async fn begin_operation(
        &self,
        scope: &CompanyScope,
        request: &ProvisioningRequest,
    ) -> ortak_control::Result<ProvisioningOperation> {
        self.inner.begin_operation(scope, request).await
    }
    async fn load_operation(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
    ) -> ortak_control::Result<Option<ProvisioningOperation>> {
        self.inner.load_operation(scope, operation_id).await
    }
    async fn update_operation(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        update: &OperationUpdate,
    ) -> ortak_control::Result<()> {
        let pending = self
            .pending_activation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(activation) = pending {
            self.inner
                .activate_revision(scope, operation_id, &activation)
                .await?;
        }
        self.inner
            .update_operation(scope, operation_id, update)
            .await
    }
    async fn record_step(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        step: &StepRecord,
    ) -> ortak_control::Result<()> {
        self.inner.record_step(scope, operation_id, step).await
    }
    async fn reserve_employee_identity(
        &self,
        scope: &CompanyScope,
        employee_id: &EmployeeId,
    ) -> ortak_control::Result<IdentityReservation> {
        self.inner
            .reserve_employee_identity(scope, employee_id)
            .await
    }
    async fn activate_revision(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        activation: &RevisionActivation,
    ) -> ortak_control::Result<Uuid> {
        self.inner
            .activate_revision(scope, operation_id, activation)
            .await
    }
}

#[tokio::test]
async fn stale_worker_converges_on_a_committed_activation() {
    let manifest = disposable();
    let harness = Harness::creatable(&manifest);
    let operation = harness
        .begin(&manifest, OperationMode::Create, false, "ada-race")
        .await;
    mark_ready_for_activation(
        &harness,
        &operation,
        &healthy_evidence(&manifest.employee.office.public_key, true),
    )
    .await;

    // Worker B loads the operation, then worker A commits the activation
    // before B's first write (marking the operation running).
    let racing = RacingRepository {
        inner: &harness.repo,
        pending_activation: Mutex::new(Some(committed_activation(&manifest, operation.id))),
    };
    let stale_worker = ProvisioningSaga::new(
        &racing,
        &harness.runtime,
        &harness.memory,
        &harness.office,
        &harness.credentials,
        SagaConfig::default(),
    );
    let outcome = stale_worker
        .resume(&harness.repo.scope(), operation.id)
        .await
        .expect("stale resume converges instead of failing");
    let converged = succeeded(outcome);
    assert_eq!(converged.status, OperationStatus::Succeeded);
    assert!(converged.result_revision_id.is_some());
    assert_eq!(harness.repo.activations(), 1, "exactly one activation");

    // The durable row still carries worker A's activation, unregressed.
    let stored = harness
        .repo
        .load_operation(&harness.repo.scope(), operation.id)
        .await
        .expect("load")
        .expect("exists");
    assert_eq!(stored.status, OperationStatus::Succeeded);
    assert_eq!(stored.result_revision_id, converged.result_revision_id);
    assert_eq!(stored.current_step, None);
    let activation = step(&stored, ProvisioningStep::ActivateRevision);
    assert_eq!(activation.state, StepState::Succeeded);
    assert_eq!(activation.attempt_count, 1, "B must not bump A's step");
    assert_eq!(
        harness
            .repo
            .employee("ada")
            .expect("row")
            .active_revision_id,
        converged.result_revision_id
    );
}

#[tokio::test]
async fn replayed_writes_never_regress_a_succeeded_operation() {
    let manifest = disposable();
    let harness = Harness::creatable(&manifest);
    let operation = harness
        .begin(&manifest, OperationMode::Create, false, "ada-replay")
        .await;
    let done = succeeded(harness.resume(&operation).await);
    let revision_id = done.result_revision_id.expect("activated");
    let scope = harness.repo.scope();

    // A replayed worker tries to mark the activation step running again.
    let mut stale = step(&done, ProvisioningStep::ActivateRevision).clone();
    stale.state = StepState::Running;
    stale.attempt_count += 1;
    let refused = harness.repo.record_step(&scope, operation.id, &stale).await;
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
        let refused = harness
            .repo
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

    let stored = harness
        .repo
        .load_operation(&scope, operation.id)
        .await
        .expect("load")
        .expect("exists");
    assert_eq!(stored.status, OperationStatus::Succeeded);
    assert_eq!(stored.result_revision_id, Some(revision_id));
    assert_eq!(stored.error_message, None);
    assert_eq!(
        step(&stored, ProvisioningStep::ActivateRevision).state,
        StepState::Succeeded
    );
    assert_eq!(
        step(&stored, ProvisioningStep::ActivateRevision).attempt_count,
        1
    );

    // Compensation refuses an operation that activated a revision.
    let refused = harness.saga().compensate(&scope, operation.id).await;
    assert!(
        matches!(
            refused,
            Err(ControlError::Provisioning(
                ProvisioningError::InvalidTransition { .. }
            ))
        ),
        "{refused:?}"
    );
    assert!(harness.runtime.deleted_profiles().is_empty());
    assert!(harness.memory.deleted_resources().is_empty());
    assert!(harness.office.removed_memberships().is_empty());
    assert_eq!(
        harness.repo.employee("ada").expect("row").status,
        EmployeeStatus::Active
    );
    assert!(matches!(
        harness.resume(&operation).await,
        SagaOutcome::AlreadyTerminal(_)
    ));
}

/// Fails a create operation at `ensure_office_identity` after runtime and
/// memory resources were created, then starts compensation with memory
/// unavailable so the memory step is left `compensating`.
async fn interrupted_compensation(key: &str) -> (Harness, ProvisioningOperation) {
    let manifest = disposable();
    let harness = Harness::creatable(&manifest);
    let harness = Harness {
        office: FakeOfficeIdentityAdapter::new(),
        ..harness
    };
    let operation = harness
        .begin(&manifest, OperationMode::Create, false, key)
        .await;
    let (operation, failed_step, _) = failed(harness.resume(&operation).await);
    assert_eq!(failed_step, ProvisioningStep::EnsureOfficeIdentity);

    harness.memory.set_unavailable(true);
    let outcome = harness
        .saga()
        .compensate(&harness.repo.scope(), operation.id)
        .await
        .expect("compensate call");
    let (operation, failed_step, error) = failed(outcome);
    assert_eq!(failed_step, ProvisioningStep::EnsureMemoryResources);
    assert_eq!(operation.status, OperationStatus::Compensating);
    let memory_step = step(&operation, ProvisioningStep::EnsureMemoryResources);
    assert_eq!(
        memory_step.state,
        StepState::Compensating,
        "a failed deletion must not return the step to succeeded"
    );
    assert_eq!(memory_step.error_message.as_deref(), Some(error.as_str()));
    (harness, operation)
}

#[tokio::test]
async fn compensation_retry_resumes_steps_left_compensating() {
    let (harness, operation) = interrupted_compensation("ada-compensate-retry").await;
    assert!(harness.runtime.profile_exists("fake://profiles/ada"));
    assert!(harness.memory.deleted_resources().is_empty());

    harness.memory.set_unavailable(false);
    let outcome = harness
        .saga()
        .compensate(&harness.repo.scope(), operation.id)
        .await
        .expect("compensate again");
    let SagaOutcome::Compensated {
        operation,
        retained_adopted,
        deleted,
    } = outcome
    else {
        panic!("expected compensation, got {outcome:?}");
    };
    assert!(retained_adopted.is_empty());
    assert_eq!(
        deleted.len(),
        4,
        "memory peers, workspace, and profile: {deleted:?}"
    );
    assert_eq!(operation.status, OperationStatus::Compensated);
    assert_eq!(
        step(&operation, ProvisioningStep::EnsureMemoryResources).state,
        StepState::Compensated
    );
    assert_eq!(
        step(&operation, ProvisioningStep::EnsureRuntimeProfile).state,
        StepState::Compensated
    );
    assert_eq!(harness.memory.deleted_resources().len(), 3);
    assert!(!harness.runtime.profile_exists("fake://profiles/ada"));
    assert_eq!(harness.repo.activations(), 0);
}

#[tokio::test]
async fn compensating_operations_cannot_be_activated_or_resumed() {
    let (harness, operation) = interrupted_compensation("ada-compensating-activate").await;
    let scope = harness.repo.scope();
    let manifest = disposable();

    let refused = harness
        .repo
        .activate_revision(
            &scope,
            operation.id,
            &committed_activation(&manifest, operation.id),
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
    let refused = harness.saga().resume(&scope, operation.id).await;
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
    // A stale worker's status write cannot turn compensation back into a run.
    let refused = harness
        .repo
        .update_operation(
            &scope,
            operation.id,
            &OperationUpdate {
                status: OperationStatus::Running,
                current_step: Some(ProvisioningStep::EnsureOfficeIdentity),
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
    assert_eq!(harness.repo.activations(), 0);
    assert_eq!(
        harness.repo.employee("ada").expect("row").status,
        EmployeeStatus::Draft
    );
}

#[tokio::test]
async fn begin_requires_matching_mode_and_dry_run_for_the_same_key() {
    let manifest = fixture("cem");
    let harness = Harness::adoptable(&manifest);
    let first = harness
        .begin(&manifest, OperationMode::Adopt, true, "cem-mode-key")
        .await;
    let same = harness
        .begin(&manifest, OperationMode::Adopt, true, "cem-mode-key")
        .await;
    assert_eq!(first.id, same.id);

    for (mode, dry_run) in [(OperationMode::Adopt, false), (OperationMode::Update, true)] {
        let conflict = harness
            .saga()
            .begin(
                &harness.repo.scope(),
                &ProvisioningRequest {
                    employee_id: manifest.employee.id.clone(),
                    mode,
                    dry_run,
                    idempotency_key: "cem-mode-key".to_owned(),
                    manifest: manifest.clone(),
                },
            )
            .await;
        assert!(
            matches!(
                conflict,
                Err(ControlError::Provisioning(
                    ProvisioningError::IdempotencyConflict { .. }
                ))
            ),
            "{mode:?} dry_run={dry_run}: {conflict:?}"
        );
    }
    // The dry-run row was never turned into a live operation.
    let stored = harness
        .repo
        .load_operation(&harness.repo.scope(), first.id)
        .await
        .expect("load")
        .expect("exists");
    assert!(stored.dry_run);
    assert_eq!(stored.mode, OperationMode::Adopt);
    assert_eq!(harness.repo.activations(), 0);
}

#[tokio::test]
async fn succeeded_steps_cannot_be_regressed_by_a_stale_worker() {
    let manifest = disposable();
    let harness = Harness::creatable(&manifest);
    harness.memory.set_unavailable(true);
    let operation = harness
        .begin(&manifest, OperationMode::Create, false, "ada-step-fence")
        .await;
    let (operation, failed_step, _) = failed(harness.resume(&operation).await);
    assert_eq!(failed_step, ProvisioningStep::EnsureMemoryResources);
    let scope = harness.repo.scope();

    // A stale worker re-running the already succeeded runtime step.
    let mut regress = step(&operation, ProvisioningStep::EnsureRuntimeProfile).clone();
    regress.state = StepState::Running;
    regress.attempt_count += 1;
    regress.result = serde_json::json!({});
    let refused = harness
        .repo
        .record_step(&scope, operation.id, &regress)
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
    let stored = harness
        .repo
        .load_operation(&scope, operation.id)
        .await
        .expect("load")
        .expect("exists");
    let runtime = step(&stored, ProvisioningStep::EnsureRuntimeProfile);
    assert_eq!(runtime.state, StepState::Succeeded);
    assert_eq!(runtime.attempt_count, 1);
    assert_eq!(
        runtime.result["resource_ref"],
        serde_json::json!("fake://profiles/ada"),
        "the receipt survives"
    );

    // The failed step is still retryable, and the operation completes.
    harness.memory.set_unavailable(false);
    let done = succeeded(harness.resume(&operation).await);
    assert!(done.result_revision_id.is_some());
    assert_eq!(harness.runtime.created_profiles().len(), 1);

    // After an interrupted compensation, a compensating step never goes back
    // to succeeded either.
    let (harness, operation) = interrupted_compensation("ada-step-fence-compensating").await;
    let mut revert = step(&operation, ProvisioningStep::EnsureMemoryResources).clone();
    revert.state = StepState::Succeeded;
    revert.error_message = None;
    let refused = harness
        .repo
        .record_step(&harness.repo.scope(), operation.id, &revert)
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
}
