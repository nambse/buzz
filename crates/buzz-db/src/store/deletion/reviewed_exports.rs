//! Pre-fence erasure proof for retained reviewed-memory exports.
use super::*;

/// Call under the community's exclusive deletion lock, or its durable fence,
/// with the schema shared lock held. Export writes take the same community
/// lock shared, so READ COMMITTED sees every earlier claim/ACK after lock grant.
/// Failure must roll back quiescence: cleanup cannot write past that boundary.
pub(super) async fn require_erased(
    connection: &mut PgConnection,
    community: CommunityId,
) -> Result<()> {
    let (isolation, unacknowledged_exports, leased_publications): (String, i64, i64) =
        sqlx::query_as(
            r#"
            SELECT current_setting('transaction_isolation'),
                (SELECT count(*) FROM public.reviewed_memory_exports x
                 WHERE x.community_id=$1 AND NOT EXISTS (
                    SELECT 1 FROM public.reviewed_memory_export_jobs j
                    JOIN public.reviewed_memory_export_receipts r
                      ON r.company_id=j.company_id AND r.fact_id=j.fact_id AND r.action=j.action
                    JOIN public.reviewed_memory_targets t
                      ON t.company_id=x.company_id AND t.id=x.target_id
                    WHERE j.company_id=x.company_id AND j.fact_id=x.fact_id
                      AND j.community_id=x.community_id AND r.community_id=x.community_id
                      AND t.community_id=x.community_id AND j.action='withdraw'
                      AND j.state='acknowledged'
                      AND j.idempotency_key='reviewed:withdraw:'||x.fact_id::text
                      AND r.request_hash=j.request_hash AND r.binding_hash=t.binding_hash
                      AND r.lease_token=j.lease_token AND r.total_attempts=j.total_attempts
                      AND r.erased_from_reviewed_store AND r.tombstone_at IS NOT NULL
                      AND r.remote_status IN ('expired','withdrawn')
                 )),
                (SELECT count(*) FROM public.reviewed_memory_export_jobs j
                 WHERE j.community_id=$1 AND j.action='publish' AND j.state='pending'
                   AND j.lease_token IS NOT NULL)
            "#,
        )
        .bind(community.as_uuid())
        .fetch_one(connection)
        .await?;
    if isolation != "read committed" {
        return Err(DbError::DeletionSafety(
            "reviewed-memory deletion drain requires READ COMMITTED isolation".to_owned(),
        ));
    }
    if unacknowledged_exports != 0 || leased_publications != 0 {
        return Err(DbError::ReviewedMemoryExportsNotDrained {
            community_id: *community.as_uuid(),
            unacknowledged_exports,
            leased_publications,
        });
    }
    Ok(())
}
