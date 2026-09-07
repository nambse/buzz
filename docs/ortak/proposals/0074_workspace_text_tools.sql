-- PROPOSAL 74: selected Work workspace text inputs, uses and bounded tool receipts.
-- Immutable migrations and live resources are not changed by this proposal.

CREATE FUNCTION ortak_workspace_canonical(value JSONB) RETURNS TEXT LANGUAGE sql IMMUTABLE STRICT AS $$
    SELECT CASE jsonb_typeof(value)
        WHEN 'object' THEN '{'||coalesce((SELECT string_agg(to_json(key)::text||':'||ortak_workspace_canonical(val),',' ORDER BY key COLLATE "C") FROM jsonb_each(value) AS entries(key,val)),'')||'}'
        WHEN 'array' THEN '['||coalesce((SELECT string_agg(ortak_workspace_canonical(val),',' ORDER BY ordinal) FROM jsonb_array_elements(value) WITH ORDINALITY AS entries(val,ordinal)),'')||']'
        ELSE value::text END
$$;

CREATE TABLE workspace_bindings (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    project_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    id UUID NOT NULL,
    workspace_ref TEXT NOT NULL CHECK(workspace_ref ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$'),
    grant_bytes BYTEA NOT NULL CHECK(octet_length(grant_bytes) BETWEEN 1 AND 16384),
    manifest_hash BYTEA NOT NULL CHECK(octet_length(manifest_hash)=32),
    verification_id UUID NOT NULL,
    verified_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,verification_id),
    FOREIGN KEY(company_id,project_id) REFERENCES projects(company_id,id),
    FOREIGN KEY(company_id,employee_id) REFERENCES employees(company_id,id),
    CHECK(expires_at>verified_at AND verified_at<=created_at)
);
CREATE INDEX idx_workspace_bindings_selection ON workspace_bindings(company_id,project_id,employee_id,workspace_ref,id);

CREATE TABLE workspace_files (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    workspace_id UUID NOT NULL,
    id UUID NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 7),
    logical_name TEXT NOT NULL CHECK(octet_length(logical_name) BETWEEN 1 AND 256
        AND logical_name ~ '^[A-Za-z0-9][A-Za-z0-9._/-]*$'
        AND logical_name !~ '(^|/)(\.|\.\.|)(/|$)'),
    media_type TEXT NOT NULL CHECK(media_type='text/plain'),
    byte_count INTEGER NOT NULL CHECK(byte_count BETWEEN 0 AND 16384),
    content_hash BYTEA NOT NULL CHECK(octet_length(content_hash)=32),
    PRIMARY KEY(company_id,workspace_id,id),
    UNIQUE(company_id,workspace_id,ordinal),
    UNIQUE(company_id,workspace_id,logical_name),
    FOREIGN KEY(company_id,workspace_id) REFERENCES workspace_bindings(company_id,id)
);

CREATE TABLE run_workspace_uses (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    workspace_id UUID NOT NULL,
    manifest_hash BYTEA NOT NULL CHECK(octet_length(manifest_hash)=32),
    store_ref TEXT NOT NULL CHECK(octet_length(store_ref)<=128),
    employee_revision_id UUID NOT NULL,
    employee_lifecycle_epoch BIGINT NOT NULL CHECK(employee_lifecycle_epoch>=0),
    outbox_id UUID NOT NULL,
    admission_lease UUID NOT NULL,
    prepared_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id),
    FOREIGN KEY(company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY(company_id,workspace_id) REFERENCES workspace_bindings(company_id,id),
    FOREIGN KEY(company_id,outbox_id) REFERENCES outbox(company_id,id),
    CHECK(store_ref='workspace-run:'||company_id::text||':'||run_id::text)
);
CREATE INDEX idx_workspace_uses_binding ON run_workspace_uses(company_id,workspace_id,run_id);

CREATE TABLE workspace_tool_actions (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    call_id TEXT NOT NULL CHECK(call_id ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$'),
    file_id UUID NOT NULL,
    arguments_hash BYTEA NOT NULL CHECK(octet_length(arguments_hash)=32),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 4),
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','result_ready','delivered','interrupted')),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count BETWEEN 0 AND 3),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id,call_id),
    UNIQUE(company_id,run_id,ordinal),
    FOREIGN KEY(company_id,run_id) REFERENCES run_workspace_uses(company_id,run_id),
    CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),
    CHECK(arguments_hash=sha256(convert_to('{"file_id":"'||file_id::text||'"}','UTF8')))
);
CREATE INDEX idx_workspace_actions_due ON workspace_tool_actions(company_id,next_attempt_at,run_id,ordinal)
    WHERE state IN('pending','result_ready');
