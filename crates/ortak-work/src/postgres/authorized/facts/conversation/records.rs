use super::*;
use ortak_control::memory::conversation::ConversationAudienceV1;
use serde::Serialize;
use sqlx::postgres::PgRow;

/// Reviewed conversation fact with its currently visible canonical metadata.
/// Stable fact/status fields preserve Stop using when source authority is lost.
#[derive(Clone, Debug, Serialize)]
pub struct ReviewedConversationFact {
    /// Shared reviewed record; unavailable source clears its content and source.
    pub fact: ReviewedFact,
    /// Canonical v1 audience, withheld together with inaccessible evidence.
    pub audience: Option<serde_json::Value>,
    /// Canonical audience digest, withheld when evidence has changed or disappeared.
    pub audience_hash: Option<String>,
    /// Canonical source metadata and hashes; no source body is ever returned.
    pub provenance: Option<serde_json::Value>,
}

/// Result of one atomic conversation approval or Stop using operation.
#[derive(Clone, Debug, Serialize)]
pub struct ReviewedConversationFactReceipt {
    /// Current authorized metadata, which can reflect a later revocation or lost source.
    pub fact: ReviewedConversationFact,
    /// False for an exact replay; no second mutation or receipt was written.
    pub created: bool,
}

/// Bounded current-project/employee conversation facts, including recovery metadata.
#[derive(Clone, Debug, Serialize)]
pub struct ReviewedConversationFactPage {
    /// At most 16 facts; text/audience/source are withheld together when stale.
    pub facts: Vec<ReviewedConversationFact>,
    /// Exclusive UUID cursor, absent at the end of this finite page.
    pub next_after: Option<Uuid>,
}

const SELECT: &str = "SELECT f.*,a.audience_bytes,a.audience_hash,a.provenance_bytes,a.source_hash,
    a.source_evidence_hash,
    (NOT EXISTS(SELECT 1 FROM reviewed_memory_exports x WHERE x.company_id=f.company_id AND x.fact_id=f.id)
        AND (SELECT count(*) FROM reviewed_memory_targets t WHERE t.company_id=f.company_id
            AND t.project_id=f.project_id AND t.employee_id=f.employee_id
            AND ortak_conversation_export_eligible(f.company_id,f.id,t.id))=1) AS publication_available,
    ortak_reviewed_export_view(f.company_id,f.id) AS reviewed_export,
    CASE WHEN f.revoked_at IS NOT NULL THEN 'revoked'
    WHEN f.expires_at<=clock_timestamp() THEN 'expired' ELSE 'active' END AS status
    FROM reviewed_memory_facts f JOIN reviewed_memory_conversation_audiences a
      ON a.company_id=f.company_id AND a.fact_id=f.id ";

impl AuthorizedWork {
    /// Inspect conversation facts under current Operator and project review authority.
    /// Archived projects or inactive employees keep metadata-only recovery; this
    /// method never publishes, recalls or reclassifies a legacy project fact.
    pub async fn conversation_facts(
        &self,
        project_id: Uuid,
        employee: EmployeeId,
        after: Option<Uuid>,
    ) -> Result<ReviewedConversationFactPage> {
        if project_id.is_nil() || after.is_some_and(|id| id.is_nil()) {
            return Err(WorkError::InvalidQuery(
                "invalid conversation fact cursor or project",
            ));
        }
        bounded(async {
            self.fact_employee_scope(&employee)?;
            let (mut tx, mut deadline) = self.begin().await?;
            let project = self.project_on(&mut tx, project_id).await?;
            self.review(project.role)?;
            let mut query = sqlx::QueryBuilder::new(SELECT);
            query.push(
                "WHERE f.company_id=$1 AND f.project_id=$2 AND f.employee_id=$3
                AND f.audience_kind='conversation' AND ($4::uuid IS NULL OR f.id>$4)
                ORDER BY f.id LIMIT 17 FOR SHARE OF f,a",
            );
            let rows = query
                .build()
                .bind(self.scope.company_id())
                .bind(project_id)
                .bind(employee.as_str())
                .bind(after)
                .fetch_all(&mut *tx)
                .await?;
            // Canonical provenance plus maximally escaped approved text and
            // retained revocation metadata must fit the API's 256 KiB envelope.
            let more = rows.len() > 16;
            let mut facts = Vec::with_capacity(rows.len().min(16));
            for row in rows.iter().take(16) {
                let (fact, current) = self.conversation_row_on(&mut tx, project_id, row).await?;
                deadline = earliest(deadline, current);
                facts.push(fact);
            }
            let next_after = if more {
                facts.last().map(|f| f.fact.id)
            } else {
                None
            };
            self.finish(tx, deadline).await?;
            Ok(ReviewedConversationFactPage { facts, next_after })
        })
        .await
    }

