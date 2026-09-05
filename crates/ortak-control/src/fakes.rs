//! In-memory fake adapters and repository for tests and dry-run tooling.
//!
//! None of these talk to a real Hermes, Honcho, Office, credential manager,
//! or database. They implement the port contracts strictly: adopt never
//! creates or modifies, create never overwrites, and deletion refuses any
//! resource not created through the fake. Tests use their inspection methods
//! to prove adopted resources survive compensation.

mod semantic;

/// Isolated semantic adapter fixture driven through the real routing service.
pub use semantic::SemanticScoringFixture;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use ortak_domain::{
    CredentialRef, Employee, EmployeeId, EmployeeStatus, MemoryBinding, OfficeBinding,
    ProvisioningMode, RuntimeBinding,
};
use uuid::Uuid;

use crate::adapter::{Detail, HealthReport, ResourceOutcome};
use crate::credentials::{CredentialError, CredentialReferenceStatus, CredentialResolver};
use crate::error::{ControlError, Result};
use crate::ids::CompanyScope;
use crate::memory::{
    MemoryAdapter, MemoryCapabilities, MemoryCapability, MemoryError, MemoryHealthReport,
    MemoryRecall, MemoryRecallRequest, MemoryRecord, MemoryResourceOutcome, MemoryResourceRequest,
    MemoryWriteReceipt, MemoryWriteRequest,
};
use crate::office_identity::{
    OfficeIdentityAdapter, OfficeIdentityError, OfficeMembershipRequest, OfficePublicKey,
    ProfilePublication, SignerVerification,
};
use crate::ports::ProvisioningRepository;
use crate::provisioning::{
    IdentityReservation, OperationStatus, OperationUpdate, ProvisioningError,
    ProvisioningOperation, ProvisioningRequest, ProvisioningStep, RevisionActivation, StepRecord,
    StepState,
};
use crate::run_event::RunEventPayload;
use crate::runtime::{
    CancelOutcome, CancelStartReceipt, RunSpec, RunStartReceipt, RuntimeAdapter,
    RuntimeCapabilities, RuntimeCapability, RuntimeCursor, RuntimeError, RuntimeEvent,
    RuntimeEventBatch, RuntimeResourceRequest, RuntimeRunRef,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ── Runtime ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct FakeProfile {
    healthy: bool,
    created_by_ortak: bool,
}

#[derive(Debug)]
struct FakeRun {
    events: Vec<RuntimeEvent>,
    terminal: bool,
}

#[derive(Debug, Default)]
struct RuntimeState {
    capabilities: BTreeSet<RuntimeCapability>,
    unavailable: bool,
    profiles: BTreeMap<String, FakeProfile>,
    unresolvable_credentials: BTreeSet<String>,
    ensure_receipts: BTreeMap<String, ResourceOutcome>,
    created: Vec<String>,
    deleted: Vec<String>,
    runs: BTreeMap<String, FakeRun>,
    start_receipts: BTreeMap<String, RunStartReceipt>,
    start_specs: Vec<RunSpec>,
    cancelled_starts: BTreeSet<String>,
    next_run: u64,
}

/// In-memory runtime adapter.
#[derive(Debug, Default)]
pub struct FakeRuntimeAdapter {
    state: Mutex<RuntimeState>,
}

/// Every runtime capability.
pub fn all_runtime_capabilities() -> BTreeSet<RuntimeCapability> {
    [
        RuntimeCapability::HealthProbe,
        RuntimeCapability::ProfileInspect,
        RuntimeCapability::ProfileCreate,
        RuntimeCapability::ProfileDelete,
        RuntimeCapability::RunStart,
        RuntimeCapability::RunEvents,
        RuntimeCapability::RunCancel,
        RuntimeCapability::RunLookup,
        RuntimeCapability::RunCancelStart,
    ]
    .into_iter()
    .collect()
}

impl FakeRuntimeAdapter {
    /// A fully capable, available runtime with no profiles.
    pub fn new() -> Self {
        let adapter = Self::default();
        lock(&adapter.state).capabilities = all_runtime_capabilities();
        adapter
    }

    /// Seeds a pre-existing (adoptable) profile that Ortak did not create.
    pub fn with_existing_profile(self, profile_ref: &str, healthy: bool) -> Self {
        lock(&self.state).profiles.insert(
            profile_ref.to_owned(),
            FakeProfile {
                healthy,
                created_by_ortak: false,
            },
        );
        self
    }

    /// Restricts the probed capability set.
    pub fn with_capabilities(self, capabilities: BTreeSet<RuntimeCapability>) -> Self {
        lock(&self.state).capabilities = capabilities;
        self
    }

    /// Marks a credential reference as unknown to the runtime's resolver.
    pub fn with_unresolvable_credential(self, credential_ref: &str) -> Self {
        lock(&self.state)
            .unresolvable_credentials
            .insert(credential_ref.to_owned());
        self
    }

    /// Makes every call fail with `Unavailable` until cleared.
    pub fn set_unavailable(&self, unavailable: bool) {
        lock(&self.state).unavailable = unavailable;
    }

    /// Changes the health of a profile.
    pub fn set_profile_health(&self, profile_ref: &str, healthy: bool) {
        if let Some(profile) = lock(&self.state).profiles.get_mut(profile_ref) {
            profile.healthy = healthy;
        }
    }

    /// True when the profile exists.
    pub fn profile_exists(&self, profile_ref: &str) -> bool {
        lock(&self.state).profiles.contains_key(profile_ref)
    }

    /// Profiles created through this adapter, in order.
    pub fn created_profiles(&self) -> Vec<String> {
        lock(&self.state).created.clone()
    }

    /// Profiles deleted through this adapter, in order.
    pub fn deleted_profiles(&self) -> Vec<String> {
        lock(&self.state).deleted.clone()
    }

    /// Specifications received by `start_run`, including retries and refusals,
    /// captured before validation so tests can detect every runtime call.
    pub fn start_specs(&self) -> Vec<RunSpec> {
        lock(&self.state).start_specs.clone()
    }

    /// Appends an event to a started run's stream.
    pub fn push_event(&self, runtime_run_ref: &RuntimeRunRef, payload: RunEventPayload) {
        let mut state = lock(&self.state);
        if let Some(run) = state.runs.get_mut(&runtime_run_ref.0) {
            let index = run.events.len();
            let terminal = payload.event_type().is_terminal();
            run.events.push(RuntimeEvent {
                cursor: RuntimeCursor(format!("{}:{index}", runtime_run_ref.0)),
                occurred_at: Utc::now(),
                payload,
            });
            run.terminal |= terminal;
        }
    }

    fn guard(state: &RuntimeState) -> std::result::Result<(), RuntimeError> {
        if state.unavailable {
            Err(RuntimeError::Unavailable {
                detail: Detail::new("fake runtime is offline"),
            })
        } else {
            Ok(())
        }
    }

    fn profile_ref(binding: &RuntimeBinding, employee_id: &EmployeeId) -> String {
        binding
            .profile_ref
            .clone()
            .unwrap_or_else(|| format!("fake://profiles/{employee_id}"))
    }
}

impl RuntimeAdapter for FakeRuntimeAdapter {
    fn adapter_name(&self) -> &str {
        "fake-runtime"
    }

    async fn probe_capabilities(&self) -> std::result::Result<RuntimeCapabilities, RuntimeError> {
        let state = lock(&self.state);
        Self::guard(&state)?;
        Ok(RuntimeCapabilities {
            adapter: "fake-runtime".to_owned(),
            api_version: "fake/v0".to_owned(),
            capabilities: state.capabilities.clone(),
        })
    }

    async fn health(
        &self,
        binding: &RuntimeBinding,
    ) -> std::result::Result<HealthReport, RuntimeError> {
        let state = lock(&self.state);
        Self::guard(&state)?;
        let profile_ref = binding
            .profile_ref
            .clone()
            .unwrap_or_else(|| "fake://profiles/unbound".to_owned());
        let Some(profile) = state.profiles.get(&profile_ref) else {
            return Ok(HealthReport::unhealthy("profile missing"));
        };
        if let Some(reference) = binding
            .credential_refs
            .iter()
            .find(|reference| state.unresolvable_credentials.contains(reference.as_str()))
        {
            return Ok(HealthReport::unhealthy(format!(
                "credential reference unresolvable: {}",
                reference.as_str()
            )));
        }
        Ok(if profile.healthy {
            HealthReport::healthy("profile, config, and workspace valid")
        } else {
            HealthReport::degraded("profile config invalid")
        })
    }

    async fn ensure_profile(
        &self,
        request: &RuntimeResourceRequest,
    ) -> std::result::Result<ResourceOutcome, RuntimeError> {
        let mut state = lock(&self.state);
        Self::guard(&state)?;
        if let Some(receipt) = state.ensure_receipts.get(&request.idempotency_key) {
            return Ok(receipt.clone());
        }
        let profile_ref = Self::profile_ref(&request.binding, &request.employee_id);
        let exists = state.profiles.contains_key(&profile_ref);
        let outcome = match request.mode {
            ProvisioningMode::Adopt => {
                if !exists {
                    return Err(RuntimeError::ProfileNotFound { profile_ref });
                }
                if !state
                    .capabilities
                    .contains(&RuntimeCapability::ProfileInspect)
                {
                    return Err(RuntimeError::Unsupported {
                        capability: RuntimeCapability::ProfileInspect,
                    });
                }
                ResourceOutcome::adopted(profile_ref)
            }
            ProvisioningMode::Create => {
                if exists {
                    return Err(RuntimeError::ProfileExists { profile_ref });
                }
                if !state
                    .capabilities
                    .contains(&RuntimeCapability::ProfileCreate)
                {
                    return Err(RuntimeError::Unsupported {
                        capability: RuntimeCapability::ProfileCreate,
                    });
                }
                state.profiles.insert(
                    profile_ref.clone(),
                    FakeProfile {
                        healthy: true,
                        created_by_ortak: true,
                    },
                );
                state.created.push(profile_ref.clone());
                ResourceOutcome::created(profile_ref)
            }
        };
        state
            .ensure_receipts
            .insert(request.idempotency_key.clone(), outcome.clone());
        Ok(outcome)
    }

    async fn delete_created_profile(
        &self,
        resource_ref: &str,
        _idempotency_key: &str,
    ) -> std::result::Result<(), RuntimeError> {
        let mut state = lock(&self.state);
        Self::guard(&state)?;
        match state.profiles.get(resource_ref) {
            None => Ok(()),
            Some(profile) if !profile.created_by_ortak => Err(RuntimeError::Rejected {
                detail: Detail::new("refusing to delete a profile Ortak did not create"),
            }),
            Some(_) => {
                state.profiles.remove(resource_ref);
                state.deleted.push(resource_ref.to_owned());
                Ok(())
            }
        }
    }

    async fn start_run(
        &self,
        spec: &RunSpec,
    ) -> std::result::Result<RunStartReceipt, RuntimeError> {
        let mut state = lock(&self.state);
        state.start_specs.push(spec.clone());
        spec.validate()?;
        Self::guard(&state)?;
        if let Some(receipt) = state.start_receipts.get(&spec.idempotency_key) {
            return Ok(receipt.clone());
        }
        if state.cancelled_starts.contains(&spec.idempotency_key) {
            return Err(RuntimeError::InvalidSpec {
                detail: Detail::new("start key was cancelled"),
            });
        }
        let profile_ref = Self::profile_ref(&spec.binding, &spec.employee_id);
        if !state.profiles.contains_key(&profile_ref) {
            return Err(RuntimeError::ProfileNotFound { profile_ref });
        }
        state.next_run += 1;
        let runtime_run_ref = RuntimeRunRef(format!("fake-run-{}", state.next_run));
        let receipt = RunStartReceipt {
            runtime_run_ref: runtime_run_ref.clone(),
            started_at: Utc::now(),
        };
        state.runs.insert(
            runtime_run_ref.0.clone(),
            FakeRun {
                events: vec![RuntimeEvent {
                    cursor: RuntimeCursor(format!("{}:0", runtime_run_ref.0)),
                    occurred_at: receipt.started_at,
                    payload: RunEventPayload::RunStarted {
                        runtime_run_ref: runtime_run_ref.0.clone(),
                    },
                }],
                terminal: false,
            },
        );
        state
            .start_receipts
            .insert(spec.idempotency_key.clone(), receipt.clone());
        Ok(receipt)
    }

    async fn lookup_start(
        &self,
        idempotency_key: &str,
    ) -> std::result::Result<Option<RunStartReceipt>, RuntimeError> {
        let state = lock(&self.state);
        Self::guard(&state)?;
        Ok(state.start_receipts.get(idempotency_key).cloned())
    }

    async fn cancel_start(
        &self,
        idempotency_key: &str,
        reason: &str,
    ) -> std::result::Result<CancelStartReceipt, RuntimeError> {
        let receipt = {
            let mut state = lock(&self.state);
            Self::guard(&state)?;
            state.cancelled_starts.insert(idempotency_key.to_owned());
            state.start_receipts.get(idempotency_key).cloned()
        };
        match receipt {
            Some(receipt) => {
                let outcome = self.cancel_run(&receipt.runtime_run_ref, reason).await?;
                Ok(CancelStartReceipt {
                    runtime_run_ref: Some(receipt.runtime_run_ref),
                    outcome,
                })
            }
            None => Ok(CancelStartReceipt {
                runtime_run_ref: None,
                outcome: CancelOutcome::Cancelled,
            }),
        }
    }

    async fn next_events(
        &self,
        runtime_run_ref: &RuntimeRunRef,
        after: Option<&RuntimeCursor>,
        limit: usize,
    ) -> std::result::Result<RuntimeEventBatch, RuntimeError> {
        let state = lock(&self.state);
        Self::guard(&state)?;
        let run = state
            .runs
            .get(&runtime_run_ref.0)
            .ok_or_else(|| RuntimeError::UnknownRun {
                runtime_run_ref: runtime_run_ref.clone(),
            })?;
        let start = match after {
            None => 0,
            Some(cursor) => run
                .events
                .iter()
                .position(|event| &event.cursor == cursor)
                .map(|index| index + 1)
                .unwrap_or(0),
        };
        let events = run
            .events
            .iter()
            .skip(start)
            .take(limit.max(1))
            .cloned()
            .collect::<Vec<_>>();
        let exhausted = start + events.len() >= run.events.len();
        Ok(RuntimeEventBatch {
            events,
            terminal: run.terminal && exhausted,
        })
    }

    async fn cancel_run(
        &self,
        runtime_run_ref: &RuntimeRunRef,
        reason: &str,
    ) -> std::result::Result<CancelOutcome, RuntimeError> {
        let mut state = lock(&self.state);
        Self::guard(&state)?;
        let run =
            state
                .runs
                .get_mut(&runtime_run_ref.0)
                .ok_or_else(|| RuntimeError::UnknownRun {
                    runtime_run_ref: runtime_run_ref.clone(),
                })?;
        if run.terminal {
            return Ok(CancelOutcome::AlreadyTerminal);
        }
        let index = run.events.len();
        run.events.push(RuntimeEvent {
            cursor: RuntimeCursor(format!("{}:{index}", runtime_run_ref.0)),
            occurred_at: Utc::now(),
            payload: RunEventPayload::RunCancelled {
                reason: crate::run_event::BoundedText::raw(reason),
            },
        });
        run.terminal = true;
        Ok(CancelOutcome::Cancelled)
    }
}

// ── Memory ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct FakeResource {
    healthy: bool,
    created_by_ortak: bool,
}

