#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Server-bound Honcho 3.1.1 / `ortak-honcho/1` memory adapter.
//!
//! Static protocol or resource health never grants memory I/O. Each fresh owned
//! binding needs an explicit write/scoped-recall validation, expires independently,
//! and loses that evidence at restart. Adoption is native list-only; deletion and
//! peer-global representations are deliberately unavailable.

mod config;
mod gate;
mod http;
mod resources;
mod validation;
mod wire;
pub use validation::{MemoryRoundtripReceipt, MemoryRoundtripRequest};

pub use config::{
    HonchoDeploymentSelection, HonchoEmployeeBinding, HonchoMemoryConfig, ResolvedHonchoToken,
};
pub use ortak_control::memory::MemoryError;

use gate::{IoGate, Witness};
use ortak_control::{
    adapter::{Detail, HealthReport},
    memory::{
        MemoryAdapter, MemoryCapabilities, MemoryCapability, MemoryHealthReport, MemoryRecall,
        MemoryRecallRequest, MemoryRecord, MemoryResourceOutcome, MemoryResourceRequest,
        MemoryWriteReceipt, MemoryWriteRequest,
    },
    CompanyScope,
};
use ortak_domain::{EmployeeId, MemoryBinding, ProvisioningMode};
use reqwest::Method;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use uuid::Uuid;

/// Reviewed extension wire protocol.
pub const PROTOCOL: &str = "ortak-honcho/1";
/// Reviewed upstream Honcho API version.
pub const HONCHO_VERSION: &str = "3.1.1";

fn invalid(detail: &'static str) -> MemoryError {
    MemoryError::InvalidRequest {
        detail: Detail::new(detail),
    }
}
fn rejected(detail: &'static str) -> MemoryError {
    MemoryError::Rejected {
        detail: Detail::new(detail),
    }
}
fn unavailable(detail: &'static str) -> MemoryError {
    MemoryError::Unavailable {
        detail: Detail::new(detail),
    }
}
fn unsupported(capability: MemoryCapability) -> MemoryError {
    MemoryError::Unsupported { capability }
}

/// Fixed company/deployment/cohort adapter. Secrets and HTTP client are not Debug.
pub struct HonchoMemoryAdapter {
    company_id: Uuid,
    config: HonchoMemoryConfig,
    http: http::Http,
    witnesses: Mutex<BTreeMap<EmployeeId, Witness>>,
    creation_receipts: Mutex<BTreeMap<EmployeeId, resources::ResourceIdentity>>,
    operations: Semaphore,
}

impl HonchoMemoryAdapter {
    /// Constructs only from server-resolved company scope and explicit token resolution.
    pub fn new(
        scope: &CompanyScope,
        config: HonchoMemoryConfig,
        token: ResolvedHonchoToken,
    ) -> Result<Self, MemoryError> {
        Self::for_company(scope.company_id(), config, token)
    }

    fn for_company(
        company_id: Uuid,
        config: HonchoMemoryConfig,
        token: ResolvedHonchoToken,
    ) -> Result<Self, MemoryError> {
        let origin = config::validate(&config)?;
        if company_id.is_nil() || token.token_ref != config.deployment.token_ref {
            return Err(invalid(
                "memory credential or company binding does not match",
            ));
        }
        let http = http::Http::new(origin, token, config.request_timeout)?;
        Ok(Self {
            company_id,
            config,
            http,
            witnesses: Mutex::new(BTreeMap::new()),
            creation_receipts: Mutex::new(BTreeMap::new()),
            operations: Semaphore::new(8),
        })
    }

    fn allowed(
        &self,
        employee: Option<&EmployeeId>,
        binding: &MemoryBinding,
    ) -> Result<&HonchoEmployeeBinding, MemoryError> {
        self.config
            .employees
            .iter()
            .find(|allowed| {
                &allowed.binding == binding && employee.is_none_or(|id| id == &allowed.employee_id)
            })
            .ok_or_else(|| {
                invalid("memory request is outside the authorized company/employee binding")
            })
    }

    async fn bounded<T>(
        &self,
        future: impl Future<Output = Result<T, MemoryError>>,
    ) -> Result<T, MemoryError> {
        tokio::time::timeout(Duration::from_secs(30), async {
            let _permit = self
                .operations
                .acquire()
                .await
                .map_err(|_| unavailable("memory operation limiter unavailable"))?;
            future.await
        })
        .await
        .map_err(|_| unavailable("memory operation deadline exceeded"))?
    }

