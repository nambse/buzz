-- Exact prior deliverable selection belongs to the same durable human request.
-- Historical executions/snapshots retain version0 and their original bytes.
ALTER TABLE work_executions
    ADD COLUMN context_version SMALLINT NOT NULL DEFAULT 0 CHECK(context_version IN(0,1)),
    ADD COLUMN reference_artifact_id UUID,
    ADD CONSTRAINT work_execution_reference_artifact_fk FOREIGN KEY(company_id,work_item_id,reference_artifact_id)
        REFERENCES artifacts(company_id,work_item_id,id),
    ADD CONSTRAINT work_execution_reference_shape CHECK(context_version=1 OR reference_artifact_id IS NULL);

CREATE FUNCTION ortak_work_context_request80() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.reference_artifact_id IS NOT NULL AND NOT EXISTS(
        SELECT 1 FROM artifacts a JOIN work_attachments attachment
            ON attachment.company_id=a.company_id AND attachment.work_item_id=a.work_item_id AND attachment.artifact_id=a.id
        WHERE a.company_id=NEW.company_id AND a.id=NEW.reference_artifact_id
          AND a.work_item_id=NEW.work_item_id AND a.project_id=NEW.project_id
          AND a.run_id<>NEW.run_id AND a.created_at<=NEW.requested_at
    ) THEN
        RAISE EXCEPTION 'ortak: Work reference must be an already attached same-item artifact'
            USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER work_context_request80 AFTER INSERT ON work_executions
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_work_context_request80();

