//! Employee provisioning saga (Architecture v0 §6).
//!
//! The saga is a resumable state machine over `provisioning_operations` and
//! `provisioning_operation_steps`. Every step records its state, attempt
//! count, idempotency key, secret-free receipt, and whether it attached to a
//! pre-existing (adopted) resource. Retry resumes at the first step that is
//! not yet `succeeded`/`skipped`.
//!
//! Safety guarantees enforced in this module, independent of any adapter:
//!
//! - Adopted resources are never created, deleted, replaced, or activated by
//!   compensation. Compensation only deletes resources whose receipt says
//!   `ownership = created`; adopted ones are recorded as retained.
//! - An adapter that returns a `created` resource for an `adopt` request
//!   violates the port contract; the step fails closed.
//! - Activation is the last step and runs only when the pure gate
//!   ([`evaluate_activation_gates`]) accepts the probe evidence: runtime
//!   capabilities and health, memory capabilities and health, Office
//!   membership, and signer/public-key correspondence.
//! - Dry runs never publish, never activate, and never create external
//!   resources; adopt steps still run because adopt is read-only.
//! - Durable writes are fenced ([`OperationStatus::can_transition_to`],
//!   [`StepState::can_transition_to`], and `result_revision_id`): a
//!   concurrent or replayed resume can never regress a succeeded operation
//!   or step, compensation refuses any operation that activated a revision,
//!   and a stale worker that finds the activation already committed
//!   converges on `succeeded` instead of failing.

use chrono::{DateTime, Utc};
use ortak_domain::{
    CredentialRef, Employee, EmployeeId, EmployeeManifest, EmployeeStatus, ProvisioningMode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::adapter::{HealthReport, HealthState, ResourceOutcome, ResourceOwnership};
use crate::credentials::{CredentialReferenceStatus, CredentialResolver};
use crate::error::{ControlError, Result};
use crate::ids::CompanyScope;
use crate::memory::{
    MemoryAdapter, MemoryCapabilities, MemoryCapability, MemoryHealthReport, MemoryResourceOutcome,
    MemoryResourceRequest, ACTIVATION_REQUIRED_MEMORY_CAPABILITIES,
};
use crate::office_identity::{
    OfficeIdentityAdapter, OfficeMembershipRequest, OfficePublicKey, SignerVerification,
};
use crate::ports::ProvisioningRepository;
use crate::runtime::{
    RuntimeAdapter, RuntimeCapabilities, RuntimeCapability, RuntimeResourceRequest,
    ACTIVATION_REQUIRED_CAPABILITIES,
};

/// Operation mode stored in `provisioning_operations.mode`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationMode {
    /// Create new external resources.
    Create,
    /// Adopt existing external resources.
    Adopt,
    /// Re-provision an existing employee from a new manifest.
    Update,
}

impl OperationMode {
    /// Column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Adopt => "adopt",
            Self::Update => "update",
        }
    }
}

/// Operation lifecycle stored in `provisioning_operations.status`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    /// Created, not yet run.
    Pending,
    /// A worker is executing steps.
    Running,
    /// Every step succeeded or was skipped.
    Succeeded,
    /// A step failed; retry resumes at that step, or an operator compensates.
    Failed,
    /// Compensation in progress.
    Compensating,
    /// Compensation finished; adopted resources were retained.
    Compensated,
}

impl OperationStatus {
    /// Column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Compensating => "compensating",
            Self::Compensated => "compensated",
        }
    }

    /// True for states that end the operation.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Compensated)
    }

    /// True when a durable row in `self` may be overwritten with `next`.
    ///
    /// This is the concurrency fence every repository applies to status
    /// updates: terminal rows never change, compensation never turns back
    /// into a run, and only a failed operation enters compensation. A write
    /// refused by this fence means another worker advanced the operation.
    pub fn can_transition_to(self, next: Self) -> bool {
        match self {
            Self::Pending => matches!(next, Self::Running | Self::Failed),
            Self::Running => matches!(next, Self::Running | Self::Failed | Self::Succeeded),
            Self::Failed => matches!(next, Self::Running | Self::Failed | Self::Compensating),
            Self::Compensating => matches!(next, Self::Compensating | Self::Compensated),
            Self::Succeeded | Self::Compensated => false,
        }
    }
}

/// Step lifecycle stored in `provisioning_operation_steps.state`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    /// Not started.
    Pending,
    /// In progress (a crash here resumes the step under the same key).
    Running,
    /// Done.
    Succeeded,
    /// Failed; retried on resume while attempts remain.
    Failed,
    /// Compensation in progress.
    Compensating,
    /// Compensated (deleted created resources, retained adopted ones).
    Compensated,
    /// Not applicable (dry run, or no binding of that kind).
    Skipped,
}

impl StepState {
    /// Column value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Compensating => "compensating",
            Self::Compensated => "compensated",
            Self::Skipped => "skipped",
        }
    }

    /// True when resume may pass over this step.
    pub fn is_done(self) -> bool {
        matches!(self, Self::Succeeded | Self::Skipped)
    }

    /// True when a durable step row in `self` may be overwritten with `next`.
    ///
    /// A succeeded or skipped step is never taken back to `pending`,
    /// `running`, or `failed` by a concurrent or replayed resume; it can only
    /// move forward into compensation. A compensating step never returns to
    /// `succeeded`, so a compensation retry resumes it instead of leaking
    /// the resource behind it.
    pub fn can_transition_to(self, next: Self) -> bool {
        match self {
            Self::Pending | Self::Running | Self::Failed => matches!(
                next,
                Self::Pending | Self::Running | Self::Succeeded | Self::Failed | Self::Skipped
            ),
            Self::Succeeded => {
                matches!(
                    next,
                    Self::Succeeded | Self::Compensating | Self::Compensated
                )
            }
            Self::Skipped => matches!(next, Self::Skipped),
            Self::Compensating => matches!(next, Self::Compensating | Self::Compensated),
            Self::Compensated => matches!(next, Self::Compensated),
        }
    }
}

/// Ordered saga steps (Architecture v0 §6).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvisioningStep {
    /// Validate the secret-free manifest.
    ValidateManifest,
    /// Reserve the stable employee identity row.
    ReserveEmployeeIdentity,
    /// Confirm every credential reference exists (no values).
    ResolveCredentialReferences,
    /// Create or adopt the runtime profile.
    EnsureRuntimeProfile,
    /// Probe runtime capabilities and validate profile/config/workspace.
    ValidateRuntimeProfile,
    /// Create or adopt memory workspace and peers.
    EnsureMemoryResources,
    /// Prove signer/public-key correspondence and create or adopt membership.
    EnsureOfficeIdentity,
    /// Publish the secret-free Office profile.
    PublishOfficeProfile,
    /// Probe runtime, memory, Office membership, and signer together.
    ProbeHealth,
    /// Activate the new employee revision.
    ActivateRevision,
}

