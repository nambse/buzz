//! Explicitly prepared memory shared by activation and execution.

use std::{collections::BTreeSet, time::Duration};

use chrono::{DateTime, Utc};
use ortak_control::credentials::{EnvCredentialBinding, EnvCredentialResolver};
use ortak_control::CompanyScope;
use ortak_domain::{CredentialRef, Employee, ProvisioningMode};
use ortak_memory::{
    HonchoCreatedResourcesReceipt, HonchoDeploymentSelection, HonchoEmployeeBinding,
    HonchoMemoryAdapter, HonchoMemoryConfig, MemoryRoundtripRequest, ResolvedHonchoToken,
    HONCHO_VERSION, PROTOCOL,
};
use serde::Deserialize;
use uuid::Uuid;

/// One operator-selected extension-owned bundle and its original diagnostic.
/// Deserialization grants no ownership or execution witness.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedMemoryConfig {
    /// Exact selected native service origin.
    pub origin: String,
    /// Opaque native service credential reference.
    pub token_ref: CredentialRef,
    /// Explicit environment name resolved only after selection validation.
    pub token_env: String,
    /// Original server-issued creation receipt, including immutable native IDs.
    pub creation_receipt: HonchoCreatedResourcesReceipt,
    /// Explicit permission to replay the retained diagnostic write and recall.
    pub validate_memory_io: bool,
    /// Original retained diagnostic run identity.
    pub validation_run_id: Uuid,
    /// Original retained diagnostic provenance time.
    pub validation_recorded_at: DateTime<Utc>,
}

impl PreparedMemoryConfig {
    /// Validates public identity/selection before any secret lookup or I/O.
    pub fn validate(&self, scope: &CompanyScope, employee: &Employee) -> Result<(), &'static str> {
        let origin = url::Url::parse(&self.origin).map_err(|_| "invalid prepared memory origin")?;
        let loopback = match origin.host() {
            Some(url::Host::Domain("localhost")) => true,
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if !(origin.scheme() == "https" || origin.scheme() == "http" && loopback)
            || origin.host().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.port() == Some(0)
        {
            return Err("invalid prepared memory origin");
        }
        EnvCredentialResolver::new(vec![EnvCredentialBinding {
            credential_ref: self.token_ref.clone(),
            environment_variable: self.token_env.clone(),
        }])
        .map_err(|_| "invalid prepared memory environment selection")?;
        let receipt = &self.creation_receipt;
        if !self.validate_memory_io
            || self.validation_run_id.is_nil()
            || receipt.company_id != scope.company_id()
            || receipt.deployment_id.is_nil()
            || receipt.employee_id != employee.id
            || employee.memory.as_ref() != Some(&receipt.binding)
            || receipt.binding.adapter != "honcho"
            || receipt.creation_key.is_empty()
            || receipt.creation_key.len() > 200
            || !receipt
                .creation_key
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
        {
            return Err("prepared memory does not match the selected employee or diagnostic");
        }
        Ok(())
    }

    /// Constructs an Adopt acquisition; original extension ownership remains in
    /// the receipt. This call neither inspects resources nor grants a witness.
    pub fn adapter(
        &self,
        scope: &CompanyScope,
        employee: &Employee,
    ) -> Result<HonchoMemoryAdapter, &'static str> {
        self.validate(scope, employee)?;
        let token = ResolvedHonchoToken::from_env(self.token_ref.clone(), &self.token_env)
            .map_err(|_| "selected memory credential unavailable")?;
        HonchoMemoryAdapter::new(
            scope,
            HonchoMemoryConfig {
                deployment: HonchoDeploymentSelection {
                    deployment_id: self.creation_receipt.deployment_id,
                    protocol: PROTOCOL.to_owned(),
                    honcho_version: HONCHO_VERSION.to_owned(),
                    endpoint_ref: self.creation_receipt.binding.endpoint_ref.clone(),
                    origin: self.origin.clone(),
                    token_ref: self.token_ref.clone(),
                },
                employees: vec![HonchoEmployeeBinding {
                    employee_id: employee.id.clone(),
                    binding: self.creation_receipt.binding.clone(),
                    mode: ProvisioningMode::Adopt,
                    allow_company_truth: false,
                    allowed_projects: BTreeSet::new(),
                }],
                request_timeout: Duration::from_secs(2),
                witness_lifetime: Duration::from_secs(900),
            },
            token,
        )
        .map_err(|_| "prepared memory adapter configuration refused")
    }

    /// Revalidates original ownership read-only, then explicitly replays the
    /// exact retained diagnostic. A historical receipt alone is never healthy.
    pub async fn prepare(&self, adapter: &HonchoMemoryAdapter) -> Result<(), &'static str> {
        if !self.validate_memory_io {
            return Err("memory diagnostic authorization is required");
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            adapter
                .recover_created_resources(&self.creation_receipt)
                .await
                .map_err(|_| "prepared memory ownership recovery failed")?;
            adapter
                .validate_memory_roundtrip(&MemoryRoundtripRequest {
                    employee_id: self.creation_receipt.employee_id.clone(),
                    binding: self.creation_receipt.binding.clone(),
                    run_id: self.validation_run_id,
                    recorded_at: self.validation_recorded_at,
                })
                .await
                .map_err(|_| "prepared memory diagnostic failed")?;
            Ok(())
        })
        .await
        .map_err(|_| "prepared memory validation deadline elapsed")?
    }
}
