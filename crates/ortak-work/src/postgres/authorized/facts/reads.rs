use super::*;

// Source filtering is part of the recall query before LIMIT. Artifact source
// visibility follows its Work item's canonical conversation, when present.
pub(super) const SOURCE_PREDICATE: &str = "ortak_reviewed_fact_source_visible(
    f.company_id,f.project_id,f.employee_id,f.source_message_id,f.source_artifact_id,$3,$4)";

fn projection() -> sqlx::QueryBuilder<sqlx::Postgres> {
    let mut q = sqlx::QueryBuilder::new(
        "SELECT f.*,CASE WHEN f.revoked_at IS NOT NULL THEN 'revoked'
        WHEN f.expires_at<=clock_timestamp() THEN 'expired' ELSE 'active' END AS status,",
    );
    q.push(SOURCE_PREDICATE)
        .push(" AS source_visible,ortak_reviewed_export_view(f.company_id,f.id) AS export,
            (NOT EXISTS(SELECT 1 FROM reviewed_memory_exports x WHERE x.company_id=f.company_id AND x.fact_id=f.id)
            AND (SELECT count(*) FROM reviewed_memory_targets t WHERE t.company_id=f.company_id AND t.project_id=f.project_id
                AND t.employee_id=f.employee_id AND ortak_reviewed_export_eligible(f.company_id,f.id,t.id))=1) AS publication_available
            FROM reviewed_memory_facts f ");
    q
}

fn fact(row: &sqlx::postgres::PgRow) -> Result<ReviewedFact> {
    let visible: bool = row.try_get("source_visible")?;
    let source = if visible {
        match (
            row.try_get::<Option<Vec<u8>>, _>("source_message_id")?,
            row.try_get::<Option<Uuid>, _>("source_artifact_id")?,
        ) {
            (Some(message), None) => Some(ReviewedFactSource::Conversation {
                message_id: hex::encode(message),
            }),
            (None, Some(artifact)) => Some(ReviewedFactSource::Artifact {
                artifact_id: artifact,
            }),
            _ => return Err(invalid("reviewed fact source is unreadable")),
        }
    } else {
        None
    };
    Ok(ReviewedFact {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        employee_id: EmployeeId::parse(row.try_get::<String, _>("employee_id")?)?,
        source,
        source_visible: visible,
        content: if visible {
            Some(row.try_get("content")?)
        } else {
            None
        },
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
        publication_available: row.try_get("publication_available")?,
        export: row
            .try_get::<Option<serde_json::Value>, _>("export")?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| invalid("reviewed export projection rejected"))?,
    })
}

impl AuthorizedWork {
    pub(in crate::postgres::authorized) async fn fact_on(
        &self,
        c: &mut PgConnection,
        project: Uuid,
        id: Uuid,
        channel: Uuid,
    ) -> Result<ReviewedFact> {
        let mut q = projection();
        q.push("WHERE f.company_id=$1 AND f.project_id=$2 AND f.id=$5 AND f.audience_kind='project' FOR SHARE OF f");
        let row = q
            .build()
            .bind(self.scope.company_id())
            .bind(project)
            .bind(self.principal.community_id)
            .bind(channel)
            .bind(id)
            .fetch_optional(c)
            .await?
            .ok_or(WorkError::AccessDenied)?;
        let result = fact(&row)?;
        self.fact_employee_scope(&result.employee_id)?;
        Ok(result)
    }

    /// Inspect a finite current-project page, retaining stop-use recovery metadata.
    pub async fn reviewed_facts(
        &self,
        project: Uuid,
        employee: EmployeeId,
        after: Option<Uuid>,
    ) -> Result<ReviewedFactPage> {
        bounded(async {
            self.fact_employee_scope(&employee)?;
            let (mut tx, deadline) = self.begin().await?;
            let project = self.project_on(&mut tx, project).await?;
            let mut q = projection();
            q.push(
                "WHERE f.company_id=$1 AND f.project_id=$2 AND f.employee_id=$5 AND f.audience_kind='project'
                AND ($6::uuid IS NULL OR f.id>$6) ORDER BY f.id LIMIT 26 FOR SHARE OF f",
            );
            let rows = q
                .build()
                .bind(self.scope.company_id())
                .bind(project.record.project.id)
                .bind(self.principal.community_id)
                .bind(project.channel_id)
                .bind(employee.as_str())
                .bind(after)
                .fetch_all(&mut *tx)
                .await?;
            let more = rows.len() > 25;
            let facts: Vec<_> = rows.iter().take(25).map(fact).collect::<Result<_>>()?;
            let next_after = if more {
                facts.last().map(|fact| fact.id)
            } else {
                None
            };
            self.finish(tx, deadline).await?;
            Ok(ReviewedFactPage { facts, next_after })
        })
        .await
    }

    /// Preview only active reviewed context within an exact current project/employee.
    /// This deterministic full-text query does not call Honcho or another provider.
    pub async fn recall_reviewed_facts(
        &self,
        project: Uuid,
        employee: EmployeeId,
        query: String,
    ) -> Result<ReviewedFactRecall> {
        if query.trim().is_empty() || query.len() > 1024 || query.chars().any(char::is_control) {
            return Err(WorkError::InvalidQuery("reviewed recall query is invalid"));
        }
        bounded(async {
            self.fact_employee_scope(&employee)?;
            let (mut tx, deadline) = self.begin().await?;
            let project = self.project_on(&mut tx, project).await?;
            if project.record.project.status != ProjectStatus::Active {
                return Err(WorkError::ProjectArchived {
                    project_id: project.record.project.id,
                });
            }
            self.employee_on(&mut tx, project.channel_id, &employee)
                .await?;
            let mut q = projection();
            q.push(
                "WHERE f.company_id=$1 AND f.project_id=$2 AND f.employee_id=$5 AND f.audience_kind='project'
                AND f.revoked_at IS NULL AND f.expires_at>clock_timestamp() AND ",
            )
            .push(SOURCE_PREDICATE)
            .push(
                " AND to_tsvector('simple',f.content) @@ plainto_tsquery('simple',$6)
                    ORDER BY f.approved_at DESC,f.id DESC LIMIT 9 FOR SHARE OF f",
            );
            let rows = q
                .build()
                .bind(self.scope.company_id())
                .bind(project.record.project.id)
                .bind(self.principal.community_id)
                .bind(project.channel_id)
                .bind(employee.as_str())
                .bind(query)
                .fetch_all(&mut *tx)
                .await?;
            let mut facts = Vec::new();
            let mut bytes = 0;
            let mut truncated = false;
            for row in rows {
                let value = fact(&row)?;
                let size = value
                    .content
                    .as_ref()
                    .ok_or_else(|| invalid("recall source visibility changed"))?
                    .len();
                if facts.len() == 8 || bytes + size > 8192 {
                    truncated = true;
                    break;
                }
                bytes += size;
                facts.push(value);
            }
            self.finish(tx, deadline).await?;
            Ok(ReviewedFactRecall { facts, truncated })
        })
        .await
    }
}