    fn witnessed(&self, allowed: &HonchoEmployeeBinding) -> Result<bool, MemoryError> {
        Ok(allowed.mode == ProvisioningMode::Create
            && self
                .witnesses
                .lock()
                .map_err(|_| unavailable("memory validation state unavailable"))?
                .get(&allowed.employee_id)
                .is_some_and(|state| {
                    state
                        .expires
                        .is_some_and(|expires| expires > Instant::now())
                }))
    }

    fn require_witness(
        &self,
        allowed: &HonchoEmployeeBinding,
        capability: MemoryCapability,
    ) -> Result<IoGate, MemoryError> {
        let states = self
            .witnesses
            .lock()
            .map_err(|_| unavailable("memory validation state unavailable"))?;
        match states.get(&allowed.employee_id) {
            Some(state)
                if allowed.mode == ProvisioningMode::Create
                    && state
                        .expires
                        .is_some_and(|expires| expires > Instant::now()) =>
            {
                Ok(IoGate::Witness(state.generation, capability))
            }
            _ => Err(unsupported(capability)),
        }
    }

    async fn protocol(&self) -> Result<(), MemoryError> {
        let (_, response) = self
            .http
            .request(Method::GET, "/v3/ortak/protocol", None)
            .await?;
        if response != json!({"protocol":PROTOCOL,"honcho_version":HONCHO_VERSION}) {
            return Err(rejected(
                "memory deployment protocol differs from explicit selection",
            ));
        }
        Ok(())
    }

    async fn require_owned(&self, allowed: &HonchoEmployeeBinding) -> Result<(), MemoryError> {
        let resources = self.inspect_resources(allowed).await?;
        if !resources.owned {
            return Err(rejected(
                "memory resources are not the configured owned bundle",
            ));
        }
        Ok(())
    }

    async fn write_on(
        &self,
        allowed: &HonchoEmployeeBinding,
        request: &MemoryWriteRequest,
        gate: IoGate,
    ) -> Result<(MemoryWriteReceipt, Vec<MemoryRecord>), MemoryError> {
        let body = wire::write_body(self.company_id, allowed, request)?;
        let session = wire::session(self.company_id, allowed, &request.scope)?;
        self.check_gate(allowed, gate)?;
        let (_, response) = self
            .http
            .request(
                Method::POST,
                &format!(
                    "/v3/ortak/workspaces/{}/sessions/{session}/remember",
                    allowed.binding.workspace
                ),
                Some(body.clone()),
            )
            .await?;
        let (receipt_ref, records) =
            wire::validate_write(self.company_id, allowed, request, &session, &body, response)?;
        Ok((
            MemoryWriteReceipt {
                receipt_ref,
                written: records.len(),
            },
            records,
        ))
    }

    async fn recall_on(
        &self,
        allowed: &HonchoEmployeeBinding,
        request: &MemoryRecallRequest,
        gate: IoGate,
    ) -> Result<MemoryRecall, MemoryError> {
        request.validate()?;
        wire::check_scope(allowed, &request.scope)?;
        if request.query.len() > 4096
            || request.query.contains('\0')
            || request.budget.max_records > 100
        {
            return Err(invalid("memory recall exceeds extension bounds"));
        }
        let session = wire::session(self.company_id, allowed, &request.scope)?;
        let body = json!({"company_id":self.company_id,"employee_id":allowed.employee_id,"scope":wire::scope_value(&request.scope)?,"query":request.query,"max_records":request.budget.max_records,"max_bytes":request.budget.max_bytes});
        self.check_gate(allowed, gate)?;
        let (_, response) = self
            .http
            .request(
                Method::POST,
                &format!(
                    "/v3/ortak/workspaces/{}/sessions/{session}/recall",
                    allowed.binding.workspace
                ),
                Some(body),
            )
            .await?;
        wire::validate_recall(
            allowed,
            &request.scope,
            request.budget.max_records,
            request.budget.max_bytes,
            response,
        )
    }
}

impl MemoryAdapter for HonchoMemoryAdapter {
    fn adapter_name(&self) -> &str {
        "honcho"
    }

