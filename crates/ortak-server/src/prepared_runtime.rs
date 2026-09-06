//! Explicit, durable selected-profile preparation; never called by read routes.
use std::{future::Future, time::Duration};

use chrono::Utc;
use ortak_control::{
    ports::ProvisioningRepository,
    postgres::ProvisioningRuntimeProbe,
    provisioning::{OperationStatus, OperationUpdate, ProvisioningStep},
    runtime::RuntimeAdapter,
    CompanyScope, PgControlPlane,
};
use ortak_runtime::hermes::{HermesAdapter, ProfileProbeStatus};
use uuid::Uuid;

use crate::provisioning::{ProvisioningConfig, RuntimeCredentialSelection};

async fn authority(
    control: &PgControlPlane,
    scope: &CompanyScope,
    operation: Uuid,
) -> Result<(), &'static str> {
    tokio::time::timeout(
        Duration::from_secs(2),
        control.check_provisioning_runtime_probe_authority(scope, operation),
    )
    .await
    .map_err(|_| "probe_authority_changed")?
    .map_err(|_| "probe_authority_changed")
}

// Recheck while an HTTP request is pending too: a slow provider admission must
// not hide authority revocation until its network timeout expires.
async fn authorized<T>(
    control: &PgControlPlane,
    scope: &CompanyScope,
    operation: Uuid,
    future: impl Future<Output = Result<T, ortak_control::runtime::RuntimeError>>,
) -> Result<T, &'static str> {
    authority(control, scope, operation).await?;
    tokio::pin!(future);
    loop {
        tokio::select! {
            result=&mut future => return result.map_err(|_|"probe_transport"),
            _=tokio::time::sleep(Duration::from_secs(1)) => authority(control,scope,operation).await?,
        }
    }
}

fn recovery_adapter(
    scope: &CompanyScope,
    selected: &ProvisioningRuntimeProbe,
) -> Result<HermesAdapter, &'static str> {
    HermesAdapter::validate_connection_origin(selected.origin())
        .map_err(|_| "probe_recovery_selection_invalid")?;
    let token = std::env::var(selected.token_environment())
        .map_err(|_| "probe_recovery_credential_unavailable")?;
    HermesAdapter::new(scope.company_id(), selected.origin(), &token)
        .map_err(|_| "probe_recovery_selection_invalid")
}

async fn contain(
    runtime: &HermesAdapter,
    selected: &ProvisioningRuntimeProbe,
) -> Result<(), &'static str> {
    tokio::time::timeout(
        Duration::from_secs(15),
        runtime.stop_profile_probe(selected.id()),
    )
    .await
    .map_err(|_| "probe_containment_pending")?
    .map_err(|_| "probe_containment_pending")
}

async fn execute(
    control: &PgControlPlane,
    scope: &CompanyScope,
    operation: Uuid,
    runtime: &HermesAdapter,
    config: &ProvisioningConfig,
    selected: &ProvisioningRuntimeProbe,
) -> Result<(), &'static str> {
    let remaining = (selected.deadline() - Utc::now())
        .to_std()
        .map_err(|_| "probe_timeout")?;
    tokio::time::timeout(remaining, async {
        let mut status = authorized(
            control,
            scope,
            operation,
            runtime.profile_probe_status(selected.id()),
        )
        .await?;
        if status.is_none() {
            status = Some(
                authorized(
                    control,
                    scope,
                    operation,
                    runtime.start_profile_probe(&config.manifest.employee.runtime, selected.id()),
                )
                .await?,
            );
        }
        loop {
            match status {
                Some(ProfileProbeStatus::Completed) => return Ok(()),
                Some(ProfileProbeStatus::Failed) => return Err("probe_failed"),
                Some(ProfileProbeStatus::Cancelled | ProfileProbeStatus::Cancelling) => {
                    return Err("probe_cancelled")
                }
                Some(ProfileProbeStatus::Accepted | ProfileProbeStatus::Running) => {}
                None => return Err("probe_transport"),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            status = authorized(
                control,
                scope,
                operation,
                runtime.profile_probe_status(selected.id()),
            )
            .await?;
        }
    })
    .await
    .map_err(|_| "probe_timeout")?
}

async fn workspace_ready(
    control: &PgControlPlane,
    scope: &CompanyScope,
    operation: Uuid,
    runtime: &HermesAdapter,
    config: &ProvisioningConfig,
) -> Result<(), &'static str> {
    let employee = &config.manifest.employee;
    if ortak_control::workspace::validate_hermes_policy(&employee.runtime, &employee.permissions)
        .map_err(|_| "workspace_policy_unsupported")?
    {
        let capabilities =
            authorized(control, scope, operation, runtime.probe_capabilities()).await?;
        if !capabilities.supports(ortak_control::runtime::RuntimeCapability::WorkspaceTextRead) {
            return Err("workspace_capability_unavailable");
        }
        let registered: bool =
            sqlx::query_scalar("SELECT ortak_workspace_profile_available($1,$2,$3)")
                .bind(scope.company_id())
                .bind(employee.id.as_str())
                .bind(employee.runtime.workspace_ref.as_str())
                .fetch_one(control.pool())
                .await
                .map_err(|_| "workspace_registry_unavailable")?;
        if !registered {
            return Err("workspace_registry_unavailable");
        }
    }
    Ok(())
}

