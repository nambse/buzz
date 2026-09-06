-- D4 runtime SOURCE FRAGMENT; root assembles additive76 after immutable75.
-- Current SQL observations are not dispatch capabilities. Callers retain the
-- ordinary Office/Work admission gates and acquire the documented locks before
-- final freeze. No remote I/O or cleanup is performed by these functions.

CREATE FUNCTION ortak_conversation_run_origin(company UUID, run UUID, project UUID)
RETURNS TABLE(requester_public_key BYTEA, provenance_bytes BYTEA,
    observed_at TIMESTAMPTZ, valid_before TIMESTAMPTZ)
LANGUAGE sql STABLE AS $$
    WITH base AS MATERIALIZED (
        SELECT r.*, b.community_id,b.channel_id AS project_channel_id,active.manifest AS active_manifest,
            pinned.manifest AS pinned_manifest
        FROM runs r
        JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id
            AND e.status='active' AND e.lifecycle_epoch=r.employee_lifecycle_epoch
        JOIN employee_revisions pinned ON pinned.company_id=r.company_id
            AND pinned.employee_id=r.employee_id AND pinned.id=r.employee_revision_id
        JOIN employee_revisions active ON active.company_id=e.company_id
            AND active.employee_id=e.id AND active.id=e.active_revision_id
        JOIN project_api_bindings b ON b.company_id=r.company_id AND b.project_id=project
        JOIN office_routing_cohorts cohort ON cohort.company_id=r.company_id
            AND cohort.community_id=b.community_id AND cohort.state='enabled'
        JOIN office_routing_channels ch ON ch.company_id=cohort.company_id
            AND ch.community_id=cohort.community_id AND ch.channel_id=b.channel_id
        JOIN office_routing_employees selected ON selected.company_id=r.company_id
            AND selected.employee_id=r.employee_id
        WHERE r.company_id=company AND r.id=run
            AND pinned.manifest->'office'=active.manifest->'office'
            AND pinned.manifest->'memory'=active.manifest->'memory'
            AND NOT EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=r.company_id AND c.run_id=r.id)
            AND NOT EXISTS(SELECT 1 FROM run_cancel_requests c WHERE c.company_id=r.company_id AND c.run_id=r.id)
    ), origins AS (
        SELECT i.author_pubkey AS human,r.message_id AS source,r.employee_id
        FROM base r
        JOIN routing_decisions d ON d.company_id=r.company_id AND d.id=r.routing_decision_id
            AND d.message_id=r.message_id AND d.root_message_id=r.root_message_id
            AND d.origin_type='human' AND d.office_authority_generation IS NOT NULL
            AND d.office_input_hash IS NOT NULL
        JOIN routing_recipients recipient ON recipient.company_id=r.company_id
            AND recipient.routing_decision_id=d.id AND recipient.employee_id=r.employee_id
            AND recipient.action='wake' AND recipient.employee_revision_id=r.employee_revision_id
            AND recipient.employee_lifecycle_epoch=r.employee_lifecycle_epoch
        JOIN delivery_chain_visits visit ON visit.company_id=r.company_id
            AND visit.root_message_id=d.root_message_id AND visit.employee_id=r.employee_id
            AND visit.routing_decision_id=d.id
        JOIN office_inbox i ON i.company_id=r.company_id AND i.event_id=r.message_id
            AND i.channel_id=r.project_channel_id AND i.state='decided'
            AND d.origin_id=encode(i.author_pubkey,'hex')
        WHERE r.work_item_id IS NULL AND r.status IN('queued','running','waiting','completed')
          -- The runtime configured exactly one project for this employee's
          -- channel. Multiple live advertisements must not make a caller's
          -- arbitrary project parameter a choice of conversation namespace.
          AND (SELECT count(DISTINCT t.project_id) FROM reviewed_memory_targets t
            WHERE t.company_id=r.company_id AND t.community_id=r.community_id
              AND t.employee_id=r.employee_id AND t.conversation_channel_id=r.project_channel_id
              AND t.enabled AND t.conversation_consumption_enabled
              AND t.valid_until>clock_timestamp())=1
          AND EXISTS(SELECT 1 FROM reviewed_memory_targets t WHERE t.company_id=r.company_id
            AND t.project_id=project AND t.employee_id=r.employee_id AND t.conversation_channel_id=r.project_channel_id
            AND t.enabled AND t.conversation_consumption_enabled AND t.valid_until>clock_timestamp())
        UNION ALL
        SELECT decode(x.requested_by,'hex'), w.source_message_id, r.employee_id
        FROM base r JOIN work_executions x ON x.company_id=r.company_id AND x.run_id=r.id
            AND x.project_id=project AND x.work_item_id=r.work_item_id
            AND x.employee_id=r.employee_id AND x.employee_revision_id=r.employee_revision_id
        JOIN work_items w ON w.company_id=x.company_id AND w.project_id=x.project_id AND w.id=x.work_item_id
        JOIN work_authority_generations g ON g.company_id=x.company_id AND g.project_id=x.project_id
        JOIN project_access_grants acl ON acl.company_id=x.company_id AND acl.project_id=x.project_id
            AND acl.actor_pubkey=x.requested_by AND acl.role IN('owner','contributor') AND acl.revoked_at IS NULL
        WHERE w.source_message_id IS NOT NULL AND r.routing_decision_id IS NULL
            AND r.message_id IS NULL AND r.root_message_id IS NULL
            AND EXISTS(SELECT 1 FROM work_assignments a WHERE a.company_id=x.company_id
                AND a.work_item_id=x.work_item_id AND a.employee_id=x.employee_id
                AND a.status='active' AND a.role IN('owner','contributor'))
            AND ((w.state='in_progress' AND w.version=x.execution_version
                AND x.reconciled_at IS NULL AND r.status IN('queued','running','waiting','completed')
                AND (r.work_admission_generation=g.generation OR r.status='queued' AND r.work_admission_generation IS NULL)
                AND NOT EXISTS(SELECT 1 FROM work_dependencies d JOIN work_items dependency
                    ON dependency.company_id=d.company_id AND dependency.id=d.depends_on_work_item_id
                    WHERE d.company_id=x.company_id AND d.work_item_id=x.work_item_id AND d.released_at IS NULL
                        AND dependency.state NOT IN('completed','cancelled'))
                AND NOT EXISTS(SELECT 1 FROM work_acceptance_criteria c WHERE c.company_id=x.company_id
                    AND c.work_item_id=x.work_item_id AND c.status<>'pending')
                AND NOT EXISTS(SELECT 1 FROM work_approvals a WHERE a.company_id=x.company_id
                    AND a.work_item_id=x.work_item_id AND a.status<>'pending'))
              -- A materialized result remains inspectable after human review.
              -- This branch cannot create a new run or first artifact: both
              -- exact retained artifact and materialized output must exist.
              OR (r.status='completed' AND w.state IN('review','completed') AND x.result_code='result_ready'
                AND x.reconciled_at IS NOT NULL AND EXISTS(SELECT 1 FROM runtime_work_outputs output
                    JOIN artifacts artifact ON artifact.company_id=output.company_id AND artifact.id=output.artifact_id
                        AND artifact.run_id=output.run_id AND artifact.project_id=x.project_id AND artifact.work_item_id=x.work_item_id
                    WHERE output.company_id=r.company_id AND output.run_id=r.id AND output.state='materialized')))
    ), unique_origin AS (
        SELECT * FROM origins WHERE (SELECT count(*) FROM origins)=1
    )
    SELECT o.human, s.provenance_bytes, s.observed_at, s.valid_before
    FROM unique_origin o CROSS JOIN LATERAL
        ortak_conversation_source_observation(company,project,o.employee_id,o.human,o.source,'thread') s
    WHERE s.valid_before IS NULL OR s.valid_before>clock_timestamp()
