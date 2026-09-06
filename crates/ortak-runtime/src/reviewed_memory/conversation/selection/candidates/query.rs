use super::*;

pub(super) async fn rows(
    connection: &mut PgConnection,
    selected: &ReviewedConversationSelection,
    run: Uuid,
    query: &str,
    include_project: bool,
    ids: Option<&[Uuid]>,
) -> RuntimeResult<Vec<sqlx::postgres::PgRow>> {
    sqlx::query(r#"SELECT f.id,f.version,f.promotion_operation_id,f.approved_by,f.expires_at,
        f.content,f.audience_kind,x.target_id,x.content_hash,x.source_hash,t.binding_hash,t.consumption_epoch,
        a.provenance_bytes,a.audience_hash,ca.epoch AS authority_epoch,t.conversation_consumption_epoch
        FROM reviewed_memory_facts f
        JOIN reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
        JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        LEFT JOIN reviewed_memory_conversation_audiences a ON a.company_id=f.company_id AND a.fact_id=f.id
        LEFT JOIN conversation_memory_authorities ca ON ca.company_id=a.company_id
            AND ca.community_id=a.community_id AND ca.project_id=a.project_id AND ca.channel_id=a.channel_id
        WHERE f.company_id=$1 AND f.project_id=$2 AND f.employee_id=$3 AND t.binding=$4
            AND ((f.audience_kind='conversation' AND ortak_conversation_runtime_eligible(
                f.company_id,$5,f.id,t.id,ca.epoch,t.conversation_consumption_epoch))
              OR ($7 AND f.audience_kind='project' AND ortak_reviewed_runtime_eligible(
                f.company_id,f.id,t.id,t.consumption_epoch)))
            AND ($8::uuid[] IS NULL OR f.id=ANY($8))
            AND to_tsvector('simple',f.content) @@ websearch_to_tsquery('simple',$6)
        ORDER BY CASE WHEN f.audience_kind='project' THEN 2 WHEN a.kind='thread' THEN 0 ELSE 1 END,f.id
        LIMIT 32"#)
        .bind(selected.company_id).bind(selected.project_id).bind(selected.employee_id.as_str())
        .bind(serde_json::to_value(&selected.binding).map_err(|_|invalid())?).bind(run).bind(query)
        .bind(include_project).bind(ids).fetch_all(connection).await.map_err(Into::into)
}