#[derive(Debug, Default)]
struct MemoryState {
    capabilities: BTreeSet<MemoryCapability>,
    unavailable: bool,
    resources: BTreeMap<String, FakeResource>,
    ensure_receipts: BTreeMap<String, MemoryResourceOutcome>,
    write_receipts: BTreeMap<String, MemoryWriteReceipt>,
    records: Vec<(String, MemoryRecord)>,
    created: Vec<String>,
    deleted: Vec<String>,
    next_record: u64,
}

/// In-memory memory adapter.
#[derive(Debug, Default)]
pub struct FakeMemoryAdapter {
    state: Mutex<MemoryState>,
}

/// Every memory capability.
pub fn all_memory_capabilities() -> BTreeSet<MemoryCapability> {
    [
        MemoryCapability::HealthProbe,
        MemoryCapability::ResourceInspect,
        MemoryCapability::ResourceCreate,
        MemoryCapability::ResourceDelete,
        MemoryCapability::Recall,
        MemoryCapability::Remember,
    ]
    .into_iter()
    .collect()
}

/// Resource references the fake uses for a binding: workspace, user peer,
/// employee peer.
pub fn memory_resource_refs(binding: &MemoryBinding) -> [String; 3] {
    let workspace = format!("{}/workspaces/{}", binding.endpoint_ref, binding.workspace);
    [
        workspace.clone(),
        format!("{workspace}/peers/{}", binding.user_peer),
        format!("{workspace}/peers/{}", binding.employee_peer),
    ]
}

