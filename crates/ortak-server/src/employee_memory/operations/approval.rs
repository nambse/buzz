use super::*;

impl Review<'_> {
    pub(in crate::employee_memory) async fn approve(&self, request: Approval) -> Result<Value> {
        nonnil(request.operation_id)?;
        request.fact.validate()?;
        let draft = request.fact;
        let bytes = wire::approval(&self.employee, request.operation_id, &draft)?;
        let (mut tx, mut deadline) = self.begin().await?;
        if let Some(replay) = self
            .operation_on(&mut tx, request.operation_id, "approve", &bytes)
            .await?
        {
            let (value, before) = self
                .receipt_on(
                    &mut tx,
                    request.operation_id,
                    replay.fact,
                    "approve",
                    replay.version,
                    false,
                )
                .await?;
            if let Some(before) = before {
                deadline = deadline.min(before);
            }
            self.finish(tx, deadline).await?;
            return Ok(value);
        }
        if !self.can_review() || !self.command_current(&mut tx, "approve").await? {
            return Err(forbidden());
        }
        let observed = self
            .observe(&mut tx, &draft.request(), draft.source_event_created_at)
            .await?
            .ok_or_else(forbidden)?;
        if observed
            .audience
            .audience_hash()
            .map_err(|_| ApiError::unavailable())?
            .to_hex()
            != draft.expected_audience_hash
        {
            return Err(conflict());
        }
        if draft.expires_at <= observed.observed_at
            || draft.expires_at > expiry_limit(observed.observed_at, observed.valid_before)?
        {
            return Err(ApiError::invalid());
        }
        if let Some(before) = observed.valid_before {
            deadline = deadline.min(before);
        }
        sqlx::query("SELECT ortak_register_employee_memory_authorities($1,$2,$3,$4,$5)")
            .bind(self.scope().company_id())
            .bind(self.state.config.community_id)
            .bind(self.employee.as_str())
            .bind(observed.source.channel_id())
            .bind(draft.destination_channel_id)
            .execute(&mut *tx)
            .await?;
        let actor = OfficePublicKey::parse_hex(&self.principal.public_key.to_hex())
            .map_err(|_| ApiError::unavailable())?;
        let approval = EmployeeSharingApprovalV1::new(
            request.operation_id,
            actor,
            digest(draft.content.as_bytes()),
            draft.expires_at,
        )
        .map_err(|_| ApiError::invalid())?;
        let provenance =
            EmployeeMemoryProvenanceV1::new(observed.audience, observed.source, approval)
                .map_err(|_| ApiError::invalid())?;
        let fact = Uuid::new_v4();
        let audience = provenance.audience();
        let source = provenance.source();
        let human = audience
            .human_public_key()
            .map(|k| hex::decode(k.to_hex()))
            .transpose()
            .map_err(|_| ApiError::unavailable())?;
        sqlx::query("INSERT INTO employee_reviewed_memory_facts
            (company_id,community_id,id,employee_id,kind,human_public_key,destination_channel_id,source_channel_id,
            source_event_id,source_event_created_at,source_author_public_key,source_evidence_hash,audience_bytes,audience_hash,
            source_hash,provenance_bytes,sharing_hash,content,content_hash,approved_by,approval_id,expires_at)
            VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$11,$20,$21)")
            .bind(self.scope().company_id()).bind(self.state.config.community_id).bind(fact).bind(self.employee.as_str())
            .bind(draft.kind.as_str()).bind(human).bind(draft.destination_channel_id).bind(source.channel_id())
            .bind(source.event_id().as_bytes().as_slice()).bind(source.event_created_at()).bind(self.actor().as_slice())
            .bind(source.evidence_hash().as_bytes().as_slice())
            .bind(audience.canonical_bytes().map_err(|_| ApiError::unavailable())?)
            .bind(audience.audience_hash().map_err(|_| ApiError::unavailable())?.as_bytes().as_slice())
            .bind(provenance.source_hash().map_err(|_| ApiError::unavailable())?.as_bytes().as_slice())
            .bind(provenance.canonical_bytes().map_err(|_| ApiError::unavailable())?)
            .bind(provenance.sharing_hash().map_err(|_| ApiError::unavailable())?.as_bytes().as_slice())
            .bind(&draft.content).bind(digest(draft.content.as_bytes()).as_bytes().as_slice())
            .bind(request.operation_id).bind(draft.expires_at).execute(&mut *tx).await?;
        self.record_on(
            &mut tx,
            request.operation_id,
            fact,
            "approve",
            &bytes,
            1,
            deadline,
        )
        .await?;
        let (value, before) = self
            .receipt_on(&mut tx, request.operation_id, fact, "approve", 1, true)
            .await?;
        if let Some(before) = before {
            deadline = deadline.min(before);
        }
        // Deferred SQL re-resolves the exact source and checks the immutable
        // fact/receipt at commit. The prior Office and scope locks remain held.
        self.finish(tx, deadline).await?;
        Ok(value)
    }
}