CREATE UNIQUE INDEX idx_workspace_actions_one_pending ON workspace_tool_actions(company_id,run_id)
    WHERE state='pending';

CREATE TABLE workspace_tool_receipts (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    call_id TEXT NOT NULL,
    arguments_hash BYTEA NOT NULL CHECK(octet_length(arguments_hash)=32),
    lease_token UUID NOT NULL,
    attempt_count INTEGER NOT NULL CHECK(attempt_count BETWEEN 1 AND 3),
    result_bytes BYTEA NOT NULL CHECK(octet_length(result_bytes) BETWEEN 1 AND 131072),
    result_hash BYTEA NOT NULL CHECK(octet_length(result_hash)=32 AND result_hash=sha256(result_bytes)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id,call_id),
    FOREIGN KEY(company_id,run_id,call_id) REFERENCES workspace_tool_actions(company_id,run_id,call_id)
);

SELECT attach_community_write_fence('workspace_bindings');
SELECT attach_community_write_fence('workspace_files');
SELECT attach_community_write_fence('run_workspace_uses');
SELECT attach_community_write_fence('workspace_tool_actions');
SELECT attach_community_write_fence('workspace_tool_receipts');

CREATE FUNCTION ortak_workspace_binding_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (to_jsonb(NEW)-'revoked_at') IS DISTINCT FROM (to_jsonb(OLD)-'revoked_at')
            OR OLD.revoked_at IS NOT NULL OR NEW.revoked_at IS NULL THEN
            RAISE EXCEPTION 'ortak: workspace revision is immutable except one withdrawal' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.revoked_at IS NOT NULL OR NEW.verified_at>clock_timestamp()
        OR NEW.verified_at<clock_timestamp()-INTERVAL '30 seconds'
        OR NEW.expires_at<=clock_timestamp() OR NEW.expires_at>clock_timestamp()+INTERVAL '30 days' THEN
        RAISE EXCEPTION 'ortak: workspace verification or retention is invalid' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workspace_binding_guard BEFORE INSERT OR UPDATE ON workspace_bindings
    FOR EACH ROW EXECUTE FUNCTION ortak_workspace_binding_guard();
CREATE TRIGGER workspace_binding_authority BEFORE INSERT OR UPDATE OR DELETE ON workspace_bindings
    FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company','company_id','revoked_at');

CREATE FUNCTION ortak_workspace_manifest_consistent() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE b workspace_bindings; wire JSONB; files JSONB; file_count INTEGER; total INTEGER;
BEGIN
    IF TG_TABLE_NAME='workspace_bindings' THEN b=NEW;
    ELSE SELECT * INTO b FROM workspace_bindings WHERE company_id=NEW.company_id AND id=NEW.workspace_id; END IF;
    wire=convert_from(b.grant_bytes,'UTF8')::jsonb;
    SELECT count(*),coalesce(sum(byte_count),0),jsonb_agg(jsonb_build_object('file_id',id,'name',logical_name,
        'media_type',media_type,'bytes',byte_count,'sha256',encode(content_hash,'hex')) ORDER BY id)
        INTO file_count,total,files FROM workspace_files WHERE company_id=b.company_id AND workspace_id=b.id AND community_id=b.community_id;
    IF file_count NOT BETWEEN 1 AND 8 OR total>65536
        OR EXISTS(SELECT 1 FROM workspace_files f WHERE f.company_id=b.company_id AND f.workspace_id=b.id AND
            (f.community_id<>b.community_id OR f.ordinal<>(SELECT count(*) FROM workspace_files p
                WHERE p.company_id=f.company_id AND p.workspace_id=f.workspace_id AND p.id<f.id)))
        OR wire IS DISTINCT FROM jsonb_build_object('format','ortak-workspace-read/v1','company_id',b.company_id,
            'project_id',b.project_id,'employee_id',b.employee_id,'workspace_ref',b.workspace_ref,'revision',b.id,
            'manifest_hash',encode(b.manifest_hash,'hex'),'files',files)
        OR b.grant_bytes<>convert_to(ortak_workspace_canonical(wire),'UTF8')
        OR b.manifest_hash<>sha256(convert_to(ortak_workspace_canonical(wire-'manifest_hash'),'UTF8')) THEN
        RAISE EXCEPTION 'ortak: workspace selected manifest differs' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_manifest_consistent AFTER INSERT ON workspace_bindings
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_manifest_consistent();
CREATE CONSTRAINT TRIGGER workspace_files_consistent AFTER INSERT ON workspace_files
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_manifest_consistent();

