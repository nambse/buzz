-- D2c PROPOSAL ONLY; root owns migration numbering, application and desired parity.
ALTER TABLE reviewed_memory_targets
    ADD COLUMN runtime_consumption_enabled BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN consumption_epoch BIGINT NOT NULL DEFAULT 0 CHECK(consumption_epoch>=0);

CREATE OR REPLACE FUNCTION ortak_reviewed_target_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' AND (to_jsonb(NEW)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at'-'runtime_consumption_enabled'-'consumption_epoch')
        IS DISTINCT FROM (to_jsonb(OLD)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at'-'runtime_consumption_enabled'-'consumption_epoch') THEN
        RAISE EXCEPTION 'ortak: reviewed target identity is immutable' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='INSERT' THEN
        IF NEW.consumption_epoch<>0 THEN RAISE EXCEPTION 'ortak: invalid initial consumption epoch' USING ERRCODE='check_violation'; END IF;
    ELSE
        IF NEW.consumption_epoch<>OLD.consumption_epoch THEN RAISE EXCEPTION 'ortak: consumption epoch is server derived' USING ERRCODE='check_violation'; END IF;
        IF OLD.runtime_consumption_enabled AND NOT NEW.runtime_consumption_enabled THEN NEW.consumption_epoch=OLD.consumption_epoch+1; END IF;
    END IF;
    IF NEW.enabled AND (NEW.valid_until<=clock_timestamp() OR NEW.valid_until>clock_timestamp()+INTERVAL '60 seconds') THEN
        RAISE EXCEPTION 'ortak: reviewed target witness must be short and live' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

-- A prior publication revision is evidence, not employee identity. Current exact
-- binding/permissions and an explicit current runtime opt-in are authoritative.
CREATE FUNCTION ortak_reviewed_runtime_eligible(company UUID, fact UUID, target UUID, epoch BIGINT)
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
        WHERE f.company_id=company AND f.id=fact AND t.id=target AND t.consumption_epoch=epoch
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

CREATE TABLE run_reviewed_memory_uses (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 7),
    fact_id UUID NOT NULL,
    target_id UUID NOT NULL,
    fact_version BIGINT NOT NULL CHECK(fact_version=1),
    consumption_epoch BIGINT NOT NULL CHECK(consumption_epoch>=0),
    content_hash BYTEA NOT NULL CHECK(octet_length(content_hash)=32),
    source_hash BYTEA NOT NULL CHECK(octet_length(source_hash)=32),
    binding_hash BYTEA NOT NULL CHECK(octet_length(binding_hash)=32),
    approval_id UUID NOT NULL,
    approved_by TEXT NOT NULL CHECK(approved_by ~ '^[0-9a-f]{64}$'),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id,ordinal),
    UNIQUE(company_id,run_id,fact_id),
    FOREIGN KEY(company_id,run_id) REFERENCES run_context_snapshots(company_id,run_id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(company_id,fact_id) REFERENCES reviewed_memory_exports(company_id,fact_id),
    FOREIGN KEY(company_id,target_id) REFERENCES reviewed_memory_targets(company_id,id)
);
CREATE INDEX idx_run_reviewed_memory_fact ON run_reviewed_memory_uses(company_id,fact_id,run_id);
CREATE INDEX idx_run_reviewed_memory_expiry ON run_reviewed_memory_uses(company_id,expires_at,run_id);
SELECT attach_community_write_fence('run_reviewed_memory_uses');

CREATE FUNCTION ortak_run_reviewed_memory_current(company UUID, run UUID) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT NOT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u
        JOIN runs r ON r.company_id=u.company_id AND r.id=u.run_id
        JOIN work_executions wx ON wx.company_id=r.company_id AND wx.run_id=r.id
        JOIN reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
        JOIN reviewed_memory_targets t ON t.company_id=u.company_id AND t.id=u.target_id
        WHERE u.company_id=company AND u.run_id=run AND (
            f.project_id<>wx.project_id OR f.employee_id<>r.employee_id OR f.community_id<>u.community_id
            OR f.version<>u.fact_version OR f.promotion_operation_id<>u.approval_id OR f.approved_by<>u.approved_by
            OR f.expires_at<>u.expires_at OR sha256(convert_to(f.content,'UTF8'))<>u.content_hash
            OR ortak_reviewed_export_source_hash(f) IS DISTINCT FROM u.source_hash OR t.binding_hash<>u.binding_hash
            OR NOT ortak_reviewed_runtime_eligible(company,u.fact_id,u.target_id,u.consumption_epoch)))
