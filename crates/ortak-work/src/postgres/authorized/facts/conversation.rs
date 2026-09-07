//! Reviewed D4 facts. Approval never publishes or enables runtime use.
use super::*;
use ortak_control::memory::conversation::ConversationProvenanceV1;
use ortak_control::postgres::conversation_memory::{
    resolve_conversation_on, ConversationObservation, ConversationReadRequest,
};

mod approval;
mod records;
pub use records::{
    ReviewedConversationFact, ReviewedConversationFactPage, ReviewedConversationFactReceipt,
};
mod stop;
mod types;
pub use types::*;

impl AuthorizedWork {
    /// Observe the exact current conversation audience for a prospective edited fact.
    /// Requires an Operator with project Owner/Reviewer authority and a selected
    /// active employee. No source text, fact write, receipt or remote call occurs.
    /// The returned deadline is an upper bound, never a cache or approval lease.
    pub async fn preview_conversation_memory(
        &self,
        project_id: Uuid,
        request: ReviewedConversationPreviewRequest,
    ) -> Result<ReviewedConversationPreview> {
        bounded(self.preview_conversation_inner(project_id, request)).await
    }

    async fn preview_conversation_inner(
        &self,
        project_id: Uuid,
        request: ReviewedConversationPreviewRequest,
    ) -> Result<ReviewedConversationPreview> {
        request.validate()?;
        if project_id.is_nil() {
            return Err(WorkError::InvalidQuery("project id must not be nil"));
        }
        let (mut tx, deadline) = self.begin().await?;
        let project = self.project_on(&mut tx, project_id).await?;
        self.review(project.role)?;
        self.fact_employee_scope(&request.employee_id)?;
        if project.record.project.status != ProjectStatus::Active {
            return Err(WorkError::ProjectArchived { project_id });
        }
        self.employee_on(&mut tx, project.channel_id, &request.employee_id)
            .await?;
        let observation = self
            .resolve_conversation_fact_on(
                &mut tx,
                project_id,
                &request.employee_id,
                types::source_id(&request.source_message_id)?,
                request.audience.kind(),
            )
            .await?
            .ok_or(WorkError::AccessDenied)?;
        let valid_before = earliest(deadline, observation.valid_before());
        let preview = preview_view(
            observation.provenance(),
            observation.observed_at(),
            valid_before,
        )?;
        self.finish(tx, valid_before).await?;
        Ok(preview)
    }

    async fn resolve_conversation_fact_on(
        &self,
        connection: &mut PgConnection,
        project_id: Uuid,
        employee_id: &EmployeeId,
        source_message_id: MessageId,
        audience_kind: ortak_control::memory::conversation::ConversationAudienceKind,
    ) -> Result<Option<ConversationObservation>> {
        let human_public_key: &[u8; 32] = self
            .principal
            .key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| WorkError::AccessDenied)?;
        let channels: Vec<_> = self.principal.channel_ids.iter().copied().collect();
        let employees: Vec<_> = self.principal.employee_ids.iter().cloned().collect();
        Ok(resolve_conversation_on(
            connection,
            &ConversationReadRequest {
                scope: &self.scope,
                project_id,
                employee_id,
                human_public_key,
                channel_grants: &channels,
                employee_grants: &employees,
                source_message_id,
                audience_kind,
            },
        )
        .await?)
    }
}

fn earliest(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn preview_view(
    provenance: &ConversationProvenanceV1,
    observed_at: DateTime<Utc>,
    valid_before: Option<DateTime<Utc>>,
) -> Result<ReviewedConversationPreview> {
    let max_expires_at = types::expiry_limit(observed_at, valid_before)?;
    // Serialize the already validated canonical v1 values, not a second DTO
    // whose field names, timestamp precision or null roots could drift.
    let audience = provenance.audience();
    let invalid = || WorkError::InvalidRecord {
        detail: "invalid conversation observation".into(),
    };
    Ok(ReviewedConversationPreview {
        audience: serde_json::from_slice(&audience.canonical_bytes().map_err(|_| invalid())?)
            .map_err(|_| invalid())?,
        audience_hash: audience.audience_hash().map_err(|_| invalid())?.to_hex(),
        provenance: serde_json::from_slice(&provenance.canonical_bytes().map_err(|_| invalid())?)
            .map_err(|_| invalid())?,
        observed_at,
        valid_before,
        max_expires_at,
    })
}

#[cfg(test)]
mod tests;