-- Availability for a Files profile is independent of model identity. The
-- selected project binding must still be current at final activation commit.
CREATE FUNCTION ortak_workspace_profile_available(company UUID, employee TEXT, workspace TEXT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM workspace_bindings b
        JOIN projects p ON p.company_id=b.company_id AND p.id=b.project_id
        JOIN project_api_bindings pb ON pb.company_id=b.company_id AND pb.project_id=b.project_id AND pb.community_id=b.community_id
        JOIN office_company_bindings ob ON ob.company_id=b.company_id AND ob.community_id=b.community_id
        JOIN companies c ON c.id=b.company_id JOIN communities cm ON cm.id=b.community_id
        WHERE b.company_id=company AND b.employee_id=employee AND b.workspace_ref=workspace
          AND b.revoked_at IS NULL AND b.expires_at>clock_timestamp() AND p.status='active' AND c.status='active'
          AND cm.deletion_state='active' AND cm.deleted_at IS NULL)
$$;

CREATE FUNCTION ortak_workspace_activation_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE manifest JSONB;
BEGIN
    IF NEW.status<>'active' OR (TG_OP='UPDATE' AND NEW.active_revision_id IS NOT DISTINCT FROM OLD.active_revision_id
        AND NEW.status IS NOT DISTINCT FROM OLD.status) THEN RETURN NEW; END IF;
    SELECT r.manifest INTO manifest FROM employee_revisions r WHERE r.company_id=NEW.company_id AND r.employee_id=NEW.id AND r.id=NEW.active_revision_id;
    IF manifest#>>'{runtime,adapter}'='hermes' AND manifest#>'{permissions,allowed_tools}'='["files"]'::jsonb
        AND NOT ortak_workspace_profile_available(NEW.company_id,NEW.id,manifest#>>'{runtime,workspace_ref}') THEN
        RAISE EXCEPTION 'ortak: Files profile requires a current selected workspace at activation' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_activation_at_commit AFTER INSERT OR UPDATE ON employees
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_activation_at_commit();

CREATE FUNCTION ortak_run_workspace_current(company UUID, run UUID, require_use BOOLEAN DEFAULT true)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT CASE WHEN u.run_id IS NULL THEN
        NOT require_use OR NOT (r.runtime_adapter='hermes' AND rev.manifest#>'{permissions,allowed_tools}'='["files"]'::jsonb)
    ELSE EXISTS(SELECT 1 FROM workspace_bindings b
        JOIN employees e ON e.company_id=b.company_id AND e.id=b.employee_id
        JOIN employee_revisions active ON active.company_id=e.company_id AND active.employee_id=e.id AND active.id=e.active_revision_id
        JOIN employee_runtime_bindings runtime ON runtime.company_id=e.company_id AND runtime.employee_id=e.id AND runtime.revision_id=e.active_revision_id
        JOIN companies c ON c.id=b.company_id JOIN communities cm ON cm.id=b.community_id
        JOIN office_company_bindings ob ON ob.company_id=b.company_id AND ob.community_id=b.community_id
        JOIN project_api_bindings pb ON pb.company_id=b.company_id AND pb.project_id=b.project_id AND pb.community_id=b.community_id
        JOIN work_executions wx ON wx.company_id=r.company_id AND wx.run_id=r.id AND wx.project_id=b.project_id
        WHERE b.company_id=u.company_id AND b.id=u.workspace_id AND b.community_id=u.community_id
          AND b.employee_id=r.employee_id AND b.manifest_hash=u.manifest_hash AND b.revoked_at IS NULL AND b.expires_at>clock_timestamp()
          AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL AND e.status='active'
          AND r.employee_revision_id=u.employee_revision_id AND r.employee_lifecycle_epoch=u.employee_lifecycle_epoch
          AND e.lifecycle_epoch=u.employee_lifecycle_epoch AND r.work_item_id=wx.work_item_id
          AND rev.manifest->'permissions'=jsonb_build_object('allowed_tools',jsonb_build_array('files'),
              'allowed_workspaces',jsonb_build_array(b.workspace_ref),'allowed_networks','[]'::jsonb,'approval_required','[]'::jsonb)
          AND active.manifest->'permissions'=rev.manifest->'permissions'
          AND active.manifest#>>'{runtime,workspace_ref}'=b.workspace_ref AND rev.manifest#>>'{runtime,workspace_ref}'=b.workspace_ref
          AND runtime.workspace_ref=b.workspace_ref AND runtime.validated_at IS NOT NULL)
    END FROM runs r JOIN employee_revisions rev ON rev.company_id=r.company_id AND rev.employee_id=r.employee_id AND rev.id=r.employee_revision_id
    LEFT JOIN run_workspace_uses u ON u.company_id=r.company_id AND u.run_id=r.id
    WHERE r.company_id=company AND r.id=run
$$;

CREATE FUNCTION ortak_lock_run_workspace(company UUID, run UUID, require_use BOOLEAN DEFAULT true)
RETURNS BOOLEAN LANGUAGE plpgsql AS $$
BEGIN
    PERFORM b.id FROM workspace_bindings b JOIN run_workspace_uses u ON u.company_id=b.company_id AND u.workspace_id=b.id
        WHERE u.company_id=company AND u.run_id=run ORDER BY b.id FOR SHARE OF b;
    RETURN coalesce(ortak_run_workspace_current(company,run,require_use),false);
END $$;

CREATE FUNCTION ortak_workspace_use_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT coalesce(ortak_run_workspace_current(NEW.company_id,NEW.run_id),false)
        OR NOT EXISTS(SELECT 1 FROM outbox o JOIN runs r ON r.company_id=o.company_id AND r.id=o.run_id
            WHERE o.company_id=NEW.company_id AND o.id=NEW.outbox_id AND o.run_id=NEW.run_id
              AND o.kind='work_run_dispatch' AND o.state='pending' AND o.lease_token=NEW.admission_lease
              AND o.lease_expires_at>clock_timestamp() AND r.status='queued' AND r.runtime_run_ref IS NULL)
        OR NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.run_id
            AND reader.workspace_id=NEW.workspace_id AND reader.request_key='prepare' AND reader.owner_lease=NEW.admission_lease AND reader.state='stopped'
            AND reader.stop_proof IN('reaped','in_process_returned'))
        OR EXISTS(SELECT 1 FROM run_cancel_requests WHERE company_id=NEW.company_id AND run_id=NEW.run_id)
        OR EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id) THEN
        RAISE EXCEPTION 'ortak: workspace use lacks current dispatch authority' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_use_at_commit AFTER INSERT ON run_workspace_uses
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_use_at_commit();

