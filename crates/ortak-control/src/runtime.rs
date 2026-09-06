//! Runtime adapter port (Architecture v0 §4.4), Hermes-oriented.
//!
//! The port maps an [`RuntimeBinding`] to an external profile, model,
//! workspace, tool policy, and opaque credential references. It never makes
//! the runtime the source of Employee truth and never sees credential values:
//! adapters resolve [`CredentialRef`]s through their own authorized resolver.
//!
//! Capabilities are probed, not assumed from a version string. A deployed
//! runtime that lacks a required capability fails activation.

use std::collections::BTreeSet;
use std::fmt;

use chrono::{DateTime, Utc};
use ortak_domain::{CredentialRef, EmployeeId, PermissionPolicy, ProvisioningMode, RuntimeBinding};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::adapter::{Detail, HealthReport, ResourceOutcome};
use crate::run_event::RunEventPayload;

/// Runtime operations an adapter may support; probed at startup.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCapability {
    /// Report the health of a bound profile.
    HealthProbe,
    /// Inspect an existing profile without modifying it (required for adopt).
    ProfileInspect,
    /// Create a new profile (required for create mode).
    ProfileCreate,
    /// Delete a profile Ortak created (compensation of created resources only).
    ProfileDelete,
    /// Start a run.
    RunStart,
    /// Read ordered run events from a cursor.
    RunEvents,
    /// Cancel a run.
    RunCancel,
    /// Find the receipt of a start by stable key without starting execution.
    RunLookup,
    /// Cancel by stable start key, including a durable pre-start tombstone.
    RunCancelStart,
    /// Execute the selected immutable workspace text-read tool through the central worker.
    WorkspaceTextRead,
    /// Separately validated protected DM transport with a sealed volatile child input.
    ConfidentialDmV1,
}

/// Capabilities every activated runtime binding must support.
pub const ACTIVATION_REQUIRED_CAPABILITIES: [RuntimeCapability; 7] = [
    RuntimeCapability::HealthProbe,
    RuntimeCapability::ProfileInspect,
    RuntimeCapability::RunStart,
    RuntimeCapability::RunEvents,
    RuntimeCapability::RunCancel,
    RuntimeCapability::RunLookup,
    RuntimeCapability::RunCancelStart,
];

/// Probed capability set for one adapter deployment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeCapabilities {
    /// Adapter name, e.g. `hermes`.
    pub adapter: String,
    /// Adapter-versioned API shape that was probed, e.g. `hermes-http/v1`.
    pub api_version: String,
    /// Supported operations.
    pub capabilities: BTreeSet<RuntimeCapability>,
}

impl RuntimeCapabilities {
    /// True when the operation is supported.
    pub fn supports(&self, capability: RuntimeCapability) -> bool {
        self.capabilities.contains(&capability)
    }

    /// Required capabilities that are missing, in stable order.
    pub fn missing(&self, required: &[RuntimeCapability]) -> Vec<RuntimeCapability> {
        required
            .iter()
            .copied()
            .filter(|capability| !self.supports(*capability))
            .collect()
    }
}

/// Create-or-adopt request for one employee's runtime profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeResourceRequest {
    /// Employee the profile belongs to.
    pub employee_id: EmployeeId,
    /// Create a new profile or adopt the one at `binding.profile_ref`.
    pub mode: ProvisioningMode,
    /// Secret-free binding.
    pub binding: RuntimeBinding,
    /// Step idempotency key; a retried request must not duplicate a create.
    pub idempotency_key: String,
}

/// Adapter-side run correlation reference.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RuntimeRunRef(pub String);

impl fmt::Display for RuntimeRunRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Adapter cursor into a run's ordered event stream.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RuntimeCursor(pub String);