// The retained probe journal has a closed diagnostic vocabulary. A withdrawn
// workspace/capability invalidates readiness; callers still receive the exact
// original gate error after its containment is recorded.
fn accounting_error(error: &'static str) -> &'static str {
    match error {
        "workspace_policy_unsupported"
        | "workspace_capability_unavailable"
        | "workspace_registry_unavailable" => "probe_authority_changed",
        other => other,
    }
}

/// Called only from the explicit production runner after immutable selection and
/// lifecycle checks. Dry runs and environment-owned profiles make no diagnostic.
pub(crate) async fn prepare(
    control: &PgControlPlane,
    scope: &CompanyScope,
    operation: Uuid,
    runtime: &HermesAdapter,
    config: &ProvisioningConfig,
) -> Result<(), &'static str> {
    if config.dry_run
        || !matches!(
            config.runtime_credentials,
            RuntimeCredentialSelection::HermesProfile { .. }
        )
    {
        return Ok(());
    }
    authority(control, scope, operation).await?;
    let mut selected = control
        .provisioning_runtime_probe(scope, operation)
        .await
        .map_err(|_| "probe_journal_unavailable")?;
    if let Some(prior) = selected
        .as_ref()
        .filter(|p| p.state() == "running" && p.operation_id() != operation)
    {
        // An interrupted earlier operation may have a different original bridge.
        // Its immutable endpoint/reference is used only for exact cancellation.
        authority(control, scope, operation).await?;
        let old_runtime = recovery_adapter(scope, prior)?;
        contain(&old_runtime, prior).await?;
        control
            .settle_provisioning_runtime_probe(scope, prior, Some("probe_interrupted"))
            .await
            .map_err(|_| "probe_cleanup_receipt_unavailable")?;
        selected = control
            .provisioning_runtime_probe(scope, operation)
            .await
            .map_err(|_| "probe_journal_unavailable")?;
    }
    if let Err(error) = workspace_ready(control, scope, operation, runtime, config).await {
        if let Some(prior) = selected.as_ref().filter(|p| p.state() == "running") {
            if prior.origin() == config.bridge_origin
                && prior.token_environment() == config.bridge_token_env
            {
                contain(runtime, prior).await?;
            } else {
                contain(&recovery_adapter(scope, prior)?, prior).await?;
            }
            control
                .settle_provisioning_runtime_probe(scope, prior, Some(accounting_error(error)))
                .await
                .map_err(|_| "probe_cleanup_receipt_unavailable")?;
        }
        return Err(error);
    }
    if let Some(prior) = selected
        .as_ref()
        .filter(|p| p.state() == "succeeded" && p.deadline() > Utc::now())
    {
        if authorized(
            control,
            scope,
            operation,
            runtime.health(&config.manifest.employee.runtime),
        )
        .await?
        .is_healthy()
        {
            // A previous acknowledgment was lost after proof persistence. The
            // exact current binding still has a fresh bridge readiness witness.
            authority(control, scope, operation).await?;
            if prior.deadline() > Utc::now() {
                workspace_ready(control, scope, operation, runtime, config).await?;
                return Ok(());
            }
        }
    }
    let selected = match selected {
        Some(p) if p.state() == "running" => p,
        prior => control
            .admit_provisioning_runtime_probe(
                scope,
                operation,
                &config.bridge_origin,
                &config.bridge_token_env,
                prior.as_ref().map(|p| p.id()),
            )
            .await
            .map_err(|_| "probe_admission_refused")?,
    };
    if selected.origin() != config.bridge_origin
        || selected.token_environment() != config.bridge_token_env
    {
        return Err("probe_recovery_selection_invalid");
    }
    let result = execute(control, scope, operation, runtime, config, &selected).await;
    // Terminal status is not a containment proof. On every outcome the same
    // child is stopped before a terminal row can release the unique admission.
    contain(runtime, &selected).await?;
    let result = match result {
        Ok(()) => match authorized(
            control,
            scope,
            operation,
            runtime.health(&config.manifest.employee.runtime),
        )
        .await
        {
            Ok(health) if health.is_healthy() => {
                workspace_ready(control, scope, operation, runtime, config).await
            }
            Ok(_) => Err("probe_unhealthy"),
            Err(error) => Err(error),
        },
        other => other,
    };
    let error = result.as_ref().err().copied();
    if control
        .settle_provisioning_runtime_probe(scope, &selected, error.map(accounting_error))
        .await
        .is_err()
    {
        // A success may lose authority immediately before its commit. Preserve
        // the containment proof as failed accounting, never as readiness.
        control
            .settle_provisioning_runtime_probe(scope, &selected, Some("probe_authority_changed"))
            .await
            .map_err(|_| "probe_cleanup_receipt_unavailable")?;
        return Err("probe_authority_changed");
    }
    if let Some(error) = error {
        if authority(control, scope, operation).await.is_ok() {
            control
                .update_operation(
                    scope,
                    operation,
                    &OperationUpdate {
                        status: OperationStatus::Failed,
                        current_step: Some(ProvisioningStep::ValidateRuntimeProfile),
                        error_message: Some(error.into()),
                    },
                )
                .await
                .map_err(|_| "probe_failure_receipt_unavailable")?;
        }
    }
    result
}

#[cfg(test)]
#[path = "prepared_runtime/postgres_tests.rs"]
mod postgres_tests;