CREATE FUNCTION ortak_workspace_action_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.state<>'pending' OR NEW.attempt_count<>0 OR NEW.lease_token IS NOT NULL THEN
            RAISE EXCEPTION 'ortak: invalid initial workspace action' USING ERRCODE='check_violation';
        END IF;
    ELSE
        IF (to_jsonb(NEW)-'state'-'lease_token'-'lease_expires_at'-'attempt_count'-'next_attempt_at'-'updated_at')
            IS DISTINCT FROM (to_jsonb(OLD)-'state'-'lease_token'-'lease_expires_at'-'attempt_count'-'next_attempt_at'-'updated_at')
            OR OLD.state IN('delivered','interrupted') OR NEW.attempt_count<OLD.attempt_count
            OR NEW.attempt_count>OLD.attempt_count+1 OR NEW.updated_at<OLD.updated_at
            OR (NEW.state='pending' AND OLD.state<>'pending') THEN
            RAISE EXCEPTION 'ortak: invalid workspace action transition' USING ERRCODE='check_violation';
        END IF;
        IF NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL THEN
            IF OLD.lease_expires_at>clock_timestamp() OR NEW.attempt_count<>OLD.attempt_count+1
                OR NEW.lease_expires_at<=clock_timestamp() OR NEW.lease_expires_at>clock_timestamp()+INTERVAL '30 seconds' THEN
                RAISE EXCEPTION 'ortak: workspace action lease is not claimable' USING ERRCODE='check_violation';
            END IF;
        ELSIF NEW.attempt_count<>OLD.attempt_count OR NEW.lease_expires_at IS DISTINCT FROM OLD.lease_expires_at THEN
            IF NOT (NEW.lease_token IS NULL AND NEW.lease_expires_at IS NULL AND NEW.attempt_count=OLD.attempt_count) THEN
                RAISE EXCEPTION 'ortak: workspace action attempt is not a fresh claim' USING ERRCODE='check_violation';
            END IF;
        END IF;
        IF NEW.state IN('result_ready','delivered') AND NOT EXISTS(SELECT 1 FROM workspace_tool_receipts x
            WHERE x.company_id=NEW.company_id AND x.run_id=NEW.run_id AND x.call_id=NEW.call_id) THEN
            RAISE EXCEPTION 'ortak: workspace action needs its exact result receipt' USING ERRCODE='check_violation';
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workspace_action_guard BEFORE INSERT OR UPDATE ON workspace_tool_actions
    FOR EACH ROW EXECUTE FUNCTION ortak_workspace_action_guard();