    pub(in crate::postgres::authorized) async fn conversation_fact_on(
        &self,
        connection: &mut PgConnection,
        project_id: Uuid,
        fact_id: Uuid,
    ) -> Result<(ReviewedConversationFact, Option<DateTime<Utc>>)> {
        let mut query = sqlx::QueryBuilder::new(SELECT);
        query.push(
            "WHERE f.company_id=$1 AND f.project_id=$2 AND f.id=$3
            AND f.audience_kind='conversation' FOR SHARE OF f,a",
        );
        let row = query
            .build()
            .bind(self.scope.company_id())
            .bind(project_id)
            .bind(fact_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(WorkError::AccessDenied)?;
        self.conversation_row_on(connection, project_id, &row).await
    }

    async fn conversation_row_on(
        &self,
        connection: &mut PgConnection,
        project_id: Uuid,
        row: &PgRow,
    ) -> Result<(ReviewedConversationFact, Option<DateTime<Utc>>)> {
        let employee = EmployeeId::parse(row.try_get::<String, _>("employee_id")?)?;
        self.fact_employee_scope(&employee)?;
        let audience_bytes: Vec<u8> = row.try_get("audience_bytes")?;
        let provenance_bytes: Vec<u8> = row.try_get("provenance_bytes")?;
        let audience = ConversationAudienceV1::from_canonical_bytes(&audience_bytes)
            .map_err(|_| invalid("invalid retained conversation audience"))?;
        let retained = ConversationProvenanceV1::from_canonical_bytes(&provenance_bytes)
            .map_err(|_| invalid("invalid retained conversation provenance"))?;
        let audience_hash = audience
            .audience_hash()
            .map_err(|_| invalid("invalid retained audience digest"))?;
        let source_hash = retained
            .source_hash()
            .map_err(|_| invalid("invalid retained source digest"))?;
        let stored_audience_hash: Vec<u8> = row.try_get("audience_hash")?;
        let stored_source_hash: Vec<u8> = row.try_get("source_hash")?;
        let evidence_hash: Vec<u8> = row.try_get("source_evidence_hash")?;
        let source: Option<Vec<u8>> = row.try_get("source_message_id")?;
        if retained.audience() != &audience
            || audience.company_id() != self.scope.company_id()
            || audience.community_id() != self.principal.community_id
            || audience.project_id() != project_id
            || audience.employee_id() != &employee
            || audience_hash.as_bytes().as_slice() != stored_audience_hash
            || source_hash.as_bytes().as_slice() != stored_source_hash
            || retained.source_evidence_hash().as_bytes().as_slice() != evidence_hash
            || source.as_deref() != Some(retained.source().event_id().as_bytes().as_slice())
            || row
                .try_get::<Option<Uuid>, _>("source_artifact_id")?
                .is_some()
        {
            return Err(invalid("inconsistent retained conversation identity"));
        }
        let current = self
            .resolve_conversation_fact_on(
                connection,
                project_id,
                &employee,
                retained.source().event_id(),
                audience.kind(),
            )
            .await?;
        let visible = current
            .as_ref()
            .is_some_and(|value| same_source(&retained, value.provenance()));
        let deadline = if visible {
            current.as_ref().and_then(|value| value.valid_before())
        } else {
            None
        };
        let fact = ReviewedFact {
            id: row.try_get("id")?,
            project_id,
            employee_id: employee,
            source: visible.then(|| ReviewedFactSource::Conversation {
                message_id: retained.source().event_id().to_hex(),
            }),
            content: if visible {
                Some(row.try_get("content")?)
            } else {
                None
            },
            source_visible: visible,
            version: row.try_get("version")?,
            status: row.try_get("status")?,
            approved_by: row.try_get("approved_by")?,
            approved_at: row.try_get("approved_at")?,
            expires_at: row.try_get("expires_at")?,
            revoked_by: row.try_get("revoked_by")?,
            revoked_at: row.try_get("revoked_at")?,
            revoke_reason: if visible {
                row.try_get("revoke_reason")?
            } else {
                None
            },
            publication_available: visible && row.try_get::<bool, _>("publication_available")?,
            export: row
                .try_get::<Option<serde_json::Value>, _>("reviewed_export")?
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| invalid("invalid reviewed conversation export view"))?,
        };
        let decode = |bytes: &[u8]| {
            serde_json::from_slice(bytes)
                .map_err(|_| invalid("invalid retained conversation wire value"))
        };
        Ok((
            ReviewedConversationFact {
                fact,
                audience: if visible {
                    Some(decode(&audience_bytes)?)
                } else {
                    None
                },
                audience_hash: visible.then(|| audience_hash.to_hex()),
                provenance: if visible {
                    Some(decode(&provenance_bytes)?)
                } else {
                    None
                },
            },
            deadline,
        ))
    }
}

/// Equality includes source locator/evidence and audience; same thread alone is insufficient.
pub(super) fn same_source(
    retained: &ConversationProvenanceV1,
    current: &ConversationProvenanceV1,
) -> bool {
    retained == current
}
