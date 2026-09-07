use super::*;

pub(super) struct Observation {
    pub audience: EmployeeMemoryAudienceV1,
    pub source: EmployeeMemorySourceV1,
    pub observed_at: DateTime<Utc>,
    pub valid_before: Option<DateTime<Utc>>,
}
impl Review<'_> {
    pub(super) async fn preview(&self, request: PreviewRequest) -> Result<Value> {
        let id = request.validate()?;
        if !self.can_review() || !self.channel_allowed(request.destination_channel_id) {
            return Err(forbidden());
        }
        let (mut tx, deadline) = self.begin().await?;
        // Inbox identity owns the partition; callers cannot select a convenient
        // event partition. The observation below checks canonical agreement.
        let at: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT event_created_at FROM office_inbox
            WHERE company_id=$1 AND event_id=$2 AND state='decided'",
        )
        .bind(self.scope().company_id())
        .bind(id.as_bytes().as_slice())
        .fetch_optional(&mut *tx)
        .await?;
        let observed = self
            .observe(&mut tx, &request, at.ok_or_else(forbidden)?)
            .await?
            .ok_or_else(forbidden)?;
        let value = json!({"employee_id":self.employee,
            "audience":serde_json::from_slice::<Value>(&observed.audience.canonical_bytes().map_err(|_| ApiError::unavailable())?)
                .map_err(|_| ApiError::unavailable())?,
            "audience_hash":observed.audience.audience_hash().map_err(|_| ApiError::unavailable())?.to_hex(),
            "source":wire::source(&observed.source)?,
            "source_hash":wire::source_hash(&observed.audience,&observed.source)?.to_hex(),
            "observed_at":observed.observed_at,"valid_before":observed.valid_before,
            "max_expires_at":expiry_limit(observed.observed_at,observed.valid_before)?});
        self.finish(
            tx,
            observed.valid_before.map_or(deadline, |v| v.min(deadline)),
        )
        .await?;
        Ok(value)
    }

    pub(super) async fn observe(
        &self,
        connection: &mut PgConnection,
        request: &PreviewRequest,
        at: DateTime<Utc>,
    ) -> Result<Option<Observation>> {
        let id = request.validate()?;
        if !self.can_review() || !self.channel_allowed(request.destination_channel_id) {
            return Ok(None);
        }
        let actor = self.actor();
        let human = request
            .human_public_key
            .as_deref()
            .map(EmployeeMemoryDigest::parse_hex)
            .transpose()
            .map_err(|_| ApiError::invalid())?;
        if matches!(request.kind, Kind::Relationship)
            && human.as_ref().map(|d| *d.as_bytes()) != Some(actor)
        {
            return Ok(None);
        }
        let rows = sqlx::query(
            "SELECT * FROM ortak_employee_memory_observation($1,$2,$3,$4,$5,$6,$7,$8) LIMIT 2",
        )
        .bind(self.scope().company_id())
        .bind(self.employee.as_str())
        .bind(actor.as_slice())
        .bind(id.as_bytes().as_slice())
        .bind(at)
        .bind(request.destination_channel_id)
        .bind(request.kind.as_str())
        .bind(human.as_ref().map(|h| h.as_bytes().as_slice()))
        .fetch_all(connection)
        .await?;
        let row = match rows.as_slice() {
            [] => return Ok(None),
            [row] => row,
            _ => return Err(ApiError::unavailable()),
        };
        let community: Uuid = row.try_get("community_id")?;
        let channel: Uuid = row.try_get("source_channel_id")?;
        let author: Vec<u8> = row.try_get("source_author_public_key")?;
        let evidence: Vec<u8> = row.try_get("source_evidence_hash")?;
        let revision: Uuid = row.try_get("employee_revision_id")?;
        let lifecycle: i64 = row.try_get("employee_lifecycle_epoch")?;
        let observed_at: DateTime<Utc> = row.try_get("observed_at")?;
        let valid_before: Option<DateTime<Utc>> = row.try_get("valid_before")?;
        if community != self.state.config.community_id
            || author.as_slice() != actor.as_slice()
            || revision.is_nil()
            || lifecycle < 0
            || evidence.len() != 32
        {
            return Err(ApiError::unavailable());
        }
        if !self.channel_allowed(channel) || valid_before.is_some_and(|v| v <= observed_at) {
            return Ok(None);
        }
        let actor_key = OfficePublicKey::parse_hex(&self.principal.public_key.to_hex())
            .map_err(|_| ApiError::unavailable())?;
        let audience = match request.kind {
            Kind::Experience => EmployeeMemoryAudienceV1::experience(
                self.scope().company_id(),
                self.employee.clone(),
                community,
                request.destination_channel_id,
            ),
            Kind::Relationship => EmployeeMemoryAudienceV1::relationship(
                self.scope().company_id(),
                self.employee.clone(),
                community,
                request.destination_channel_id,
                actor_key,
            ),
        }
        .map_err(|_| ApiError::unavailable())?;
        let evidence_bytes: [u8; 32] = evidence.try_into().map_err(|_| ApiError::unavailable())?;
        let source = EmployeeMemorySourceV1::new(
            community,
            channel,
            id,
            at,
            actor_key,
            EmployeeMemoryDigest::from_bytes(evidence_bytes),
        )
        .map_err(|_| ApiError::unavailable())?;
        Ok(Some(Observation {
            audience,
            source,
            observed_at,
            valid_before,
        }))
    }
}