CREATE FUNCTION ortak_workspace_action_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' AND NEW.state='interrupted' THEN RETURN NEW; END IF;
    IF NOT EXISTS(SELECT 1 FROM run_workspace_uses u JOIN workspace_files f ON f.company_id=u.company_id AND f.workspace_id=u.workspace_id
        WHERE u.company_id=NEW.company_id AND u.run_id=NEW.run_id AND f.id=NEW.file_id AND u.community_id=NEW.community_id)
        OR (NEW.state='pending' AND NOT coalesce(ortak_run_workspace_current(NEW.company_id,NEW.run_id),false)) THEN
        RAISE EXCEPTION 'ortak: workspace action input is not currently selected' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_action_at_commit AFTER INSERT OR UPDATE ON workspace_tool_actions
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_action_at_commit();

CREATE FUNCTION ortak_workspace_receipt_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE a workspace_tool_actions; f workspace_files; wire JSONB;
BEGIN
    SELECT * INTO a FROM workspace_tool_actions WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND call_id=NEW.call_id;
    SELECT file.* INTO f FROM workspace_files file JOIN run_workspace_uses u ON u.company_id=file.company_id AND u.workspace_id=file.workspace_id
        WHERE u.company_id=NEW.company_id AND u.run_id=NEW.run_id AND file.id=a.file_id;
    wire=convert_from(NEW.result_bytes,'UTF8')::jsonb;
    IF a.call_id IS NULL OR f.id IS NULL OR a.community_id<>NEW.community_id OR a.arguments_hash<>NEW.arguments_hash
        OR a.state<>'result_ready' OR a.lease_token IS DISTINCT FROM NEW.lease_token OR a.attempt_count<>NEW.attempt_count
        OR a.lease_expires_at<=clock_timestamp() OR NOT coalesce(ortak_run_workspace_current(NEW.company_id,NEW.run_id),false)
        OR NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.run_id
            AND reader.request_key='read:'||NEW.call_id AND reader.owner_lease=NEW.lease_token AND reader.state='stopped'
            AND reader.stop_proof IN('reaped','in_process_returned'))
        OR NOT EXISTS(SELECT 1 FROM runs r WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.status IN('running','waiting'))
        OR EXISTS(SELECT 1 FROM run_cancel_requests WHERE company_id=NEW.company_id AND run_id=NEW.run_id)
        OR EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id) THEN
        RAISE EXCEPTION 'ortak: workspace result has no exact live authority/lease' USING ERRCODE='check_violation';
    END IF;
    IF wire->>'status'='completed' THEN
        IF wire IS DISTINCT FROM jsonb_build_object('status','completed','content',wire->>'content','sha256',encode(f.content_hash,'hex'),
            'bytes',f.byte_count,'name',f.logical_name) OR octet_length(wire->>'content') IS DISTINCT FROM f.byte_count
            OR sha256(convert_to(wire->>'content','UTF8')) IS DISTINCT FROM f.content_hash THEN
            RAISE EXCEPTION 'ortak: workspace result bytes differ from selected input' USING ERRCODE='check_violation';
        END IF;
    ELSIF wire IS DISTINCT FROM jsonb_build_object('status','failed','code',wire->>'code')
        OR wire->>'code' IS NULL OR wire->>'code' NOT IN('authority_changed','workspace_unavailable','file_unavailable','input_changed','deadline_exceeded','cancelled') THEN
        RAISE EXCEPTION 'ortak: invalid workspace failure result' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_receipt_at_commit AFTER INSERT ON workspace_tool_receipts
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_receipt_at_commit();