impl FakeMemoryAdapter {
    /// A fully capable, available service with no resources.
    pub fn new() -> Self {
        let adapter = Self::default();
        lock(&adapter.state).capabilities = all_memory_capabilities();
        adapter
    }

    /// Seeds the workspace and both peers of a binding as pre-existing resources.
    pub fn with_existing_binding(self, binding: &MemoryBinding) -> Self {
        {
            let mut state = lock(&self.state);
            for reference in memory_resource_refs(binding) {
                state.resources.insert(
                    reference,
                    FakeResource {
                        healthy: true,
                        created_by_ortak: false,
                    },
                );
            }
        }
        self
    }

    /// Restricts the probed capability set.
    pub fn with_capabilities(self, capabilities: BTreeSet<MemoryCapability>) -> Self {
        lock(&self.state).capabilities = capabilities;
        self
    }

    /// Makes every call fail with `Unavailable` until cleared.
    pub fn set_unavailable(&self, unavailable: bool) {
        lock(&self.state).unavailable = unavailable;
    }

    /// Changes the health of one resource.
    pub fn set_resource_health(&self, resource_ref: &str, healthy: bool) {
        if let Some(resource) = lock(&self.state).resources.get_mut(resource_ref) {
            resource.healthy = healthy;
        }
    }

    /// True when the resource exists.
    pub fn resource_exists(&self, resource_ref: &str) -> bool {
        lock(&self.state).resources.contains_key(resource_ref)
    }