$$;

-- Source hash extension is explicit and only for new conversation facts. Keep
-- the legacy message/artifact preimages byte-for-byte unchanged.
CREATE OR REPLACE FUNCTION ortak_reviewed_export_source_hash(f reviewed_memory_facts)
RETURNS BYTEA LANGUAGE sql STABLE AS $$
    SELECT CASE WHEN f.audience_kind='conversation' THEN
        (SELECT a.source_hash FROM reviewed_memory_conversation_audiences a
            WHERE a.company_id=f.company_id AND a.fact_id=f.id)
    WHEN f.source_message_id IS NOT NULL
        THEN sha256(convert_to('message:'||encode(f.source_message_id,'hex'),'UTF8'))
    ELSE (SELECT sha256(convert_to('artifact:'||a.id::text||':'||encode(a.content_hash,'hex'),'UTF8'))
        FROM artifacts a WHERE a.company_id=f.company_id AND a.id=f.source_artifact_id) END
$$;

-- A still-live owned target can survive a model-only revision at runtime.
-- New publication additionally pins the exact current employee revision.
CREATE FUNCTION ortak_conversation_target_eligible76(company UUID, fact UUID, target UUID, publication BOOLEAN)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM reviewed_memory_facts f
        JOIN reviewed_memory_conversation_audiences a ON a.company_id=f.company_id AND a.fact_id=f.id
        JOIN reviewed_memory_targets t ON t.company_id=f.company_id AND t.id=target
        JOIN employees e ON e.company_id=f.company_id AND e.id=f.employee_id AND e.status='active'
        JOIN employee_revisions revision ON revision.company_id=e.company_id
            AND revision.employee_id=e.id AND revision.id=e.active_revision_id
        JOIN employee_memory_bindings memory ON memory.company_id=e.company_id
            AND memory.employee_id=e.id AND memory.revision_id=e.active_revision_id
        CROSS JOIN LATERAL ortak_conversation_source_observation(f.company_id,f.project_id,f.employee_id,
            decode(f.approved_by,'hex'),f.source_message_id,a.kind) source
        WHERE publication IS NOT NULL AND f.company_id=company AND f.id=fact AND f.audience_kind='conversation'
            AND f.version=1 AND f.revoked_at IS NULL AND f.expires_at>clock_timestamp()
            AND a.community_id=f.community_id AND a.project_id=f.project_id AND a.employee_id=f.employee_id
            AND source.provenance_bytes=a.provenance_bytes AND source.source_hash=a.source_hash
            AND source.audience_hash=a.audience_hash
            AND (source.valid_before IS NULL OR source.valid_before>clock_timestamp())
            AND t.enabled AND t.valid_until>clock_timestamp()
            AND t.community_id=f.community_id AND t.project_id=f.project_id AND t.employee_id=f.employee_id
            AND t.conversation_channel_id=a.channel_id
            AND (NOT publication OR t.employee_revision_id=e.active_revision_id)
            AND t.employee_lifecycle_epoch=e.lifecycle_epoch AND memory.validated_at IS NOT NULL
            AND t.binding=revision.manifest->'memory'
            AND t.binding=jsonb_build_object('adapter',memory.adapter,'endpoint_ref',memory.endpoint_ref,
                'workspace',memory.workspace,'user_peer',memory.user_peer,'employee_peer',memory.employee_peer,'options',memory.options)
            AND t.creation_receipt->'binding'=t.binding
            AND t.creation_receipt->>'company_id'=company::text
            AND t.creation_receipt->>'employee_id'=f.employee_id
            AND t.creation_receipt->>'deployment_id'=t.deployment_id::text)
