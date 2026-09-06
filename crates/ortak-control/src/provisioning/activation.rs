//! Fresh activation authority, issued before external probes and consumed once.
use super::*;
use crate::office_authority::OfficeAuthority;
use std::time::Duration;

/// Repository-issued activation target. Callers cannot manufacture authority fields.
#[derive(Clone, Debug, PartialEq)]
pub struct ActivationTarget {
    company_id: Uuid,
    operation_id: Uuid,
    employee_id: EmployeeId,
    operation_fingerprint: [u8; 32],
    operation_manifest: EmployeeManifest,
    manifest_fingerprint: [u8; 32],
    mode: ProvisioningMode,
    step: StepRecord,
    baseline_status: EmployeeStatus,
    baseline_revision: Option<Uuid>,
    lifecycle_epoch: Option<i64>,
    office: OfficeAuthority,
    observed_at: DateTime<Utc>,
    valid_before: DateTime<Utc>,
}

pub(crate) fn lifetime(value: Duration) -> Duration {
    value.clamp(Duration::from_millis(1), Duration::from_secs(15))
}
fn refused(operation_id: Uuid, detail: &'static str) -> ControlError {
    ProvisioningError::Superseded {
        operation_id,
        detail,
    }
    .into()
}
fn active_employee(operation: &ProvisioningOperation) -> Employee {
    let mut employee = operation.effective_employee();
    employee.status = EmployeeStatus::Active;
    employee
}
fn fingerprint(employee: &Employee) -> Result<[u8; 32]> {
    Ok(Sha256::digest(serde_json::to_vec(employee)?).into())
}
fn same_attempt(left: &StepRecord, right: &StepRecord) -> bool {
    left.step == ProvisioningStep::ActivateRevision
        && right.step == left.step
        && left.state == StepState::Running
        && right.state == StepState::Running
        && left.started_at.is_some()
        && left.attempt_count > 0
        && left.attempt_count == right.attempt_count
        && left.idempotency_key == right.idempotency_key
        && left.started_at.map(|v| v.timestamp_micros())
            == right.started_at.map(|v| v.timestamp_micros())
}