$$;

-- Caller holds Office -> project -> Work before acquiring sorted fact locks;
-- run/outbox locks follow. No provider work occurs under this fence.
CREATE FUNCTION ortak_lock_run_reviewed_memory(company UUID, run UUID) RETURNS BOOLEAN LANGUAGE plpgsql AS $$
BEGIN
    PERFORM f.id FROM reviewed_memory_facts f JOIN run_reviewed_memory_uses u ON u.company_id=f.company_id AND u.fact_id=f.id
        WHERE u.company_id=company AND u.run_id=run ORDER BY f.id FOR SHARE OF f;
    PERFORM t.id FROM reviewed_memory_targets t JOIN run_reviewed_memory_uses u ON u.company_id=t.company_id AND u.target_id=t.id
        WHERE u.company_id=company AND u.run_id=run ORDER BY t.id FOR SHARE OF t;
    RETURN ortak_run_reviewed_memory_current(company,run);
END $$;

CREATE FUNCTION ortak_reviewed_use_immutable() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'ortak: reviewed run uses are retained and immutable' USING ERRCODE='check_violation'; END $$;
CREATE TRIGGER ortak_reviewed_use_immutable BEFORE UPDATE OR DELETE ON run_reviewed_memory_uses
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_use_immutable();
CREATE TRIGGER ortak_reviewed_use_no_truncate BEFORE TRUNCATE ON run_reviewed_memory_uses
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reviewed_use_immutable();

