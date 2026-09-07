use super::*;

#[derive(sqlx::FromRow)]
pub(super) struct Fact {
    pub id: Uuid,
    pub community_id: Uuid,
    pub employee_id: String,
    pub kind: String,
    pub human_public_key: Option<Vec<u8>>,
    pub source_channel_id: Uuid,
    pub destination_channel_id: Uuid,
    pub source_event_id: Vec<u8>,
    pub source_event_created_at: DateTime<Utc>,
    pub source_author_public_key: Vec<u8>,
    pub source_evidence_hash: Vec<u8>,
    pub audience_bytes: Vec<u8>,
    pub audience_hash: Vec<u8>,
    pub source_hash: Vec<u8>,
    pub provenance_bytes: Vec<u8>,
    pub sharing_hash: Vec<u8>,
    pub content: String,
    pub content_hash: Vec<u8>,
    pub approved_by: Vec<u8>,
    pub approval_id: Uuid,
    pub approved_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub version: i32,
    pub revoked_at: Option<DateTime<Utc>>,
}
impl Fact {
    fn provenance(&self, access: &Review<'_>) -> Result<EmployeeMemoryProvenanceV1> {
        let p = EmployeeMemoryProvenanceV1::from_canonical_bytes(&self.provenance_bytes)
            .map_err(|_| ApiError::unavailable())?;
        let audience = p.audience();
        let source = p.source();
        let approval = p.approval();
        if audience.company_id() != access.scope().company_id()
            || audience.employee_id() != &access.employee
            || self.employee_id != access.employee.as_str()
            || self.community_id != access.state.config.community_id
            || audience.destination_community_id() != self.community_id
            || audience.destination_channel_id() != self.destination_channel_id
            || source.community_id() != self.community_id
            || source.channel_id() != self.source_channel_id
            || source.event_id().as_bytes().as_slice() != self.source_event_id
            || source.event_created_at() != self.source_event_created_at
            || source.author_public_key().to_hex() != hex::encode(&self.source_author_public_key)
            || source.evidence_hash().as_bytes().as_slice() != self.source_evidence_hash
            || approval.approval_id() != self.approval_id
            || approval.approved_by().to_hex() != hex::encode(&self.approved_by)
            || self.approved_by.as_slice() != access.actor().as_slice()
            || approval.expires_at() != self.expires_at
            || approval.content_hash().as_bytes().as_slice() != self.content_hash
            || digest(self.content.as_bytes()) != approval.content_hash()
            || audience
                .canonical_bytes()
                .map_err(|_| ApiError::unavailable())?
                != self.audience_bytes
            || audience
                .audience_hash()
                .map_err(|_| ApiError::unavailable())?
                .as_bytes()
                .as_slice()
                != self.audience_hash
            || p.source_hash()
                .map_err(|_| ApiError::unavailable())?
                .as_bytes()
                .as_slice()
                != self.source_hash
            || p.sharing_hash()
                .map_err(|_| ApiError::unavailable())?
                .as_bytes()
                .as_slice()
                != self.sharing_hash
            || audience.human_public_key().map(|k| k.to_hex())
                != self.human_public_key.as_ref().map(hex::encode)
            || match audience.kind() {
                EmployeeMemoryKind::Experience => "experience",
                EmployeeMemoryKind::Relationship => "relationship",
            } != self.kind
        {
            return Err(ApiError::unavailable());
        }
        Ok(p)
    }
}
impl Review<'_> {
    pub(super) async fn fact_on(&self, connection: &mut PgConnection, id: Uuid) -> Result<Fact> {
        sqlx::query_as("SELECT * FROM employee_reviewed_memory_facts
            WHERE company_id=$1 AND community_id=$2 AND employee_id=$3 AND approved_by=$4 AND id=$5")
            .bind(self.scope().company_id()).bind(self.state.config.community_id).bind(self.employee.as_str())
            .bind(self.actor().as_slice()).bind(id).fetch_optional(connection).await?.ok_or_else(forbidden)
    }
    pub(super) async fn project_fact(
        &self,
        connection: &mut PgConnection,
        fact: &Fact,
    ) -> Result<(Value, Option<DateTime<Utc>>)> {
        let provenance = fact.provenance(self)?;
        let request = PreviewRequest {
            source_event_id: provenance.source().event_id().to_hex(),
            destination_channel_id: fact.destination_channel_id,
            kind: match provenance.audience().kind() {
                EmployeeMemoryKind::Experience => Kind::Experience,
                EmployeeMemoryKind::Relationship => Kind::Relationship,
            },
            human_public_key: provenance.audience().human_public_key().map(|k| k.to_hex()),
        };
        let observation = self
            .observe(connection, &request, fact.source_event_created_at)
            .await?;
        let current = observation.as_ref().is_some_and(|o| {
            o.audience == *provenance.audience() && o.source == *provenance.source()
        });
        let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *connection)
            .await?;
        let status = if fact.version == 2 {
            "stopped"
        } else if fact.expires_at <= now {
            "expired"
        } else {
            "approved"
        };
        let mut value = json!({"id":fact.id,"employee_id":self.employee,"kind":fact.kind,"status":status,
            "version":fact.version,"approved_at":fact.approved_at,"expires_at":fact.expires_at,"revoked_at":fact.revoked_at,
            "source_current":current,"can_stop":fact.version==1,
            "content":null,"audience":null,"audience_hash":null,"source":null,"source_hash":null,
            "provenance":null,"sharing_hash":null});
        // The same current predicate gates every sensitive field. Expiry alone
        // does not erase an original approver's otherwise readable history.
        if current {
            value["content"] = json!(fact.content);
            value["audience"] = serde_json::from_slice(&fact.audience_bytes)
                .map_err(|_| ApiError::unavailable())?;
            value["audience_hash"] = json!(hex::encode(&fact.audience_hash));
            value["source"] = json!(wire::source(provenance.source())?);
            value["source_hash"] = json!(hex::encode(&fact.source_hash));
            value["provenance"] = serde_json::from_slice(&fact.provenance_bytes)
                .map_err(|_| ApiError::unavailable())?;
            value["sharing_hash"] = json!(hex::encode(&fact.sharing_hash));
        }
        Ok((
            value,
            if current {
                observation.and_then(|o| o.valid_before)
            } else {
                None
            },
        ))
    }
    pub(super) async fn list(&self, query: Page) -> Result<Value> {
        if let Some(id) = query.after {
            nonnil(id)?;
        }
        let (mut tx, mut deadline) = self.begin().await?;
        // Owner/employee filtering precedes LIMIT, including recovery reads.
        let mut rows: Vec<Fact> = sqlx::query_as(
            "SELECT * FROM employee_reviewed_memory_facts
            WHERE company_id=$1 AND community_id=$2 AND employee_id=$3 AND approved_by=$4
            AND ($5::uuid IS NULL OR id>$5) ORDER BY id LIMIT 17",
        )
        .bind(self.scope().company_id())
        .bind(self.state.config.community_id)
        .bind(self.employee.as_str())
        .bind(self.actor().as_slice())
        .bind(query.after)
        .fetch_all(&mut *tx)
        .await?;
        let more = rows.len() > 16;
        rows.truncate(16);
        let next = if more {
            rows.last().map(|f| f.id)
        } else {
            None
        };
        let mut facts = Vec::with_capacity(rows.len());
        for fact in rows {
            let (view, before) = self.project_fact(&mut tx, &fact).await?;
            if let Some(before) = before {
                deadline = deadline.min(before);
            }
            facts.push(view);
        }
        let can_approve = self.can_review() && self.command_current(&mut tx, "approve").await?;
        self.finish(tx, deadline).await?;
        Ok(json!({"can_approve":can_approve,"facts":facts,"next_after":next}))
    }
}
