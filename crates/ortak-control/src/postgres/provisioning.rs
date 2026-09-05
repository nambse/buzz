use chrono::{DateTime, Utc};
use ortak_domain::{normalize_alias, EmployeeId, EmployeeStatus};
use sqlx::postgres::PgRow;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use super::{bytes32, column_value, is_unique_violation, parse_column, PgControlPlane};
use crate::error::{ControlError, Result};
use crate::ids::CompanyScope;
use crate::office_identity::OfficePublicKey;
use crate::ports::ProvisioningRepository;
use crate::provisioning::{
    ActivationTarget, IdentityReservation, OperationStatus, OperationUpdate, ProvisioningError,
    ProvisioningOperation, ProvisioningRequest, ProvisioningStep, RevisionActivation, StepRecord,
    StepState,
};

mod activation;

fn step_from_row(row: &PgRow) -> Result<StepRecord> {
    let name: String = row.try_get("step_name")?;
    let state: String = row.try_get("state")?;
    Ok(StepRecord {
        step: ProvisioningStep::parse(&name).ok_or_else(|| {
            ControlError::InvalidData(format!("provisioning step name {name:?} is unknown"))
        })?,
        state: parse_column("provisioning_operation_steps.state", &state)?,
        idempotency_key: row.try_get("idempotency_key")?,
        attempt_count: row.try_get("attempt_count")?,
        adopted_existing: row.try_get("adopted_existing")?,
        result: row.try_get("result")?,
        error_message: row.try_get("error_message")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
    })
}

async fn load_steps(
    conn: &mut PgConnection,
    company_id: Uuid,
    operation_id: Uuid,
) -> Result<Vec<StepRecord>> {
    let rows = sqlx::query(
        "SELECT step_name, state, idempotency_key, attempt_count, adopted_existing,
                result, error_message, started_at, finished_at
           FROM provisioning_operation_steps
          WHERE company_id = $1 AND operation_id = $2
          ORDER BY step_index",
    )
    .bind(company_id)
    .bind(operation_id)
    .fetch_all(conn)
    .await?;
    rows.iter().map(step_from_row).collect()
}

