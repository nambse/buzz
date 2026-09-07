//! Explicit private-worker memory composition and bounded validation refresh.
#[path = "worker_memory/employee.rs"]
mod employee;
#[path = "worker_memory/employee_advertisement.rs"]
mod employee_advertisement;
#[path = "worker_memory/employee_exports.rs"]
mod employee_exports;
#[path = "worker_memory/selected.rs"]
mod selected;

use std::{
    collections::BTreeSet,
    sync::Mutex,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use ortak_control::{
    memory::{
        MemoryAdapter, MemoryCapabilities, MemoryCapability, MemoryError, MemoryHealthReport,
        MemoryRecall, MemoryRecallRequest, MemoryResourceOutcome, MemoryResourceRequest,
        MemoryWriteReceipt, MemoryWriteRequest,
    },
    CompanyScope,
};
use ortak_domain::{CredentialRef, EmployeeId, MemoryBinding, ProvisioningMode};
use ortak_memory::{
    HonchoCreatedResourcesReceipt, HonchoDeploymentSelection, HonchoEmployeeBinding,
    HonchoMemoryAdapter, HonchoMemoryConfig, MemoryRoundtripRequest, ResolvedHonchoToken,
    HONCHO_VERSION, PROTOCOL,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemoryConfig {
    deployment_id: Uuid,
    origin: String,
    endpoint_ref: String,
    token_ref: CredentialRef,
    token_env: String,
    // Explicit authorization to write/replay the configured diagnostic scratch
    // records. Ordinary health/probe/read-only ownership inspection never writes.
    validate_memory_io: bool,
    // New shared activation/worker recipes forbid falling back to key-only
    // recovery. Old private fragments omit this and retain their Create path.
    #[serde(default)]
    require_creation_receipts: bool,
    employees: Vec<EmployeeConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmployeeConfig {
    employee_id: EmployeeId,
    binding: MemoryBinding,
    creation_key: String,
    #[serde(default)]
    creation_receipt: Option<HonchoCreatedResourcesReceipt>,
    validation_run_id: Uuid,
    validation_recorded_at: DateTime<Utc>,
    #[serde(default)]
    reviewed_projects: BTreeSet<Uuid>,
    #[serde(default)]
    reviewed_runtime_projects: BTreeSet<Uuid>,
    #[serde(default)]
    reviewed_conversations: Vec<ConversationSelection>,
    #[serde(default)]
    reviewed_employee_destinations:
        Vec<ortak_runtime::reviewed_memory::EmployeeReviewedDestination>,
}

/// Explicit project/channel selection; it never changes Employee identity.
#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConversationSelection {
    project_id: Uuid,
    channel_id: Uuid,
}

struct Validation {
    resource: MemoryResourceRequest,
    creation_receipt: Option<HonchoCreatedResourcesReceipt>,
    roundtrip: MemoryRoundtripRequest,
    next_attempt: Instant,
    ready_until: Option<Instant>,
    failures: u8,
    in_flight: bool,
    reviewed_projects: BTreeSet<Uuid>,
    reviewed_runtime_projects: BTreeSet<Uuid>,
    reviewed_conversations: Vec<ConversationSelection>,
    reviewed_employee_destinations:
        Vec<ortak_runtime::reviewed_memory::EmployeeReviewedDestination>,
}

pub(crate) struct WorkerMemory {
    adapter: Option<HonchoMemoryAdapter>,
    validations: Mutex<Vec<Validation>>,
    reviewed_advertisement_due: Mutex<Instant>,
}

impl MemoryConfig {
    fn validate(&self, scope: &CompanyScope) -> Result<(), &'static str> {
        if !self.validate_memory_io || self.employees.is_empty() || self.employees.len() > 64 {
            return Err("explicit bounded memory validation configuration required");
        }
        let mut employees = BTreeSet::new();
        let mut workspaces = BTreeSet::new();
        let mut keys = BTreeSet::new();
        let mut projects = 0;
        let mut conversations = 0;
        let mut employee_destinations = 0;
        for entry in &self.employees {
            projects += entry.reviewed_projects.len();
            conversations += entry.reviewed_conversations.len();
            employee_destinations += entry.reviewed_employee_destinations.len();
            let mut conversation_projects = BTreeSet::new();
            let mut conversation_channels = BTreeSet::new();
            let mut destination_channels = BTreeSet::new();
            let mut destination_targets = BTreeSet::new();
            if self.deployment_id.is_nil()
                || entry.validation_run_id.is_nil()
                || entry.binding.adapter != "honcho"
                || entry.binding.endpoint_ref != self.endpoint_ref
                || entry.creation_key.is_empty()
                || entry.creation_key.len() > 200
                || !entry
                    .creation_key
                    .bytes()
                    .all(|byte| (0x21..=0x7e).contains(&byte))
                || !employees.insert(&entry.employee_id)
                || !workspaces.insert(&entry.binding.workspace)
                || !keys.insert(&entry.creation_key)
                || (self.require_creation_receipts && entry.creation_receipt.is_none())
                || entry.reviewed_projects.len() > 16
                || projects > 128
                || entry.reviewed_conversations.len() > 16
                || conversations > 128
                || entry.reviewed_employee_destinations.len() > 16
                || employee_destinations > 128
                || (!entry.reviewed_employee_destinations.is_empty()
                    && entry.creation_receipt.is_none())
                || entry.reviewed_employee_destinations.iter().any(|s| {
                    s.target_id.is_nil()
                        || s.destination_channel_id.is_nil()
                        || !destination_channels.insert(s.destination_channel_id)
                        || !destination_targets.insert(s.target_id)
                })
                || entry.reviewed_conversations.iter().any(|selection| {
                    selection.project_id.is_nil()
                        || selection.channel_id.is_nil()
                        || !entry.reviewed_projects.contains(&selection.project_id)
                        || !conversation_projects.insert(selection.project_id)
                        || !conversation_channels.insert(selection.channel_id)
                })
                || entry.reviewed_projects.iter().any(Uuid::is_nil)
                || !entry
                    .reviewed_runtime_projects
                    .is_subset(&entry.reviewed_projects)
                || (!entry.reviewed_projects.is_empty() && entry.creation_receipt.is_none())
                || entry.creation_receipt.as_ref().is_some_and(|receipt| {
                    receipt.company_id != scope.company_id()
                        || receipt.deployment_id != self.deployment_id
                        || receipt.employee_id != entry.employee_id
                        || receipt.binding != entry.binding
                        || receipt.creation_key != entry.creation_key
                })
            {
                return Err("memory recipe identity, original receipt or diagnostic differs");
            }
        }
        Ok(())
    }
}

impl WorkerMemory {
    pub(crate) fn disabled() -> Self {
        Self {
            adapter: None,
            validations: Mutex::new(Vec::new()),
            reviewed_advertisement_due: Mutex::new(Instant::now()),
        }
    }

    pub(crate) fn new(scope: &CompanyScope, config: MemoryConfig) -> Result<Self, &'static str> {
        config.validate(scope)?;
        let token = ResolvedHonchoToken::from_env(config.token_ref.clone(), &config.token_env)
            .map_err(|_| "selected memory credential unavailable")?;
        let bindings = config
            .employees
            .iter()
            .map(|entry| HonchoEmployeeBinding {
                employee_id: entry.employee_id.clone(),
                binding: entry.binding.clone(),
                mode: if entry.creation_receipt.is_some() {
                    ProvisioningMode::Adopt
                } else {
                    ProvisioningMode::Create
                },
                allow_company_truth: false,
                allowed_projects: entry.reviewed_projects.clone(),
            })
            .collect();
        let adapter = HonchoMemoryAdapter::new(
            scope,
            HonchoMemoryConfig {
                deployment: HonchoDeploymentSelection {
                    deployment_id: config.deployment_id,
                    protocol: PROTOCOL.to_owned(),
                    honcho_version: HONCHO_VERSION.to_owned(),
                    endpoint_ref: config.endpoint_ref,
                    origin: config.origin,
                    token_ref: config.token_ref,
                },
                employees: bindings,
                request_timeout: Duration::from_secs(2),
                witness_lifetime: Duration::from_secs(900),
            },
            token,
        )
        .map_err(|_| "memory binding configuration refused")?;
        let validations = config
            .employees
            .into_iter()
            .map(|entry| Validation {
                resource: MemoryResourceRequest {
                    employee_id: entry.employee_id.clone(),
                    binding: entry.binding.clone(),
                    mode: if entry.creation_receipt.is_some() {
                        ProvisioningMode::Adopt
                    } else {
                        ProvisioningMode::Create
                    },
                    idempotency_key: entry.creation_key,
                },
                creation_receipt: entry.creation_receipt,
                roundtrip: MemoryRoundtripRequest {
                    employee_id: entry.employee_id,
                    binding: entry.binding,
                    run_id: entry.validation_run_id,
                    recorded_at: entry.validation_recorded_at,
                },
                next_attempt: Instant::now(),
                ready_until: None,
                failures: 0,
                in_flight: false,
                reviewed_projects: entry.reviewed_projects,
                reviewed_runtime_projects: entry.reviewed_runtime_projects,
                reviewed_conversations: entry.reviewed_conversations,
                reviewed_employee_destinations: entry.reviewed_employee_destinations,
            })
            .collect();
        Ok(Self {
            adapter: Some(adapter),
            validations: Mutex::new(validations),
            reviewed_advertisement_due: Mutex::new(Instant::now()),
        })
    }

    pub(crate) fn ready(&self) -> bool {
        self.adapter.is_some()
            && self.validations.lock().is_ok_and(|values| {
                !values.is_empty()
                    && values.iter().all(|value| {
                        value
                            .ready_until
                            .is_some_and(|deadline| deadline > Instant::now())
                    })
            })
    }

    /// Revalidates at most one already-created bundle, after cancellation has
    /// been serviced. Stable diagnostic scope/time/key come from persisted
    /// configuration. Never creates missing resources or activates employees.
    pub(crate) async fn refresh_one(&self) -> Option<bool> {
        let adapter = self.adapter.as_ref()?;
        let (index, resource, creation_receipt, roundtrip) = {
            let mut values = self.validations.lock().ok()?;
            let index = values.iter().position(|value| {
                !value.in_flight && value.failures < 20 && value.next_attempt <= Instant::now()
            })?;
            let value = &mut values[index];
            value.in_flight = true;
            (
                index,
                value.resource.clone(),
                value.creation_receipt.clone(),
                value.roundtrip.clone(),
            )
        };
        let result = tokio::time::timeout(Duration::from_secs(6), async {
            if let Some(receipt) = creation_receipt {
                adapter.recover_created_resources(&receipt).await?;
            } else {
                adapter.resume_created_resources(&resource).await?;
            }
            adapter.validate_memory_roundtrip(&roundtrip).await
        })
        .await;
        let successful = matches!(result, Ok(Ok(_)));
        let mut values = self.validations.lock().ok()?;
        let value = &mut values[index];
        value.in_flight = false;
        if successful {
            value.failures = 0;
            // Conservative worker cache; the adapter independently enforces its
            // shorter-lived witness again at the actual network dispatch seam.
            value.ready_until = Some(Instant::now() + Duration::from_secs(840));
            value.next_attempt = Instant::now() + Duration::from_secs(300);
        } else {
            value.failures += 1;
            value.ready_until = None;
            let backoff = (30_u64 << value.failures.min(4)).min(300);
            value.next_attempt = Instant::now() + Duration::from_secs(backoff);
        }
        Some(successful)
    }

    fn selected(&self, capability: MemoryCapability) -> Result<&HonchoMemoryAdapter, MemoryError> {
        self.adapter
            .as_ref()
            .ok_or(MemoryError::Unsupported { capability })
    }
}

