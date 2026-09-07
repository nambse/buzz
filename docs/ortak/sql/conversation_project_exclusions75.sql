-- Additive conversation75: legacy project publication/recall stay project-only.
-- This source fragment is assembled by root after storage75; it enables no conversation use.

CREATE OR REPLACE FUNCTION ortak_reviewed_export_eligible(company UUID, fact UUID, target UUID) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM reviewed_memory_facts f
        JOIN reviewed_memory_targets t ON t.company_id=f.company_id AND t.project_id=f.project_id AND t.employee_id=f.employee_id
        JOIN companies c ON c.id=f.company_id
        JOIN communities cm ON cm.id=f.community_id
        JOIN office_company_bindings ob ON ob.company_id=f.company_id AND ob.community_id=f.community_id
        JOIN project_api_bindings b ON b.company_id=f.company_id AND b.project_id=f.project_id AND b.community_id=f.community_id
        JOIN projects p ON p.company_id=f.company_id AND p.id=f.project_id
        JOIN employees e ON e.company_id=f.company_id AND e.id=f.employee_id
        JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
        JOIN employee_memory_bindings mb ON mb.company_id=e.company_id AND mb.employee_id=e.id AND mb.revision_id=e.active_revision_id
        JOIN employee_office_bindings eb ON eb.company_id=e.company_id AND eb.employee_id=e.id
        JOIN channel_members m ON m.community_id=f.community_id AND m.channel_id=b.channel_id AND m.pubkey=eb.public_key AND m.removed_at IS NULL
        WHERE f.company_id=company AND f.id=fact AND f.audience_kind='project' AND t.id=target AND f.version=1 AND f.expires_at>clock_timestamp()
          AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL AND p.status='active' AND e.status='active'
          AND t.enabled AND t.valid_until>clock_timestamp() AND t.community_id=f.community_id
          AND t.employee_revision_id=e.active_revision_id AND t.employee_lifecycle_epoch=e.lifecycle_epoch
          AND t.binding=r.manifest->'memory' AND mb.validated_at IS NOT NULL
          AND t.binding=jsonb_build_object('adapter',mb.adapter,'endpoint_ref',mb.endpoint_ref,'workspace',mb.workspace,'user_peer',mb.user_peer,'employee_peer',mb.employee_peer,'options',mb.options)
          AND eb.verified_at IS NOT NULL AND eb.valid_from<=clock_timestamp() AND (eb.valid_until IS NULL OR eb.valid_until>clock_timestamp())
          AND encode(eb.public_key,'hex')=r.manifest#>>'{office,public_key}' AND eb.signer_ref=r.manifest#>>'{office,signer_ref}'
          AND NOT EXISTS(SELECT 1 FROM users u WHERE u.community_id=f.community_id AND u.pubkey=eb.public_key AND u.deactivated_at IS NOT NULL)
          AND ortak_reviewed_fact_source_visible(f.company_id,f.project_id,f.employee_id,f.source_message_id,f.source_artifact_id,f.community_id,b.channel_id))
$$;

CREATE OR REPLACE FUNCTION ortak_reviewed_runtime_eligible(company UUID, fact UUID, target UUID, epoch BIGINT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM reviewed_memory_facts f
        JOIN reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
        JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        JOIN reviewed_memory_export_receipts ack ON ack.company_id=x.company_id AND ack.fact_id=x.fact_id AND ack.action='publish'
        JOIN companies c ON c.id=f.company_id JOIN communities cm ON cm.id=f.community_id
        JOIN office_company_bindings ob ON ob.company_id=f.company_id AND ob.community_id=f.community_id
        JOIN projects p ON p.company_id=f.company_id AND p.id=f.project_id
        JOIN project_api_bindings b ON b.company_id=f.company_id AND b.project_id=f.project_id AND b.community_id=f.community_id
        JOIN employees e ON e.company_id=f.company_id AND e.id=f.employee_id
        JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
        JOIN employee_memory_bindings mb ON mb.company_id=e.company_id AND mb.employee_id=e.id AND mb.revision_id=e.active_revision_id
        JOIN employee_office_bindings eb ON eb.company_id=e.company_id AND eb.employee_id=e.id
        JOIN channel_members m ON m.community_id=f.community_id AND m.channel_id=b.channel_id AND m.pubkey=eb.public_key AND m.removed_at IS NULL
        WHERE f.company_id=company AND f.id=fact AND f.audience_kind='project' AND t.id=target AND t.consumption_epoch=epoch
          AND f.version=1 AND f.revoked_at IS NULL AND f.expires_at>clock_timestamp()
          AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL AND p.status='active' AND e.status='active'
          AND t.enabled AND t.runtime_consumption_enabled AND t.valid_until>clock_timestamp()
          AND t.company_id=f.company_id AND t.community_id=f.community_id AND t.project_id=f.project_id AND t.employee_id=f.employee_id
          AND t.binding=r.manifest->'memory' AND mb.validated_at IS NOT NULL
          AND t.binding=jsonb_build_object('adapter',mb.adapter,'endpoint_ref',mb.endpoint_ref,'workspace',mb.workspace,'user_peer',mb.user_peer,'employee_peer',mb.employee_peer,'options',mb.options)
          AND ack.remote_status='active' AND NOT ack.erased_from_reviewed_store AND ack.binding_hash=t.binding_hash
          AND ack.content_hash=x.content_hash AND x.content_hash=sha256(convert_to(f.content,'UTF8'))
          AND x.source_hash=ortak_reviewed_export_source_hash(f)
          AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts stop WHERE stop.company_id=f.company_id AND stop.fact_id=f.id AND stop.action='withdraw')
          AND eb.verified_at IS NOT NULL AND eb.valid_from<=clock_timestamp() AND (eb.valid_until IS NULL OR eb.valid_until>clock_timestamp())
          AND encode(eb.public_key,'hex')=r.manifest#>>'{office,public_key}' AND eb.signer_ref=r.manifest#>>'{office,signer_ref}'
          AND NOT EXISTS(SELECT 1 FROM users u WHERE u.community_id=f.community_id AND u.pubkey=eb.public_key AND u.deactivated_at IS NOT NULL)
          AND ortak_reviewed_fact_source_visible(f.company_id,f.project_id,f.employee_id,f.source_message_id,f.source_artifact_id,f.community_id,b.channel_id))
$$;