impl ProvisioningStep {
    /// Every step in execution order.
    pub const ALL: [Self; 10] = [
        Self::ValidateManifest,
        Self::ReserveEmployeeIdentity,
        Self::ResolveCredentialReferences,
        Self::EnsureRuntimeProfile,
        Self::ValidateRuntimeProfile,
        Self::EnsureMemoryResources,
        Self::EnsureOfficeIdentity,
        Self::PublishOfficeProfile,
        Self::ProbeHealth,
        Self::ActivateRevision,
    ];

    /// Column value; matches `^[a-z][a-z0-9_]{0,63}$`.
    pub fn name(self) -> &'static str {
        match self {
            Self::ValidateManifest => "validate_manifest",
            Self::ReserveEmployeeIdentity => "reserve_employee_identity",
            Self::ResolveCredentialReferences => "resolve_credential_references",
            Self::EnsureRuntimeProfile => "ensure_runtime_profile",
            Self::ValidateRuntimeProfile => "validate_runtime_profile",
            Self::EnsureMemoryResources => "ensure_memory_resources",
            Self::EnsureOfficeIdentity => "ensure_office_identity",
            Self::PublishOfficeProfile => "publish_office_profile",
            Self::ProbeHealth => "probe_health",
            Self::ActivateRevision => "activate_revision",
        }
    }

    /// Parses a column value.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|step| step.name() == value)
    }

    /// Position in [`Self::ALL`].
    pub fn index(self) -> i16 {
        Self::ALL
            .iter()
            .position(|step| *step == self)
            .and_then(|index| i16::try_from(index).ok())
            .unwrap_or(0)
    }

    /// True for steps that publish or activate; always skipped in dry runs.
    pub fn is_publishing(self) -> bool {
        matches!(self, Self::PublishOfficeProfile | Self::ActivateRevision)
    }

    /// True for steps that create external resources in `create` mode.
    pub fn creates_external_resources(self) -> bool {
        matches!(
            self,
            Self::EnsureRuntimeProfile | Self::EnsureMemoryResources | Self::EnsureOfficeIdentity
        )
    }
}

/// Durable state of one step.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StepRecord {
    /// Which step.
    pub step: ProvisioningStep,
    /// Current state.
    pub state: StepState,
    /// Per-step adapter idempotency key.
    pub idempotency_key: String,
    /// Attempts so far.
    pub attempt_count: i32,
    /// True when the step attached to a pre-existing resource.
    pub adopted_existing: bool,
    /// Secret-free receipt (JSON object).
    pub result: serde_json::Value,
    /// Bounded error message of the last failure.
    pub error_message: Option<String>,
    /// Last start.
    pub started_at: Option<DateTime<Utc>>,
    /// Last finish.
    pub finished_at: Option<DateTime<Utc>>,
}

impl StepRecord {
    /// A pending record for `step` of `operation_id`.
    pub fn pending(operation_id: Uuid, step: ProvisioningStep) -> Self {
        Self {
            step,
            state: StepState::Pending,
            idempotency_key: step_idempotency_key(operation_id, step),
            attempt_count: 0,
            adopted_existing: false,
            result: serde_json::Value::Object(Default::default()),
            error_message: None,
            started_at: None,
            finished_at: None,
        }
    }

    fn receipt<T>(&self, key: &str) -> Option<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.result
            .get(key)
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }
}

/// Deterministic per-step key: unique per company because operation ids are.
pub fn step_idempotency_key(operation_id: Uuid, step: ProvisioningStep) -> String {
    format!("provisioning:{operation_id}:{}", step.name())
}

/// Durable operation with its steps in execution order.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProvisioningOperation {
    /// Operation id.
    pub id: Uuid,
    /// Employee being provisioned.
    pub employee_id: EmployeeId,
    /// Mode.
    pub mode: OperationMode,
    /// Dry runs plan and validate without publishing, activating, or creating.
    pub dry_run: bool,
    /// Operator idempotency key.
    pub idempotency_key: String,
    /// Secret-free manifest snapshot.
    pub manifest: EmployeeManifest,
    /// SHA-256 of the canonical manifest JSON.
    pub manifest_fingerprint: [u8; 32],
    /// Status.
    pub status: OperationStatus,
    /// Step being executed or that failed.
    pub current_step: Option<ProvisioningStep>,
    /// Revision activated by a successful non-dry-run operation.
    pub result_revision_id: Option<Uuid>,
    /// Bounded error message.
    pub error_message: Option<String>,
    /// Every step in [`ProvisioningStep::ALL`] order.
    pub steps: Vec<StepRecord>,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last update.
    pub updated_at: DateTime<Utc>,
    /// Finish time.
    pub finished_at: Option<DateTime<Utc>>,
}

impl ProvisioningOperation {
    /// Returns the record for `step`.
    pub fn step(&self, step: ProvisioningStep) -> Option<&StepRecord> {
        self.steps.iter().find(|record| record.step == step)
    }

    fn step_mut(&mut self, step: ProvisioningStep) -> Option<&mut StepRecord> {
        self.steps.iter_mut().find(|record| record.step == step)
    }

    /// The manifest employee with references learned from succeeded steps
    /// folded in: a profile created by `ensure_runtime_profile` becomes the
    /// runtime binding's `profile_ref` so later validation, probing, and the
    /// activated revision point at the real resource.
    pub fn effective_employee(&self) -> Employee {
        let mut employee = self.manifest.employee.clone();
        if employee.runtime.profile_ref.is_none() {
            if let Some(receipt) = self
                .step(ProvisioningStep::EnsureRuntimeProfile)
                .filter(|record| record.state == StepState::Succeeded)
                .and_then(|record| {
                    serde_json::from_value::<ResourceOutcome>(record.result.clone()).ok()
                })
            {
                employee.runtime.profile_ref = Some(receipt.resource_ref);
            }
        }
        employee
    }

    /// Provisioning mode the adapters receive for create-or-adopt steps.
    pub fn resource_mode(&self) -> ProvisioningMode {
        match self.mode {
            OperationMode::Create => ProvisioningMode::Create,
            OperationMode::Adopt => ProvisioningMode::Adopt,
            OperationMode::Update => self.manifest.provisioning,
        }
    }
}

/// Request to begin (or resume by idempotency key) an operation.
#[derive(Clone, Debug, PartialEq)]
pub struct ProvisioningRequest {
    /// Employee id; must equal the manifest's employee id.
    pub employee_id: EmployeeId,
    /// Mode.
    pub mode: OperationMode,
    /// Dry run.
    pub dry_run: bool,
    /// Operator idempotency key.
    pub idempotency_key: String,
    /// Secret-free manifest.
    pub manifest: EmployeeManifest,
}

impl ProvisioningRequest {
    /// Validates the manifest and its consistency with the request.
    pub fn validate(&self) -> Result<()> {
        self.manifest.validate()?;
        if self.manifest.employee.id != self.employee_id {
            return Err(ProvisioningError::ManifestMismatch {
                detail: "manifest employee id differs from the request",
            }
            .into());
        }
        let consistent = match self.mode {
            OperationMode::Create => self.manifest.provisioning == ProvisioningMode::Create,
            OperationMode::Adopt => self.manifest.provisioning == ProvisioningMode::Adopt,
            OperationMode::Update => true,
        };
        if !consistent {
            return Err(ProvisioningError::ManifestMismatch {
                detail: "manifest provisioning mode differs from the operation mode",
            }
            .into());
        }
        if self.idempotency_key.trim().is_empty() || self.idempotency_key.len() > 256 {
            return Err(ProvisioningError::ManifestMismatch {
                detail: "operation idempotency key must be 1..=256 bytes",
            }
            .into());
        }
        Ok(())
    }

