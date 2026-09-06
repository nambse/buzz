//! Explicit human Work dispatch. The transaction creates durable work only.
use super::*;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Current execution receipt; runtime details and artifact bytes have separate authorization.
#[derive(Clone, Debug, Serialize)]
pub struct WorkExecutionReceipt {
    /// Durable run identity, stable across operation retries.
    pub run_id: Uuid,
    /// Work item owning the run.
    pub work_item_id: Uuid,
    /// Work version created by the original request.
    pub execution_version: i64,
    /// Currently recorded run status.
    pub status: String,
    /// True only for the first committed request.
    pub created: bool,
}

impl AuthorizedWork {
    /// Queue the selected assignment with an immutable definition and exact retry receipt.
    /// The existing supervisor alone may start its runtime after fresh admission and recall.
    pub async fn request_execution(
        &self,
        operation_id: Uuid,
        work_item_id: Uuid,
        expected_version: i64,
        employee_id: EmployeeId,
    ) -> Result<WorkExecutionReceipt> {
        bounded(self.request_execution_inner(
            operation_id,
            work_item_id,
            expected_version,
            employee_id,
        ))
        .await
    }

    async fn request_execution_inner(
        &self,
        op: Uuid,
        id: Uuid,
        version: i64,
        employee: EmployeeId,
    ) -> Result<WorkExecutionReceipt> {
        if version < 1 {
            return Err(WorkError::InvalidQuery(
                "expected_version must be at least 1",
            ));
        }
        let hash = fingerprint(("start_execution", id, version, &employee))?;
        let (mut tx, deadline) = self.begin().await?;
        let replay = self
            .operation_on(&mut tx, op, "mutate_work_item", &hash)
            .await?;
        let (project, aggregate) = self.item_on(&mut tx, id, true).await?;
        self.contribute(project.role)?;
        if !self.principal.employee_ids.contains(&employee) {
            return Err(WorkError::AccessDenied);
        }
        if let Some(replay) = replay {
            if replay.project_id != project.record.project.id || replay.work_item_id != Some(id) {
                return Err(WorkError::OperationConflict);
            }
            let row = sqlx::query(
                "SELECT x.run_id,x.execution_version,r.status FROM work_executions x
                JOIN runs r ON r.company_id=x.company_id AND r.id=x.run_id
                WHERE x.company_id=$1 AND x.requested_by=$2 AND x.operation_id=$3
                AND x.work_item_id=$4 AND x.employee_id=$5 AND x.requested_version=$6",
            )
            .bind(self.scope.company_id())
            .bind(&self.principal.public_key)
            .bind(op)
            .bind(id)
            .bind(employee.as_str())
            .bind(version)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| invalid("execution receipt provenance missing"))?;
            let execution_version = row.try_get("execution_version")?;
            if execution_version != replay.result_version {
                return Err(invalid("execution receipt version disagrees"));
            }
            let receipt = WorkExecutionReceipt {
                run_id: row.try_get("run_id")?,
                work_item_id: id,
                execution_version,
                status: row.try_get("status")?,
                created: false,
            };
            self.finish(tx, deadline).await?;
            return Ok(receipt);
        }
        if project.record.project.status != ProjectStatus::Active {
            return Err(WorkError::ProjectArchived {
                project_id: project.record.project.id,
            });
        }
        if aggregate.item.version != version {
            return Err(WorkError::VersionConflict {
                record_id: id,
                expected: version,
                actual: aggregate.item.version,
            });
        }
        self.execution_employee_on(&mut tx, project.channel_id, &employee)
            .await?;
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM work_executions
            WHERE company_id=$1 AND work_item_id=$2 AND reconciled_at IS NULL)",
        )
        .bind(self.scope.company_id())
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if active {
            return Err(WorkError::OperationConflict);
        }
        let revision = sqlx::query("SELECT e.active_revision_id,b.adapter FROM employees e
            JOIN employee_runtime_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id AND b.revision_id=e.active_revision_id
            WHERE e.company_id=$1 AND e.id=$2 AND e.status='active' AND b.validated_at IS NOT NULL")
            .bind(self.scope.company_id()).bind(employee.as_str()).fetch_optional(&mut *tx).await?
            .ok_or_else(|| WorkError::EmployeeNotAssignable { employee_id: employee.clone() })?;
        let revision_id: Uuid = revision.try_get("active_revision_id")?;
        let adapter: String = revision.try_get("adapter")?;
        let mut item = aggregate.item;
        let definition = item.execution_input()?;
        let definition_hash = Sha256::digest(&definition).to_vec();
        let run_id = Uuid::new_v4();
        let attachment_id = Uuid::new_v4();
        let event = item.request_execution(run_id, &employee, attachment_id)?;
        let witness =
            ortak_control::postgres::lock_office_authority_on(&mut tx, &self.scope).await?;
        sqlx::query("INSERT INTO runs(company_id,id,employee_id,employee_revision_id,work_item_id,runtime_adapter,status,
            office_admission_generation,office_admission_valid_before,office_admission_token)
            VALUES($1,$2,$3,$4,$5,$6,'queued',$7,$8,$9)")
            .bind(self.scope.company_id()).bind(run_id).bind(employee.as_str()).bind(revision_id).bind(id)
            .bind(adapter).bind(witness.generation()).bind(witness.valid_before()).bind(Uuid::new_v4())
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO work_executions(company_id,run_id,project_id,work_item_id,employee_id,employee_revision_id,
            requested_by,operation_id,requested_version,execution_version,definition_bytes,definition_hash)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
            .bind(self.scope.company_id()).bind(run_id).bind(item.project_id).bind(id).bind(employee.as_str())
            .bind(revision_id).bind(&self.principal.public_key).bind(op).bind(version).bind(item.version)
            .bind(definition).bind(definition_hash).execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO run_events(company_id,run_id,sequence,event_type,occurred_at,payload)
            VALUES($1,$2,0,'run.queued',clock_timestamp(),$3)",
        )
        .bind(self.scope.company_id())
        .bind(run_id)
        .bind(
            serde_json::to_value(ortak_control::run_event::RunEventPayload::RunQueued)
                .map_err(ControlError::Serde)?,
        )
        .execute(&mut *tx)
        .await?;
        let attachment = item
            .attachments
            .iter()
            .find(|a| a.id == attachment_id)
            .ok_or_else(|| invalid("new execution attachment missing"))?;
        insert_attachment(&mut tx, &self.scope, id, attachment, &self.actor()).await?;
        persist_event(&mut tx, &self.scope, &item, version, &self.actor(), &event).await?;
        sqlx::query(
            "INSERT INTO outbox(company_id,kind,dedup_key,employee_id,run_id,payload,max_attempts)
            VALUES($1,'work_run_dispatch',$2,$3,$4,'{}',8)",
        )
        .bind(self.scope.company_id())
        .bind(format!("work-run:{run_id}"))
        .bind(employee.as_str())
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE runs SET work_admission_generation=g.generation,work_admission_token=$3
            FROM work_authority_generations g WHERE runs.company_id=$1 AND runs.id=$2
            AND g.company_id=runs.company_id AND g.project_id=$4",
        )
        .bind(self.scope.company_id())
        .bind(run_id)
        .bind(Uuid::new_v4())
        .bind(item.project_id)
        .execute(&mut *tx)
        .await?;
        self.record_on(
            &mut tx,
            op,
            "mutate_work_item",
            &hash,
            item.project_id,
            Some(id),
            item.version,
            deadline,
        )
        .await?;
        self.finish(tx, deadline).await?;
        Ok(WorkExecutionReceipt {
            run_id,
            work_item_id: id,
            execution_version: item.version,
            status: "queued".into(),
            created: true,
        })
    }
}