    async fn probe_capabilities(
        &self,
        binding: &MemoryBinding,
    ) -> Result<MemoryCapabilities, MemoryError> {
        self.bounded(async {
            let allowed = self.allowed(None, binding)?;
            self.protocol().await?;
            let resources = self.inspect_resources(allowed).await?;
            let mut capabilities = BTreeSet::from([
                MemoryCapability::HealthProbe,
                MemoryCapability::ResourceInspect,
            ]);
            if allowed.mode == ProvisioningMode::Create {
                capabilities.insert(MemoryCapability::ResourceCreate);
            }
            if resources.owned && self.witnessed(allowed)? {
                capabilities.extend([MemoryCapability::Recall, MemoryCapability::Remember]);
            }
            Ok(MemoryCapabilities {
                adapter: "honcho".into(),
                api_version: PROTOCOL.into(),
                capabilities,
            })
        })
        .await
    }

    async fn health(&self, binding: &MemoryBinding) -> Result<MemoryHealthReport, MemoryError> {
        self.bounded(async {
            let allowed = self.allowed(None, binding)?;
            self.protocol().await?;
            let resources = self.inspect_resources(allowed).await?;
            let usable = resources.owned && self.witnessed(allowed)?;
            let report = |exists: bool| {
                if !exists {
                    HealthReport::unhealthy("memory resource is absent")
                } else if !usable {
                    HealthReport::degraded(
                        "memory resource exists; binding-specific roundtrip validation required",
                    )
                } else {
                    HealthReport::healthy(
                        "owned memory resource and bounded roundtrip witness verified",
                    )
                }
            };
            Ok(MemoryHealthReport {
                workspace: report(resources.workspace),
                user_peer: report(resources.user),
                employee_peer: report(resources.employee),
            })
        })
        .await
    }

    async fn ensure_resources(
        &self,
        request: &MemoryResourceRequest,
    ) -> Result<MemoryResourceOutcome, MemoryError> {
        self.bounded(async {
            let allowed = self.allowed(Some(&request.employee_id), &request.binding)?;
            if request.mode != allowed.mode || !config::key(&request.idempotency_key) {
                return Err(invalid(
                    "memory resource operation differs from authorized mode or key",
                ));
            }
            self.protocol().await?;
            match request.mode {
                ProvisioningMode::Adopt => {
                    let r = self.inspect_resources(allowed).await?;
                    if !r.workspace || !r.user || !r.employee {
                        return Err(MemoryError::ResourceNotFound {
                            resource_ref: format!("workspace:{}", allowed.binding.workspace),
                        });
                    }
                    Ok(resources::outcome(allowed, false))
                }
                ProvisioningMode::Create => {
                    let body = self.creation_body(allowed, &request.idempotency_key);
                    let (_, value) = self
                        .http
                        .request(
                            Method::POST,
                            "/v3/ortak/resources/create",
                            Some(body.clone()),
                        )
                        .await?;
                    let outcome = resources::validate_create(allowed, value)?;
                    self.retain_creation_identity(allowed, &body).await?;
                    Ok(outcome)
                }
            }
        })
        .await
    }

    async fn delete_created_resource(
        &self,
        _resource_ref: &str,
        _idempotency_key: &str,
    ) -> Result<(), MemoryError> {
        Err(unsupported(MemoryCapability::ResourceDelete))
    }

    async fn recall(&self, request: &MemoryRecallRequest) -> Result<MemoryRecall, MemoryError> {
        self.bounded(async {
            let allowed = self.allowed(Some(&request.employee_id), &request.binding)?;
            let gate = self.require_witness(allowed, MemoryCapability::Recall)?;
            self.require_owned(allowed).await?;
            self.recall_on(allowed, request, gate).await
        })
        .await
    }

    async fn remember(
        &self,
        request: &MemoryWriteRequest,
    ) -> Result<MemoryWriteReceipt, MemoryError> {
        self.bounded(async {
            let allowed = self.allowed(Some(&request.employee_id), &request.binding)?;
            let gate = self.require_witness(allowed, MemoryCapability::Remember)?;
            self.require_owned(allowed).await?;
            self.write_on(allowed, request, gate)
                .await
                .map(|(receipt, _)| receipt)
        })
        .await
    }
}

#[cfg(test)]
mod tests;
