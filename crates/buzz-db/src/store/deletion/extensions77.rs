//! Retain employee and protected history only after real external settlement.
use super::*;

/// Run before quiescing, at fencing, and again at PostgreSQL purge. The caller
/// owns the schema shared and community exclusive lock (or its durable fence).
/// A lease deadline is never a containment or remote-erasure acknowledgement.
pub(super) async fn require_settled(
    connection: &mut PgConnection,
    community: CommunityId,
) -> Result<()> {
    let (isolation, exports, consumers, protected): (String, bool, bool, bool) = sqlx::query_as(
        r#"
        SELECT current_setting('transaction_isolation'),
          EXISTS(SELECT 1 FROM public.employee_reviewed_memory_exports x
            WHERE x.community_id=$1 AND NOT EXISTS(
              SELECT 1 FROM public.employee_reviewed_memory_export_jobs j
              JOIN public.employee_reviewed_memory_export_receipts a
                ON a.company_id=j.company_id AND a.fact_id=j.fact_id AND a.action=j.action
              JOIN public.employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
              WHERE j.company_id=x.company_id AND j.fact_id=x.fact_id AND j.action='withdraw'
                AND j.community_id=x.community_id AND a.community_id=x.community_id AND t.community_id=x.community_id
                AND j.state='acknowledged'
                AND j.idempotency_key='employee-reviewed:withdraw:'||x.company_id::text||':'||x.fact_id::text
                AND j.request_hash=public.ortak_employee_reviewed_request_hash(x.company_id,x.fact_id,'withdraw')
                AND a.request_hash=j.request_hash AND a.binding_hash=t.binding_hash
                AND (a.content_hash=x.content_hash OR a.content_hash IS NULL)
                AND a.lease_token=j.lease_token AND a.total_attempts=j.total_attempts
                AND a.remote_status='withdrawn' AND a.erased_from_reviewed_store AND a.tombstone_at IS NOT NULL))
          OR EXISTS(SELECT 1 FROM public.employee_reviewed_memory_export_jobs j
            WHERE j.community_id=$1 AND (j.state='pending'
              OR j.state='failed' AND j.lease_token IS NOT NULL)),
          EXISTS(SELECT 1 FROM public.run_employee_reviewed_memory_uses u
            LEFT JOIN public.runs r ON r.company_id=u.company_id AND r.id=u.run_id
            WHERE u.community_id=$1 AND (r.id IS NULL OR r.status NOT IN('completed','failed','cancelled'))),
          EXISTS(SELECT 1 FROM public.encrypted_dm_decrypt_jobs j WHERE j.community_id=$1
            AND (j.state IN('pending','claimed') OR (j.state='verified' AND NOT EXISTS(
              SELECT 1 FROM public.confidential_dm_receipts a WHERE a.company_id=j.company_id
                AND a.source_id=j.source_id AND a.community_id=j.community_id
                AND a.claim_generation=j.claim_generation AND a.claim_token=j.claim_token AND a.claim_worker=j.worker_id))))
          OR EXISTS(SELECT 1 FROM public.confidential_runs c
            LEFT JOIN public.runs r ON r.company_id=c.company_id AND r.id=c.run_id
            LEFT JOIN public.confidential_run_dispatches d ON d.company_id=c.company_id AND d.run_id=c.run_id
            LEFT JOIN public.confidential_execution_leases e ON e.company_id=c.company_id AND e.run_id=c.run_id
            WHERE c.community_id=$1 AND NOT coalesce(
              r.payload_mode='confidential_dm_v1' AND r.status IN('completed','failed','cancelled')
              AND d.community_id=c.community_id AND e.community_id=c.community_id
              AND d.state IN('delivered','failed','cancelled') AND d.lease_token IS NULL AND d.lease_expires_at IS NULL
              AND e.state IN('complete','stopped') AND e.lease_token IS NULL AND e.lease_expires_at IS NULL
              AND (d.state='delivered' OR e.state='stopped')
              AND ((e.state='stopped' AND EXISTS(SELECT 1 FROM public.runtime_cancellations stop
                  WHERE stop.company_id=c.company_id AND stop.run_id=c.run_id AND stop.state='acknowledged'))
                OR (e.state='complete' AND r.status='completed' AND (
                  (r.delivery_intent='silent' AND (SELECT count(*) FROM public.confidential_event_receipts p
                    WHERE p.company_id=c.company_id AND p.run_id=c.run_id)=3)
                  OR (r.delivery_intent='reply' AND (SELECT count(*) FROM public.confidential_event_receipts p
                    WHERE p.company_id=c.company_id AND p.run_id=c.run_id)=4
                    AND EXISTS(SELECT 1 FROM public.confidential_run_payloads p WHERE p.company_id=c.company_id
                      AND p.run_id=c.run_id AND p.purpose='reply_draft' AND p.ordinal=0)
                    AND EXISTS(SELECT 1 FROM public.confidential_reply_bundles b WHERE b.company_id=c.company_id
                      AND b.run_id=c.run_id AND b.community_id=c.community_id))))),false))
          OR EXISTS(SELECT 1 FROM public.confidential_reply_bundles b WHERE b.community_id=$1
            AND (SELECT count(*) FROM public.confidential_reply_outbox o
              WHERE o.company_id=b.company_id AND o.run_id=b.run_id AND o.community_id=b.community_id)<>2)
          OR EXISTS(SELECT 1 FROM public.confidential_reply_outbox o WHERE o.community_id=$1
            AND (o.state='pending' OR o.lease_token IS NOT NULL OR o.lease_expires_at IS NOT NULL
              OR o.finished_at IS NULL OR (o.state='acked') IS DISTINCT FROM (o.acknowledged_at IS NOT NULL)))
        "#,
    )
    .bind(community.as_uuid())
    .fetch_one(connection)
    .await?;
    let refusal = if isolation != "read committed" {
        Some("employee_protected_deletion_requires_read_committed")
    } else if exports {
        Some("employee_reviewed_exports_not_erased")
    } else if consumers {
        Some("employee_reviewed_consumers_not_terminal")
    } else if protected {
        Some("confidential_execution_not_settled")
    } else {
        None
    };
    if let Some(code) = refusal {
        return Err(DbError::DeletionSafety(code.to_owned()));
    }
    Ok(())
}