$$;

CREATE FUNCTION ortak_conversation_export_eligible(company UUID, fact UUID, target UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT ortak_conversation_target_eligible76(company,fact,target,true)
$$;

CREATE FUNCTION ortak_conversation_runtime_eligible(company UUID, run UUID, fact UUID, target UUID,
    authority_epoch BIGINT, consumption_epoch BIGINT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM reviewed_memory_facts f
        JOIN reviewed_memory_conversation_audiences a ON a.company_id=f.company_id AND a.fact_id=f.id
        JOIN conversation_memory_authorities authority ON authority.company_id=a.company_id
            AND authority.community_id=a.community_id AND authority.project_id=a.project_id AND authority.channel_id=a.channel_id
        JOIN reviewed_memory_exports export ON export.company_id=f.company_id AND export.fact_id=f.id
        JOIN reviewed_memory_targets t ON t.company_id=export.company_id AND t.id=export.target_id
        JOIN reviewed_memory_export_receipts ack ON ack.company_id=f.company_id AND ack.fact_id=f.id AND ack.action='publish'
        JOIN runs r ON r.company_id=f.company_id AND r.id=run AND r.employee_id=f.employee_id
        CROSS JOIN LATERAL ortak_conversation_run_origin(company,run,f.project_id) origin
        CROSS JOIN LATERAL (SELECT convert_from(origin.provenance_bytes,'UTF8')::jsonb AS value) op
        WHERE f.company_id=company AND f.id=fact AND t.id=target
          AND ortak_conversation_target_eligible76(company,fact,target,false)
          AND authority.epoch=$5 AND t.conversation_consumption_epoch=$6
          AND t.conversation_consumption_enabled AND t.conversation_channel_id=a.channel_id
          AND op.value#>>'{audience,company_id}'=company::text
          AND op.value#>>'{audience,community_id}'=a.community_id::text
          AND op.value#>>'{audience,project_id}'=a.project_id::text
          AND op.value#>>'{audience,employee_id}'=a.employee_id
          AND op.value#>>'{audience,channel_id}'=a.channel_id::text
          AND (a.kind='channel' OR
            (op.value#>>'{audience,thread_root_event_id}'=encode(a.thread_root_event_id,'hex')
              AND (op.value#>>'{audience,thread_root_event_created_at}')::timestamptz=a.thread_root_event_created_at))
          AND export.community_id=f.community_id AND export.project_id=f.project_id AND export.employee_id=f.employee_id
          AND export.content_hash=sha256(convert_to(f.content,'UTF8')) AND export.source_hash=a.source_hash
          AND ack.remote_status='active' AND NOT ack.erased_from_reviewed_store
          AND ack.binding_hash=t.binding_hash AND ack.content_hash=export.content_hash
          AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts stop
            WHERE stop.company_id=f.company_id AND stop.fact_id=f.id AND stop.action='withdraw'))
$$;

-- A single missing use join is refusal, never an empty invalid-use set. This
-- closes the old INNER JOIN work_executions hole for Office uses and corruption.
CREATE OR REPLACE FUNCTION ortak_run_reviewed_memory_current(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT NOT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u
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
                    WHERE (ortak_snapshot_scratch_jsonb(convert_from(snapshot.spec_bytes,'UTF8')::json)#>'{conversation,origin}')
                        =ortak_snapshot_scratch_jsonb(jsonb_build_object('requester_public_key',encode(origin.requester_public_key,'hex'),
                            'provenance',convert_from(origin.provenance_bytes,'UTF8'))::json))
              ELSE true END))
$$;

-- Office -> project -> optional Work -> scoped authority -> facts -> targets;
-- callers take run/outbox afterwards. NOWAIT prevents a late-order caller from
-- waiting behind a mutation which holds its tuple before requesting Office.
CREATE OR REPLACE FUNCTION ortak_lock_run_reviewed_memory(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE plpgsql AS $$
BEGIN
    PERFORM ortak_lock_office_authority(company);
    PERFORM p.id FROM projects p WHERE p.company_id=company AND p.id IN
        (SELECT f.project_id FROM reviewed_memory_facts f JOIN run_reviewed_memory_uses u
            ON u.company_id=f.company_id AND u.fact_id=f.id WHERE u.company_id=company AND u.run_id=run)
        ORDER BY p.id FOR SHARE OF p NOWAIT;
    PERFORM w.id FROM work_items w JOIN work_executions x ON x.company_id=w.company_id AND x.work_item_id=w.id
        WHERE x.company_id=company AND x.run_id=run ORDER BY w.id FOR SHARE OF w NOWAIT;
    PERFORM a.channel_id FROM conversation_memory_authorities a WHERE a.company_id=company
        AND EXISTS(SELECT 1 FROM run_reviewed_memory_uses u JOIN reviewed_memory_conversation_audiences f
            ON f.company_id=u.company_id AND f.fact_id=u.fact_id WHERE u.company_id=company AND u.run_id=run
                AND f.project_id=a.project_id AND f.channel_id=a.channel_id)
        ORDER BY a.company_id,a.project_id,a.channel_id FOR SHARE OF a NOWAIT;
    PERFORM f.id FROM reviewed_memory_facts f JOIN run_reviewed_memory_uses u ON u.company_id=f.company_id AND u.fact_id=f.id
        WHERE u.company_id=company AND u.run_id=run ORDER BY f.id FOR SHARE OF f NOWAIT;
    PERFORM t.id FROM reviewed_memory_targets t WHERE t.company_id=company AND EXISTS
        (SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=company AND u.run_id=run AND u.target_id=t.id)
        ORDER BY t.id FOR SHARE OF t NOWAIT;
    RETURN ortak_run_reviewed_memory_current(company,run);
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_run_admission() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE selected_run UUID; conversation BOOLEAN;
BEGIN
    IF TG_TABLE_NAME='runs' THEN selected_run=NEW.id; ELSE selected_run=NEW.run_id; END IF;
    SELECT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
        AND u.run_id=selected_run AND u.conversation_audience_hash IS NOT NULL) INTO conversation;
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

-- Effect boundaries, not remote acknowledgement accounting. In particular a
-- delivered Office ACK may create a pending scratch-write job after revocation;
-- only a fresh memory admission can give that retained job permission to write.
-- Outbox lease claims/clears only account for an attempt. They must remain
-- drainable after revocation; every actual publish reauthorizes frozen bytes.
CREATE FUNCTION ortak_conversation_effect_admission76() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE effect BOOLEAN=false; previous JSONB; proposed JSONB;
BEGIN
    IF NOT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
        AND u.run_id=NEW.run_id AND u.conversation_audience_hash IS NOT NULL) THEN RETURN NEW; END IF;
    previous=CASE WHEN TG_OP='UPDATE' THEN to_jsonb(OLD) END; proposed=to_jsonb(NEW);
    CASE TG_TABLE_NAME
    WHEN 'runtime_work_outputs' THEN effect=NEW.state='materialized';
    WHEN 'runtime_office_outputs' THEN effect=NEW.state='enqueued' OR
        (NEW.office_authority_token IS NOT NULL AND (TG_OP='INSERT'
            OR (proposed->'office_authority_token',proposed->'office_authority_generation',proposed->'office_authority_valid_before')
              IS DISTINCT FROM (previous->'office_authority_token',previous->'office_authority_generation',previous->'office_authority_valid_before')));
    WHEN 'runtime_memory_writes' THEN effect=NEW.state='pending' AND NEW.admission_token IS NOT NULL
        AND (TG_OP='INSERT' OR (proposed->'admission_token',proposed->'admission_generation',proposed->'admission_valid_before')
            IS DISTINCT FROM (previous->'admission_token',previous->'admission_generation',previous->'admission_valid_before'));
    WHEN 'outbox' THEN effect=NEW.kind='office_publish' AND NEW.state='pending'
        AND (TG_OP='INSERT' OR (proposed->'signed_event_id',proposed->'signed_event_bytes')
            IS DISTINCT FROM (previous->'signed_event_id',previous->'signed_event_bytes'));
    ELSE RAISE EXCEPTION 'ortak: unknown conversation effect' USING ERRCODE='check_violation';
    END CASE;
    IF effect AND NOT ortak_run_reviewed_memory_current(NEW.company_id,NEW.run_id) THEN
        RAISE EXCEPTION 'ortak: conversation output authority changed' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER conversation_work_output_at_commit AFTER INSERT OR UPDATE ON runtime_work_outputs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();
CREATE CONSTRAINT TRIGGER conversation_office_output_at_commit AFTER INSERT OR UPDATE ON runtime_office_outputs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();
CREATE CONSTRAINT TRIGGER conversation_memory_write_at_commit AFTER INSERT OR UPDATE ON runtime_memory_writes
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();
CREATE CONSTRAINT TRIGGER conversation_delivery_at_commit AFTER INSERT OR UPDATE ON outbox
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();

CREATE OR REPLACE FUNCTION ortak_reviewed_export_view(company UUID,fact UUID) RETURNS JSONB LANGUAGE sql STABLE AS $$
    SELECT jsonb_build_object('fact_id',x.fact_id,'runtime_consumption_enabled',
        CASE WHEN f.audience_kind='conversation' THEN
            t.conversation_consumption_enabled AND ortak_conversation_target_eligible76(company,fact,t.id,false)
            AND EXISTS(SELECT 1 FROM reviewed_memory_export_receipts ack WHERE ack.company_id=x.company_id
                AND ack.fact_id=x.fact_id AND ack.action='publish' AND ack.remote_status='active'
                AND NOT ack.erased_from_reviewed_store AND ack.binding_hash=t.binding_hash
                AND ack.content_hash=x.content_hash AND x.content_hash=sha256(convert_to(f.content,'UTF8'))
                AND x.source_hash=ortak_reviewed_export_source_hash(f))
            AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts stop WHERE stop.company_id=x.company_id
                AND stop.fact_id=x.fact_id AND stop.action='withdraw')
        ELSE ortak_reviewed_runtime_eligible(company,fact,t.id,t.consumption_epoch) END,
        'publication',jsonb_build_object('state',p.state,'retry_version',p.retry_version,'attempt_count',p.attempt_count,
            'next_attempt_at',p.next_attempt_at,'error_code',p.last_error_code),
        'cleanup',jsonb_build_object('state',w.state,'retry_version',w.retry_version,'attempt_count',w.attempt_count,
            'next_attempt_at',w.next_attempt_at,'error_code',w.last_error_code),
        'erased_from_reviewed_store',coalesce(r.erased_from_reviewed_store,false))
    FROM reviewed_memory_exports x
    JOIN reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
    JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
    JOIN reviewed_memory_export_jobs p ON p.company_id=x.company_id AND p.fact_id=x.fact_id AND p.action='publish'
    JOIN reviewed_memory_export_jobs w ON w.company_id=x.company_id AND w.fact_id=x.fact_id AND w.action='withdraw'
    LEFT JOIN reviewed_memory_export_receipts r ON r.company_id=x.company_id AND r.fact_id=x.fact_id AND r.action='withdraw'
    WHERE x.company_id=company AND x.fact_id=fact