/// Server-derived conversation/work references and bounded, provenance-tagged
/// memory snippets. Memory content remains untrusted data; credentials are absent.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunContext {
    /// Office conversation reference.
    pub conversation_ref: Option<String>,
    /// Triggering message id (hex) for `reply` intents.
    pub reply_to_message_id: Option<String>,
    /// Attached work item.
    pub work_item_id: Option<Uuid>,
    /// Bounded, provenance-tagged memory snippets already recalled by the
    /// control layer.
    pub memory_context: Vec<String>,
}

/// Everything a runtime needs to start one run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunSpec {
    /// Durable run id (already inserted in `runs`).
    pub run_id: Uuid,
    /// Employee executing the run.
    pub employee_id: EmployeeId,
    /// Immutable revision the run is pinned to.
    pub revision_id: Uuid,
    /// Runtime binding of that revision.
    pub binding: RuntimeBinding,
    /// Authoritative permission policy from the same immutable revision.
    /// Transport and structural validation do not enforce tool access; the
    /// runtime adapter must enforce this policy at its tool boundary.
    pub permissions: PermissionPolicy,
    /// Bounded input text.
    pub input: String,
    /// Trusted context.
    pub context: RunContext,
    /// Idempotency key; a retried start must return the same run reference.
    pub idempotency_key: String,
}

/// Ceiling for run input text handed to a runtime.
pub const MAX_RUN_INPUT_BYTES: usize = 64 * 1024;

impl RunSpec {
    /// Validates bounds before any adapter call.
    pub fn validate(&self) -> Result<(), RuntimeError> {
        self.permissions
            .validate()
            .map_err(|_| RuntimeError::InvalidSpec {
                detail: Detail::new("run permission policy is invalid"),
            })?;
        if self.input.trim().is_empty() || self.input.len() > MAX_RUN_INPUT_BYTES {
            return Err(RuntimeError::InvalidSpec {
                detail: Detail::new("run input is empty or above the ceiling"),
            });
        }
        if self.idempotency_key.trim().is_empty() {
            return Err(RuntimeError::InvalidSpec {
                detail: Detail::new("run idempotency key is empty"),
            });
        }
        if self.context.memory_context.len() > 64
            || self
                .context
                .memory_context
                .iter()
                .any(|item| item.len() > 8 * 1024)
        {
            return Err(RuntimeError::InvalidSpec {
                detail: Detail::new("memory context exceeds bounds"),
            });
        }
        Ok(())
    }
}

/// Acknowledgement that a run was started.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunStartReceipt {
    /// Adapter-side reference for events and cancellation.
    pub runtime_run_ref: RuntimeRunRef,
    /// When the runtime accepted the run.
    pub started_at: DateTime<Utc>,
}

/// One adapter event with its resume cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeEvent {
    /// Cursor identifying this event; replaying it is rejected by the store.
    pub cursor: RuntimeCursor,
    /// When the runtime observed it.
    pub occurred_at: DateTime<Utc>,
    /// Raw (not yet normalized) payload.
    pub payload: RunEventPayload,
}

/// Ordered batch of events after a cursor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeEventBatch {
    /// Events in stream order.
    pub events: Vec<RuntimeEvent>,
    /// True when the runtime reported a terminal state and no more events follow.
    pub terminal: bool,
}

/// Terminal acknowledgement of a cancel request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelOutcome {
    /// The run was cancelled by this request.
    Cancelled,
    /// The run had already reached a terminal state.
    AlreadyTerminal,
}

/// Terminal acknowledgement for cancelling a stable start key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CancelStartReceipt {
    /// Existing runtime identity, absent when a tombstone prevented any start.
    pub runtime_run_ref: Option<RuntimeRunRef>,
    /// Confirmed terminal result; a pending request is never an acknowledgement.
    pub outcome: CancelOutcome,
}