#[path = "worker_memory/reviewed.rs"]
mod reviewed;

impl MemoryAdapter for WorkerMemory {
    fn adapter_name(&self) -> &str {
        "honcho"
    }
    async fn probe_capabilities(
        &self,
        binding: &MemoryBinding,
    ) -> Result<MemoryCapabilities, MemoryError> {
        let mut capabilities = self
            .selected(MemoryCapability::HealthProbe)?
            .probe_capabilities(binding)
            .await?;
        capabilities.capabilities.retain(|capability| {
            !matches!(
                capability,
                MemoryCapability::ResourceCreate | MemoryCapability::ResourceDelete
            )
        });
        Ok(capabilities)
    }
    async fn health(&self, binding: &MemoryBinding) -> Result<MemoryHealthReport, MemoryError> {
        self.selected(MemoryCapability::HealthProbe)?
            .health(binding)
            .await
    }
    async fn ensure_resources(
        &self,
        _request: &MemoryResourceRequest,
    ) -> Result<MemoryResourceOutcome, MemoryError> {
        Err(MemoryError::Unsupported {
            capability: MemoryCapability::ResourceCreate,
        })
    }
    async fn delete_created_resource(
        &self,
        _reference: &str,
        _key: &str,
    ) -> Result<(), MemoryError> {
        Err(MemoryError::Unsupported {
            capability: MemoryCapability::ResourceDelete,
        })
    }
    async fn recall(&self, request: &MemoryRecallRequest) -> Result<MemoryRecall, MemoryError> {
        self.selected(MemoryCapability::Recall)?
            .recall(request)
            .await
    }
    async fn remember(
        &self,
        request: &MemoryWriteRequest,
    ) -> Result<MemoryWriteReceipt, MemoryError> {
        self.selected(MemoryCapability::Remember)?
            .remember(request)
            .await
    }
}

#[cfg(test)]
#[path = "worker_memory/tests.rs"]
mod tests;