$$;

CREATE FUNCTION ortak_conversation_snapshot76(company UUID, run UUID, wire JSONB)
RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE
    r runs; revision employee_revisions; work work_executions;
    selected_project UUID; origin RECORD; context JSONB; record JSONB; pin JSONB;
    wrapped JSONB; rendered JSONB; expected_pin JSONB; expected_record JSONB;
    u run_reviewed_memory_uses; f reviewed_memory_facts; a reviewed_memory_conversation_audiences;
    used_count INTEGER; scratch_count INTEGER; i INTEGER=0; conversations INTEGER=0;
    reviewed_bytes INTEGER=0; total_bytes INTEGER=0; content TEXT; seen UUID[]=ARRAY[]::uuid[];
BEGIN
    SELECT * INTO r FROM runs x WHERE x.company_id=company AND x.id=run;
    SELECT * INTO revision FROM employee_revisions x WHERE x.company_id=company
        AND x.employee_id=r.employee_id AND x.id=r.employee_revision_id;
    context=wire->'conversation';
    IF r.id IS NULL OR revision.id IS NULL OR r.status NOT IN('queued','running','waiting')
        OR wire->'version' IS DISTINCT FROM '4'::jsonb
        OR wire ? 'reviewed' OR jsonb_typeof(context) IS DISTINCT FROM 'object'
        OR (context-'origin'-'records'-'truncated')<>'{}'::jsonb
        OR jsonb_typeof(context->'truncated') IS DISTINCT FROM 'boolean'
        OR jsonb_typeof(context->'records') IS DISTINCT FROM 'array'
        OR jsonb_typeof(wire#>'{recall,records}') IS DISTINCT FROM 'array'
        OR jsonb_typeof(wire#>'{spec,context,memory_context}') IS DISTINCT FROM 'array'
        OR wire->>'company_id' IS DISTINCT FROM company::text
        OR wire#>>'{spec,run_id}' IS DISTINCT FROM run::text
        OR wire#>>'{spec,employee_id}' IS DISTINCT FROM r.employee_id
        OR wire#>>'{spec,revision_id}' IS DISTINCT FROM r.employee_revision_id::text
        OR wire#>>'{spec,idempotency_key}' IS DISTINCT FROM 'ortak-run:'||company::text||':'||run::text
        OR wire#>'{spec,binding}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'runtime')::json)
        OR wire#>'{spec,permissions}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'permissions')::json)
        OR wire->'memory_binding' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'memory')::json) THEN
        RAISE EXCEPTION 'ortak: conversation snapshot shape or run identity differs' USING ERRCODE='check_violation';
    END IF;
    SELECT count(*) INTO used_count FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run;
    scratch_count=jsonb_array_length(wire#>'{recall,records}');
    IF used_count NOT BETWEEN 1 AND 8 OR jsonb_array_length(context->'records')<>used_count
        OR scratch_count+used_count>8
        OR jsonb_array_length(wire#>'{spec,context,memory_context}')<>scratch_count+used_count THEN
        RAISE EXCEPTION 'ortak: conversation snapshot count differs' USING ERRCODE='check_violation';
    END IF;
    -- Select the project from immutable use/fact rows, never from the caller's
    -- JSON provenance. Every reviewed record below must have this same project.
    SELECT min(fact.project_id::text)::uuid INTO selected_project
        FROM run_reviewed_memory_uses used JOIN reviewed_memory_facts fact
            ON fact.company_id=used.company_id AND fact.id=used.fact_id
        WHERE used.company_id=company AND used.run_id=run
        HAVING count(DISTINCT fact.project_id)=1;
    SELECT * INTO origin FROM ortak_conversation_run_origin(company,run,selected_project);
    IF NOT FOUND OR context->'origin' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(
        jsonb_build_object('requester_public_key',encode(origin.requester_public_key,'hex'),
            'provenance',convert_from(origin.provenance_bytes,'UTF8'))::json) THEN
        RAISE EXCEPTION 'ortak: conversation snapshot origin differs' USING ERRCODE='check_violation';
    END IF;
    IF r.work_item_id IS NULL THEN
        IF wire ? 'work_origin' OR wire->>'message_id' IS DISTINCT FROM encode(r.message_id,'hex')
            OR wire->>'root_message_id' IS DISTINCT FROM encode(r.root_message_id,'hex')
            OR wire->>'routing_decision_id' IS DISTINCT FROM r.routing_decision_id::text
            OR wire->'input_truncated' IS DISTINCT FROM 'false'::jsonb
            OR wire#>>'{spec,context,reply_to_message_id}' IS DISTINCT FROM encode(r.message_id,'hex')
            OR wire#>'{spec,context,work_item_id}' IS DISTINCT FROM 'null'::jsonb
            OR NOT EXISTS(SELECT 1 FROM office_inbox inbox
                JOIN office_company_bindings office ON office.company_id=inbox.company_id
                JOIN events event ON event.community_id=office.community_id AND event.id=inbox.event_id
                    AND event.created_at=inbox.event_created_at AND event.kind=inbox.event_kind
                    AND event.channel_id=inbox.channel_id AND event.pubkey=inbox.author_pubkey
                CROSS JOIN LATERAL (SELECT regexp_replace(event.content,
                    U&'[\0001-\0008\000B\000C\000E-\001F\007F-\009F]','','g') AS cleaned) input
                WHERE inbox.company_id=company AND inbox.event_id=r.message_id
                AND wire->'event_kind'=to_jsonb(inbox.event_kind)
                AND wire#>>'{spec,context,conversation_ref}'=inbox.channel_id::text
                -- Source75 already caps the original text at65536 bytes;
                -- control removal cannot require UTF-8 truncation afterwards.
                AND event.deleted_at IS NULL AND octet_length(event.content)<=65536
                AND btrim(input.cleaned,U&'\0009\000A\000B\000C\000D\0020\0085\00A0\1680\2000\2001\2002\2003\2004\2005\2006\2007\2008\2009\200A\2028\2029\202F\205F\3000')<>''
                AND wire#>'{spec,input}'=ortak_snapshot_scratch_jsonb(to_json(input.cleaned))) THEN
            RAISE EXCEPTION 'ortak: conversation Office origin differs' USING ERRCODE='check_violation';
        END IF;
    ELSE
        SELECT * INTO work FROM work_executions x WHERE x.company_id=company AND x.run_id=run;
        IF work.run_id IS NULL OR work.project_id<>selected_project
            OR wire ? 'message_id' OR wire ? 'root_message_id' OR wire ? 'routing_decision_id'
            OR wire->'event_kind' IS DISTINCT FROM '0'::jsonb
            OR wire->'input_truncated' IS DISTINCT FROM 'false'::jsonb
            OR wire->'work_origin' IS DISTINCT FROM jsonb_build_object('run_id',work.run_id,
                'work_item_id',work.work_item_id,'project_id',work.project_id,'execution_version',work.execution_version,
                'definition_hash',encode(work.definition_hash,'hex'))
            OR wire#>'{spec,input}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(to_json(convert_from(work.definition_bytes,'UTF8')))
            OR wire#>>'{spec,context,work_item_id}' IS DISTINCT FROM r.work_item_id::text
            OR wire#>'{spec,context,reply_to_message_id}' IS DISTINCT FROM 'null'::jsonb
            OR wire#>'{spec,context,conversation_ref}' IS DISTINCT FROM 'null'::jsonb THEN
            RAISE EXCEPTION 'ortak: conversation Work origin differs' USING ERRCODE='check_violation';
        END IF;
    END IF;
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{recall,records}') LOOP
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',i::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type','run_scratch_memory','trust','untrusted_data','record',record)
            OR octet_length(wire#>>ARRAY['spec','context','memory_context',i::text])>8192
            OR jsonb_typeof(record->'content') IS DISTINCT FROM 'string' THEN
            RAISE EXCEPTION 'ortak: conversation scratch rendering differs' USING ERRCODE='check_violation';
        END IF;
        content=record->>'content';
        total_bytes=total_bytes+octet_length(content)
            -(octet_length(content)-octet_length(regexp_replace(content,E'\x01[\x01\x02]','','g')))/2;
        i=i+1;
    END LOOP;
    i=0;
    FOR wrapped IN SELECT value FROM jsonb_array_elements(context->'records') LOOP
        record=wrapped->'record'; pin=record->'pin';
        SELECT * INTO u FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run AND x.ordinal=i;
        SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=company AND x.id=u.fact_id;
        IF u.run_id IS NULL OR f.id IS NULL OR f.project_id<>selected_project OR u.fact_id=ANY(seen)
            OR NOT EXISTS(SELECT 1 FROM reviewed_memory_targets target WHERE target.company_id=company
                AND target.id=u.target_id AND ortak_snapshot_scratch_jsonb(target.binding::json)=wire->'memory_binding') THEN
            RAISE EXCEPTION 'ortak: conversation retained record identity differs' USING ERRCODE='check_violation';
        END IF;
        seen=array_append(seen,u.fact_id);
        expected_pin=jsonb_build_object('fact_id',u.fact_id,'target_id',u.target_id,'fact_version',u.fact_version,
            'consumption_epoch',u.consumption_epoch,'content_hash',encode(u.content_hash,'hex'),
            'source_hash',encode(u.source_hash,'hex'),'binding_hash',encode(u.binding_hash,'hex'),
            'approval_id',u.approval_id,'approved_by',u.approved_by,'expires_at',pin->>'expires_at');
        IF wrapped->>'scope'='conversation' AND f.audience_kind='conversation' THEN
            SELECT * INTO a FROM reviewed_memory_conversation_audiences x WHERE x.company_id=company AND x.fact_id=f.id;
            IF NOT FOUND OR u.consumption_epoch<>0 OR u.conversation_audience_hash IS DISTINCT FROM a.audience_hash THEN
                RAISE EXCEPTION 'ortak: conversation audience pin differs' USING ERRCODE='check_violation';
            END IF;
            expected_pin=expected_pin||jsonb_build_object('conversation_audience_hash',encode(u.conversation_audience_hash,'hex'),
                'conversation_authority_epoch',u.conversation_authority_epoch,
                'conversation_consumption_epoch',u.conversation_consumption_epoch);
            expected_record=jsonb_build_object('pin',expected_pin,'content',f.content,'provenance',convert_from(a.provenance_bytes,'UTF8'));
            conversations=conversations+1;
        ELSIF wrapped->>'scope'='project' AND f.audience_kind='project' AND r.work_item_id IS NOT NULL THEN
            expected_record=jsonb_build_object('pin',expected_pin,'content',f.content);
        ELSE RAISE EXCEPTION 'ortak: conversation record scope differs' USING ERRCODE='check_violation';
        END IF;
        IF record IS DISTINCT FROM ortak_snapshot_scratch_jsonb(expected_record::json)
            OR wrapped IS DISTINCT FROM jsonb_build_object('scope',wrapped->>'scope','record',record)
            OR (pin->>'expires_at')::timestamptz IS DISTINCT FROM u.expires_at THEN
            RAISE EXCEPTION 'ortak: conversation record bytes differ from retained use' USING ERRCODE='check_violation';
        END IF;
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',(scratch_count+i)::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type',CASE WHEN wrapped->>'scope'='project'
                THEN 'reviewed_project_memory' ELSE 'reviewed_conversation_memory' END,'trust','untrusted_data','record',record)
            OR octet_length(wire#>>ARRAY['spec','context','memory_context',(scratch_count+i)::text])>8192 THEN
            RAISE EXCEPTION 'ortak: conversation rendered bytes differ' USING ERRCODE='check_violation';
        END IF;
        reviewed_bytes=reviewed_bytes+octet_length(f.content); i=i+1;
    END LOOP;
    IF conversations=0 OR reviewed_bytes>8192 OR total_bytes+reviewed_bytes>16384
        OR NOT ortak_run_reviewed_memory_current(company,run) THEN
        RAISE EXCEPTION 'ortak: conversation budget or current authority differs' USING ERRCODE='check_violation';
    END IF;
END $$;

-- Exact72 comparison/byte accounting remains the legacy branch. Only explicit
-- version4 is delegated; a conversation field cannot masquerade as version1–3.
CREATE OR REPLACE FUNCTION ortak_reviewed_snapshot_consistent() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; run UUID; wire JSONB; used_count INTEGER; record JSONB; pin JSONB; i INTEGER=0; scratch_count INTEGER; total_bytes INTEGER=0; rendered JSONB; u run_reviewed_memory_uses; f reviewed_memory_facts;
BEGIN
    company=NEW.company_id; run=NEW.run_id;
    -- Even PostgreSQL json field access may unescape unrelated NUL values.
    -- Encode the whole comparison document before performing any field access.
    SELECT ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json) INTO wire FROM run_context_snapshots s WHERE s.company_id=company AND s.run_id=run;
    SELECT count(*) INTO used_count FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run;
    IF wire IS NULL THEN RAISE EXCEPTION 'ortak: reviewed snapshot missing' USING ERRCODE='check_violation'; END IF;
    IF wire->'version'='4'::jsonb THEN
        PERFORM ortak_conversation_snapshot76(company,run,wire);
        RETURN NEW;
    END IF;
    IF wire ? 'conversation' THEN
        RAISE EXCEPTION 'ortak: legacy snapshot cannot carry conversation context' USING ERRCODE='check_violation';
    END IF;
    IF wire->'version' IS DISTINCT FROM '3'::jsonb THEN
        IF used_count<>0 OR wire ? 'reviewed' THEN RAISE EXCEPTION 'ortak: legacy snapshot cannot contain reviewed context' USING ERRCODE='check_violation'; END IF;
        RETURN NEW;
    END IF;
    IF jsonb_typeof(wire#>'{reviewed,records}') IS DISTINCT FROM 'array'
        OR jsonb_array_length(wire#>'{reviewed,records}')<>used_count OR used_count>8
        OR NOT EXISTS(SELECT 1 FROM work_executions wx JOIN runs r ON r.company_id=wx.company_id AND r.id=wx.run_id
            WHERE wx.company_id=company AND wx.run_id=run AND wire#>>'{work_origin,project_id}'=wx.project_id::text
              AND wire#>>'{spec,employee_id}'=r.employee_id) THEN
        RAISE EXCEPTION 'ortak: reviewed snapshot scope or count differs' USING ERRCODE='check_violation';
    END IF;
    IF jsonb_typeof(wire#>'{recall,records}') IS DISTINCT FROM 'array'
        OR jsonb_typeof(wire#>'{spec,context,memory_context}') IS DISTINCT FROM 'array' THEN
        RAISE EXCEPTION 'ortak: reviewed snapshot context arrays missing' USING ERRCODE='check_violation';
    END IF;
    scratch_count=jsonb_array_length(wire#>'{recall,records}');
    IF scratch_count+used_count>8 OR jsonb_array_length(wire#>'{spec,context,memory_context}')<>scratch_count+used_count THEN
        RAISE EXCEPTION 'ortak: reviewed snapshot total record budget differs' USING ERRCODE='check_violation';
    END IF;
    -- Outer records are already encoded once. Serialized memory_context strings
    -- still contain original inner JSON escapes and need their own one encoding.
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{recall,records}') LOOP
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',i::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type','run_scratch_memory','trust','untrusted_data','record',record) THEN
            RAISE EXCEPTION 'ortak: scratch rendered context differs' USING ERRCODE='check_violation';
        END IF;
        -- Each encoded SOH pair represents exactly one original UTF-8 byte.
        -- Count bytes from the original content, not the comparison encoding.
        total_bytes=total_bytes+octet_length(record->>'content')
            -(octet_length(record->>'content')-octet_length(regexp_replace(record->>'content',E'\x01[\x01\x02]','','g')))/2;
        i=i+1;
    END LOOP;
    i=0;
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{reviewed,records}') LOOP
        pin=record->'pin';
        SELECT * INTO u FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run AND x.ordinal=i;
        SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=company AND x.id=u.fact_id;
        IF u.run_id IS NULL OR f.id IS NULL OR record->'content' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(to_json(f.content))
            OR NOT EXISTS(SELECT 1 FROM reviewed_memory_targets t WHERE t.company_id=company AND t.id=u.target_id AND ortak_snapshot_scratch_jsonb(t.binding::json)=wire->'memory_binding')
            OR pin IS DISTINCT FROM ortak_snapshot_scratch_jsonb(jsonb_build_object('fact_id',u.fact_id,'target_id',u.target_id,
                'fact_version',u.fact_version,'consumption_epoch',u.consumption_epoch,'content_hash',encode(u.content_hash,'hex'),
                'source_hash',encode(u.source_hash,'hex'),'binding_hash',encode(u.binding_hash,'hex'),
                'approval_id',u.approval_id,'approved_by',u.approved_by,'expires_at',pin->>'expires_at')::json)
            OR (pin->>'expires_at')::timestamptz IS DISTINCT FROM u.expires_at THEN
            RAISE EXCEPTION 'ortak: reviewed snapshot bytes differ from retained uses' USING ERRCODE='check_violation';
        END IF;
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',(scratch_count+i)::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type','reviewed_project_memory','trust','untrusted_data','record',record) THEN
            RAISE EXCEPTION 'ortak: reviewed rendered context differs' USING ERRCODE='check_violation';
        END IF;
        total_bytes=total_bytes+octet_length(f.content);
        i=i+1;
    END LOOP;
    IF total_bytes>16384 OR NOT ortak_run_reviewed_memory_current(company,run) THEN
        RAISE EXCEPTION 'ortak: reviewed context authority expired before commit' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

-- Only this internal atomic publication guard opts into the distinct predicate.
-- Existing public project callers still use the unchanged project-only helper.
CREATE OR REPLACE FUNCTION ortak_reviewed_export_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM reviewed_memory_facts f JOIN reviewed_memory_targets t ON t.company_id=f.company_id AND t.id=NEW.target_id
        WHERE f.company_id=NEW.company_id AND f.id=NEW.fact_id AND f.project_id=NEW.project_id AND f.employee_id=NEW.employee_id
        AND f.community_id=NEW.community_id AND NEW.content_hash=sha256(convert_to(f.content,'UTF8'))
        AND NEW.source_hash=ortak_reviewed_export_source_hash(f) AND t.employee_revision_id=NEW.employee_revision_id
        AND t.employee_lifecycle_epoch=NEW.employee_lifecycle_epoch AND (CASE WHEN f.audience_kind='conversation' THEN ortak_conversation_export_eligible(f.company_id,f.id,t.id) ELSE ortak_reviewed_export_eligible(f.company_id,f.id,t.id) END))
      OR NOT EXISTS(SELECT 1 FROM reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id AND o.fact_id=NEW.fact_id
        AND o.actor_pubkey=NEW.requested_by AND o.operation_id=NEW.operation_id AND o.action='publish' AND o.result_version=0
        AND o.xmin::text::bigint=txid_current()%4294967296)
      OR (SELECT count(*) FROM reviewed_memory_export_jobs j WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id
        AND j.state='pending' AND j.attempt_count=0 AND j.xmin::text::bigint=txid_current()%4294967296)<>2 THEN
        RAISE EXCEPTION 'ortak: reviewed export requires current fact, atomic instruction and two jobs' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