/// Runtime adapter failures. Details are bounded and never carry secrets.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RuntimeError {
    /// The deployed runtime lacks a required operation.
    #[error("runtime does not support {capability:?}")]
    Unsupported {
        /// Missing capability.
        capability: RuntimeCapability,
    },
    /// The runtime could not be reached or answered with a transport error.
    #[error("runtime unavailable: {detail}")]
    Unavailable {
        /// Bounded detail.
        detail: Detail,
    },
    /// Adopt mode named a profile that does not exist.
    #[error("runtime profile not found: {profile_ref}")]
    ProfileNotFound {
        /// Requested profile reference.
        profile_ref: String,
    },
    /// Create mode would overwrite an existing profile; Ortak never replaces.
    #[error("runtime profile already exists: {profile_ref}")]
    ProfileExists {
        /// Conflicting profile reference.
        profile_ref: String,
    },
    /// A credential reference could not be resolved. Only the reference is
    /// reported, never any value.
    #[error("credential reference cannot be resolved: {credential_ref}")]
    CredentialUnresolvable {
        /// Opaque reference.
        credential_ref: String,
    },
    /// The request violates a local bound.
    #[error("invalid run spec: {detail}")]
    InvalidSpec {
        /// Bounded detail.
        detail: Detail,
    },
    /// The run reference is unknown to the runtime.
    #[error("unknown runtime run: {runtime_run_ref}")]
    UnknownRun {
        /// Unknown reference.
        runtime_run_ref: RuntimeRunRef,
    },
    /// The runtime rejected the request for a policy or validation reason.
    #[error("runtime rejected the request: {detail}")]
    Rejected {
        /// Bounded detail.
        detail: Detail,
    },
}

impl RuntimeError {
    /// Builds a credential error from an opaque reference.
    pub fn credential(credential_ref: &CredentialRef) -> Self {
        Self::CredentialUnresolvable {
            credential_ref: credential_ref.as_str().to_owned(),
        }
    }

    /// True when a retry may succeed without operator action.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
}

/// Runtime port. Implementations select statically; services are generic.
///
/// Contract for [`RuntimeAdapter::ensure_profile`]:
/// - `Adopt`: the profile at `binding.profile_ref` must already exist; the
///   adapter returns it as [`crate::adapter::ResourceOwnership::Adopted`]
///   without modifying, replacing, or recreating it. A missing profile is
///   [`RuntimeError::ProfileNotFound`], never an implicit create.
/// - `Create`: the adapter creates a new profile once per idempotency key and
///   returns it as `Created`. An existing profile at the target is
///   [`RuntimeError::ProfileExists`], never an overwrite.
#[allow(async_fn_in_trait)]
pub trait RuntimeAdapter {
    /// Stable adapter name stored in bindings and runs.
    fn adapter_name(&self) -> &str;

    /// Probes the deployed runtime's supported operations and API shape.
    async fn probe_capabilities(&self) -> Result<RuntimeCapabilities, RuntimeError>;

    /// Reports the health of a bound profile: exists, config/workspace valid,
    /// credential references resolvable.
    async fn health(&self, binding: &RuntimeBinding) -> Result<HealthReport, RuntimeError>;

    /// Creates or adopts the employee's runtime profile (see trait docs).
    async fn ensure_profile(
        &self,
        request: &RuntimeResourceRequest,
    ) -> Result<ResourceOutcome, RuntimeError>;

    /// Deletes a profile this operation created. The saga never calls this
    /// for adopted resources; an adapter must still refuse when it can tell
    /// the profile was not created by Ortak.
    async fn delete_created_profile(
        &self,
        resource_ref: &str,
        idempotency_key: &str,
    ) -> Result<(), RuntimeError>;

    /// Starts a run; idempotent per `spec.idempotency_key`.
    async fn start_run(&self, spec: &RunSpec) -> Result<RunStartReceipt, RuntimeError>;

    /// Starts with a separately frozen workspace grant. Legacy adapters remain
    /// unchanged for absent grants and must explicitly implement selected tools.
    async fn start_run_with_workspace(
        &self,
        spec: &RunSpec,
        workspace: Option<&crate::workspace::WorkspaceGrant>,
    ) -> Result<RunStartReceipt, RuntimeError> {
        if workspace.is_some() {
            return Err(RuntimeError::Unsupported {
                capability: RuntimeCapability::WorkspaceTextRead,
            });
        }
        self.start_run(spec).await
    }