    /// Resources created through this adapter.
    pub fn created_resources(&self) -> Vec<String> {
        lock(&self.state).created.clone()
    }

    /// Resources deleted through this adapter.
    pub fn deleted_resources(&self) -> Vec<String> {
        lock(&self.state).deleted.clone()
    }

    fn guard(state: &MemoryState) -> std::result::Result<(), MemoryError> {
        if state.unavailable {
            Err(MemoryError::Unavailable {
                detail: Detail::new("fake memory service is offline"),
            })
        } else {
            Ok(())
        }
    }

    fn resource_health(state: &MemoryState, reference: &str) -> HealthReport {
        match state.resources.get(reference) {
            None => HealthReport::unhealthy("resource missing"),
            Some(resource) if resource.healthy => HealthReport::healthy("resource reachable"),
            Some(_) => HealthReport::degraded("resource impaired"),
        }
    }

    fn scope_key(binding: &MemoryBinding, scope: &crate::memory::MemoryScope) -> String {
        format!(
            "{}/{}/{}",
            binding.endpoint_ref,
            binding.workspace,
            serde_json::to_string(scope).unwrap_or_default()
        )
    }
}

impl MemoryAdapter for FakeMemoryAdapter {
    fn adapter_name(&self) -> &str {
        "fake-memory"
    }

    async fn probe_capabilities(
        &self,
        _binding: &MemoryBinding,
    ) -> std::result::Result<MemoryCapabilities, MemoryError> {
        let state = lock(&self.state);
        Self::guard(&state)?;
        Ok(MemoryCapabilities {
            adapter: "fake-memory".to_owned(),
            api_version: "fake/v0".to_owned(),
            capabilities: state.capabilities.clone(),
        })
    }

    async fn health(
        &self,
        binding: &MemoryBinding,
    ) -> std::result::Result<MemoryHealthReport, MemoryError> {
        let state = lock(&self.state);
        Self::guard(&state)?;
        let [workspace, user_peer, employee_peer] = memory_resource_refs(binding);
        Ok(MemoryHealthReport {
            workspace: Self::resource_health(&state, &workspace),
            user_peer: Self::resource_health(&state, &user_peer),
            employee_peer: Self::resource_health(&state, &employee_peer),
        })
    }

