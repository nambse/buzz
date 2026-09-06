//! Reviewed project facts reuse the current project/human authority facade.
use super::*;
use chrono::{DateTime, Utc};

mod reads;
mod receipt;
mod types;
pub use types::*;
mod conversation;
pub use conversation::*;

impl AuthorizedWork {
    /// Approve one edited fact for one project/employee, atomically with its receipt.
    /// No Honcho request, provider call or runtime dispatch is performed.
    pub async fn promote_reviewed_fact(
        &self,
        operation: Uuid,
        project: Uuid,
        draft: ReviewedFactDraft,
    ) -> Result<ReviewedFactReceipt> {
        bounded(self.promote_fact_inner(operation, project, draft)).await
    }

    async fn promote_fact_inner(
        &self,
        operation: Uuid,
        project_id: Uuid,
        draft: ReviewedFactDraft,
    ) -> Result<ReviewedFactReceipt> {
        draft.validate()?;
        let hash = fingerprint((project_id, &draft))?;
        let (mut tx, deadline) = self.begin().await?;
        let replay = self
            .fact_operation_on(&mut tx, operation, "promote", &hash)
            .await?;
        let project = self.project_on(&mut tx, project_id).await?;
        self.review(project.role)?;
        self.fact_employee_scope(&draft.employee_id)?;
        if let Some(id) = replay {
            let fact = self
                .fact_on(&mut tx, project_id, id, project.channel_id)
                .await?;
            self.finish(tx, deadline).await?;
            return Ok(ReviewedFactReceipt {
                fact,
                created: false,
            });
        }
        if project.record.project.status != ProjectStatus::Active {
            return Err(WorkError::ProjectArchived { project_id });
        }
        self.employee_on(&mut tx, project.channel_id, &draft.employee_id)
            .await?;
        if !self
            .fact_source_visible_on(
                &mut tx,
                project_id,
                project.channel_id,
                &draft.employee_id,
                &draft.source,
            )
            .await?
        {
            return Err(WorkError::AccessDenied);
        }
        let approved_at: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx)
            .await?;
        if draft.expires_at <= approved_at
            || draft.expires_at > approved_at + chrono::Duration::days(90)
        {
            return Err(WorkError::InvalidQuery(
                "fact expiry must be in the next 90 days",
            ));
        }
        let (message, artifact) = match &draft.source {
            ReviewedFactSource::Conversation { message_id } => (
                Some(MessageId::parse_hex(message_id)?.as_bytes().to_vec()),
                None,
            ),
            ReviewedFactSource::Artifact { artifact_id } => (None, Some(*artifact_id)),
        };
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO reviewed_memory_facts(company_id,id,project_id,employee_id,source_message_id,
            source_artifact_id,content,approved_by,approved_at,expires_at,promotion_operation_id,community_id)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
            .bind(self.scope.company_id()).bind(id).bind(project_id).bind(draft.employee_id.as_str())
            .bind(message).bind(artifact).bind(&draft.content).bind(&self.principal.public_key)
            .bind(approved_at).bind(draft.expires_at).bind(operation).bind(self.principal.community_id).execute(&mut *tx).await?;
        let fact = self
            .fact_on(&mut tx, project_id, id, project.channel_id)
            .await?;
        self.record_fact_operation_on(&mut tx, operation, "promote", &hash, &fact, deadline)
            .await?;
        self.finish(tx, deadline).await?;
        Ok(ReviewedFactReceipt {
            fact,
            created: true,
        })
    }

    /// Permanently stop use; retain the record and audit even if evidence disappeared.
    /// Replays return the same fact without another version or revocation event.
    pub async fn revoke_reviewed_fact(
        &self,
        operation: Uuid,
        project: Uuid,
        fact: Uuid,
        expected_version: i64,
        reason: String,
    ) -> Result<ReviewedFactReceipt> {
        bounded(self.revoke_fact_inner(operation, project, fact, expected_version, reason)).await
    }

    async fn revoke_fact_inner(
        &self,
        operation: Uuid,
        project_id: Uuid,
        id: Uuid,
        version: i64,
        reason: String,
    ) -> Result<ReviewedFactReceipt> {
        if version != 1
            || reason.trim().is_empty()
            || reason.len() > 512
            || reason.chars().any(char::is_control)
            || ortak_control::run_event::RedactionPolicy::new().redact(&reason) != reason
        {
            return Err(WorkError::InvalidQuery(
                "fact revocation version or reason is invalid",
            ));
        }
        let hash = fingerprint((project_id, id, version, &reason))?;
        let (mut tx, deadline) = self.begin().await?;
        let replay = self
            .fact_operation_on(&mut tx, operation, "revoke", &hash)
            .await?;
        let project = self.project_on(&mut tx, project_id).await?;
        self.review(project.role)?;
        let target = sqlx::query("SELECT employee_id,version FROM reviewed_memory_facts WHERE company_id=$1 AND project_id=$2 AND id=$3 AND audience_kind='project' FOR UPDATE")
            .bind(self.scope.company_id()).bind(project_id).bind(id).fetch_optional(&mut *tx).await?
            .ok_or(WorkError::AccessDenied)?;
        self.fact_employee_scope(&EmployeeId::parse(
            target.try_get::<String, _>("employee_id")?,
        )?)?;
        if let Some(replayed_id) = replay {
            if replayed_id != id {
                return Err(WorkError::OperationConflict);
            }
        } else {
            if target.try_get::<i64, _>("version")? != version {
                return Err(WorkError::OperationConflict);
            }
            sqlx::query("UPDATE reviewed_memory_facts SET version=version+1,revoked_at=clock_timestamp(),revoked_by=$4,
                revoke_reason=$5,revocation_operation_id=$6 WHERE company_id=$1 AND project_id=$2 AND id=$3")
                .bind(self.scope.company_id()).bind(project_id).bind(id).bind(&self.principal.public_key)
                .bind(reason).bind(operation).execute(&mut *tx).await?;
        }
        // Source content may now be hidden. Current project authority still permits
        // the stop operation, including for an archived project/inactive employee.
        let fact = self
            .fact_on(&mut tx, project_id, id, project.channel_id)
            .await?;
        if replay.is_none() {
            self.record_fact_operation_on(&mut tx, operation, "revoke", &hash, &fact, deadline)
                .await?;
        }
        self.finish(tx, deadline).await?;
        Ok(ReviewedFactReceipt {
            fact,
            created: replay.is_none(),
        })
    }

    pub(super) fn fact_employee_scope(&self, employee: &EmployeeId) -> Result<()> {
        if !self.principal.employee_ids.contains(employee) {
            return Err(WorkError::AccessDenied);
        }
        Ok(())
    }

    pub(super) async fn fact_source_visible_on(
        &self,
        c: &mut PgConnection,
        project: Uuid,
        channel: Uuid,
        employee: &EmployeeId,
        source: &ReviewedFactSource,
    ) -> Result<bool> {
        let (message, artifact) = match source {
            ReviewedFactSource::Conversation { message_id } => (
                Some(MessageId::parse_hex(message_id)?.as_bytes().to_vec()),
                None,
            ),
            ReviewedFactSource::Artifact { artifact_id } => (None, Some(*artifact_id)),
        };
        let mut q = sqlx::QueryBuilder::new("SELECT ");
        q.push(reads::SOURCE_PREDICATE).push(" FROM (SELECT $1::uuid AS company_id,$2::uuid AS project_id,
            $5::text AS employee_id,$6::bytea AS source_message_id,$7::uuid AS source_artifact_id) f");
        Ok(q.build_query_scalar()
            .bind(self.scope.company_id())
            .bind(project)
            .bind(self.principal.community_id)
            .bind(channel)
            .bind(employee.as_str())
            .bind(message)
            .bind(artifact)
            .fetch_one(c)
            .await?)
    }
}