    /// Canonical manifest fingerprint.
    pub fn fingerprint(&self) -> Result<[u8; 32]> {
        manifest_fingerprint(&self.manifest)
    }
}

/// SHA-256 of the canonical (serde) manifest JSON.
pub fn manifest_fingerprint(manifest: &EmployeeManifest) -> Result<[u8; 32]> {
    let json = serde_json::to_vec(manifest)?;
    Ok(Sha256::digest(&json).into())
}

/// Update applied to the operation row.
#[derive(Clone, Debug, PartialEq)]
pub struct OperationUpdate {
    /// New status.
    pub status: OperationStatus,
    /// Current step.
    pub current_step: Option<ProvisioningStep>,
    /// Error message (bounded).
    pub error_message: Option<String>,
}

impl OperationUpdate {
    /// True for statuses whose row must carry `finished_at`.
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            OperationStatus::Succeeded | OperationStatus::Failed | OperationStatus::Compensated
        )
    }
}

/// Outcome of reserving the employee identity row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "identity")]
pub enum IdentityReservation {
    /// A draft row was created by this operation.
    Created,
    /// The row already existed.
    Existing {
        /// Its lifecycle state at reservation time.
        status: EmployeeStatus,
    },
}

/// Everything the repository needs to activate a revision in one transaction.
#[derive(Clone, Debug, PartialEq)]
pub struct RevisionActivation {
    /// Employee definition to persist as the revision manifest (status active).
    pub employee: Employee,
    /// Provisioning mode recorded on the revision and bindings.
    pub provisioning_mode: ProvisioningMode,
    /// Canonical fingerprint of `employee` as serialized.
    pub manifest_fingerprint: [u8; 32],
    /// The succeeded activation step record, persisted in the same commit.
    pub activation_step: StepRecord,
    /// Time the runtime binding was validated.
    pub runtime_validated_at: DateTime<Utc>,
    /// Time the memory binding was validated, when bound.
    pub memory_validated_at: Option<DateTime<Utc>>,
    /// Time the signer proved the public key.
    pub office_verified_at: DateTime<Utc>,
}

/// Provisioning failures.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProvisioningError {
    /// The idempotency key already names an operation with a different
    /// manifest, mode, or dry-run flag.
    #[error(
        "idempotency key already used by operation {operation_id} with a different manifest, mode, or dry-run flag"
    )]
    IdempotencyConflict {
        /// Existing operation.
        operation_id: Uuid,
    },
    /// A durable write was refused because the operation or step had already
    /// advanced past this worker's view (a concurrent or replayed resume).
    /// The worker must reload; it must not retry the write.
    #[error("provisioning operation {operation_id} was superseded: {detail}")]
    Superseded {
        /// Operation.
        operation_id: Uuid,
        /// Stable detail.
        detail: &'static str,
    },
    /// No such operation in this company.
    #[error("unknown provisioning operation {operation_id}")]
    UnknownOperation {
        /// Requested operation.
        operation_id: Uuid,
    },
    /// The action is not allowed from the current status.
    #[error("cannot {action} a provisioning operation in status {status:?}")]
    InvalidTransition {
        /// Current status.
        status: OperationStatus,
        /// Attempted action.
        action: &'static str,
    },
    /// A step has used every attempt.
    #[error("step {step:?} exhausted {attempts} attempts")]
    StepExhausted {
        /// Step.
        step: ProvisioningStep,
        /// Attempts made.
        attempts: i32,
    },
    /// The request and manifest disagree.
    #[error("manifest mismatch: {detail}")]
    ManifestMismatch {
        /// Stable detail.
        detail: &'static str,
    },
    /// The employee is not in a state that allows this operation.
    #[error("employee {employee_id} is {status:?}; {detail}")]
    EmployeeState {
        /// Employee.
        employee_id: EmployeeId,
        /// Current status.
        status: EmployeeStatus,
        /// Stable detail.
        detail: &'static str,
    },
    /// The durable operation row is inconsistent (e.g. missing steps).
    #[error("provisioning operation {operation_id} is inconsistent: {detail}")]
    Inconsistent {
        /// Operation.
        operation_id: Uuid,
        /// Stable detail.
        detail: &'static str,
    },
}

/// Evidence collected by the `probe_health` step and evaluated by the gate.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct GateEvidence {
    /// Probed runtime capabilities.
    pub runtime_capabilities: Option<RuntimeCapabilities>,
    /// Runtime profile health.
    pub runtime_health: Option<HealthReport>,
    /// Probed memory capabilities (absent when the employee has no memory binding).
    pub memory_capabilities: Option<MemoryCapabilities>,
    /// Memory health (absent when the employee has no memory binding).
    pub memory_health: Option<MemoryHealthReport>,
    /// Office membership health.
    pub office_membership: Option<HealthReport>,
    /// Signer proof.
    pub signer: Option<SignerVerification>,
}

/// Why activation was refused.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "gate")]
pub enum GateFailure {
    /// Runtime capabilities were not probed.
    RuntimeNotProbed,
    /// Runtime lacks required capabilities.
    RuntimeMissingCapabilities {
        /// Missing set.
        missing: Vec<RuntimeCapability>,
    },
    /// Runtime health was not healthy.
    RuntimeUnhealthy {
        /// Observed state.
        state: HealthState,
    },
    /// Memory capabilities were not probed although the employee has a binding.
    MemoryNotProbed,
    /// Memory lacks required capabilities.
    MemoryMissingCapabilities {
        /// Missing set.
        missing: Vec<MemoryCapability>,
    },
    /// Memory workspace or a peer was not healthy.
    MemoryUnhealthy {
        /// Workspace state.
        workspace: HealthState,
        /// User peer state.
        user_peer: HealthState,
        /// Employee peer state.
        employee_peer: HealthState,
    },
    /// Office membership was not probed.
    OfficeMembershipNotProbed,
    /// The key is not a healthy Office member.
    OfficeMembershipUnhealthy {
        /// Observed state.
        state: HealthState,
    },
    /// The signer was not asked to prove its key.
    SignerNotVerified,
    /// The signer produced a different key than the manifest declares.
    SignerKeyMismatch,
}