    async fn ensure_resources(
        &self,
        request: &MemoryResourceRequest,
    ) -> std::result::Result<MemoryResourceOutcome, MemoryError> {
        let mut state = lock(&self.state);
        Self::guard(&state)?;
        if let Some(receipt) = state.ensure_receipts.get(&request.idempotency_key) {
            return Ok(receipt.clone());
        }
        let references = memory_resource_refs(&request.binding);
        let mut outcomes = Vec::with_capacity(3);
        match request.mode {
            ProvisioningMode::Adopt => {
                for reference in &references {
                    if !state.resources.contains_key(reference) {
                        return Err(MemoryError::ResourceNotFound {
                            resource_ref: reference.clone(),
                        });
                    }
                    outcomes.push(ResourceOutcome::adopted(reference.clone()));
                }
            }
            ProvisioningMode::Create => {
                if let Some(existing) = references
                    .iter()
                    .find(|reference| state.resources.contains_key(*reference))
                {
                    return Err(MemoryError::ResourceExists {
                        resource_ref: existing.clone(),
                    });
                }
                for reference in &references {
                    state.resources.insert(
                        reference.clone(),
                        FakeResource {
                            healthy: true,
                            created_by_ortak: true,
                        },
                    );
                    state.created.push(reference.clone());
                    outcomes.push(ResourceOutcome::created(reference.clone()));
                }
            }
        }
        let [workspace, user_peer, employee_peer]: [ResourceOutcome; 3] =
            outcomes.try_into().map_err(|_| MemoryError::Rejected {
                detail: Detail::new("fake produced the wrong resource count"),
            })?;
        let outcome = MemoryResourceOutcome {
            workspace,
            user_peer,
            employee_peer,
        };
        state
            .ensure_receipts
            .insert(request.idempotency_key.clone(), outcome.clone());
        Ok(outcome)
    }

    async fn delete_created_resource(
        &self,
        resource_ref: &str,
        _idempotency_key: &str,
    ) -> std::result::Result<(), MemoryError> {
        let mut state = lock(&self.state);
        Self::guard(&state)?;
        match state.resources.get(resource_ref) {
            None => Ok(()),
            Some(resource) if !resource.created_by_ortak => Err(MemoryError::Rejected {
                detail: Detail::new("refusing to delete a resource Ortak did not create"),
            }),
            Some(_) => {
                state.resources.remove(resource_ref);
                state.deleted.push(resource_ref.to_owned());
                Ok(())
            }
        }
    }

    async fn recall(
        &self,
        request: &MemoryRecallRequest,
    ) -> std::result::Result<MemoryRecall, MemoryError> {
        request.validate()?;
        let state = lock(&self.state);
        Self::guard(&state)?;
        let key = Self::scope_key(&request.binding, &request.scope);
        let mut records = Vec::new();
        let mut bytes = 0usize;
        let mut truncated = false;
        for (_, record) in state.records.iter().filter(|(scope, _)| *scope == key) {
            if records.len() >= request.budget.max_records
                || bytes + record.content.len() > request.budget.max_bytes
            {
                truncated = true;
                break;
            }
            bytes += record.content.len();
            records.push(record.clone());
        }
        Ok(MemoryRecall { records, truncated })
    }

    async fn remember(
        &self,
        request: &MemoryWriteRequest,
    ) -> std::result::Result<MemoryWriteReceipt, MemoryError> {
        request.validate()?;
        let mut state = lock(&self.state);
        Self::guard(&state)?;
        if let Some(receipt) = state.write_receipts.get(&request.idempotency_key) {
            return Ok(receipt.clone());
        }
        let key = Self::scope_key(&request.binding, &request.scope);
        for fact in &request.facts {
            state.next_record += 1;
            let record = MemoryRecord {
                record_ref: format!("{key}/records/{}", state.next_record),
                scope: request.scope.clone(),
                content: fact.content.clone(),
                provenance: fact.provenance.clone(),
            };
            state.records.push((key.clone(), record));
        }
        let receipt = MemoryWriteReceipt {
            receipt_ref: format!("fake-write:{}", request.idempotency_key),
            written: request.facts.len(),
        };
        state
            .write_receipts
            .insert(request.idempotency_key.clone(), receipt.clone());
        Ok(receipt)
    }
}

// ── Office identity ──────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct FakeMember {
    created_by_ortak: bool,
}

#[derive(Debug, Default)]
struct OfficeState {
    unavailable: bool,
    signers: BTreeMap<String, OfficePublicKey>,
    members: BTreeMap<String, FakeMember>,
    ensure_receipts: BTreeMap<String, ResourceOutcome>,
    publications: BTreeMap<String, ProfilePublication>,
    published: Vec<String>,
    removed: Vec<String>,
}

/// In-memory Office identity adapter.
#[derive(Debug, Default)]
pub struct FakeOfficeIdentityAdapter {
    state: Mutex<OfficeState>,
}

impl FakeOfficeIdentityAdapter {
    /// An available Office with no signers or members.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers which public key a signer reference produces.
    pub fn with_signer(self, signer_ref: &str, public_key_hex: &str) -> Self {
        if let Ok(key) = OfficePublicKey::parse_hex(public_key_hex) {
            lock(&self.state).signers.insert(signer_ref.to_owned(), key);
        }
        self
    }

    /// Seeds a pre-existing Office member that Ortak did not create.
    pub fn with_existing_member(self, public_key_hex: &str) -> Self {
        lock(&self.state).members.insert(
            public_key_hex.to_ascii_lowercase(),
            FakeMember {
                created_by_ortak: false,
            },
        );
        self
    }