CREATE FUNCTION ortak_workspace_run_admission() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE run UUID; required BOOLEAN=true;
BEGIN
    IF TG_TABLE_NAME='runs' THEN
        IF NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token
            AND NEW.runtime_run_ref IS NOT DISTINCT FROM OLD.runtime_run_ref THEN RETURN NEW; END IF;
        -- A confirmed stop can discover the reference of an accepted start
        -- whose response was lost. Restore only that metadata under the live
        -- cancellation lease (or its ACK), never renew execution authority.
        IF OLD.runtime_run_ref IS NULL AND NEW.runtime_run_ref IS NOT NULL
            AND (to_jsonb(NEW)-'runtime_run_ref'-'updated_at') IS NOT DISTINCT FROM (to_jsonb(OLD)-'runtime_run_ref'-'updated_at')
            AND EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.id
                AND (c.state='acknowledged' OR (c.state='pending' AND c.lease_token IS NOT NULL AND c.lease_expires_at>clock_timestamp())))
            AND NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.id AND reader.state<>'stopped') THEN
            RETURN NEW;
        END IF;
        run=NEW.id; required=NEW.runtime_run_ref IS NOT NULL;
    ELSE run=NEW.run_id;
    END IF;
    IF run IS NULL THEN RETURN NEW; END IF;
    IF NOT coalesce(ortak_run_workspace_current(NEW.company_id,run,required),false) THEN
        RAISE EXCEPTION 'ortak: selected workspace permission changed' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_run_admission AFTER UPDATE ON runs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_run_admission();
CREATE CONSTRAINT TRIGGER workspace_artifact_admission AFTER INSERT ON artifacts
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_run_admission();

DO $$ DECLARE relation TEXT; BEGIN
    FOREACH relation IN ARRAY ARRAY['workspace_bindings','workspace_files','run_workspace_uses','workspace_tool_actions','workspace_tool_receipts'] LOOP
        EXECUTE format('CREATE TRIGGER workspace_no_delete BEFORE DELETE ON %I FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()',relation);
        EXECUTE format('CREATE TRIGGER workspace_no_truncate BEFORE TRUNCATE ON %I FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()',relation);
    END LOOP;
    FOREACH relation IN ARRAY ARRAY['workspace_files','run_workspace_uses','workspace_tool_receipts'] LOOP
        EXECUTE format('CREATE TRIGGER workspace_immutable BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()',relation);
    END LOOP;
END $$;

