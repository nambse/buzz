//! Close workspace execution before the universal community write fence.
use super::*;

/// The caller holds the schema shared lock and community exclusive deletion
/// lock. READ COMMITTED then observes every earlier admission and settlement.
/// Expiry is not containment; terminal runs may retain all input/result bytes.
pub(super) async fn require_settled(
    connection: &mut PgConnection,
    community: CommunityId,
) -> Result<()> {
    let (isolation, readers, actions, runs): (String, bool, bool, bool) = sqlx::query_as(
        r#"
        SELECT current_setting('transaction_isolation'),
            EXISTS(SELECT 1 FROM public.workspace_reader_executions e
                WHERE e.community_id=$1 AND (e.state<>'stopped'
                    OR e.stopped_at IS NULL OR e.stop_proof IS NULL)),
            EXISTS(SELECT 1 FROM public.workspace_tool_actions a
                WHERE a.community_id=$1 AND (a.state NOT IN ('delivered','interrupted')
                    OR a.lease_token IS NOT NULL OR a.lease_expires_at IS NOT NULL)),
            EXISTS(SELECT 1 FROM (
                SELECT company_id,run_id FROM public.run_workspace_uses WHERE community_id=$1
                UNION
                SELECT company_id,run_id FROM public.workspace_reader_executions WHERE community_id=$1
            ) u LEFT JOIN public.runs r ON r.company_id=u.company_id AND r.id=u.run_id
              WHERE r.id IS NULL OR r.status NOT IN ('completed','failed','cancelled'))
        "#,
    )
    .bind(community.as_uuid())
    .fetch_one(connection)
    .await?;
    let refusal = if isolation != "read committed" {
        Some("workspace_deletion_requires_read_committed")
    } else if readers {
        Some("workspace_readers_not_contained")
    } else if actions {
        Some("workspace_actions_not_settled")
    } else if runs {
        Some("workspace_runs_not_terminal")
    } else {
        None
    };
    if let Some(code) = refusal {
        return Err(DbError::DeletionSafety(code.to_owned()));
    }
    Ok(())
}