    /// Makes every call fail with `Unavailable` until cleared.
    pub fn set_unavailable(&self, unavailable: bool) {
        lock(&self.state).unavailable = unavailable;
    }

    /// True when the key is a member.
    pub fn is_member(&self, public_key_hex: &str) -> bool {
        lock(&self.state)
            .members
            .contains_key(&public_key_hex.to_ascii_lowercase())
    }

    /// Employee ids whose profile was published, in order.
    pub fn published_profiles(&self) -> Vec<String> {
        lock(&self.state).published.clone()
    }

    /// Memberships removed through this adapter.
    pub fn removed_memberships(&self) -> Vec<String> {
        lock(&self.state).removed.clone()
    }

    fn guard(state: &OfficeState) -> std::result::Result<(), OfficeIdentityError> {
        if state.unavailable {
            Err(OfficeIdentityError::Unavailable {
                detail: Detail::new("fake office is offline"),
            })
        } else {
            Ok(())
        }
    }

    fn member_ref(public_key: &OfficePublicKey) -> String {
        format!("office-member://{}", public_key.to_hex())
    }
}

impl OfficeIdentityAdapter for FakeOfficeIdentityAdapter {
    async fn verify_signer(
        &self,
        signer_ref: &CredentialRef,
        expected: &OfficePublicKey,
    ) -> std::result::Result<SignerVerification, OfficeIdentityError> {
        let state = lock(&self.state);
        Self::guard(&state)?;
        let produced = state
            .signers
            .get(signer_ref.as_str())
            .copied()
            .ok_or_else(|| OfficeIdentityError::signer(signer_ref))?;
        Ok(SignerVerification {
            produced_public_key: produced,
            matches_expected: &produced == expected,
        })
    }

    async fn ensure_membership(
        &self,
        request: &OfficeMembershipRequest,
    ) -> std::result::Result<ResourceOutcome, OfficeIdentityError> {
        let mut state = lock(&self.state);
        Self::guard(&state)?;
        if let Some(receipt) = state.ensure_receipts.get(&request.idempotency_key) {
            return Ok(receipt.clone());
        }
        let key = OfficePublicKey::parse_hex(&request.binding.public_key)?;
        let hex = key.to_hex();
        let exists = state.members.contains_key(&hex);
        let outcome = match request.mode {
            ProvisioningMode::Adopt => {
                if !exists {
                    return Err(OfficeIdentityError::MemberNotFound { public_key: hex });
                }
                ResourceOutcome::adopted(Self::member_ref(&key))
            }
            ProvisioningMode::Create => {
                if exists {
                    return Err(OfficeIdentityError::MemberExists { public_key: hex });
                }
                state.members.insert(
                    hex,
                    FakeMember {
                        created_by_ortak: true,
                    },
                );
                ResourceOutcome::created(Self::member_ref(&key))
            }
        };
        state
            .ensure_receipts
            .insert(request.idempotency_key.clone(), outcome.clone());
        Ok(outcome)
    }

    async fn remove_created_membership(
        &self,
        resource_ref: &str,
        _idempotency_key: &str,
    ) -> std::result::Result<(), OfficeIdentityError> {
        let mut state = lock(&self.state);
        Self::guard(&state)?;
        let hex = resource_ref
            .strip_prefix("office-member://")
            .unwrap_or(resource_ref)
            .to_ascii_lowercase();
        match state.members.get(&hex) {
            None => Ok(()),
            Some(member) if !member.created_by_ortak => Err(OfficeIdentityError::Rejected {
                detail: Detail::new("refusing to remove a member Ortak did not create"),
            }),
            Some(_) => {
                state.members.remove(&hex);
                state.removed.push(resource_ref.to_owned());
                Ok(())
            }
        }
    }

    async fn membership_health(
        &self,
        public_key: &OfficePublicKey,
    ) -> std::result::Result<HealthReport, OfficeIdentityError> {
        let state = lock(&self.state);
        Self::guard(&state)?;
        Ok(if state.members.contains_key(&public_key.to_hex()) {
            HealthReport::healthy("member")
        } else {
            HealthReport::unhealthy("not a member")
        })
    }

    async fn publish_profile(
        &self,
        employee_id: &EmployeeId,
        _binding: &OfficeBinding,
        _display_name: &str,
        idempotency_key: &str,
    ) -> std::result::Result<ProfilePublication, OfficeIdentityError> {
        let mut state = lock(&self.state);
        Self::guard(&state)?;
        if let Some(publication) = state.publications.get(idempotency_key) {
            return Ok(publication.clone());
        }
        let publication = ProfilePublication {
            receipt_ref: format!("fake-profile-event:{idempotency_key}"),
        };
        state.published.push(employee_id.to_string());
        state
            .publications
            .insert(idempotency_key.to_owned(), publication.clone());
        Ok(publication)
    }
}

// ── Credentials ──────────────────────────────────────────────────────────────

/// In-memory credential manager that knows references, never values.
#[derive(Debug, Default)]
pub struct FakeCredentialResolver {
    known: Mutex<BTreeSet<String>>,
    unavailable: Mutex<bool>,
}

impl FakeCredentialResolver {
    /// A manager with no references.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers references as resolvable.
    pub fn with_references<I, S>(self, references: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        lock(&self.known).extend(references.into_iter().map(Into::into));
        self
    }