-- Preparation precedes the immutable run use; this separate retained journal
-- accounts for both preparation and tool reads without weakening use immutability.
CREATE TABLE workspace_reader_executions (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    id UUID NOT NULL,
    workspace_id UUID NOT NULL,
    request_key TEXT NOT NULL CHECK(octet_length(request_key) BETWEEN 1 AND 160),
    owner_lease UUID NOT NULL,
    owner_deadline TIMESTAMPTZ NOT NULL,
    executable TEXT,
    executable_hash BYTEA,
    operating_uid BIGINT,
    pid BIGINT CHECK(pid BETWEEN 1 AND 4294967295),
    state TEXT NOT NULL DEFAULT 'planned' CHECK(state IN('planned','running','stopped')),
    stop_proof TEXT CHECK(stop_proof IN('reaped','in_process_returned','confirmed_absence')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    stopped_at TIMESTAMPTZ,
    PRIMARY KEY(company_id,id),
    FOREIGN KEY(company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY(company_id,workspace_id) REFERENCES workspace_bindings(company_id,id),
    CHECK((executable IS NULL)=(executable_hash IS NULL) AND (executable IS NULL)=(operating_uid IS NULL)),
    CHECK(executable IS NULL OR (octet_length(executable) BETWEEN 1 AND 4096 AND left(executable,1)='/' AND octet_length(executable_hash)=32 AND operating_uid BETWEEN 0 AND 4294967295)),
    CHECK((state='stopped')=(stopped_at IS NOT NULL) AND (state='stopped')=(stop_proof IS NOT NULL)),
    CHECK(stop_proof IS NULL OR (stop_proof='in_process_returned')=(executable IS NULL))
);
CREATE UNIQUE INDEX idx_workspace_reader_one_unresolved ON workspace_reader_executions(company_id,run_id) WHERE state<>'stopped';
CREATE UNIQUE INDEX idx_workspace_reader_attempt ON workspace_reader_executions(company_id,run_id,request_key,owner_lease);
CREATE INDEX idx_workspace_reader_recovery ON workspace_reader_executions(company_id,owner_deadline,id) WHERE state<>'stopped';
SELECT attach_community_write_fence('workspace_reader_executions');
CREATE TRIGGER workspace_reader_no_delete BEFORE DELETE ON workspace_reader_executions FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER workspace_reader_no_truncate BEFORE TRUNCATE ON workspace_reader_executions FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_workspace_reader_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM id FROM runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    IF TG_OP='INSERT' THEN
        IF NEW.state<>'planned' OR NEW.pid IS NOT NULL OR NEW.owner_deadline<=clock_timestamp()
            OR EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)
            OR EXISTS(SELECT 1 FROM run_cancel_requests c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)
            OR NOT EXISTS(SELECT 1 FROM workspace_bindings b WHERE b.company_id=NEW.company_id AND b.id=NEW.workspace_id AND b.community_id=NEW.community_id)
            OR NOT (EXISTS(SELECT 1 FROM outbox o WHERE o.company_id=NEW.company_id AND o.run_id=NEW.run_id AND o.kind='work_run_dispatch'
                AND o.state='pending' AND o.lease_token=NEW.owner_lease AND o.lease_expires_at=NEW.owner_deadline AND o.lease_expires_at>clock_timestamp() AND NEW.request_key='prepare')
                OR EXISTS(SELECT 1 FROM workspace_tool_actions a WHERE a.company_id=NEW.company_id AND a.run_id=NEW.run_id
                    AND NEW.request_key='read:'||a.call_id AND a.state='pending' AND a.lease_token=NEW.owner_lease
                    AND a.lease_expires_at=NEW.owner_deadline AND a.lease_expires_at>clock_timestamp())) THEN
            RAISE EXCEPTION 'ortak: reader execution needs its exact live owner lease' USING ERRCODE='check_violation';
        END IF;
    ELSE
        IF (to_jsonb(NEW)-'pid'-'state'-'stop_proof'-'stopped_at') IS DISTINCT FROM (to_jsonb(OLD)-'pid'-'state'-'stop_proof'-'stopped_at')
            OR OLD.state='stopped' OR NEW.state='planned' OR (OLD.pid IS NOT NULL AND NEW.pid IS DISTINCT FROM OLD.pid)
            OR (NEW.state='running' AND (OLD.state<>'planned' OR NEW.owner_deadline<=clock_timestamp()
                OR (NEW.executable IS NOT NULL AND NEW.pid IS NULL)
                OR EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)
                OR EXISTS(SELECT 1 FROM run_cancel_requests c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)))
            OR (NEW.state='stopped' AND NEW.stop_proof='confirmed_absence' AND NEW.owner_deadline>clock_timestamp()) THEN
            RAISE EXCEPTION 'ortak: reader identity or stop proof changed' USING ERRCODE='check_violation';
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workspace_reader_guard BEFORE INSERT OR UPDATE ON workspace_reader_executions
    FOR EACH ROW EXECUTE FUNCTION ortak_workspace_reader_guard();

CREATE FUNCTION ortak_workspace_reader_cancel_fence() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM id FROM runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    IF NEW.state='acknowledged' AND EXISTS(SELECT 1 FROM workspace_reader_executions r
        WHERE r.company_id=NEW.company_id AND r.run_id=NEW.run_id AND r.state<>'stopped') THEN
        RAISE EXCEPTION 'ortak: unresolved workspace reader prevents cancellation acknowledgement' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_reader_cancel_fence AFTER UPDATE ON runtime_cancellations
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_reader_cancel_fence();