CREATE FUNCTION ortak_run_work_context_current(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE plpgsql STABLE AS $$
DECLARE ctx JSONB; wire JSONB; execution work_executions; r runs; origin events;
    source JSONB; event_row events; metadata thread_metadata; artifact artifacts;
    community UUID; channel UUID; linked BYTEA; root BYTEA; parent BYTEA; clean TEXT; total INTEGER=0;
BEGIN
    SELECT * INTO execution FROM work_executions WHERE company_id=company AND run_id=run;
    SELECT ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json) INTO wire
        FROM run_context_snapshots s WHERE s.company_id=company AND s.run_id=run;
    ctx=wire#>'{spec,context,work_context}';
    IF ctx IS NULL THEN
        RETURN wire IS NULL OR coalesce(execution.context_version,0)=0;
    END IF;
    SELECT * INTO r FROM runs WHERE company_id=company AND id=run;
    IF execution.run_id IS NULL OR execution.context_version<>1 OR r.id IS NULL OR r.payload_mode<>'ordinary'
        OR r.work_item_id IS DISTINCT FROM execution.work_item_id
        OR r.employee_id IS DISTINCT FROM execution.employee_id
        OR r.employee_revision_id IS DISTINCT FROM execution.employee_revision_id
        OR ctx->>'version' IS DISTINCT FROM '1' OR ctx->>'snapshot_id' IS DISTINCT FROM r.id::text
        OR ctx->>'work_item_id' IS DISTINCT FROM execution.work_item_id::text
        OR ctx->>'project_id' IS DISTINCT FROM execution.project_id::text
        OR ctx->>'requested_version' IS DISTINCT FROM execution.requested_version::text
        OR ctx->>'execution_version' IS DISTINCT FROM execution.execution_version::text
        OR ctx#>>'{employee,employee_id}' IS DISTINCT FROM r.employee_id
        OR ctx#>>'{employee,revision_id}' IS DISTINCT FROM r.employee_revision_id::text
        OR (ctx->>'cutoff_received_at')::timestamptz IS DISTINCT FROM execution.requested_at
        OR wire#>>'{spec,context,conversation_ref}' IS NOT NULL
        OR wire#>>'{spec,context,reply_to_message_id}' IS NOT NULL
        OR (wire#>'{spec,context}') ? 'conversation_context'
        OR jsonb_typeof(ctx->'messages') IS DISTINCT FROM 'array'
        OR jsonb_array_length(ctx->'messages')>16 THEN RETURN false; END IF;
    SELECT a.community_id,a.channel_id,w.source_message_id INTO community,channel,linked
        FROM project_api_bindings a JOIN work_items w ON w.company_id=a.company_id AND w.project_id=a.project_id
        JOIN projects p ON p.company_id=a.company_id AND p.id=a.project_id
        JOIN office_company_bindings b ON b.company_id=a.company_id AND b.community_id=a.community_id
        WHERE a.company_id=company AND a.project_id=execution.project_id AND w.id=execution.work_item_id AND p.status='active';
    IF community IS NULL OR ctx->>'channel_id' IS DISTINCT FROM channel::text
        OR ctx->>'source_message_id' IS DISTINCT FROM encode(linked,'hex')
        OR NOT EXISTS(SELECT 1 FROM channels c WHERE c.community_id=community AND c.id=channel
            AND c.deleted_at IS NULL AND c.archived_at IS NULL AND c.channel_type IN('stream','dm')
            AND (c.ttl_deadline IS NULL OR c.ttl_deadline>clock_timestamp()))
        OR NOT EXISTS(SELECT 1 FROM project_access_grants g WHERE g.company_id=company AND g.project_id=execution.project_id
            AND g.actor_pubkey=execution.requested_by AND g.role IN('owner','contributor') AND g.revoked_at IS NULL)
        OR NOT EXISTS(SELECT 1 FROM channel_members m WHERE m.community_id=community AND m.channel_id=channel
            AND m.pubkey=decode(execution.requested_by,'hex') AND m.removed_at IS NULL AND m.role<>'bot')
        OR EXISTS(SELECT 1 FROM users u WHERE u.community_id=community AND u.pubkey=decode(execution.requested_by,'hex')
            AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
        OR EXISTS(SELECT 1 FROM employee_office_bindings b WHERE b.company_id=company AND b.public_key=decode(execution.requested_by,'hex'))
        OR NOT EXISTS(SELECT 1 FROM employees e JOIN employee_revisions rev
            ON rev.company_id=e.company_id AND rev.employee_id=e.id AND rev.id=e.active_revision_id
            JOIN employee_office_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id
                AND encode(b.public_key,'hex')=lower(rev.manifest#>>'{office,public_key}')
                AND b.signer_ref=rev.manifest#>>'{office,signer_ref}'
            JOIN channel_members m ON m.community_id=community AND m.channel_id=channel AND m.pubkey=b.public_key
            WHERE e.company_id=company AND e.id=r.employee_id AND e.status='active'
                AND b.verified_at IS NOT NULL AND b.valid_from<=clock_timestamp()
                AND (b.valid_until IS NULL OR b.valid_until>clock_timestamp()) AND m.removed_at IS NULL)
        THEN RETURN false; END IF;
    IF execution.reference_artifact_id IS NULL THEN
        IF ctx->'prior_artifact' IS DISTINCT FROM 'null'::jsonb THEN RETURN false; END IF;
    ELSE
        SELECT * INTO artifact FROM artifacts a WHERE a.company_id=company AND a.id=execution.reference_artifact_id
            AND a.work_item_id=execution.work_item_id AND a.project_id=execution.project_id;
        IF artifact.id IS NULL OR ctx#>>'{prior_artifact,artifact_id}' IS DISTINCT FROM artifact.id::text
            OR ctx#>>'{prior_artifact,run_id}' IS DISTINCT FROM artifact.run_id::text
            OR ctx#>>'{prior_artifact,employee_id}' IS DISTINCT FROM artifact.employee_id
            OR ctx#>>'{prior_artifact,sha256}' IS DISTINCT FROM encode(artifact.content_hash,'hex')
            OR ctx#>>'{prior_artifact,content}' IS DISTINCT FROM convert_from(artifact.content_bytes,'UTF8')
            OR ctx#>>'{prior_artifact,execution_version}' IS DISTINCT FROM
                (SELECT x.execution_version::text FROM work_executions x WHERE x.company_id=company AND x.run_id=artifact.run_id)
            THEN RETURN false; END IF;
    END IF;
    IF linked IS NULL THEN
        RETURN ctx->>'thread_root_message_id' IS NULL AND jsonb_array_length(ctx->'messages')=0
            AND ctx->'omitted_history'='false'::jsonb;
    END IF;
    SELECT ev.* INTO origin FROM office_inbox i JOIN events ev ON ev.community_id=community
        AND ev.id=i.event_id AND ev.created_at=i.event_created_at
        WHERE i.company_id=company AND i.event_id=linked AND i.channel_id=channel AND i.state='decided';
    IF origin.id IS NULL OR origin.channel_id IS DISTINCT FROM channel OR origin.deleted_at IS NOT NULL
        OR origin.kind NOT IN(9,40002) OR origin.received_at>execution.requested_at THEN RETURN false; END IF;
    SELECT * INTO metadata FROM thread_metadata t WHERE t.community_id=community AND t.event_id=origin.id AND t.event_created_at=origin.created_at;
    root=coalesce(metadata.root_event_id,origin.id); parent=metadata.parent_event_id;
    IF ctx->>'thread_root_message_id' IS DISTINCT FROM encode(root,'hex')
        OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(ctx->'messages') s WHERE s->>'message_id'=encode(linked,'hex'))
        OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(ctx->'messages') s WHERE s->>'message_id'=encode(root,'hex'))
        OR (parent IS NOT NULL AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(ctx->'messages') s WHERE s->>'message_id'=encode(parent,'hex')))
        OR (SELECT count(DISTINCT s->>'message_id') FROM jsonb_array_elements(ctx->'messages') s)<>jsonb_array_length(ctx->'messages')
        THEN RETURN false; END IF;
    FOR source IN SELECT * FROM jsonb_array_elements(ctx->'messages') LOOP
        SELECT ev.* INTO event_row FROM events ev WHERE ev.community_id=community
            AND ev.id=decode(source->>'message_id','hex') AND ev.created_at=(source->>'created_at')::timestamptz;
        IF event_row.id IS NULL OR event_row.channel_id IS DISTINCT FROM channel OR event_row.kind NOT IN(9,40002)
            OR event_row.deleted_at IS NOT NULL OR event_row.received_at>execution.requested_at
            OR source->>'author_public_key' IS DISTINCT FROM encode(event_row.pubkey,'hex')
            OR source->>'source_content_hash' IS DISTINCT FROM encode(sha256(convert_to(event_row.content,'UTF8')),'hex')
            THEN RETURN false; END IF;
        clean=ortak_conversation_plaintext79(event_row.content);
        IF nullif(btrim(source->>'content'),'') IS NULL OR octet_length(source->>'content')>2048
            OR left(clean,char_length(source->>'content')) IS DISTINCT FROM source->>'content'
            OR (event_row.content IS DISTINCT FROM source->>'content') IS DISTINCT FROM (source->>'truncated')::boolean
            THEN RETURN false; END IF;
        total=total+octet_length(source->>'content');
        IF total>16384 THEN RETURN false; END IF;
        SELECT * INTO metadata FROM thread_metadata t WHERE t.community_id=community
            AND t.event_id=event_row.id AND t.event_created_at=event_row.created_at;
        IF source->>'parent_message_id' IS DISTINCT FROM encode(metadata.parent_event_id,'hex')
            OR source->>'thread_root_message_id' IS DISTINCT FROM encode(metadata.root_event_id,'hex')
            OR (event_row.id<>root AND metadata.root_event_id IS DISTINCT FROM root)
            OR source->>'selection' IS DISTINCT FROM (CASE WHEN event_row.id=root THEN 'thread_root' ELSE 'thread_recent' END)
            THEN RETURN false; END IF;
    END LOOP;
    RETURN true;