    /// Makes every call fail with `Unavailable` until cleared.
    pub fn set_unavailable(&self, unavailable: bool) {
        *lock(&self.unavailable) = unavailable;
    }
}

impl CredentialResolver for FakeCredentialResolver {
    async fn verify_reference(
        &self,
        credential_ref: &CredentialRef,
    ) -> std::result::Result<CredentialReferenceStatus, CredentialError> {
        if *lock(&self.unavailable) {
            return Err(CredentialError::Unavailable {
                detail: Detail::new("fake credential manager is offline"),
            });
        }
        Ok(if lock(&self.known).contains(credential_ref.as_str()) {
            CredentialReferenceStatus::Resolvable
        } else {
            CredentialReferenceStatus::Missing
        })
    }
}

// ── Provisioning repository ──────────────────────────────────────────────────

/// Employee row as the fake repository sees it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FakeEmployeeRow {
    /// Lifecycle status.
    pub status: EmployeeStatus,
    /// Active revision, if any.
    pub active_revision_id: Option<Uuid>,
}

#[derive(Debug, Default)]
struct RepositoryState {
    operations: BTreeMap<Uuid, ProvisioningOperation>,
    by_key: BTreeMap<(Uuid, String), Uuid>,
    employees: BTreeMap<(Uuid, String), FakeEmployeeRow>,
    revisions: BTreeMap<Uuid, Employee>,
    activations: u32,
}

/// In-memory provisioning repository mirroring the 0045 constraints that
/// matter to the saga: idempotent begin, upserted steps, draft identity
/// reservation, atomic activation, and company-unique aliases.
#[derive(Debug)]
pub struct InMemoryProvisioningRepository {
    scope: CompanyScope,
    state: Mutex<RepositoryState>,
}

impl Default for InMemoryProvisioningRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryProvisioningRepository {
    /// A repository for one fresh company.
    pub fn new() -> Self {
        Self {
            scope: CompanyScope::new(Uuid::new_v4(), None),
            state: Mutex::default(),
        }
    }

    /// The company scope this repository serves.
    pub fn scope(&self) -> CompanyScope {
        self.scope.clone()
    }

    /// Seeds an employee row.
    pub fn with_employee(self, employee_id: &str, status: EmployeeStatus) -> Self {
        lock(&self.state).employees.insert(
            (self.scope.company_id(), employee_id.to_owned()),
            FakeEmployeeRow {
                status,
                active_revision_id: None,
            },
        );
        self
    }

    /// Reads an employee row.
    pub fn employee(&self, employee_id: &str) -> Option<FakeEmployeeRow> {
        lock(&self.state)
            .employees
            .get(&(self.scope.company_id(), employee_id.to_owned()))
            .cloned()
    }

    /// Number of activation transactions committed.
    pub fn activations(&self) -> u32 {
        lock(&self.state).activations
    }

    /// Reads a persisted revision manifest.
    pub fn revision(&self, revision_id: Uuid) -> Option<Employee> {
        lock(&self.state).revisions.get(&revision_id).cloned()
    }

    fn check_scope(&self, scope: &CompanyScope) -> Result<()> {
        if scope.company_id() == self.scope.company_id() {
            Ok(())
        } else {
            Err(ControlError::InvalidData(
                "fake repository serves a different company".to_owned(),
            ))
        }
    }
}

impl ProvisioningRepository for InMemoryProvisioningRepository {
    async fn begin_operation(
        &self,
        scope: &CompanyScope,
        request: &ProvisioningRequest,
    ) -> Result<ProvisioningOperation> {
        self.check_scope(scope)?;
        let fingerprint = request.fingerprint()?;
        let mut state = lock(&self.state);
        let key = (scope.company_id(), request.idempotency_key.clone());
        if let Some(existing_id) = state.by_key.get(&key).copied() {
            let existing =
                state.operations.get(&existing_id).cloned().ok_or_else(|| {
                    ControlError::InvalidData("dangling idempotency key".to_owned())
                })?;
            if existing.manifest_fingerprint != fingerprint
                || existing.mode != request.mode
                || existing.dry_run != request.dry_run
            {
                return Err(ProvisioningError::IdempotencyConflict {
                    operation_id: existing_id,
                }
                .into());
            }
            return Ok(existing);
        }
        // The operation row references the employee row, so identity is
        // reserved as a draft here (mirrors the 0045 foreign key).
        state
            .employees
            .entry((scope.company_id(), request.employee_id.to_string()))
            .or_insert(FakeEmployeeRow {
                status: EmployeeStatus::Draft,
                active_revision_id: None,
            });
        let id = Uuid::new_v4();
        let now = Utc::now();
        let operation = ProvisioningOperation {
            id,
            employee_id: request.employee_id.clone(),
            mode: request.mode,
            dry_run: request.dry_run,
            idempotency_key: request.idempotency_key.clone(),
            manifest: request.manifest.clone(),
            manifest_fingerprint: fingerprint,
            status: OperationStatus::Pending,
            current_step: None,
            result_revision_id: None,
            error_message: None,
            steps: ProvisioningStep::ALL
                .iter()
                .map(|step| StepRecord::pending(id, *step))
                .collect(),
            created_at: now,
            updated_at: now,
            finished_at: None,
        };
        state.by_key.insert(key, id);
        state.operations.insert(id, operation.clone());
        Ok(operation)
    }