/// Pure activation gate. Every gate must pass; failures are returned in
/// stable order so they are explainable in the operation record.
pub fn evaluate_activation_gates(
    evidence: &GateEvidence,
    requires_memory: bool,
) -> std::result::Result<(), Vec<GateFailure>> {
    let mut failures = Vec::new();

    match &evidence.runtime_capabilities {
        None => failures.push(GateFailure::RuntimeNotProbed),
        Some(capabilities) => {
            let missing = capabilities.missing(&ACTIVATION_REQUIRED_CAPABILITIES);
            if !missing.is_empty() {
                failures.push(GateFailure::RuntimeMissingCapabilities { missing });
            }
        }
    }
    match &evidence.runtime_health {
        None => {
            if !failures.contains(&GateFailure::RuntimeNotProbed) {
                failures.push(GateFailure::RuntimeNotProbed);
            }
        }
        Some(health) if !health.is_healthy() => {
            failures.push(GateFailure::RuntimeUnhealthy {
                state: health.state,
            });
        }
        Some(_) => {}
    }

    if requires_memory {
        match &evidence.memory_capabilities {
            None => failures.push(GateFailure::MemoryNotProbed),
            Some(capabilities) => {
                let missing = capabilities.missing(&ACTIVATION_REQUIRED_MEMORY_CAPABILITIES);
                if !missing.is_empty() {
                    failures.push(GateFailure::MemoryMissingCapabilities { missing });
                }
            }
        }
        match &evidence.memory_health {
            None => {
                if !failures.contains(&GateFailure::MemoryNotProbed) {
                    failures.push(GateFailure::MemoryNotProbed);
                }
            }
            Some(health) if !health.is_healthy() => failures.push(GateFailure::MemoryUnhealthy {
                workspace: health.workspace.state,
                user_peer: health.user_peer.state,
                employee_peer: health.employee_peer.state,
            }),
            Some(_) => {}
        }
    }

    match &evidence.office_membership {
        None => failures.push(GateFailure::OfficeMembershipNotProbed),
        Some(health) if !health.is_healthy() => {
            failures.push(GateFailure::OfficeMembershipUnhealthy {
                state: health.state,
            });
        }
        Some(_) => {}
    }

    match &evidence.signer {
        None => failures.push(GateFailure::SignerNotVerified),
        Some(verification) if !verification.matches_expected => {
            failures.push(GateFailure::SignerKeyMismatch);
        }
        Some(_) => {}
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

/// Saga tuning.
#[derive(Clone, Debug)]
pub struct SagaConfig {
    /// Attempts per step before the operation needs operator action.
    pub max_step_attempts: i32,
}

impl Default for SagaConfig {
    fn default() -> Self {
        Self {
            max_step_attempts: 3,
        }
    }
}

/// Result of driving an operation.
#[derive(Clone, Debug, PartialEq)]
pub enum SagaOutcome {
    /// Every step succeeded or was skipped.
    Succeeded(ProvisioningOperation),
    /// A step failed; the operation is `failed` and resumable.
    Failed {
        /// Operation after the failure.
        operation: ProvisioningOperation,
        /// Failed step.
        step: ProvisioningStep,
        /// Bounded error.
        error: String,
    },
    /// Compensation finished.
    Compensated {
        /// Operation after compensation.
        operation: ProvisioningOperation,
        /// Adopted resources that were deliberately retained.
        retained_adopted: Vec<String>,
        /// Created resources that were deleted.
        deleted: Vec<String>,
    },
    /// The operation was already terminal; nothing ran.
    AlreadyTerminal(ProvisioningOperation),
}

/// Adapter or contract failure inside one step.
struct StepFailure {
    message: String,
}

impl StepFailure {
    fn new(message: impl std::fmt::Display) -> Self {
        Self {
            message: crate::adapter::Detail::new(message.to_string())
                .as_str()
                .to_owned(),
        }
    }
}

/// Successful step result.
struct StepSuccess {
    result: serde_json::Value,
    adopted_existing: bool,
    skipped: bool,
}

impl StepSuccess {
    fn ok<T: Serialize>(
        receipt: &T,
        adopted_existing: bool,
    ) -> std::result::Result<Self, StepFailure> {
        Ok(Self {
            result: object(receipt)?,
            adopted_existing,
            skipped: false,
        })
    }

    fn skipped(reason: &str) -> std::result::Result<Self, StepFailure> {
        Ok(Self {
            result: serde_json::json!({ "skipped": reason }),
            adopted_existing: false,
            skipped: true,
        })
    }
}

fn object<T: Serialize>(value: &T) -> std::result::Result<serde_json::Value, StepFailure> {
    match serde_json::to_value(value) {
        Ok(value @ serde_json::Value::Object(_)) => Ok(value),
        Ok(other) => Ok(serde_json::json!({ "value": other })),
        Err(error) => Err(StepFailure::new(error)),
    }
}

/// The provisioning saga executor.
///
/// Generic over the repository and adapter ports so tests bind fakes and
/// production binds PostgreSQL plus the Hermes/Honcho/Office adapters.
pub struct ProvisioningSaga<'a, Repo, Runtime, Memory, Office, Credentials> {
    repository: &'a Repo,
    runtime: &'a Runtime,
    memory: &'a Memory,
    office: &'a Office,
    credentials: &'a Credentials,
    config: SagaConfig,
}

impl<'a, Repo, Runtime, Memory, Office, Credentials>
    ProvisioningSaga<'a, Repo, Runtime, Memory, Office, Credentials>
where
    Repo: ProvisioningRepository,
    Runtime: RuntimeAdapter,
    Memory: MemoryAdapter,
    Office: OfficeIdentityAdapter,
    Credentials: CredentialResolver,
{
    /// Binds the saga to its ports.
    pub fn new(
        repository: &'a Repo,
        runtime: &'a Runtime,
        memory: &'a Memory,
        office: &'a Office,
        credentials: &'a Credentials,
        config: SagaConfig,
    ) -> Self {
        Self {
            repository,
            runtime,
            memory,
            office,
            credentials,
            config,
        }
    }

    /// Begins an operation or returns the existing one for the idempotency key.
    pub async fn begin(
        &self,
        scope: &CompanyScope,
        request: &ProvisioningRequest,
    ) -> Result<ProvisioningOperation> {
        request.validate()?;
        self.repository.begin_operation(scope, request).await
    }

    /// Drives the operation from its first unfinished step to success or the
    /// first failure. Safe to call repeatedly: finished steps are not re-run.
    ///
    /// Concurrent or replayed resumes are fenced by the repository: a write
    /// that would regress a succeeded operation or step is refused with
    /// [`ProvisioningError::Superseded`]. This worker then reloads and
    /// converges on the durable outcome instead of failing or regressing;
    /// a committed activation is reported as [`SagaOutcome::Succeeded`].
    pub async fn resume(&self, scope: &CompanyScope, operation_id: Uuid) -> Result<SagaOutcome> {
        let operation = self.load(scope, operation_id).await?;
        if operation.result_revision_id.is_some() {
            return Ok(SagaOutcome::AlreadyTerminal(operation));
        }
        match operation.status {
            OperationStatus::Succeeded | OperationStatus::Compensated => {
                return Ok(SagaOutcome::AlreadyTerminal(operation));
            }
            OperationStatus::Compensating => {
                return Err(ProvisioningError::InvalidTransition {
                    status: operation.status,
                    action: "resume",
                }
                .into());
            }
            OperationStatus::Pending | OperationStatus::Running | OperationStatus::Failed => {}
        }

        match self.drive(scope, operation).await {
            Err(ControlError::Provisioning(ProvisioningError::Superseded { .. })) => {
                self.converge(scope, operation_id).await
            }
            other => other,
        }
    }

    /// Reloads after a fenced write and reports the durable outcome another
    /// worker committed.
    async fn converge(&self, scope: &CompanyScope, operation_id: Uuid) -> Result<SagaOutcome> {
        let operation = self.load(scope, operation_id).await?;
        match operation.status {
            OperationStatus::Succeeded => Ok(SagaOutcome::Succeeded(operation)),
            OperationStatus::Compensated => Ok(SagaOutcome::AlreadyTerminal(operation)),
            OperationStatus::Compensating => Err(ProvisioningError::InvalidTransition {
                status: operation.status,
                action: "resume",
            }
            .into()),
            OperationStatus::Pending | OperationStatus::Running | OperationStatus::Failed => {
                Err(ProvisioningError::Superseded {
                    operation_id,
                    detail: "another worker advanced the operation; resume again",
                }
                .into())
            }
        }
    }

    async fn drive(
        &self,
        scope: &CompanyScope,
        mut operation: ProvisioningOperation,
    ) -> Result<SagaOutcome> {
        let operation_id = operation.id;
        for step in ProvisioningStep::ALL {
            let record = operation
                .step(step)
                .cloned()
                .ok_or(ProvisioningError::Inconsistent {
                    operation_id,
                    detail: "missing step record",
                })?;
            if record.state.is_done() {
                continue;
            }
            if record.attempt_count >= self.config.max_step_attempts {
                let error = ProvisioningError::StepExhausted {
                    step,
                    attempts: record.attempt_count,
                };
                self.fail_operation(scope, &mut operation, step, error.to_string())
                    .await?;
                return Ok(SagaOutcome::Failed {
                    operation,
                    step,
                    error: error.to_string(),
                });
            }

            self.set_status(scope, &mut operation, OperationStatus::Running, Some(step))
                .await?;
            let mut running = record.clone();
            running.state = StepState::Running;
            running.attempt_count += 1;
            running.started_at = Some(Utc::now());
            running.finished_at = None;
            running.error_message = None;
            self.repository
                .record_step(scope, operation_id, &running)
                .await?;
            replace_step(&mut operation, running.clone());

            if step == ProvisioningStep::ActivateRevision {
                match self.activate(scope, &operation, &running).await? {
                    Ok(activated) => {
                        operation = activated;
                        continue;
                    }
                    Err(failure) => {
                        let mut failed = running;
                        failed.state = StepState::Failed;
                        failed.error_message = Some(failure.message.clone());
                        failed.finished_at = Some(Utc::now());
                        self.repository
                            .record_step(scope, operation_id, &failed)
                            .await?;
                        replace_step(&mut operation, failed);
                        self.fail_operation(scope, &mut operation, step, failure.message.clone())
                            .await?;
                        return Ok(SagaOutcome::Failed {
                            operation,
                            step,
                            error: failure.message,
                        });
                    }
                }
            }

            match self.execute(scope, &operation, step, &running).await? {
                Ok(success) => {
                    let mut done = running;
                    done.state = if success.skipped {
                        StepState::Skipped
                    } else {
                        StepState::Succeeded
                    };
                    done.adopted_existing = success.adopted_existing;
                    done.result = success.result;
                    done.finished_at = Some(Utc::now());
                    self.repository
                        .record_step(scope, operation_id, &done)
                        .await?;
                    replace_step(&mut operation, done);
                }
                Err(failure) => {
                    let mut failed = running;
                    failed.state = StepState::Failed;
                    failed.error_message = Some(failure.message.clone());
                    failed.finished_at = Some(Utc::now());
                    self.repository
                        .record_step(scope, operation_id, &failed)
                        .await?;
                    replace_step(&mut operation, failed);
                    self.fail_operation(scope, &mut operation, step, failure.message.clone())
                        .await?;
                    return Ok(SagaOutcome::Failed {
                        operation,
                        step,
                        error: failure.message,
                    });
                }
            }
        }

        if operation.status != OperationStatus::Succeeded {
            // Dry runs (and operations whose activation was skipped) finish here.
            self.repository
                .update_operation(
                    scope,
                    operation_id,
                    &OperationUpdate {
                        status: OperationStatus::Succeeded,
                        current_step: None,
                        error_message: None,
                    },
                )
                .await?;
            operation.status = OperationStatus::Succeeded;
            operation.current_step = None;
            operation.finished_at = Some(Utc::now());
        }
        Ok(SagaOutcome::Succeeded(operation))
    }

    /// Operator-initiated compensation of a failed operation.
    ///
    /// Walks succeeded steps in reverse. Resources whose receipt says
    /// `created` are deleted through their adapter; adopted resources are
    /// retained and recorded. Nothing is ever activated, published, or
    /// re-created here.
    ///
    /// An operation that activated a revision (`result_revision_id` set) is
    /// refused regardless of its status column. A retry resumes every step
    /// left `compensating` by an earlier failure or crash, so a created
    /// resource is never skipped and leaked.
    pub async fn compensate(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
    ) -> Result<SagaOutcome> {
        let mut operation = self.load(scope, operation_id).await?;
        if operation.result_revision_id.is_some() {
            return Err(ProvisioningError::InvalidTransition {
                status: operation.status,
                action: "compensate an activated",
            }
            .into());
        }
        match operation.status {
            OperationStatus::Failed | OperationStatus::Compensating => {}
            OperationStatus::Compensated => {
                return Ok(SagaOutcome::AlreadyTerminal(operation));
            }
            status => {
                return Err(ProvisioningError::InvalidTransition {
                    status,
                    action: "compensate",
                }
                .into());
            }
        }
        self.set_status(scope, &mut operation, OperationStatus::Compensating, None)
            .await?;

        let mut retained_adopted = Vec::new();
        let mut deleted = Vec::new();
        for step in ProvisioningStep::ALL.iter().rev().copied() {
            let record = operation
                .step(step)
                .cloned()
                .ok_or(ProvisioningError::Inconsistent {
                    operation_id,
                    detail: "missing step record",
                })?;
            if !matches!(record.state, StepState::Succeeded | StepState::Compensating) {
                continue;
            }
            let mut compensating = record.clone();
            compensating.state = StepState::Compensating;
            compensating.error_message = None;
            self.repository
                .record_step(scope, operation_id, &compensating)
                .await?;

            match self
                .compensate_step(&operation, &record, &mut retained_adopted, &mut deleted)
                .await
            {
                Ok(receipt) => {
                    let mut done = compensating;
                    done.state = StepState::Compensated;
                    done.result = merge(done.result, receipt);
                    done.finished_at = Some(Utc::now());
                    self.repository
                        .record_step(scope, operation_id, &done)
                        .await?;
                    replace_step(&mut operation, done);
                }
                Err(failure) => {
                    // Leave the step `compensating` with the error so the next
                    // compensation resumes the deletion; the operation stays
                    // `compensating`. The step never returns to `succeeded`.
                    let mut still = compensating;
                    still.error_message = Some(failure.message.clone());
                    self.repository
                        .record_step(scope, operation_id, &still)
                        .await?;
                    replace_step(&mut operation, still);
                    self.repository
                        .update_operation(
                            scope,
                            operation_id,
                            &OperationUpdate {
                                status: OperationStatus::Compensating,
                                current_step: Some(step),
                                error_message: Some(failure.message.clone()),
                            },
                        )
                        .await?;
                    operation.current_step = Some(step);
                    operation.error_message = Some(failure.message.clone());
                    return Ok(SagaOutcome::Failed {
                        operation,
                        step,
                        error: failure.message,
                    });
                }
            }
        }

        self.repository
            .update_operation(
                scope,
                operation_id,
                &OperationUpdate {
                    status: OperationStatus::Compensated,
                    current_step: None,
                    error_message: operation.error_message.clone(),
                },
            )
            .await?;
        operation.status = OperationStatus::Compensated;
        operation.current_step = None;
        operation.finished_at = Some(Utc::now());
        Ok(SagaOutcome::Compensated {
            operation,
            retained_adopted,
            deleted,
        })
    }

    async fn load(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
    ) -> Result<ProvisioningOperation> {
        let operation = self
            .repository
            .load_operation(scope, operation_id)
            .await?
            .ok_or(ProvisioningError::UnknownOperation { operation_id })?;
        if operation.steps.len() != ProvisioningStep::ALL.len() {
            return Err(ProvisioningError::Inconsistent {
                operation_id,
                detail: "step count differs from the saga definition",
            }
            .into());
        }
        Ok(operation)
    }

    async fn set_status(
        &self,
        scope: &CompanyScope,
        operation: &mut ProvisioningOperation,
        status: OperationStatus,
        current_step: Option<ProvisioningStep>,
    ) -> Result<()> {
        self.repository
            .update_operation(
                scope,
                operation.id,
                &OperationUpdate {
                    status,
                    current_step,
                    error_message: None,
                },
            )
            .await?;
        operation.status = status;
        operation.current_step = current_step;
        operation.error_message = None;
        Ok(())
    }

    async fn fail_operation(
        &self,
        scope: &CompanyScope,
        operation: &mut ProvisioningOperation,
        step: ProvisioningStep,
        error: String,
    ) -> Result<()> {
        self.repository
            .update_operation(
                scope,
                operation.id,
                &OperationUpdate {
                    status: OperationStatus::Failed,
                    current_step: Some(step),
                    error_message: Some(error.clone()),
                },
            )
            .await?;
        operation.status = OperationStatus::Failed;
        operation.current_step = Some(step);
        operation.error_message = Some(error);
        Ok(())
    }

    /// Executes one non-activation step. Repository/database errors propagate;
    /// adapter and contract failures become a recorded step failure.
    async fn execute(
        &self,
        scope: &CompanyScope,
        operation: &ProvisioningOperation,
        step: ProvisioningStep,
        record: &StepRecord,
    ) -> Result<std::result::Result<StepSuccess, StepFailure>> {
        let employee = &operation.effective_employee();
        let mode = operation.resource_mode();
        let key = record.idempotency_key.as_str();
        let outcome = match step {
            ProvisioningStep::ValidateManifest => match operation.manifest.validate() {
                Ok(()) => StepSuccess::ok(
                    &serde_json::json!({
                        "manifest_fingerprint": hex::encode(operation.manifest_fingerprint),
                        "provisioning": operation.manifest.provisioning,
                    }),
                    false,
                ),
                Err(error) => Err(StepFailure::new(error)),
            },
            ProvisioningStep::ReserveEmployeeIdentity => {
                let reservation = self
                    .repository
                    .reserve_employee_identity(scope, &employee.id)
                    .await?;
                match (reservation, operation.mode) {
                    (IdentityReservation::Existing { status }, OperationMode::Create)
                        if status != EmployeeStatus::Draft =>
                    {
                        Err(StepFailure::new(ProvisioningError::EmployeeState {
                            employee_id: employee.id.clone(),
                            status,
                            detail: "create requires a new or draft employee",
                        }))
                    }
                    (IdentityReservation::Existing { status }, _)
                        if status == EmployeeStatus::Disabled =>
                    {
                        Err(StepFailure::new(ProvisioningError::EmployeeState {
                            employee_id: employee.id.clone(),
                            status,
                            detail: "disabled employees cannot be provisioned",
                        }))
                    }
                    (reservation, _) => StepSuccess::ok(
                        &reservation,
                        matches!(reservation, IdentityReservation::Existing { .. }),
                    ),
                }
            }
            ProvisioningStep::ResolveCredentialReferences => {
                let mut references: Vec<&CredentialRef> =
                    employee.runtime.credential_refs.iter().collect();
                references.push(&employee.office.signer_ref);
                let mut resolved = Vec::new();
                let mut missing = Vec::new();
                for reference in references {
                    match self.credentials.verify_reference(reference).await {
                        Ok(CredentialReferenceStatus::Resolvable) => {
                            resolved.push(reference.as_str().to_owned());
                        }
                        Ok(CredentialReferenceStatus::Missing) => {
                            missing.push(reference.as_str().to_owned());
                        }
                        Err(error) => return Ok(Err(StepFailure::new(error))),
                    }
                }
                if missing.is_empty() {
                    StepSuccess::ok(&serde_json::json!({ "resolved": resolved }), false)
                } else {
                    Err(StepFailure::new(format!(
                        "unresolvable credential references: {}",
                        missing.join(", ")
                    )))
                }
            }
            ProvisioningStep::EnsureRuntimeProfile => {
                if operation.dry_run && mode == ProvisioningMode::Create {
                    return Ok(StepSuccess::skipped("dry_run_create"));
                }
                let request = RuntimeResourceRequest {
                    employee_id: employee.id.clone(),
                    mode,
                    binding: employee.runtime.clone(),
                    idempotency_key: key.to_owned(),
                };
                match self.runtime.ensure_profile(&request).await {
                    Ok(outcome) => match check_ownership(mode, &outcome) {
                        Ok(()) => StepSuccess::ok(&outcome, outcome.ownership.is_adopted()),
                        Err(failure) => Err(failure),
                    },
                    Err(error) => Err(StepFailure::new(error)),
                }
            }
            ProvisioningStep::ValidateRuntimeProfile => {
                let capabilities = match self.runtime.probe_capabilities().await {
                    Ok(capabilities) => capabilities,
                    Err(error) => return Ok(Err(StepFailure::new(error))),
                };
                let missing = capabilities.missing(&ACTIVATION_REQUIRED_CAPABILITIES);
                if !missing.is_empty() {
                    return Ok(Err(StepFailure::new(format!(
                        "runtime lacks required capabilities: {missing:?}"
                    ))));
                }
                if operation.dry_run && mode == ProvisioningMode::Create {
                    return Ok(StepSuccess::skipped("dry_run_create"));
                }
                match self.runtime.health(&employee.runtime).await {
                    Ok(health) if health.is_healthy() => StepSuccess::ok(
                        &serde_json::json!({ "capabilities": capabilities, "health": health }),
                        false,
                    ),
                    Ok(health) => Err(StepFailure::new(format!(
                        "runtime profile is {:?}: {}",
                        health.state, health.detail
                    ))),
                    Err(error) => Err(StepFailure::new(error)),
                }
            }
            ProvisioningStep::EnsureMemoryResources => {
                let Some(memory) = &employee.memory else {
                    return Ok(StepSuccess::skipped("no_memory_binding"));
                };
                if operation.dry_run && mode == ProvisioningMode::Create {
                    return Ok(StepSuccess::skipped("dry_run_create"));
                }
                let request = MemoryResourceRequest {
                    employee_id: employee.id.clone(),
                    mode,
                    binding: memory.clone(),
                    idempotency_key: key.to_owned(),
                };
                match self.memory.ensure_resources(&request).await {
                    Ok(outcome) => {
                        let violation = outcome
                            .outcomes()
                            .iter()
                            .find_map(|resource| check_ownership(mode, resource).err());
                        match violation {
                            Some(failure) => Err(failure),
                            None => StepSuccess::ok(&outcome, outcome.any_adopted()),
                        }
                    }
                    Err(error) => Err(StepFailure::new(error)),
                }
            }
            ProvisioningStep::EnsureOfficeIdentity => {
                let expected = match OfficePublicKey::parse_hex(&employee.office.public_key) {
                    Ok(key) => key,
                    Err(error) => return Ok(Err(StepFailure::new(error))),
                };
                let signer = match self
                    .office
                    .verify_signer(&employee.office.signer_ref, &expected)
                    .await
                {
                    Ok(verification) => verification,
                    Err(error) => return Ok(Err(StepFailure::new(error))),
                };
                if !signer.matches_expected {
                    return Ok(Err(StepFailure::new(
                        "signer does not produce the configured public key",
                    )));
                }
                if operation.dry_run && mode == ProvisioningMode::Create {
                    return Ok(StepSuccess::ok(
                        &serde_json::json!({ "signer": signer, "membership": "planned_create" }),
                        false,
                    ));
                }
                let request = OfficeMembershipRequest {
                    employee_id: employee.id.clone(),
                    mode,
                    binding: employee.office.clone(),
                    idempotency_key: key.to_owned(),
                };
                match self.office.ensure_membership(&request).await {
                    Ok(membership) => match check_ownership(mode, &membership) {
                        Ok(()) => StepSuccess::ok(
                            &serde_json::json!({ "signer": signer, "membership": membership }),
                            membership.ownership.is_adopted(),
                        ),
                        Err(failure) => Err(failure),
                    },
                    Err(error) => Err(StepFailure::new(error)),
                }
            }
            ProvisioningStep::PublishOfficeProfile => {
                if operation.dry_run {
                    return Ok(StepSuccess::skipped("dry_run"));
                }
                match self
                    .office
                    .publish_profile(&employee.id, &employee.office, &employee.name, key)
                    .await
                {
                    Ok(publication) => StepSuccess::ok(&publication, false),
                    Err(error) => Err(StepFailure::new(error)),
                }
            }
            ProvisioningStep::ProbeHealth => {
                let evidence = match self.probe(operation).await {
                    Ok(evidence) => evidence,
                    Err(failure) => return Ok(Err(failure)),
                };
                let requires_memory = employee.memory.is_some();
                if operation.dry_run && mode == ProvisioningMode::Create {
                    // Nothing exists yet to probe; record the plan only.
                    return Ok(StepSuccess::skipped("dry_run_create"));
                }
                match evaluate_activation_gates(&evidence, requires_memory) {
                    Ok(()) => StepSuccess::ok(
                        &serde_json::json!({ "evidence": evidence, "gates": "passed" }),
                        false,
                    ),
                    Err(failures) => {
                        let summary = failures
                            .iter()
                            .map(|failure| format!("{failure:?}"))
                            .collect::<Vec<_>>()
                            .join("; ");
                        Err(StepFailure::new(format!(
                            "activation gates failed: {summary}"
                        )))
                    }
                }
            }
            ProvisioningStep::ActivateRevision => {
                // Handled by `activate`; never reached.
                Err(StepFailure::new(
                    "activation must go through the activation path",
                ))
            }
        };
        Ok(outcome)
    }

    /// Collects the activation evidence from every adapter.
    async fn probe(
        &self,
        operation: &ProvisioningOperation,
    ) -> std::result::Result<GateEvidence, StepFailure> {
        let employee = &operation.effective_employee();
        let runtime_capabilities = self
            .runtime
            .probe_capabilities()
            .await
            .map_err(StepFailure::new)?;
        let runtime_health = self
            .runtime
            .health(&employee.runtime)
            .await
            .map_err(StepFailure::new)?;
        let (memory_capabilities, memory_health) = match &employee.memory {
            Some(memory) => (
                Some(
                    self.memory
                        .probe_capabilities(memory)
                        .await
                        .map_err(StepFailure::new)?,
                ),
                Some(self.memory.health(memory).await.map_err(StepFailure::new)?),
            ),
            None => (None, None),
        };
        let expected =
            OfficePublicKey::parse_hex(&employee.office.public_key).map_err(StepFailure::new)?;
        let office_membership = self
            .office
            .membership_health(&expected)
            .await
            .map_err(StepFailure::new)?;
        let signer = self
            .office
            .verify_signer(&employee.office.signer_ref, &expected)
            .await
            .map_err(StepFailure::new)?;
        Ok(GateEvidence {
            runtime_capabilities: Some(runtime_capabilities),
            runtime_health: Some(runtime_health),
            memory_capabilities,
            memory_health,
            office_membership: Some(office_membership),
            signer: Some(signer),
        })
    }

    /// Activation: re-evaluates the gate from the durable probe evidence and
    /// commits the revision, bindings, aliases, employee status, the step, and
    /// the operation result in one repository transaction.
    async fn activate(
        &self,
        scope: &CompanyScope,
        operation: &ProvisioningOperation,
        running: &StepRecord,
    ) -> Result<std::result::Result<ProvisioningOperation, StepFailure>> {
        if operation.dry_run {
            let mut skipped = running.clone();
            skipped.state = StepState::Skipped;
            skipped.result = serde_json::json!({ "skipped": "dry_run" });
            skipped.finished_at = Some(Utc::now());
            self.repository
                .record_step(scope, operation.id, &skipped)
                .await?;
            let mut updated = operation.clone();
            replace_step(&mut updated, skipped);
            return Ok(Ok(updated));
        }

        let employee = &operation.effective_employee();
        let probe = operation.step(ProvisioningStep::ProbeHealth);
        let evidence: Option<GateEvidence> = probe
            .filter(|record| record.state == StepState::Succeeded)
            .and_then(|record| record.receipt("evidence"));
        let Some(evidence) = evidence else {
            return Ok(Err(StepFailure::new(
                "activation requires succeeded probe_health evidence",
            )));
        };
        if let Err(failures) = evaluate_activation_gates(&evidence, employee.memory.is_some()) {
            return Ok(Err(StepFailure::new(format!(
                "activation gates failed: {failures:?}"
            ))));
        }
        let unfinished = ProvisioningStep::ALL
            .iter()
            .filter(|step| **step != ProvisioningStep::ActivateRevision)
            .filter(|step| {
                !operation
                    .step(**step)
                    .is_some_and(|record| record.state.is_done())
            })
            .map(|step| step.name())
            .collect::<Vec<_>>();
        if !unfinished.is_empty() {
            return Ok(Err(StepFailure::new(format!(
                "activation requires every prior step to be done; unfinished: {}",
                unfinished.join(", ")
            ))));
        }

        let mut active = employee.clone();
        active.status = EmployeeStatus::Active;
        let fingerprint: [u8; 32] = Sha256::digest(&serde_json::to_vec(&active)?).into();
        let now = Utc::now();
        let mut activation_step = running.clone();
        activation_step.state = StepState::Succeeded;
        activation_step.finished_at = Some(now);
        let activation = RevisionActivation {
            employee: active,
            provisioning_mode: operation.resource_mode(),
            manifest_fingerprint: fingerprint,
            activation_step,
            runtime_validated_at: now,
            memory_validated_at: employee.memory.as_ref().map(|_| now),
            office_verified_at: now,
        };
        let revision_id = self
            .repository
            .activate_revision(scope, operation.id, &activation)
            .await?;
        let mut activated = self.load(scope, operation.id).await?;
        if activated.result_revision_id != Some(revision_id)
            || activated.status != OperationStatus::Succeeded
        {
            return Err(ProvisioningError::Inconsistent {
                operation_id: operation.id,
                detail: "activation did not record the succeeded revision",
            }
            .into());
        }
        if let Some(record) = activated.step_mut(ProvisioningStep::ActivateRevision) {
            record.result = merge(
                record.result.clone(),
                serde_json::json!({ "revision_id": revision_id }),
            );
        }
        Ok(Ok(activated))
    }

    /// Undoes one succeeded step without ever touching adopted resources.
    async fn compensate_step(
        &self,
        operation: &ProvisioningOperation,
        record: &StepRecord,
        retained_adopted: &mut Vec<String>,
        deleted: &mut Vec<String>,
    ) -> std::result::Result<serde_json::Value, StepFailure> {
        let key = format!("{}:compensate", record.idempotency_key);
        match record.step {
            ProvisioningStep::EnsureRuntimeProfile => {
                let outcome: ResourceOutcome = serde_json::from_value(record.result.clone())
                    .map_err(|_| StepFailure::new("runtime receipt is unreadable"))?;
                if outcome.ownership.is_adopted() || record.adopted_existing {
                    retained_adopted.push(outcome.resource_ref.clone());
                    return Ok(serde_json::json!({ "retained_adopted": [outcome.resource_ref] }));
                }
                self.runtime
                    .delete_created_profile(&outcome.resource_ref, &key)
                    .await
                    .map_err(StepFailure::new)?;
                deleted.push(outcome.resource_ref.clone());
                Ok(serde_json::json!({ "deleted": [outcome.resource_ref] }))
            }
            ProvisioningStep::EnsureMemoryResources => {
                let outcome: MemoryResourceOutcome = serde_json::from_value(record.result.clone())
                    .map_err(|_| StepFailure::new("memory receipt is unreadable"))?;
                let mut retained = Vec::new();
                let mut removed = Vec::new();
                // Peers before workspace so a partial failure never orphans peers.
                for resource in [
                    &outcome.employee_peer,
                    &outcome.user_peer,
                    &outcome.workspace,
                ] {
                    if resource.ownership.is_adopted() {
                        retained.push(resource.resource_ref.clone());
                        continue;
                    }
                    self.memory
                        .delete_created_resource(&resource.resource_ref, &key)
                        .await
                        .map_err(StepFailure::new)?;
                    removed.push(resource.resource_ref.clone());
                }
                retained_adopted.extend(retained.iter().cloned());
                deleted.extend(removed.iter().cloned());
                Ok(serde_json::json!({ "retained_adopted": retained, "deleted": removed }))
            }
            ProvisioningStep::EnsureOfficeIdentity => {
                let membership: Option<ResourceOutcome> = record.receipt("membership");
                let Some(membership) = membership else {
                    return Ok(serde_json::json!({ "retained": "no_membership_created" }));
                };
                if membership.ownership.is_adopted() || record.adopted_existing {
                    retained_adopted.push(membership.resource_ref.clone());
                    return Ok(
                        serde_json::json!({ "retained_adopted": [membership.resource_ref] }),
                    );
                }
                self.office
                    .remove_created_membership(&membership.resource_ref, &key)
                    .await
                    .map_err(StepFailure::new)?;
                deleted.push(membership.resource_ref.clone());
                Ok(serde_json::json!({ "deleted": [membership.resource_ref] }))
            }
            ProvisioningStep::ActivateRevision => {
                // Never deactivate or delete a revision during compensation.
                Err(StepFailure::new(format!(
                    "operation {} activated a revision; compensation cannot undo activation",
                    operation.id
                )))
            }
            ProvisioningStep::ReserveEmployeeIdentity => {
                Ok(serde_json::json!({ "retained": "employee_identity" }))
            }
            ProvisioningStep::PublishOfficeProfile => {
                Ok(serde_json::json!({ "retained": "office_profile_publication" }))
            }
            ProvisioningStep::ValidateManifest
            | ProvisioningStep::ResolveCredentialReferences
            | ProvisioningStep::ValidateRuntimeProfile
            | ProvisioningStep::ProbeHealth => Ok(serde_json::json!({ "retained": "read_only" })),
        }
    }
}

/// Fails closed when an adapter's reported ownership contradicts the mode.
fn check_ownership(
    mode: ProvisioningMode,
    outcome: &ResourceOutcome,
) -> std::result::Result<(), StepFailure> {
    match (mode, outcome.ownership) {
        (ProvisioningMode::Adopt, ResourceOwnership::Created) => Err(StepFailure::new(format!(
            "adapter created {} during an adopt request; contract violation",
            outcome.resource_ref
        ))),
        (ProvisioningMode::Create, ResourceOwnership::Adopted) => Err(StepFailure::new(format!(
            "adapter adopted {} during a create request; contract violation",
            outcome.resource_ref
        ))),
        _ => Ok(()),
    }
}

fn replace_step(operation: &mut ProvisioningOperation, record: StepRecord) {
    if let Some(existing) = operation.step_mut(record.step) {
        *existing = record;
    }
}

fn merge(base: serde_json::Value, extra: serde_json::Value) -> serde_json::Value {
    match (base, extra) {
        (serde_json::Value::Object(mut base), serde_json::Value::Object(extra)) => {
            base.extend(extra);
            serde_json::Value::Object(base)
        }
        (base, serde_json::Value::Object(mut extra)) => {
            extra.insert("previous".to_owned(), base);
            serde_json::Value::Object(extra)
        }
        (base, _) => base,
    }
}

impl From<ProvisioningError> for ControlError {
    fn from(error: ProvisioningError) -> Self {
        ControlError::Provisioning(error)
    }
}
