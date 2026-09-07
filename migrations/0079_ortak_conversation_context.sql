-- Ordinary near-conversation context remains a bounded, immutable RunSpec field.
-- No ciphertext, credentials, model session or second transcript store is added.
-- Existing snapshots without this field retain their historical behavior.
CREATE FUNCTION ortak_conversation_plaintext79(value TEXT) RETURNS TEXT
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT translate(value,(SELECT string_agg(chr(i),'') FROM generate_series(1,159) i
        WHERE (i<32 AND i NOT IN(9,10,13)) OR i>=127),'')
$$;

CREATE FUNCTION ortak_run_conversation_context_current(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE plpgsql STABLE AS $$
DECLARE ctx JSONB; source JSONB; r runs; origin events; channel UUID; community UUID;
    event_row events; metadata thread_metadata; clean TEXT; total INTEGER=0;
BEGIN
    SELECT ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json)#>'{spec,context,conversation_context}'
      INTO ctx FROM run_context_snapshots s WHERE s.company_id=company AND s.run_id=run;
    IF ctx IS NULL THEN RETURN true; END IF;
    SELECT * INTO r FROM runs WHERE company_id=company AND id=run;
    IF r.id IS NULL OR r.payload_mode<>'ordinary' OR r.work_item_id IS NOT NULL
        OR ctx->>'version'<>'1' OR ctx->>'snapshot_id'<>r.id::text
        OR ctx#>>'{employee,employee_id}'<>r.employee_id
        OR ctx#>>'{employee,revision_id}'<>r.employee_revision_id::text
        OR ctx->>'trigger_message_id'<>encode(r.message_id,'hex')
        OR jsonb_typeof(ctx->'messages') IS DISTINCT FROM 'array'
        OR jsonb_array_length(ctx->'messages')>32 THEN RETURN false; END IF;
    channel=(ctx->>'channel_id')::uuid;
    SELECT community_id INTO community FROM office_company_bindings WHERE company_id=company;
    SELECT ev.* INTO origin FROM office_inbox i JOIN events ev ON ev.community_id=community
        AND ev.id=i.event_id AND ev.created_at=i.event_created_at
        WHERE i.company_id=company AND i.event_id=r.message_id;
    IF origin.id IS NULL OR origin.channel_id IS DISTINCT FROM channel OR origin.kind NOT IN(9,40002)
        OR origin.deleted_at IS NOT NULL OR origin.received_at IS DISTINCT FROM (ctx->>'cutoff_received_at')::timestamptz
        OR NOT EXISTS(SELECT 1 FROM channels c WHERE c.community_id=community AND c.id=channel
            AND c.deleted_at IS NULL AND c.archived_at IS NULL AND c.channel_type IN('stream','dm')
            AND (c.ttl_deadline IS NULL OR c.ttl_deadline>clock_timestamp()))
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
    SELECT * INTO metadata FROM thread_metadata t WHERE t.community_id=community
        AND t.event_id=origin.id AND t.event_created_at=origin.created_at;
    IF (CASE WHEN metadata.parent_event_id IS NULL THEN NULL ELSE encode(metadata.root_event_id,'hex') END)
        IS DISTINCT FROM ctx->>'thread_root_message_id' THEN RETURN false; END IF;
    IF metadata.parent_event_id IS NOT NULL AND (
        NOT EXISTS(SELECT 1 FROM jsonb_array_elements(ctx->'messages') s WHERE s->>'message_id'=encode(metadata.parent_event_id,'hex'))
        OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(ctx->'messages') s WHERE s->>'message_id'=encode(metadata.root_event_id,'hex'))
    ) THEN RETURN false; END IF;
    IF (SELECT count(DISTINCT s->>'message_id') FROM jsonb_array_elements(ctx->'messages') s)
        <>jsonb_array_length(ctx->'messages') THEN RETURN false; END IF;
    FOR source IN SELECT * FROM jsonb_array_elements(ctx->'messages') LOOP
        SELECT ev.* INTO event_row FROM events ev WHERE ev.community_id=community
            AND ev.id=decode(source->>'message_id','hex') AND ev.created_at=(source->>'created_at')::timestamptz;
        IF event_row.id IS NULL OR event_row.id=origin.id OR event_row.channel_id IS DISTINCT FROM channel
            OR event_row.kind NOT IN(9,40002) OR event_row.deleted_at IS NOT NULL
            OR event_row.created_at>origin.created_at OR event_row.received_at>origin.received_at
            OR encode(event_row.pubkey,'hex') IS DISTINCT FROM source->>'author_public_key'
            OR encode(sha256(convert_to(event_row.content,'UTF8')),'hex') IS DISTINCT FROM source->>'source_content_hash'
            THEN RETURN false; END IF;
        clean=ortak_conversation_plaintext79(event_row.content);
        IF nullif(btrim(source->>'content'),'') IS NULL OR octet_length(source->>'content')>8192
            OR left(clean,char_length(source->>'content')) IS DISTINCT FROM source->>'content'
            OR (event_row.content IS DISTINCT FROM source->>'content') IS DISTINCT FROM (source->>'truncated')::boolean
            THEN RETURN false; END IF;
        total=total+octet_length(source->>'content');
        IF total>49152 THEN RETURN false; END IF;
        SELECT * INTO metadata FROM thread_metadata t WHERE t.community_id=community
            AND t.event_id=event_row.id AND t.event_created_at=event_row.created_at;
        IF encode(metadata.parent_event_id,'hex') IS DISTINCT FROM source->>'parent_message_id'
            OR encode(metadata.root_event_id,'hex') IS DISTINCT FROM source->>'thread_root_message_id'
            OR (ctx->>'thread_root_message_id' IS NOT NULL AND source->>'message_id'<>ctx->>'thread_root_message_id'
                AND source->>'thread_root_message_id' IS DISTINCT FROM ctx->>'thread_root_message_id')
            THEN RETURN false; END IF;
    END LOOP;
    RETURN true;
END $$;

CREATE FUNCTION ortak_conversation_snapshot_admission79() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    IF NOT ortak_run_conversation_context_current(NEW.company_id,NEW.run_id) THEN
        RAISE EXCEPTION 'ortak: conversation context no longer permitted' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER ortak_conversation_snapshot_admission79 AFTER INSERT ON run_context_snapshots
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_snapshot_admission79();

CREATE OR REPLACE FUNCTION ortak_run_reviewed_memory_current(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT ortak_run_conversation_context_current(company,run) AND ortak_run_employee_memory_current(company,run) AND NOT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u
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
            AND (ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json)#>'{spec,context}') ? 'conversation_context') INTO conversation;
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