    async fn load_operation(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
    ) -> Result<Option<ProvisioningOperation>> {
        self.check_scope(scope)?;
        Ok(lock(&self.state).operations.get(&operation_id).cloned())
    }

    async fn update_operation(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        update: &OperationUpdate,
    ) -> Result<()> {
        self.check_scope(scope)?;
        let mut state = lock(&self.state);
        let operation = state
            .operations
            .get_mut(&operation_id)
            .ok_or(ProvisioningError::UnknownOperation { operation_id })?;
        if operation.result_revision_id.is_some() {
            return Err(ProvisioningError::Superseded {
                operation_id,
                detail: "operation already activated a revision",
            }
            .into());
        }
        if !operation.status.can_transition_to(update.status) {
            return Err(ProvisioningError::Superseded {
                operation_id,
                detail: "operation status does not allow this update",
            }
            .into());
        }
        operation.status = update.status;
        operation.current_step = update.current_step;
        operation.error_message = update.error_message.clone();
        operation.updated_at = Utc::now();
        operation.finished_at = update.is_finished().then(Utc::now);
        Ok(())
    }

    async fn record_step(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        step: &StepRecord,
    ) -> Result<()> {
        self.check_scope(scope)?;
        let mut state = lock(&self.state);
        let operation = state
            .operations
            .get_mut(&operation_id)
            .ok_or(ProvisioningError::UnknownOperation { operation_id })?;
        if operation.result_revision_id.is_some() || operation.status.is_terminal() {
            return Err(ProvisioningError::Superseded {
                operation_id,
                detail: "operation is terminal; step writes are refused",
            }
            .into());
        }
        if let Some(existing) = operation
            .steps
            .iter_mut()
            .find(|record| record.step == step.step)
        {
            if !existing.state.can_transition_to(step.state) {
                return Err(ProvisioningError::Superseded {
                    operation_id,
                    detail: "step state does not allow this write",
                }
                .into());
            }
            *existing = step.clone();
        }
        operation.updated_at = Utc::now();
        Ok(())
    }

    async fn reserve_employee_identity(
        &self,
        scope: &CompanyScope,
        employee_id: &EmployeeId,
    ) -> Result<IdentityReservation> {
        self.check_scope(scope)?;
        let mut state = lock(&self.state);
        let key = (scope.company_id(), employee_id.to_string());
        Ok(match state.employees.get(&key) {
            Some(row) => IdentityReservation::Existing { status: row.status },
            None => {
                state.employees.insert(
                    key,
                    FakeEmployeeRow {
                        status: EmployeeStatus::Draft,
                        active_revision_id: None,
                    },
                );
                IdentityReservation::Created
            }
        })
    }

    async fn activate_revision(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        activation: &RevisionActivation,
    ) -> Result<Uuid> {
        self.check_scope(scope)?;
        let mut state = lock(&self.state);
        let operation = state
            .operations
            .get(&operation_id)
            .ok_or(ProvisioningError::UnknownOperation { operation_id })?;
        if operation.dry_run {
            return Err(ProvisioningError::InvalidTransition {
                status: operation.status,
                action: "activate a dry-run",
            }
            .into());
        }
        if let Some(existing) = operation.result_revision_id {
            // Idempotent replay of a committed activation.
            return Ok(existing);
        }
        if operation.status.is_terminal() || operation.status == OperationStatus::Compensating {
            return Err(ProvisioningError::InvalidTransition {
                status: operation.status,
                action: "activate",
            }
            .into());
        }
        let employee_key = (scope.company_id(), activation.employee.id.to_string());
        if !state.employees.contains_key(&employee_key) {
            return Err(ControlError::InvalidData(
                "activation requires a reserved employee row".to_owned(),
            ));
        }
        // Company-unique aliases (mirrors the employee_aliases primary key).
        let new_aliases = activation.employee.normalized_aliases();
        for (other_key, row) in &state.employees {
            if other_key == &employee_key {
                continue;
            }
            let Some(revision_id) = row.active_revision_id else {
                continue;
            };
            if let Some(other) = state.revisions.get(&revision_id) {
                if let Some(alias) = other.normalized_aliases().intersection(&new_aliases).next() {
                    return Err(ControlError::InvalidData(format!(
                        "alias {alias:?} already belongs to {}",
                        other.id
                    )));
                }
            }
        }

        let revision_id = Uuid::new_v4();
        state
            .revisions
            .insert(revision_id, activation.employee.clone());
        if let Some(row) = state.employees.get_mut(&employee_key) {
            row.status = EmployeeStatus::Active;
            row.active_revision_id = Some(revision_id);
        }
        let now = Utc::now();
        if let Some(operation) = state.operations.get_mut(&operation_id) {
            if let Some(record) = operation
                .steps
                .iter_mut()
                .find(|record| record.step == ProvisioningStep::ActivateRevision)
            {
                *record = activation.activation_step.clone();
                record.state = StepState::Succeeded;
                record.finished_at.get_or_insert(now);
            }
            operation.status = OperationStatus::Succeeded;
            operation.result_revision_id = Some(revision_id);
            operation.current_step = None;
            operation.error_message = None;
            operation.updated_at = now;
            operation.finished_at = Some(now);
        }
        state.activations += 1;
        Ok(revision_id)
    }
}
