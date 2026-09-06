//! Operator-selected, bounded production activation over real adapters.
//!
//! One invocation makes one attempt. Failed and interrupted operations keep
//! their durable step keys; restarting with the same configuration resumes them.

use std::{collections::BTreeSet, time::Duration};

use ortak_control::{
    credentials::{EnvCredentialBinding, EnvCredentialResolver},
    ports::{CompanyDirectory, ProvisioningRepository},
    provisioning::{OperationMode, ProvisioningRequest, ProvisioningSaga, SagaConfig, SagaOutcome},
    CompanyScope, PgControlPlane,
};
use ortak_domain::{EmployeeManifest, ProvisioningMode};
use ortak_office::{
    identity::{OfficeIdentityConfig, PgOfficeIdentityAdapter},
    transport::{EnvOfficeSigner, OfficeSignerBinding},
};
use ortak_runtime::hermes::HermesAdapter;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::prepared_memory::PreparedMemoryConfig;

#[path = "provisioning_credentials.rs"]
mod credentials;
use credentials::PreparedCredentialResolver;
pub use credentials::RuntimeCredentialSelection;

/// Explicit configuration for one prepared employee. All fields are public
/// selections or opaque references; secrets are resolved by the owning adapter.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvisioningConfig {
    /// Company resolved from this selected Office community on every attempt.
    pub community_id: Uuid,
    /// Operator-stable request key; reuse on interruption and acknowledgement loss.
    pub operation_key: String,
    /// Adopt prepared resources, or update from an immutable Adopt manifest.
    pub mode: OperationMode,
    /// Required explicit choice; dry run never publishes or activates.
    pub dry_run: bool,
    /// Desired immutable employee definition.
    pub manifest: EmployeeManifest,
    /// Exact production bridge origin.
    pub bridge_origin: String,
    /// Environment name of the selected bridge authentication token.
    pub bridge_token_env: String,
    /// Explicit manager for the exact runtime references in the manifest.
    pub runtime_credentials: RuntimeCredentialSelection,
    /// Only this employee's Office signer may be loaded.
    pub office_signer: OfficeSignerBinding,
    /// Canonical relay and selected channel cohort.
    pub office: OfficeIdentityConfig,
    /// Previously prepared extension-owned bundle and stable diagnostic.
    pub memory: PreparedMemoryConfig,
}

impl ProvisioningConfig {
    /// Checks scope and all mappings before any credential value is read.
    pub fn validate(&self, scope: &CompanyScope) -> Result<(), &'static str> {
        let employee = &self.manifest.employee;
        let selected = self.office.employees.first();
        if scope.community_id() != Some(self.community_id)
            || self.office.company_id != scope.company_id()
            || self.office.community_id != self.community_id
            || self.office.employees.len() != 1
            || selected.is_none_or(|entry| {
                entry.employee_id != employee.id || entry.office != employee.office
            })
            || self.office_signer.company_id != scope.company_id()
            || self.office_signer.employee_id != employee.id
            || self.office_signer.signer_ref != employee.office.signer_ref
            || self.office_signer.public_key.to_hex() != employee.office.public_key
            || self.manifest.provisioning != ProvisioningMode::Adopt
            || self.mode == OperationMode::Create
            || employee.runtime.adapter != "hermes"
        {
            return Err("provisioning selection does not match the prepared employee scope");
        }
        ortak_control::workspace::validate_hermes_policy(&employee.runtime, &employee.permissions)
            .map_err(|_| "unsupported selected runtime permission policy")?;
        self.request()
            .validate()
            .map_err(|_| "invalid provisioning request")?;
        self.office
            .validate()
            .map_err(|_| "invalid Office identity selection")?;
        self.office_signer
            .validate()
            .map_err(|_| "invalid Office signer selection")?;
        HermesAdapter::validate_connection_origin(&self.bridge_origin)
            .map_err(|_| "invalid bridge origin")?;
        self.memory.validate(scope, employee)?;
        let required: BTreeSet<_> = employee.runtime.credential_refs.iter().collect();
        let bindings = self.runtime_credentials.environment_bindings();
        let selected: BTreeSet<_> = bindings.iter().map(|entry| &entry.credential_ref).collect();
        if required.is_empty()
            || required.len() != employee.runtime.credential_refs.len()
            || required.len() > 127
            || (matches!(
                self.runtime_credentials,
                RuntimeCredentialSelection::Environment { .. }
            ) && (required != selected || selected.len() != bindings.len()))
            || required.contains(&employee.office.signer_ref)
            || required.contains(&self.memory.token_ref)
            || self.memory.token_ref == employee.office.signer_ref
        {
            return Err("runtime credential mappings do not match the selected binding");
        }
        let mut environments = BTreeSet::new();
        for name in bindings
            .iter()
            .map(|entry| entry.environment_variable.as_str())
            .chain([
                self.office_signer.secret_env.as_str(),
                self.bridge_token_env.as_str(),
                self.memory.token_env.as_str(),
            ])
        {
            if !environments.insert(name) {
                return Err("credential environment selections overlap across owning adapters");
            }
        }
        // Validate every environment name without reading it, including the
        // bridge and memory credentials owned by their respective adapters.
        EnvCredentialResolver::new(vec![EnvCredentialBinding {
            credential_ref: self.memory.token_ref.clone(),
            environment_variable: self.bridge_token_env.clone(),
        }])
        .map_err(|_| "invalid bridge environment selection")?;
        self.credentials()?;
        Ok(())
    }

    fn request(&self) -> ProvisioningRequest {
        ProvisioningRequest {
            employee_id: self.manifest.employee.id.clone(),
            mode: self.mode,
            dry_run: self.dry_run,
            idempotency_key: self.operation_key.clone(),
            manifest: self.manifest.clone(),
        }
    }

    fn credentials(&self) -> Result<EnvCredentialResolver, &'static str> {
        let mut bindings = self.runtime_credentials.environment_bindings().to_vec();
        bindings.push(EnvCredentialBinding {
            credential_ref: self.office_signer.signer_ref.clone(),
            environment_variable: self.office_signer.secret_env.clone(),
        });
        EnvCredentialResolver::new(bindings)
            .map_err(|_| "invalid provisioning credential allowlist")
    }
}

