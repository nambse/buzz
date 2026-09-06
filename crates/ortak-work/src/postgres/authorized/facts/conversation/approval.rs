use super::*;
use ortak_control::memory::conversation::ConversationAudienceKind;

impl AuthorizedWork {
    /// Approve one edited conversation fact and its exact receipt atomically.
    /// Replay checks immutable submitted fields before resolving a fresh source
    /// or applying new-admission expiry. This never publishes or enables recall.
    pub async fn promote_conversation_fact(
        &self,
        operation: Uuid,
        project_id: Uuid,
        draft: ReviewedConversationFactDraft,
    ) -> Result<ReviewedConversationFactReceipt> {
        bounded(self.promote_conversation_inner(operation, project_id, draft)).await
    }

    async fn promote_conversation_inner(
        &self,
        operation: Uuid,
        project_id: Uuid,
        draft: ReviewedConversationFactDraft,
    ) -> Result<ReviewedConversationFactReceipt> {
        let hash = draft.submitted_fingerprint(project_id)?;
        let (mut tx, deadline) = self.begin().await?;
        let replay = self
            .fact_operation_on(&mut tx, operation, "promote", &hash)
            .await?;
        let project = self.project_on(&mut tx, project_id).await?;
        self.review(project.role)?;
        self.fact_employee_scope(&draft.employee_id)?;
        if let Some(id) = replay {
            let (fact, current_deadline) =
                self.conversation_fact_on(&mut tx, project_id, id).await?;
            self.finish(tx, earliest(deadline, current_deadline))
                .await?;
            return Ok(ReviewedConversationFactReceipt {
                fact,
                created: false,
            });
        }
        if project.record.project.status != ProjectStatus::Active {
            return Err(WorkError::ProjectArchived { project_id });
        }
        self.employee_on(&mut tx, project.channel_id, &draft.employee_id)
            .await?;
        let source = types::source_id(&draft.source_message_id)?;
        let observed = self
            .resolve_conversation_fact_on(
                &mut tx,
                project_id,
                &draft.employee_id,
                source,
                draft.audience.kind(),
            )
            .await?
            .ok_or(WorkError::AccessDenied)?;
        if observed
            .audience()
            .audience_hash()
            .map_err(|_| invalid("invalid conversation audience"))?
            .to_hex()
            != draft.expected_audience_hash
        {
            return Err(WorkError::OperationConflict);
        }
        let deadline = earliest(deadline, observed.valid_before());
        draft.validate_expiry(observed.observed_at(), deadline)?;
        // Registration serializes its retained per-company cap and locks this
        // existing channel tuple. It grants no publication or runtime access.
        let _: i64 =
            sqlx::query_scalar("SELECT ortak_register_conversation_authority($1,$2,$3,$4)")
                .bind(self.scope.company_id())
                .bind(self.principal.community_id)
                .bind(project_id)
                .bind(project.channel_id)
                .fetch_one(&mut *tx)
                .await?;
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO reviewed_memory_facts(company_id,id,project_id,employee_id,source_message_id,
            content,approved_by,approved_at,expires_at,promotion_operation_id,community_id,audience_kind)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'conversation')")
            .bind(self.scope.company_id()).bind(id).bind(project_id).bind(draft.employee_id.as_str())
            .bind(source.as_bytes().as_slice()).bind(&draft.content).bind(&self.principal.public_key)
            .bind(observed.observed_at()).bind(draft.expires_at).bind(operation)
            .bind(self.principal.community_id).execute(&mut *tx).await?;
        let provenance = observed.provenance();
        let audience = provenance.audience();
        let root = audience.thread_root();
        let audience_bytes = audience
            .canonical_bytes()
            .map_err(|_| invalid("invalid conversation audience"))?;
        let audience_hash = audience
            .audience_hash()
            .map_err(|_| invalid("invalid conversation audience"))?;
        let source_hash = provenance
            .source_hash()
            .map_err(|_| invalid("invalid conversation provenance"))?;
        let provenance_bytes = provenance
            .canonical_bytes()
            .map_err(|_| invalid("invalid conversation provenance"))?;
        sqlx::query("INSERT INTO reviewed_memory_conversation_audiences(company_id,community_id,fact_id,project_id,
            employee_id,channel_id,kind,thread_root_event_id,thread_root_event_created_at,source_event_id,
            source_event_created_at,audience_bytes,audience_hash,source_evidence_hash,source_hash,provenance_bytes)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
            .bind(self.scope.company_id()).bind(self.principal.community_id).bind(id).bind(project_id)
            .bind(draft.employee_id.as_str()).bind(audience.channel_id())
            .bind(match audience.kind() { ConversationAudienceKind::Thread => "thread", ConversationAudienceKind::Channel => "channel" })
            .bind(root.map(|r| r.event_id().as_bytes().to_vec())).bind(root.map(|r| r.created_at()))
            .bind(source.as_bytes().as_slice()).bind(provenance.source().created_at()).bind(audience_bytes)
            .bind(audience_hash.as_bytes().as_slice()).bind(provenance.source_evidence_hash().as_bytes().as_slice())
            .bind(source_hash.as_bytes().as_slice()).bind(provenance_bytes).execute(&mut *tx).await?;
        // Resolve again after registration/insertion and compare the full source
        // evidence, not only its audience hash. The deferred SQL guard remains
        // the final admission fence against a concurrent canonical mutation.
        let (fact, current_deadline) = self.conversation_fact_on(&mut tx, project_id, id).await?;
        if !fact.fact.source_visible {
            return Err(WorkError::AccessDenied);
        }
        let deadline = earliest(deadline, current_deadline);
        let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx)
            .await?;
        draft.validate_expiry(now, deadline)?;
        self.record_fact_operation_on(&mut tx, operation, "promote", &hash, &fact.fact, deadline)
            .await?;
        self.finish(tx, deadline).await?;
        Ok(ReviewedConversationFactReceipt {
            fact,
            created: true,
        })
    }
}