CREATE FUNCTION ortak_reviewed_snapshot_consistent() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; run UUID; wire JSONB; used_count INTEGER; record JSONB; pin JSONB; i INTEGER=0; scratch_count INTEGER; total_bytes INTEGER=0; rendered JSONB; u run_reviewed_memory_uses; f reviewed_memory_facts;
BEGIN
    company=NEW.company_id; run=NEW.run_id;
    SELECT convert_from(s.spec_bytes,'UTF8')::jsonb INTO wire FROM run_context_snapshots s WHERE s.company_id=company AND s.run_id=run;
    SELECT count(*) INTO used_count FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run;
    IF wire IS NULL THEN RAISE EXCEPTION 'ortak: reviewed snapshot missing' USING ERRCODE='check_violation'; END IF;
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
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{recall,records}') LOOP
        rendered=(wire#>>ARRAY['spec','context','memory_context',i::text])::jsonb;
        IF rendered IS DISTINCT FROM jsonb_build_object('type','run_scratch_memory','trust','untrusted_data','record',record) THEN
            RAISE EXCEPTION 'ortak: scratch rendered context differs' USING ERRCODE='check_violation';
        END IF;
        total_bytes=total_bytes+octet_length(record->>'content'); i=i+1;
    END LOOP;
    i=0;
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{reviewed,records}') LOOP
        pin=record->'pin';
        SELECT * INTO u FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run AND x.ordinal=i;
        SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=company AND x.id=u.fact_id;
        IF u.run_id IS NULL OR f.id IS NULL OR record->>'content' IS DISTINCT FROM f.content
            OR NOT EXISTS(SELECT 1 FROM reviewed_memory_targets t WHERE t.company_id=company AND t.id=u.target_id AND t.binding=wire->'memory_binding')
            OR pin IS DISTINCT FROM jsonb_build_object('fact_id',u.fact_id,'target_id',u.target_id,
                'fact_version',u.fact_version,'consumption_epoch',u.consumption_epoch,'content_hash',encode(u.content_hash,'hex'),
                'source_hash',encode(u.source_hash,'hex'),'binding_hash',encode(u.binding_hash,'hex'),
                'approval_id',u.approval_id,'approved_by',u.approved_by,'expires_at',pin->>'expires_at')
            OR (pin->>'expires_at')::timestamptz IS DISTINCT FROM u.expires_at THEN
            RAISE EXCEPTION 'ortak: reviewed snapshot bytes differ from retained uses' USING ERRCODE='check_violation';
        END IF;
        rendered=(wire#>>ARRAY['spec','context','memory_context',(scratch_count+i)::text])::jsonb;
        IF rendered IS DISTINCT FROM jsonb_build_object('type','reviewed_project_memory','trust','untrusted_data','record',record) THEN
            RAISE EXCEPTION 'ortak: reviewed rendered context differs' USING ERRCODE='check_violation';
        END IF;
        total_bytes=total_bytes+octet_length(record->>'content');
        i=i+1;
    END LOOP;
    IF total_bytes>16384 OR NOT ortak_run_reviewed_memory_current(company,run) THEN
        RAISE EXCEPTION 'ortak: reviewed context authority expired before commit' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER ortak_reviewed_snapshot_consistent AFTER INSERT ON run_context_snapshots
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_snapshot_consistent();
CREATE CONSTRAINT TRIGGER ortak_reviewed_use_consistent AFTER INSERT ON run_reviewed_memory_uses
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_snapshot_consistent();

CREATE FUNCTION ortak_reviewed_run_admission() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE run UUID;
BEGIN
    IF TG_TABLE_NAME='runs' THEN
        IF NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token THEN RETURN NEW; END IF;
        run=NEW.id;
    ELSE run=NEW.run_id;
    END IF;
    IF NOT ortak_run_reviewed_memory_current(NEW.company_id,run) THEN
        RAISE EXCEPTION 'ortak: reviewed memory use no longer permitted' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER ortak_reviewed_run_admission AFTER UPDATE ON runs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_run_admission();
CREATE CONSTRAINT TRIGGER ortak_reviewed_artifact_admission AFTER INSERT ON artifacts
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_run_admission();

CREATE OR REPLACE FUNCTION ortak_reviewed_export_view(company UUID,fact UUID) RETURNS JSONB LANGUAGE sql STABLE AS $$
    SELECT jsonb_build_object('fact_id',x.fact_id,'runtime_consumption_enabled',
        ortak_reviewed_runtime_eligible(company,fact,t.id,t.consumption_epoch),
        'publication',jsonb_build_object('state',p.state,'retry_version',p.retry_version,'attempt_count',p.attempt_count,
            'next_attempt_at',p.next_attempt_at,'error_code',p.last_error_code),
        'cleanup',jsonb_build_object('state',w.state,'retry_version',w.retry_version,'attempt_count',w.attempt_count,
            'next_attempt_at',w.next_attempt_at,'error_code',w.last_error_code),
        'erased_from_reviewed_store',coalesce(r.erased_from_reviewed_store,false))
    FROM reviewed_memory_exports x
    JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
    JOIN reviewed_memory_export_jobs p ON p.company_id=x.company_id AND p.fact_id=x.fact_id AND p.action='publish'
    JOIN reviewed_memory_export_jobs w ON w.company_id=x.company_id AND w.fact_id=x.fact_id AND w.action='withdraw'
    LEFT JOIN reviewed_memory_export_receipts r ON r.company_id=x.company_id AND r.fact_id=x.fact_id AND r.action='withdraw'
    WHERE x.company_id=company AND x.fact_id=fact
$$;

-- One company notification wakes authorized streams; no per-run fan-out or
-- provider work occurs in withdrawal/target transactions.
CREATE TRIGGER trg_activity_reviewed_fact_use AFTER UPDATE OF version ON reviewed_memory_facts
    FOR EACH ROW WHEN(NEW.version IS DISTINCT FROM OLD.version) EXECUTE FUNCTION ortak_activity_notify('');
CREATE TRIGGER trg_activity_reviewed_target_use AFTER UPDATE ON reviewed_memory_targets
    FOR EACH ROW WHEN(NEW.consumption_epoch IS DISTINCT FROM OLD.consumption_epoch) EXECUTE FUNCTION ortak_activity_notify('');