async fn load_operation_on(
    conn: &mut PgConnection,
    company_id: Uuid,
    operation_id: Uuid,
) -> Result<Option<ProvisioningOperation>> {
    let row = sqlx::query(
        "SELECT id, employee_id, mode, dry_run, idempotency_key, manifest,
                manifest_fingerprint, status, current_step, result_revision_id,
                error_message, created_at, updated_at, finished_at
           FROM provisioning_operations
          WHERE company_id = $1 AND id = $2",
    )
    .bind(company_id)
    .bind(operation_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let steps = load_steps(conn, company_id, operation_id).await?;
    let employee_id: String = row.try_get("employee_id")?;
    let mode: String = row.try_get("mode")?;
    let status: String = row.try_get("status")?;
    let current_step: Option<String> = row.try_get("current_step")?;
    let fingerprint: Vec<u8> = row.try_get("manifest_fingerprint")?;
    Ok(Some(ProvisioningOperation {
        id: row.try_get("id")?,
        employee_id: EmployeeId::parse(employee_id)?,
        mode: parse_column("provisioning_operations.mode", &mode)?,
        dry_run: row.try_get("dry_run")?,
        idempotency_key: row.try_get("idempotency_key")?,
        manifest: serde_json::from_value(row.try_get("manifest")?)?,
        manifest_fingerprint: bytes32(
            "provisioning_operations.manifest_fingerprint",
            &fingerprint,
        )?,
        status: parse_column("provisioning_operations.status", &status)?,
        current_step: current_step
            .as_deref()
            .map(|name| {
                ProvisioningStep::parse(name).ok_or_else(|| {
                    ControlError::InvalidData(format!("current_step {name:?} is unknown"))
                })
            })
            .transpose()?,
        result_revision_id: row.try_get("result_revision_id")?,
        error_message: row.try_get("error_message")?,
        steps,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        finished_at: row.try_get("finished_at")?,
    }))
}

async fn upsert_step(
    conn: &mut PgConnection,
    company_id: Uuid,
    operation_id: Uuid,
    step: &StepRecord,
) -> Result<()> {
    let finished = matches!(
        step.state,
        StepState::Succeeded | StepState::Failed | StepState::Compensated | StepState::Skipped
    );
    sqlx::query(
        "INSERT INTO provisioning_operation_steps
             (company_id, operation_id, step_index, step_name, state, idempotency_key,
              attempt_count, adopted_existing, result, error_message, started_at, finished_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                 CASE WHEN $13 THEN coalesce($12, now()) ELSE $12 END)
         ON CONFLICT (company_id, operation_id, step_index) DO UPDATE
            SET state = EXCLUDED.state,
                attempt_count = EXCLUDED.attempt_count,
                adopted_existing = EXCLUDED.adopted_existing,
                result = EXCLUDED.result,
                error_message = EXCLUDED.error_message,
                started_at = EXCLUDED.started_at,
                finished_at = EXCLUDED.finished_at,
                updated_at = now()",
    )
    .bind(company_id)
    .bind(operation_id)
    .bind(step.step.index())
    .bind(step.step.name())
    .bind(step.state.as_str())
    .bind(&step.idempotency_key)
    .bind(step.attempt_count)
    .bind(step.adopted_existing)
    .bind(&step.result)
    .bind(step.error_message.as_deref())
    .bind(step.started_at)
    .bind(step.finished_at)
    .bind(finished)
    .execute(conn)
    .await?;
    Ok(())
}

/// Locks the operation row for the rest of the transaction and returns its
/// status and whether it already activated a revision.
async fn lock_operation_row(
    conn: &mut PgConnection,
    company_id: Uuid,
    operation_id: Uuid,
) -> Result<(OperationStatus, bool)> {
    let row = sqlx::query(
        "SELECT status, result_revision_id FROM provisioning_operations
          WHERE company_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(company_id)
    .bind(operation_id)
    .fetch_optional(conn)
    .await?
    .ok_or(ProvisioningError::UnknownOperation { operation_id })?;
    let status: String = row.try_get("status")?;
    let status: OperationStatus = parse_column("provisioning_operations.status", &status)?;
    let activated = row
        .try_get::<Option<Uuid>, _>("result_revision_id")?
        .is_some();
    Ok((status, activated))
}

impl ProvisioningRepository for PgControlPlane {
    async fn begin_operation(
        &self,
        scope: &CompanyScope,
        request: &ProvisioningRequest,
    ) -> Result<ProvisioningOperation> {
        let fingerprint = request.fingerprint()?;
        let manifest = serde_json::to_value(&request.manifest)?;
        let company_id = scope.company_id();
        let mut tx = self.pool.begin().await?;

        // The operation row references the employee row; reserve the draft
        // identity here and let the saga step verify it.
        sqlx::query(
            "INSERT INTO employees (company_id, id, status) VALUES ($1, $2, 'draft')
             ON CONFLICT DO NOTHING",
        )
        .bind(company_id)
        .bind(request.employee_id.as_str())
        .execute(&mut *tx)
        .await?;

        let inserted = sqlx::query(
            "INSERT INTO provisioning_operations
                 (company_id, employee_id, mode, dry_run, idempotency_key, manifest, manifest_fingerprint)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (company_id, idempotency_key) DO NOTHING
             RETURNING id",
        )
        .bind(company_id)
        .bind(request.employee_id.as_str())
        .bind(request.mode.as_str())
        .bind(request.dry_run)
        .bind(&request.idempotency_key)
        .bind(&manifest)
        .bind(fingerprint.to_vec())
        .fetch_optional(&mut *tx)
        .await?;

        let operation_id: Uuid = match inserted {
            Some(row) => {
                let id: Uuid = row.try_get("id")?;
                for step in ProvisioningStep::ALL {
                    upsert_step(&mut tx, company_id, id, &StepRecord::pending(id, step)).await?;
                }
                id
            }
            None => {
                let row = sqlx::query(
                    "SELECT id, manifest_fingerprint, mode, dry_run FROM provisioning_operations
                      WHERE company_id = $1 AND idempotency_key = $2",
                )
                .bind(company_id)
                .bind(&request.idempotency_key)
                .fetch_one(&mut *tx)
                .await?;
                let id: Uuid = row.try_get("id")?;
                let existing: Vec<u8> = row.try_get("manifest_fingerprint")?;
                let existing_mode: String = row.try_get("mode")?;
                let existing_dry_run: bool = row.try_get("dry_run")?;
                if existing != fingerprint
                    || existing_mode != request.mode.as_str()
                    || existing_dry_run != request.dry_run
                {
                    return Err(ProvisioningError::IdempotencyConflict { operation_id: id }.into());
                }
                id
            }
        };
        let operation = load_operation_on(&mut tx, company_id, operation_id)
            .await?
            .ok_or(ProvisioningError::UnknownOperation { operation_id })?;
        tx.commit().await?;
        Ok(operation)
    }

    async fn load_operation(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
    ) -> Result<Option<ProvisioningOperation>> {
        let mut conn = self.pool.acquire().await?;
        load_operation_on(&mut conn, scope.company_id(), operation_id).await
    }

    async fn update_operation(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        update: &OperationUpdate,
    ) -> Result<()> {
        let company_id = scope.company_id();
        let mut tx = self.pool.begin().await?;
        let (status, activated) = lock_operation_row(&mut tx, company_id, operation_id).await?;
        if activated {
            return Err(ProvisioningError::Superseded {
                operation_id,
                detail: "operation already activated a revision",
            }
            .into());
        }
        if !status.can_transition_to(update.status) {
            return Err(ProvisioningError::Superseded {
                operation_id,
                detail: "operation status does not allow this update",
            }
            .into());
        }
        sqlx::query(
            "UPDATE provisioning_operations
                SET status = $3,
                    current_step = $4,
                    error_message = $5,
                    finished_at = CASE WHEN $6 THEN coalesce(finished_at, now()) ELSE NULL END,
                    updated_at = now()
              WHERE company_id = $1 AND id = $2 AND result_revision_id IS NULL",
        )
        .bind(company_id)
        .bind(operation_id)
        .bind(update.status.as_str())
        .bind(update.current_step.map(ProvisioningStep::name))
        .bind(update.error_message.as_deref())
        .bind(update.is_finished())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn record_step(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        step: &StepRecord,
    ) -> Result<()> {
        let company_id = scope.company_id();
        let mut tx = self.pool.begin().await?;
        let (status, activated) = lock_operation_row(&mut tx, company_id, operation_id).await?;
        if activated || status.is_terminal() {
            return Err(ProvisioningError::Superseded {
                operation_id,
                detail: "operation is terminal; step writes are refused",
            }
            .into());
        }
        let existing = sqlx::query(
            "SELECT state FROM provisioning_operation_steps
              WHERE company_id = $1 AND operation_id = $2 AND step_index = $3",
        )
        .bind(company_id)
        .bind(operation_id)
        .bind(step.step.index())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = existing {
            let state: String = row.try_get("state")?;
            let state: StepState = parse_column("provisioning_operation_steps.state", &state)?;
            if !state.can_transition_to(step.state) {
                return Err(ProvisioningError::Superseded {
                    operation_id,
                    detail: "step state does not allow this write",
                }
                .into());
            }
        }
        upsert_step(&mut tx, company_id, operation_id, step).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn reserve_employee_identity(
        &self,
        scope: &CompanyScope,
        employee_id: &EmployeeId,
    ) -> Result<IdentityReservation> {
        let row = sqlx::query(
            "WITH inserted AS (
                 INSERT INTO employees (company_id, id, status) VALUES ($1, $2, 'draft')
                 ON CONFLICT DO NOTHING
                 RETURNING status
             )
             SELECT true AS created, status FROM inserted
             UNION ALL
             SELECT false AS created, status FROM employees
              WHERE company_id = $1 AND id = $2
                AND NOT EXISTS (SELECT 1 FROM inserted)",
        )
        .bind(scope.company_id())
        .bind(employee_id.as_str())
        .fetch_one(&self.pool)
        .await?;
        let created: bool = row.try_get("created")?;
        let status: String = row.try_get("status")?;
        let status: EmployeeStatus = parse_column("employees.status", &status)?;
        Ok(if created {
            IdentityReservation::Created
        } else {
            IdentityReservation::Existing { status }
        })
    }

    async fn prepare_activation(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        running: &StepRecord,
        lifetime: std::time::Duration,
    ) -> Result<ActivationTarget> {
        activation::prepare(self, scope, operation_id, running, lifetime).await
    }

    async fn activate_revision(
        &self,
        scope: &CompanyScope,
        operation_id: Uuid,
        activation: &RevisionActivation,
    ) -> Result<Uuid> {
        let company_id = scope.company_id();
        let employee = &activation.employee;
        let employee_id = employee.id.as_str();
        let mode = column_value(&activation.provisioning_mode)?;
        let mut tx = self.pool.begin().await?;
        activation::configure(&mut tx).await?;
        // Must precede operation/employee/step row locks. Hold through commit.
        let office = super::lock_office_authority_on(&mut tx, scope).await?;

        let operation = sqlx::query(
            "SELECT status, dry_run, result_revision_id FROM provisioning_operations
              WHERE company_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(company_id)
        .bind(operation_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ProvisioningError::UnknownOperation { operation_id })?;
        let status: String = operation.try_get("status")?;
        let status: OperationStatus = parse_column("provisioning_operations.status", &status)?;
        let dry_run: bool = operation.try_get("dry_run")?;
        if dry_run {
            return Err(ProvisioningError::InvalidTransition {
                status,
                action: "activate a dry-run",
            }
            .into());
        }
        if let Some(existing) = operation.try_get::<Option<Uuid>, _>("result_revision_id")? {
            // Idempotent replay of a committed activation.
            return Ok(existing);
        }
        if status.is_terminal() || status == OperationStatus::Compensating {
            return Err(ProvisioningError::InvalidTransition {
                status,
                action: "activate",
            }
            .into());
        }

        activation::validate(&mut tx, scope, operation_id, activation, &office).await?;
        // Upgrade before the first authority mutation, failing the whole attempt
        // if another reader prevents it; never wait while holding row locks.
        let exclusive: bool = sqlx::query_scalar(
            "SELECT pg_try_advisory_xact_lock(ortak_office_company_lock_key($1))",
        )
        .bind(company_id)
        .fetch_one(&mut *tx)
        .await?;
        if !exclusive {
            return Err(ProvisioningError::Superseded {
                operation_id,
                detail: "activation mutation fence is busy",
            }
            .into());
        }
        let manifest = serde_json::to_value(employee)?;
        let revision_id: Uuid = sqlx::query(
            "INSERT INTO employee_revisions
                 (company_id, employee_id, revision_number, manifest, manifest_fingerprint,
                  provisioning_mode, created_by)
             VALUES ($1, $2,
                     (SELECT coalesce(max(revision_number), 0) + 1 FROM employee_revisions
                       WHERE company_id = $1 AND employee_id = $2),
                     $3, $4, $5, $6)
             RETURNING id",
        )
        .bind(company_id)
        .bind(employee_id)
        .bind(&manifest)
        .bind(activation.manifest_fingerprint.to_vec())
        .bind(&mode)
        .bind(format!("provisioning:{operation_id}"))
        .fetch_one(&mut *tx)
        .await?
        .try_get("id")?;

        sqlx::query(
            "INSERT INTO employee_runtime_bindings
                 (company_id, revision_id, employee_id, adapter, provisioning_mode, profile_ref,
                  model, workspace_ref, credential_refs, options, validated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(company_id)
        .bind(revision_id)
        .bind(employee_id)
        .bind(&employee.runtime.adapter)
        .bind(&mode)
        .bind(employee.runtime.profile_ref.as_deref())
        .bind(&employee.runtime.model)
        .bind(&employee.runtime.workspace_ref)
        .bind(serde_json::to_value(&employee.runtime.credential_refs)?)
        .bind(serde_json::to_value(&employee.runtime.options)?)
        .bind(activation.runtime_validated_at)
        .execute(&mut *tx)
        .await?;

        if let Some(memory) = &employee.memory {
            sqlx::query(
                "INSERT INTO employee_memory_bindings
                     (company_id, revision_id, employee_id, adapter, provisioning_mode,
                      endpoint_ref, workspace, user_peer, employee_peer, options, validated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            )
            .bind(company_id)
            .bind(revision_id)
            .bind(employee_id)
            .bind(&memory.adapter)
            .bind(&mode)
            .bind(&memory.endpoint_ref)
            .bind(&memory.workspace)
            .bind(&memory.user_peer)
            .bind(&memory.employee_peer)
            .bind(serde_json::to_value(&memory.options)?)
            .bind(activation.memory_validated_at)
            .execute(&mut *tx)
            .await?;
        }

        upsert_office_binding(
            &mut tx,
            company_id,
            revision_id,
            employee,
            &mode,
            activation.office_verified_at,
        )
        .await?;

        replace_aliases(&mut tx, company_id, revision_id, employee).await?;

        sqlx::query(
            "UPDATE employees
                SET active_revision_id = $3, status = 'active', updated_at = now()
              WHERE company_id = $1 AND id = $2",
        )
        .bind(company_id)
        .bind(employee_id)
        .bind(revision_id)
        .execute(&mut *tx)
        .await?;

        let mut step = activation.activation_step.clone();
        step.result["result_revision_id"] = serde_json::json!(revision_id);
        step.state = StepState::Succeeded;
        step.finished_at.get_or_insert_with(Utc::now);
        upsert_step(&mut tx, company_id, operation_id, &step).await?;

        // Restore the supported deferred mode immediately before the final
        // success write; COMMIT is next so its fresh clock covers final waits.
        sqlx::query("SET CONSTRAINTS ortak_activation_admission_at_commit DEFERRED")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE provisioning_operations
                SET status = 'succeeded', result_revision_id = $3, current_step = NULL,
                    error_message = NULL, finished_at = now(), updated_at = now()
              WHERE company_id = $1 AND id = $2",
        )
        .bind(company_id)
        .bind(operation_id)
        .bind(revision_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(revision_id)
    }
}

/// Reuses the employee's current binding for this public key (refreshing its
/// verification), rotates any other open-ended binding, and refuses a key
/// that belongs to another employee.
///
/// A retired binding (`valid_until` set) is never reactivated or reused as
/// the current signing identity, and a current binding is only reused when
/// the manifest names the same signer reference; both cases fail closed so
/// historical signatures stay attributable to exactly one signer window.
async fn upsert_office_binding(
    tx: &mut PgConnection,
    company_id: Uuid,
    revision_id: Uuid,
    employee: &ortak_domain::Employee,
    mode: &str,
    verified_at: DateTime<Utc>,
) -> Result<()> {
    let public_key = OfficePublicKey::parse_hex(&employee.office.public_key)?;
    let existing = sqlx::query(
        "SELECT id, employee_id, signer_ref, valid_until FROM employee_office_bindings
          WHERE company_id = $1 AND public_key = $2 FOR UPDATE",
    )
    .bind(company_id)
    .bind(public_key.as_bytes().to_vec())
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(row) = existing {
        let owner: String = row.try_get("employee_id")?;
        if owner != employee.id.as_str() {
            return Err(ControlError::InvalidData(format!(
                "office public key already belongs to employee {owner}"
            )));
        }
        let retired: Option<DateTime<Utc>> = row.try_get("valid_until")?;
        if retired.is_some() {
            return Err(ControlError::InvalidData(format!(
                "office public key {} was retired for employee {owner} and cannot be reactivated; rotate to a new key",
                public_key.to_hex()
            )));
        }
        let signer_ref: String = row.try_get("signer_ref")?;
        if signer_ref != employee.office.signer_ref.as_str() {
            return Err(ControlError::InvalidData(format!(
                "office public key {} is bound to a different signer reference for employee {owner}",
                public_key.to_hex()
            )));
        }
        let binding_id: Uuid = row.try_get("id")?;
        sqlx::query(
            "UPDATE employee_office_bindings
                SET verified_at = $3, home_channel_ref = $4
              WHERE company_id = $1 AND id = $2",
        )
        .bind(company_id)
        .bind(binding_id)
        .bind(verified_at)
        .bind(employee.office.home_channel_ref.as_deref())
        .execute(&mut *tx)
        .await?;
        return Ok(());
    }

    let rotated_from: Option<Uuid> = sqlx::query(
        "UPDATE employee_office_bindings
            SET valid_until = now()
          WHERE company_id = $1 AND employee_id = $2 AND valid_until IS NULL
          RETURNING id",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .fetch_optional(&mut *tx)
    .await?
    .map(|row| row.try_get("id"))
    .transpose()?;

    sqlx::query(
        "INSERT INTO employee_office_bindings
             (company_id, employee_id, revision_id, provisioning_mode, public_key, signer_ref,
              home_channel_ref, rotated_from_binding_id, verified_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(company_id)
    .bind(employee.id.as_str())
    .bind(revision_id)
    .bind(mode)
    .bind(public_key.as_bytes().to_vec())
    .bind(employee.office.signer_ref.as_str())
    .bind(employee.office.home_channel_ref.as_deref())
    .bind(rotated_from)
    .bind(verified_at)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Replaces the employee's alias set with the new revision's; a collision with
/// another employee is a validation error, not a database error.
async fn replace_aliases(
    tx: &mut PgConnection,
    company_id: Uuid,
    revision_id: Uuid,
    employee: &ortak_domain::Employee,
) -> Result<()> {
    sqlx::query("DELETE FROM employee_aliases WHERE company_id = $1 AND employee_id = $2")
        .bind(company_id)
        .bind(employee.id.as_str())
        .execute(&mut *tx)
        .await?;
    let sources = std::iter::once((employee.id.as_str(), "id"))
        .chain(std::iter::once((employee.name.as_str(), "name")))
        .chain(
            employee
                .aliases
                .iter()
                .map(|alias| (alias.as_str(), "alias")),
        );
    let mut seen = std::collections::BTreeSet::new();
    for (raw, source) in sources {
        let alias = normalize_alias(raw);
        if alias.is_empty() || !seen.insert(alias.clone()) {
            continue;
        }
        let inserted = sqlx::query(
            "INSERT INTO employee_aliases (company_id, alias, employee_id, revision_id, source)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(company_id)
        .bind(&alias)
        .bind(employee.id.as_str())
        .bind(revision_id)
        .bind(source)
        .execute(&mut *tx)
        .await;
        match inserted {
            Ok(_) => {}
            Err(error) if is_unique_violation(&error) => {
                return Err(ControlError::InvalidData(format!(
                    "alias {alias:?} already belongs to another employee"
                )));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