    /// Looks up an existing receipt without causing execution. This recovers a
    /// lost start acknowledgement using the original stable idempotency key.
    async fn lookup_start(
        &self,
        _idempotency_key: &str,
    ) -> Result<Option<RunStartReceipt>, RuntimeError> {
        Err(RuntimeError::Unsupported {
            capability: RuntimeCapability::RunLookup,
        })
    }

    /// Persists a cancellation tombstone for the stable start key, even when
    /// no start is registered yet. Every later start with that key must remain
    /// stopped. Success requires confirmed termination of contained execution;
    /// a transient failure must propagate so the durable request can retry.
    async fn cancel_start(
        &self,
        _idempotency_key: &str,
        _reason: &str,
    ) -> Result<CancelStartReceipt, RuntimeError> {
        Err(RuntimeError::Unsupported {
            capability: RuntimeCapability::RunCancelStart,
        })
    }

    /// Reads up to `limit` ordered events after `after` (`None` from the start).
    async fn next_events(
        &self,
        runtime_run_ref: &RuntimeRunRef,
        after: Option<&RuntimeCursor>,
        limit: usize,
    ) -> Result<RuntimeEventBatch, RuntimeError>;

    /// Cancels a run and returns a terminal acknowledgement.
    async fn cancel_run(
        &self,
        runtime_run_ref: &RuntimeRunRef,
        reason: &str,
    ) -> Result<CancelOutcome, RuntimeError>;
}

impl<T: RuntimeAdapter + ?Sized> RuntimeAdapter for &T {
    fn adapter_name(&self) -> &str {
        (**self).adapter_name()
    }

    async fn probe_capabilities(&self) -> Result<RuntimeCapabilities, RuntimeError> {
        (**self).probe_capabilities().await
    }

    async fn health(&self, binding: &RuntimeBinding) -> Result<HealthReport, RuntimeError> {
        (**self).health(binding).await
    }

    async fn ensure_profile(
        &self,
        request: &RuntimeResourceRequest,
    ) -> Result<ResourceOutcome, RuntimeError> {
        (**self).ensure_profile(request).await
    }

    async fn delete_created_profile(
        &self,
        resource_ref: &str,
        idempotency_key: &str,
    ) -> Result<(), RuntimeError> {
        (**self)
            .delete_created_profile(resource_ref, idempotency_key)
            .await
    }

    async fn start_run(&self, spec: &RunSpec) -> Result<RunStartReceipt, RuntimeError> {
        (**self).start_run(spec).await
    }

    async fn start_run_with_workspace(
        &self,
        spec: &RunSpec,
        workspace: Option<&crate::workspace::WorkspaceGrant>,
    ) -> Result<RunStartReceipt, RuntimeError> {
        (**self).start_run_with_workspace(spec, workspace).await
    }

    async fn lookup_start(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<RunStartReceipt>, RuntimeError> {
        (**self).lookup_start(idempotency_key).await
    }

    async fn cancel_start(
        &self,
        idempotency_key: &str,
        reason: &str,
    ) -> Result<CancelStartReceipt, RuntimeError> {
        (**self).cancel_start(idempotency_key, reason).await
    }

    async fn next_events(
        &self,
        runtime_run_ref: &RuntimeRunRef,
        after: Option<&RuntimeCursor>,
        limit: usize,
    ) -> Result<RuntimeEventBatch, RuntimeError> {
        (**self).next_events(runtime_run_ref, after, limit).await
    }

    async fn cancel_run(
        &self,
        runtime_run_ref: &RuntimeRunRef,
        reason: &str,
    ) -> Result<CancelOutcome, RuntimeError> {
        (**self).cancel_run(runtime_run_ref, reason).await
    }
}