impl ActivationTarget {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn issue(
        scope: &CompanyScope,
        operation: &ProvisioningOperation,
        running: &StepRecord,
        baseline_status: EmployeeStatus,
        baseline_revision: Option<Uuid>,
        office: OfficeAuthority,
        observed_at: DateTime<Utc>,
        duration: Duration,
    ) -> Result<Self> {
        Self::issue_with_lifecycle(
            scope,
            operation,
            running,
            baseline_status,
            baseline_revision,
            office,
            observed_at,
            duration,
            false,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn issue_with_lifecycle(
        scope: &CompanyScope,
        operation: &ProvisioningOperation,
        running: &StepRecord,
        baseline_status: EmployeeStatus,
        baseline_revision: Option<Uuid>,
        office: OfficeAuthority,
        observed_at: DateTime<Utc>,
        duration: Duration,
        reenable: bool,
        lifecycle_epoch: Option<i64>,
    ) -> Result<Self> {
        let current = operation
            .step(ProvisioningStep::ActivateRevision)
            .ok_or_else(|| refused(operation.id, "activation step is missing"))?;
        if operation.dry_run
            || operation.result_revision_id.is_some()
            || operation.status != OperationStatus::Running
            || operation.current_step != Some(ProvisioningStep::ActivateRevision)
            || !same_attempt(current, running)
            || (baseline_status == EmployeeStatus::Disabled && !reenable)
            || (operation.mode == OperationMode::Create && baseline_status != EmployeeStatus::Draft)
            || operation
                .step(ProvisioningStep::ProbeHealth)
                .is_none_or(|s| s.state != StepState::Succeeded)
            || ProvisioningStep::ALL
                .iter()
                .filter(|s| **s != ProvisioningStep::ActivateRevision)
                .any(|s| {
                    operation
                        .step(*s)
                        .is_none_or(|record| !record.state.is_done())
                })
        {
            return Err(refused(
                operation.id,
                "activation target is no longer eligible",
            ));
        }
        operation.manifest.validate()?;
        let duration = chrono::Duration::from_std(lifetime(duration))
            .map_err(|_| refused(operation.id, "invalid activation lifetime"))?;
        let mut valid_before = observed_at + duration;
        if let Some(boundary) = office.valid_before() {
            valid_before = valid_before.min(boundary);
        }
        if valid_before <= observed_at {
            return Err(refused(operation.id, "activation authority expired"));
        }
        Ok(Self {
            company_id: scope.company_id(),
            operation_id: operation.id,
            employee_id: operation.employee_id.clone(),
            operation_fingerprint: operation.manifest_fingerprint,
            operation_manifest: operation.manifest.clone(),
            manifest_fingerprint: fingerprint(&active_employee(operation))?,
            mode: operation.resource_mode(),
            step: current.clone(),
            baseline_status,
            baseline_revision,
            lifecycle_epoch,
            office,
            observed_at,
            valid_before,
        })
    }
    /// Conservative observation time: database issuance before any fresh health probe.
    pub fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }
    /// Absolute database-clock deadline, capped at fifteen seconds from issuance.
    pub fn valid_before(&self) -> DateTime<Utc> {
        self.valid_before
    }
    pub(crate) fn office(&self) -> &OfficeAuthority {
        &self.office
    }
    pub(crate) fn validate_lifecycle_epoch(&self, epoch: i64) -> Result<()> {
        if self.lifecycle_epoch != Some(epoch) {
            return Err(refused(self.operation_id, "activation lifecycle changed"));
        }
        Ok(())
    }
    pub(crate) fn validate_current(
        &self,
        scope: &CompanyScope,
        operation: &ProvisioningOperation,
        status: EmployeeStatus,
        revision: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let current = operation
            .step(ProvisioningStep::ActivateRevision)
            .ok_or_else(|| refused(operation.id, "activation step is missing"))?;
        if operation.dry_run
            || operation.result_revision_id.is_some()
            || operation.status != OperationStatus::Running
            || operation.current_step != Some(ProvisioningStep::ActivateRevision)
            || operation
                .step(ProvisioningStep::ProbeHealth)
                .is_none_or(|s| s.state != StepState::Succeeded)
            || ProvisioningStep::ALL
                .iter()
                .filter(|s| **s != ProvisioningStep::ActivateRevision)
                .any(|s| {
                    operation
                        .step(*s)
                        .is_none_or(|record| !record.state.is_done())
                })
            || scope.company_id() != self.company_id
            || operation.id != self.operation_id
            || operation.employee_id != self.employee_id
            || operation.manifest_fingerprint != self.operation_fingerprint
            || operation.manifest != self.operation_manifest
            || operation.resource_mode() != self.mode
            || fingerprint(&active_employee(operation))? != self.manifest_fingerprint
            || !same_attempt(&self.step, current)
            || status != self.baseline_status
            || revision != self.baseline_revision
            || now < self.observed_at
            || now >= self.valid_before
        {
            return Err(refused(
                operation.id,
                "activation authority changed or expired",
            ));
        }
        Ok(())
    }
    pub(crate) fn validate_activation(&self, activation: &RevisionActivation) -> Result<()> {
        let evidence: GateEvidence =
            serde_json::from_value(activation.activation_step.result["evidence"].clone())
                .map_err(|_| refused(self.operation_id, "fresh activation evidence is missing"))?;
        let expected = OfficePublicKey::parse_hex(&activation.employee.office.public_key)?;
        if activation.employee.id != self.employee_id
            || activation.employee.status != EmployeeStatus::Active
            || activation.provisioning_mode != self.mode
            || fingerprint(&activation.employee)? != self.manifest_fingerprint
            || activation.manifest_fingerprint != self.manifest_fingerprint
            || activation.runtime_validated_at != self.observed_at
            || activation.office_verified_at != self.observed_at
            || activation.memory_validated_at
                != activation
                    .employee
                    .memory
                    .as_ref()
                    .map(|_| self.observed_at)
            || activation.activation_step.attempt_count != self.step.attempt_count
            || activation.activation_step.idempotency_key != self.step.idempotency_key
            || activation.activation_step.step != ProvisioningStep::ActivateRevision
            || activation.activation_step.result["admission"] != self.envelope()
            || evidence
                .runtime_capabilities
                .as_ref()
                .is_none_or(|c| c.adapter != activation.employee.runtime.adapter)
            || activation.employee.memory.as_ref().is_some_and(|binding| {
                evidence
                    .memory_capabilities
                    .as_ref()
                    .is_none_or(|c| c.adapter != binding.adapter)
            })
            || evidence
                .signer
                .as_ref()
                .is_none_or(|s| !s.matches_expected || s.produced_public_key != expected)
            || evaluate_activation_gates(&evidence, activation.employee.memory.is_some()).is_err()
        {
            return Err(refused(
                self.operation_id,
                "fresh activation evidence does not match target",
            ));
        }
        Ok(())
    }
    pub(crate) fn envelope(&self) -> serde_json::Value {
        serde_json::json!({"format":"ortak.activation/v1","observed_at":self.observed_at,"valid_before":self.valid_before,
            "operation_id":self.operation_id,"employee_id":self.employee_id,"attempt_count":self.step.attempt_count,
            "manifest_fingerprint":hex::encode(self.manifest_fingerprint)})
    }
}
