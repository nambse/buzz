use super::*;

impl AuthorizedWork {
    /// Permanently stop a conversation fact under current project review authority.
    /// Missing/changed source, archived project and inactive employee retain this
    /// recovery path. No remote erasure or publication receipt is manufactured.
    pub async fn revoke_conversation_fact(
        &self,
        operation: Uuid,
        project_id: Uuid,
        id: Uuid,
        expected_version: i64,
        reason: String,
    ) -> Result<ReviewedConversationFactReceipt> {
        bounded(self.revoke_conversation_inner(operation, project_id, id, expected_version, reason))
            .await
    }

    async fn revoke_conversation_inner(
        &self,
        operation: Uuid,
        project_id: Uuid,
        id: Uuid,
        version: i64,
        reason: String,
    ) -> Result<ReviewedConversationFactReceipt> {
        if project_id.is_nil()
            || id.is_nil()
            || version != 1
            || reason.trim().is_empty()
            || reason.len() > 512
            || reason.chars().any(char::is_control)
            || ortak_control::run_event::RedactionPolicy::new().redact(&reason) != reason
        {
            return Err(WorkError::InvalidQuery("invalid conversation stop request"));
        }
        let hash = fingerprint((
            "ortak-reviewed-conversation-revoke/1",
            project_id,
            id,
            version,
            &reason,
        ))?;
        let (mut tx, deadline) = self.begin().await?;
        let replay = self
            .fact_operation_on(&mut tx, operation, "revoke", &hash)
            .await?;
        let project = self.project_on(&mut tx, project_id).await?;
        self.review(project.role)?;
        let row = sqlx::query("SELECT employee_id,version FROM reviewed_memory_facts
            WHERE company_id=$1 AND project_id=$2 AND id=$3 AND audience_kind='conversation' FOR UPDATE")
            .bind(self.scope.company_id()).bind(project_id).bind(id).fetch_optional(&mut *tx).await?
            .ok_or(WorkError::AccessDenied)?;
        self.fact_employee_scope(&EmployeeId::parse(
            row.try_get::<String, _>("employee_id")?,
        )?)?;
        if let Some(replayed) = replay {
            if replayed != id {
                return Err(WorkError::OperationConflict);
            }
        } else {
            if row.try_get::<i64, _>("version")? != version {
                return Err(WorkError::OperationConflict);
            }
            sqlx::query(
                "UPDATE reviewed_memory_facts SET version=version+1,revoked_at=clock_timestamp(),
                revoked_by=$4,revoke_reason=$5,revocation_operation_id=$6
                WHERE company_id=$1 AND project_id=$2 AND id=$3 AND audience_kind='conversation'",
            )
            .bind(self.scope.company_id())
            .bind(project_id)
            .bind(id)
            .bind(&self.principal.public_key)
            .bind(reason)
            .bind(operation)
            .execute(&mut *tx)
            .await?;
        }
        let (fact, current_deadline) = self.conversation_fact_on(&mut tx, project_id, id).await?;
        let deadline = earliest(deadline, current_deadline);
        if replay.is_none() {
            self.record_fact_operation_on(
                &mut tx, operation, "revoke", &hash, &fact.fact, deadline,
            )
            .await?;
        }
        self.finish(tx, deadline).await?;
        Ok(ReviewedConversationFactReceipt {
            fact,
            created: replay.is_none(),
        })
    }
}