/// Bounded summary suitable for a terminal; no manifest or remote error is printed.
#[derive(serde::Serialize)]
pub struct ProvisioningResult {
    /// Durable operation identity, also returned on a failed step.
    pub operation_id: Uuid,
    /// Durable operation status.
    pub status: String,
    /// Exact failed/current step, if any.
    pub step: Option<String>,
    /// Activated revision only when the repository committed it.
    pub revision_id: Option<Uuid>,
}

/// Makes one explicit production attempt from bounded public configuration.
/// The operation and immutable selection are committed before external probes.
/// No timer, API read or worker health check calls this function implicitly.
pub async fn provision_once(
    pool: PgPool,
    json: &str,
    compensate: bool,
) -> Result<ProvisioningResult, &'static str> {
    provision_with_control(PgControlPlane::new(pool), json, compensate).await
}

// The management executor supplies a repository sealed to its live lease;
// the public operator CLI uses the original unrestricted repository behavior.
pub(crate) async fn provision_with_control(
    control: PgControlPlane,
    json: &str,
    compensate: bool,
) -> Result<ProvisioningResult, &'static str> {
    if json.len() > 65_536 {
        return Err("provisioning configuration exceeds limit");
    }
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|_| "invalid provisioning configuration")?;
    let config: ProvisioningConfig =
        serde_json::from_value(value.clone()).map_err(|_| "invalid provisioning configuration")?;
    let pool = control.pool().clone();
    let scope = control
        .resolve_company_for_community(config.community_id)
        .await
        .map_err(|_| "provisioning company resolution failed")?;
    config.validate(&scope)?;
    control
        .check_provisioning_execution(&scope)
        .await
        .map_err(|_| "management authority refused")?;
    let canonical = serde_json::to_vec(&value).map_err(|_| "invalid provisioning configuration")?;
    let fingerprint: [u8; 32] = Sha256::digest(canonical).into();

    // A session lock excludes overlapping operators without keeping a SQL
    // transaction open during external I/O. Closing on drop releases it even
    // when a signal or whole-command deadline cancels this future.
    let mut lease = pool
        .acquire()
        .await
        .map_err(|_| "provisioning lock connection failed")?;
    lease.close_on_drop();
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(hashtextextended($1, 0))")
        .bind(format!(
            "ortak-provision-employee:{}:{}",
            scope.company_id(),
            config.manifest.employee.id
        ))
        .fetch_one(&mut *lease)
        .await
        .map_err(|_| "provisioning lock failed")?;
    if !locked {
        return Err("selected provisioning operation is already running");
    }
    control
        .check_provisioning_execution(&scope)
        .await
        .map_err(|_| "management authority refused")?;
    let operation = control
        .begin_operation(&scope, &config.request())
        .await
        .map_err(|_| "provisioning begin failed; inspect the retained operation")?;
    sqlx::query("INSERT INTO provisioning_runner_selections (company_id, operation_id, configuration_fingerprint) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(scope.company_id()).bind(operation.id).bind(fingerprint.as_slice())
        .execute(&pool).await.map_err(|_| "provisioning selection persistence failed")?;
    let retained: Vec<u8> = sqlx::query_scalar("SELECT configuration_fingerprint FROM provisioning_runner_selections WHERE company_id=$1 AND operation_id=$2")
        .bind(scope.company_id()).bind(operation.id).fetch_one(&pool).await.map_err(|_| "provisioning selection read failed")?;
    if retained != fingerprint {
        return Err("provisioning selection changed; retained operation requires its original configuration");
    }
    control
        .check_provisioning_execution(&scope)
        .await
        .map_err(|_| "management authority refused")?;
    if operation.status.is_terminal() {
        return Ok(summary(&operation));
    }
    if compensate {
        let outcome =
            ortak_control::provisioning::compensate_adopted(&control, &scope, operation.id)
                .await
                .map_err(|_| "adopted resource retention refused; inspect the durable operation")?;
        let operation = match outcome {
            SagaOutcome::Compensated { operation, .. }
            | SagaOutcome::AlreadyTerminal(operation) => operation,
            _ => return Err("unexpected adopted retention outcome"),
        };
        return Ok(summary(&operation));
    }
    control
        .check_operation_lifecycle(&scope, operation.id)
        .await
        .map_err(|_| "provisioning lifecycle changed")?;
    let environment_credentials = config.credentials()?;
    let token = std::env::var(&config.bridge_token_env)
        .map_err(|_| "selected bridge credential unavailable")?;
    let runtime = HermesAdapter::new(scope.company_id(), &config.bridge_origin, &token)
        .map_err(|_| "selected bridge configuration refused")?;
    let credentials = PreparedCredentialResolver {
        environment: environment_credentials,
        runtime: &runtime,
        bridge_binding: matches!(
            config.runtime_credentials,
            RuntimeCredentialSelection::HermesProfile {}
        )
        .then_some(&config.manifest.employee.runtime),
    };
    let signer = EnvOfficeSigner::from_env(vec![config.office_signer.clone()])
        .map_err(|_| "selected Office signer unavailable")?;
    let office = PgOfficeIdentityAdapter::new(
        control.clone(),
        signer,
        config.office.clone(),
        Duration::from_secs(5),
    )
    .map_err(|_| "selected Office identity configuration refused")?;
    let memory = config.memory.adapter(&scope, &config.manifest.employee)?;
    let saga = ProvisioningSaga::new(
        &control,
        &runtime,
        &memory,
        &office,
        &credentials,
        SagaConfig::default(),
    );
    // Explicitly authorized diagnostic replay; ordinary adapter health and
    // capability probes never acquire extension ownership or write a witness.
    if !config.dry_run {
        control
            .check_operation_lifecycle(&scope, operation.id)
            .await
            .map_err(|_| "management authority refused")?;
        crate::prepared_runtime::prepare(&control, &scope, operation.id, &runtime, &config).await?;
        config.memory.prepare(&memory).await?;
    }
    let outcome = saga.resume(&scope, operation.id).await.map_err(|_| {
        "provisioning attempt interrupted or refused; retained operation is resumable"
    })?;
    let operation = match outcome {
        SagaOutcome::Succeeded(operation)
        | SagaOutcome::AlreadyTerminal(operation)
        | SagaOutcome::Failed { operation, .. }
        | SagaOutcome::Compensated { operation, .. } => operation,
    };
    Ok(summary(&operation))
}

fn summary(operation: &ortak_control::provisioning::ProvisioningOperation) -> ProvisioningResult {
    ProvisioningResult {
        operation_id: operation.id,
        status: operation.status.as_str().to_owned(),
        step: operation.current_step.map(|step| step.name().to_owned()),
        revision_id: operation.result_revision_id,
    }
}
