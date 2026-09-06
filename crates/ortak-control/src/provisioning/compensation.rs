//! Database-only retention of prepared resources; no credential or adapter I/O.

use super::*;

/// Compensates an Adopt operation by retaining all prepared external resources.
///
/// This entry point needs only the repository, so revoked credentials or an
/// unavailable provider cannot hide the recovery path. Created/ambiguous
/// receipts fail closed before any step changes. No resource deletion exists
/// on this path, and an activated revision can never be compensated.
pub async fn compensate_adopted<Repo: ProvisioningRepository>(
    repository: &Repo,
    scope: &CompanyScope,
    operation_id: Uuid,
) -> Result<SagaOutcome> {
    let mut operation = repository
        .load_operation(scope, operation_id)
        .await?
        .ok_or(ProvisioningError::UnknownOperation { operation_id })?;
    if operation.resource_mode() != ProvisioningMode::Adopt
        || operation.result_revision_id.is_some()
    {
        return Err(ProvisioningError::InvalidTransition {
            status: operation.status,
            action: "retain resources of a non-adopt or activated operation",
        }
        .into());
    }
    if operation.status == OperationStatus::Compensated {
        return Ok(SagaOutcome::AlreadyTerminal(operation));
    }
    if !matches!(
        operation.status,
        OperationStatus::Failed | OperationStatus::Compensating
    ) {
        return Err(ProvisioningError::InvalidTransition {
            status: operation.status,
            action: "compensate",
        }
        .into());
    }
    let mut plan = Vec::new();
    for step in ProvisioningStep::ALL.iter().rev().copied() {
        let record = operation
            .step(step)
            .ok_or(ProvisioningError::Inconsistent {
                operation_id,
                detail: "missing step record",
            })?;
        if !matches!(record.state, StepState::Succeeded | StepState::Compensating) {
            continue;
        }
        let retained = retained(record).ok_or(ProvisioningError::Inconsistent {
            operation_id,
            detail: "adopted compensation requires exact adopted resource receipts",
        })?;
        plan.push((record.clone(), retained));
    }
    repository
        .update_operation(
            scope,
            operation_id,
            &OperationUpdate {
                status: OperationStatus::Compensating,
                current_step: None,
                error_message: operation.error_message.clone(),
            },
        )
        .await?;
    let mut retained_adopted = Vec::new();
    for (record, retained) in plan {
        let mut done = record;
        done.state = StepState::Compensating;
        done.error_message = None;
        repository.record_step(scope, operation_id, &done).await?;
        done.state = StepState::Compensated;
        done.result = merge(
            done.result,
            serde_json::json!({ "retained_adopted": retained }),
        );
        done.finished_at = Some(Utc::now());
        repository.record_step(scope, operation_id, &done).await?;
        retained_adopted.extend(retained);
        replace_step(&mut operation, done);
    }
    repository
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
        deleted: Vec::new(),
    })
}

fn retained(record: &StepRecord) -> Option<Vec<String>> {
    let resources = match record.step {
        ProvisioningStep::EnsureRuntimeProfile => {
            vec![serde_json::from_value::<ResourceOutcome>(record.result.clone()).ok()?]
        }
        ProvisioningStep::EnsureMemoryResources => {
            let outcome: MemoryResourceOutcome =
                serde_json::from_value(record.result.clone()).ok()?;
            vec![outcome.employee_peer, outcome.user_peer, outcome.workspace]
        }
        ProvisioningStep::EnsureOfficeIdentity => {
            vec![record.receipt::<ResourceOutcome>("membership")?]
        }
        ProvisioningStep::ActivateRevision => return None,
        _ => Vec::new(),
    };
    resources
        .into_iter()
        .map(|resource| {
            resource
                .ownership
                .is_adopted()
                .then_some(resource.resource_ref)
        })
        .collect()
}