END $$;

CREATE FUNCTION ortak_work_snapshot_admission80() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    IF NOT ortak_run_work_context_current(NEW.company_id,NEW.run_id) THEN
        RAISE EXCEPTION 'ortak: Work reference context no longer permitted' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER work_snapshot_admission80 AFTER INSERT ON run_context_snapshots
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_work_snapshot_admission80();

CREATE OR REPLACE FUNCTION ortak_run_reviewed_memory_current(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT ortak_run_work_context_current(company,run) AND ortak_run_conversation_context_current(company,run) AND ortak_run_employee_memory_current(company,run) AND NOT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u
        LEFT JOIN runs r ON r.company_id=u.company_id AND r.id=u.run_id
        LEFT JOIN work_executions wx ON wx.company_id=r.company_id AND wx.run_id=r.id
        LEFT JOIN reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
        LEFT JOIN reviewed_memory_targets t ON t.company_id=u.company_id AND t.id=u.target_id
        LEFT JOIN run_context_snapshots snapshot ON snapshot.company_id=u.company_id AND snapshot.run_id=u.run_id
        WHERE u.company_id=company AND u.run_id=run AND (
            r.id IS NULL OR f.id IS NULL OR t.id IS NULL OR snapshot.run_id IS NULL
            OR f.employee_id IS DISTINCT FROM r.employee_id OR f.community_id IS DISTINCT FROM u.community_id
            OR f.version IS DISTINCT FROM u.fact_version OR f.promotion_operation_id IS DISTINCT FROM u.approval_id
            OR f.approved_by IS DISTINCT FROM u.approved_by OR f.expires_at IS DISTINCT FROM u.expires_at
            OR sha256(convert_to(f.content,'UTF8')) IS DISTINCT FROM u.content_hash
            OR ortak_reviewed_export_source_hash(f) IS DISTINCT FROM u.source_hash OR t.binding_hash IS DISTINCT FROM u.binding_hash
            OR CASE WHEN f.audience_kind='project' THEN
                wx.run_id IS NULL OR f.project_id IS DISTINCT FROM wx.project_id
                OR NOT ortak_reviewed_runtime_eligible(company,u.fact_id,u.target_id,u.consumption_epoch)
              WHEN f.audience_kind='conversation' THEN
                u.consumption_epoch<>0 OR u.conversation_audience_hash IS DISTINCT FROM
                    (SELECT a.audience_hash FROM reviewed_memory_conversation_audiences a WHERE a.company_id=company AND a.fact_id=u.fact_id)
                OR NOT coalesce(ortak_conversation_runtime_eligible(company,run,u.fact_id,u.target_id,
                    u.conversation_authority_epoch,u.conversation_consumption_epoch),false)
                OR NOT EXISTS(SELECT 1 FROM ortak_conversation_run_origin(company,run,f.project_id) origin
                    WHERE (CASE WHEN ortak_snapshot_scratch_jsonb(convert_from(snapshot.spec_bytes,'UTF8')::json)->'version'='5'::jsonb
                        THEN ortak_snapshot_scratch_jsonb(convert_from(snapshot.spec_bytes,'UTF8')::json)#>'{employee,conversation_origin}'
                        ELSE ortak_snapshot_scratch_jsonb(convert_from(snapshot.spec_bytes,'UTF8')::json)#>'{conversation,origin}' END)
                        =ortak_snapshot_scratch_jsonb(jsonb_build_object('requester_public_key',encode(origin.requester_public_key,'hex'),
                            'provenance',convert_from(origin.provenance_bytes,'UTF8'))::json))
              ELSE true END))
$$;

CREATE OR REPLACE FUNCTION ortak_reviewed_run_admission() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE selected_run UUID; conversation BOOLEAN;
BEGIN
    IF TG_TABLE_NAME='runs' THEN selected_run=NEW.id; ELSE selected_run=NEW.run_id; END IF;
    SELECT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
        AND u.run_id=selected_run AND u.conversation_audience_hash IS NOT NULL) OR EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u
        WHERE u.company_id=NEW.company_id AND u.run_id=selected_run) OR EXISTS(SELECT 1 FROM run_context_snapshots s WHERE s.company_id=NEW.company_id AND s.run_id=selected_run
            AND ((ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json)#>'{spec,context}') ? 'conversation_context' OR (ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json)#>'{spec,context}') ? 'work_context')) INTO conversation;
    IF TG_TABLE_NAME='runs' THEN
        IF NOT conversation THEN
            -- Preserve the reviewed-project admission trigger's legacy effect.
            IF NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token THEN RETURN NEW; END IF;
        ELSE
            IF (NEW.office_admission_token,NEW.office_admission_generation,NEW.office_admission_valid_before,
                NEW.work_admission_token,NEW.work_admission_generation,NEW.runtime_run_ref)
              IS NOT DISTINCT FROM
               (OLD.office_admission_token,OLD.office_admission_generation,OLD.office_admission_valid_before,
                OLD.work_admission_token,OLD.work_admission_generation,OLD.runtime_run_ref) THEN RETURN NEW; END IF;
            -- Exact74 lost-start ACK correlation is accounting after confirmed
            -- stop; no new token, output, bytes or active status can ride along.
            IF OLD.runtime_run_ref IS NULL AND NEW.runtime_run_ref IS NOT NULL
                AND (to_jsonb(NEW)-'runtime_run_ref'-'updated_at') IS NOT DISTINCT FROM (to_jsonb(OLD)-'runtime_run_ref'-'updated_at')
                AND EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.id
                    AND (c.state='acknowledged' OR c.state='pending' AND c.lease_token IS NOT NULL AND c.lease_expires_at>clock_timestamp()))
                AND NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader
                    WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.id AND reader.state<>'stopped') THEN RETURN NEW; END IF;
            IF NEW.status NOT IN('queued','running','waiting') THEN
                RAISE EXCEPTION 'ortak: terminal conversation run cannot gain fresh admission' USING ERRCODE='check_violation';
            END IF;
        END IF;
    END IF;
    IF NOT ortak_run_reviewed_memory_current(NEW.company_id,selected_run) THEN
        RAISE EXCEPTION 'ortak: reviewed memory use no longer permitted' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
