//! Explicit private-worker memory composition and bounded validation refresh.

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
    HonchoDeploymentSelection, HonchoEmployeeBinding, HonchoMemoryAdapter, HonchoMemoryConfig,
    MemoryRoundtripRequest, ResolvedHonchoToken, HONCHO_VERSION, PROTOCOL,
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
    employees: Vec<EmployeeConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmployeeConfig {
    employee_id: EmployeeId,
    binding: MemoryBinding,
    creation_key: String,
    validation_run_id: Uuid,
    validation_recorded_at: DateTime<Utc>,
}

struct Validation {
    resource: MemoryResourceRequest,
    roundtrip: MemoryRoundtripRequest,
    next_attempt: Instant,
    ready_until: Option<Instant>,
    failures: u8,
    in_flight: bool,
}

pub(crate) struct WorkerMemory {
    adapter: Option<HonchoMemoryAdapter>,
    validations: Mutex<Vec<Validation>>,
}

impl WorkerMemory {
    pub(crate) fn disabled() -> Self {
        Self {
            adapter: None,
            validations: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn new(scope: &CompanyScope, config: MemoryConfig) -> Result<Self, &'static str> {
        if !config.validate_memory_io || config.employees.is_empty() || config.employees.len() > 64
        {
            return Err("explicit bounded memory validation configuration required");
        }
        let token = ResolvedHonchoToken::from_env(config.token_ref.clone(), &config.token_env)
            .map_err(|_| "selected memory credential unavailable")?;
        let bindings = config
            .employees
            .iter()
            .map(|entry| HonchoEmployeeBinding {
                employee_id: entry.employee_id.clone(),
                binding: entry.binding.clone(),
                mode: ProvisioningMode::Create,
                allow_company_truth: false,
                allowed_projects: BTreeSet::new(),
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
                    mode: ProvisioningMode::Create,
                    idempotency_key: entry.creation_key,
                },
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
            })
            .collect();
        Ok(Self {
            adapter: Some(adapter),
            validations: Mutex::new(validations),
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
        let (index, resource, roundtrip) = {
            let mut values = self.validations.lock().ok()?;
            let index = values.iter().position(|value| {
                !value.in_flight && value.failures < 20 && value.next_attempt <= Instant::now()
            })?;
            let value = &mut values[index];
            value.in_flight = true;
            (index, value.resource.clone(), value.roundtrip.clone())
        };
        let result = tokio::time::timeout(Duration::from_secs(6), async {
            adapter.resume_created_resources(&resource).await?;
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
