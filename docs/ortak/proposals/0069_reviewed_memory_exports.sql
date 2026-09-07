-- PROPOSAL ONLY. D2b publication/cleanup; no runtime consumption or migration edit.
CREATE TABLE reviewed_memory_targets (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    deployment_id UUID NOT NULL,
    binding JSONB NOT NULL CHECK(jsonb_typeof(binding)='object' AND octet_length(binding::text)<=8192),
    creation_receipt JSONB NOT NULL CHECK(jsonb_typeof(creation_receipt)='object' AND octet_length(creation_receipt::text)<=16384),
    binding_hash BYTEA NOT NULL CHECK(octet_length(binding_hash)=32),
    employee_revision_id UUID NOT NULL,
    employee_lifecycle_epoch BIGINT NOT NULL CHECK(employee_lifecycle_epoch>=0),
    enabled BOOLEAN NOT NULL,
    valid_until TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,project_id,employee_id,deployment_id,binding_hash),
    FOREIGN KEY(company_id,project_id) REFERENCES projects(company_id,id),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id),
    CHECK(coalesce(creation_receipt->>'company_id'=company_id::text AND creation_receipt->>'employee_id'=employee_id
        AND creation_receipt->>'deployment_id'=deployment_id::text AND creation_receipt->'binding'=binding
        AND creation_receipt->>'request_hash' ~ '^[0-9a-f]{64}$' AND jsonb_typeof(creation_receipt->'native_ids')='object',false))
);

CREATE TABLE reviewed_memory_exports (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    project_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    target_id UUID NOT NULL,
    employee_revision_id UUID NOT NULL,
    employee_lifecycle_epoch BIGINT NOT NULL CHECK(employee_lifecycle_epoch>=0),
    content_hash BYTEA NOT NULL CHECK(octet_length(content_hash)=32),
    source_hash BYTEA NOT NULL CHECK(octet_length(source_hash)=32),
    requested_by TEXT NOT NULL CHECK(requested_by ~ '^[0-9a-f]{64}$'),
    operation_id UUID NOT NULL CHECK(operation_id<>'00000000-0000-0000-0000-000000000000'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,fact_id),
    FOREIGN KEY(company_id,fact_id) REFERENCES reviewed_memory_facts(company_id,id),
    FOREIGN KEY(company_id,project_id) REFERENCES projects(company_id,id),
    FOREIGN KEY(company_id,target_id) REFERENCES reviewed_memory_targets(company_id,id),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id)
);

-- Two stable operations suffice: scheduled withdrawal also handles expiry and
-- may precede an uncertain publication. Distinct expiry/withdraw keys would race
-- for the extension's one irreversible withdrawal identity.
CREATE TABLE reviewed_memory_export_jobs (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    action TEXT NOT NULL CHECK(action IN('publish','withdraw')),
    idempotency_key TEXT NOT NULL CHECK(idempotency_key ~ '^[a-z0-9:-]{1,200}$'),
    request_hash BYTEA NOT NULL CHECK(octet_length(request_hash)=32),
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','acknowledged','failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count BETWEEN 0 AND 20),
    total_attempts INTEGER NOT NULL DEFAULT 0 CHECK(total_attempts BETWEEN 0 AND 180),
    retry_version INTEGER NOT NULL DEFAULT 0 CHECK(retry_version BETWEEN 0 AND 8),
    next_attempt_at TIMESTAMPTZ NOT NULL,
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK(last_error_code IN('authority_refused','target_unavailable','service_retry','service_refused','database_retry','deadline','lease_exhausted')),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,fact_id,action),
    UNIQUE(company_id,idempotency_key),
    FOREIGN KEY(company_id,fact_id) REFERENCES reviewed_memory_exports(company_id,fact_id),
    CHECK((lease_token IS NULL)=(lease_expires_at IS NULL))
);
CREATE INDEX reviewed_memory_export_due ON reviewed_memory_export_jobs(company_id,next_attempt_at,fact_id,action)
    WHERE state='pending';

CREATE TABLE reviewed_memory_export_commands (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    actor_pubkey TEXT NOT NULL CHECK(actor_pubkey ~ '^[0-9a-f]{64}$'),
    operation_id UUID NOT NULL CHECK(operation_id<>'00000000-0000-0000-0000-000000000000'),
    fact_id UUID NOT NULL,
    action TEXT NOT NULL CHECK(action IN('publish','retry_publish','retry_withdraw')),
    request_hash BYTEA NOT NULL CHECK(octet_length(request_hash)=32),
    result_version INTEGER NOT NULL CHECK(result_version BETWEEN 0 AND 8),
    auth_event_id BYTEA NOT NULL CHECK(octet_length(auth_event_id)=32),
    valid_before TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,actor_pubkey,operation_id),
    UNIQUE(company_id,fact_id,action,result_version),
    FOREIGN KEY(company_id,fact_id) REFERENCES reviewed_memory_exports(company_id,fact_id) DEFERRABLE INITIALLY DEFERRED
);
ALTER TABLE reviewed_memory_exports ADD CONSTRAINT reviewed_export_instruction
    FOREIGN KEY(company_id,requested_by,operation_id)
    REFERENCES reviewed_memory_export_commands(company_id,actor_pubkey,operation_id) DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE reviewed_memory_export_receipts (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    action TEXT NOT NULL CHECK(action IN('publish','withdraw')),
    request_hash BYTEA NOT NULL CHECK(octet_length(request_hash)=32),
    binding_hash BYTEA NOT NULL CHECK(octet_length(binding_hash)=32),
    content_hash BYTEA CHECK(octet_length(content_hash)=32),
    remote_status TEXT NOT NULL CHECK(remote_status IN('active','expired','withdrawn')),
    erased_from_reviewed_store BOOLEAN NOT NULL,
    tombstone_at TIMESTAMPTZ,
    lease_token UUID NOT NULL,
    total_attempts INTEGER NOT NULL CHECK(total_attempts BETWEEN 1 AND 180),
    acknowledged_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,fact_id,action),
    FOREIGN KEY(company_id,fact_id,action) REFERENCES reviewed_memory_export_jobs(company_id,fact_id,action),
    CHECK(erased_from_reviewed_store=(tombstone_at IS NOT NULL)),
    CHECK(action<>'withdraw' OR (erased_from_reviewed_store AND remote_status<>'active'))
);

CREATE FUNCTION ortak_reviewed_export_source_hash(f reviewed_memory_facts) RETURNS BYTEA LANGUAGE sql STABLE AS $$
    SELECT CASE WHEN f.source_message_id IS NOT NULL
        THEN sha256(convert_to('message:'||encode(f.source_message_id,'hex'),'UTF8'))
        ELSE (SELECT sha256(convert_to('artifact:'||a.id::text||':'||encode(a.content_hash,'hex'),'UTF8'))
            FROM artifacts a WHERE a.company_id=f.company_id AND a.id=f.source_artifact_id) END
$$;

CREATE FUNCTION ortak_reviewed_export_eligible(company UUID, fact UUID, target UUID) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
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
        WHERE f.company_id=company AND f.id=fact AND t.id=target AND f.version=1 AND f.expires_at>clock_timestamp()
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

CREATE FUNCTION ortak_reviewed_target_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' AND (to_jsonb(NEW)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at')
        IS DISTINCT FROM (to_jsonb(OLD)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at') THEN
        RAISE EXCEPTION 'ortak: reviewed target identity is immutable' USING ERRCODE='check_violation';
    END IF;
    IF NEW.enabled AND (NEW.valid_until<=clock_timestamp() OR NEW.valid_until>clock_timestamp()+INTERVAL '60 seconds') THEN
        RAISE EXCEPTION 'ortak: reviewed target witness must be short and live' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER reviewed_target_guard BEFORE INSERT OR UPDATE ON reviewed_memory_targets FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_target_guard();

CREATE FUNCTION ortak_reviewed_export_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM reviewed_memory_facts f JOIN reviewed_memory_targets t ON t.company_id=f.company_id AND t.id=NEW.target_id
        WHERE f.company_id=NEW.company_id AND f.id=NEW.fact_id AND f.project_id=NEW.project_id AND f.employee_id=NEW.employee_id
        AND f.community_id=NEW.community_id AND NEW.content_hash=sha256(convert_to(f.content,'UTF8'))
        AND NEW.source_hash=ortak_reviewed_export_source_hash(f) AND t.employee_revision_id=NEW.employee_revision_id
        AND t.employee_lifecycle_epoch=NEW.employee_lifecycle_epoch AND ortak_reviewed_export_eligible(f.company_id,f.id,t.id))
      OR NOT EXISTS(SELECT 1 FROM reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id AND o.fact_id=NEW.fact_id
        AND o.actor_pubkey=NEW.requested_by AND o.operation_id=NEW.operation_id AND o.action='publish' AND o.result_version=0
        AND o.xmin::text::bigint=txid_current()%4294967296)
      OR (SELECT count(*) FROM reviewed_memory_export_jobs j WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id
        AND j.state='pending' AND j.attempt_count=0 AND j.xmin::text::bigint=txid_current()%4294967296)<>2 THEN
        RAISE EXCEPTION 'ortak: reviewed export requires current fact, atomic instruction and two jobs' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER reviewed_export_at_commit AFTER INSERT ON reviewed_memory_exports DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_at_commit();

CREATE FUNCTION ortak_reviewed_export_stop() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    UPDATE reviewed_memory_export_jobs SET next_attempt_at=least(next_attempt_at,NEW.revoked_at),updated_at=clock_timestamp()
        WHERE company_id=NEW.company_id AND fact_id=NEW.id AND action='withdraw' AND state='pending';
    RETURN NEW;
END $$;
CREATE TRIGGER reviewed_export_stop AFTER UPDATE ON reviewed_memory_facts FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_stop();

CREATE FUNCTION ortak_reviewed_export_job_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE allowed BOOLEAN:=false;
BEGIN
    IF (NEW.company_id,NEW.community_id,NEW.fact_id,NEW.action,NEW.idempotency_key,NEW.request_hash)
        IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.fact_id,OLD.action,OLD.idempotency_key,OLD.request_hash)
        OR OLD.state='acknowledged' OR NEW.total_attempts<OLD.total_attempts OR NEW.total_attempts>OLD.total_attempts+1
        OR NEW.retry_version<OLD.retry_version OR NEW.retry_version>OLD.retry_version+1 THEN
        RAISE EXCEPTION 'ortak: reviewed job identity and progress are retained' USING ERRCODE='check_violation';
    END IF;
    IF NEW.retry_version=OLD.retry_version+1 THEN
        allowed:=OLD.state='failed' AND OLD.lease_token IS NULL AND NEW.state='pending' AND NEW.attempt_count=0
            AND NEW.total_attempts=OLD.total_attempts AND NEW.lease_token IS NULL AND NEW.last_error_code IS NULL
            AND NEW.next_attempt_at<=clock_timestamp();
    ELSIF NEW.attempt_count=OLD.attempt_count+1 AND NEW.total_attempts=OLD.total_attempts+1 THEN
        allowed:=OLD.state='pending' AND NEW.state='pending' AND OLD.next_attempt_at<=clock_timestamp()
            AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at<=clock_timestamp())
            AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token
            AND NEW.lease_expires_at>clock_timestamp() AND NEW.lease_expires_at<=clock_timestamp()+INTERVAL '60 seconds'
            AND NEW.next_attempt_at=OLD.next_attempt_at AND NEW.last_error_code IS NOT DISTINCT FROM OLD.last_error_code;
    ELSIF NEW.attempt_count=OLD.attempt_count AND NEW.total_attempts=OLD.total_attempts AND OLD.state='pending' THEN
        IF NEW.state='acknowledged' THEN
            allowed:=OLD.lease_token IS NOT NULL AND OLD.lease_expires_at>clock_timestamp()
                AND NEW.lease_token=OLD.lease_token AND NEW.lease_expires_at=OLD.lease_expires_at
                AND NEW.next_attempt_at=OLD.next_attempt_at AND NEW.last_error_code IS NULL;
        ELSIF NEW.state='failed' AND NEW.last_error_code='lease_exhausted' THEN
            allowed:=OLD.attempt_count=20 AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at<=clock_timestamp())
                AND NEW.lease_token IS NULL AND NEW.next_attempt_at=OLD.next_attempt_at;
        ELSIF NEW.state='pending' AND NEW.action='withdraw' AND NEW.next_attempt_at<=OLD.next_attempt_at THEN
            allowed:=(NEW.lease_token,NEW.lease_expires_at,NEW.last_error_code)
                IS NOT DISTINCT FROM (OLD.lease_token,OLD.lease_expires_at,OLD.last_error_code)
                AND EXISTS(SELECT 1 FROM reviewed_memory_facts f WHERE f.company_id=NEW.company_id AND f.id=NEW.fact_id
                    AND f.revoked_at IS NOT NULL AND NEW.next_attempt_at=least(OLD.next_attempt_at,f.revoked_at)
                    AND f.xmin::text::bigint=txid_current()%4294967296);
        ELSIF NEW.lease_token IS NULL AND NEW.last_error_code IS NOT NULL THEN
            allowed:=OLD.lease_token IS NOT NULL AND OLD.lease_expires_at>clock_timestamp()
                AND NEW.next_attempt_at>clock_timestamp() AND NEW.next_attempt_at<=clock_timestamp()+INTERVAL '301 seconds'
                AND (NEW.state='failed' OR NEW.state='pending' AND OLD.attempt_count<20);
        END IF;
    END IF;
    IF NOT coalesce(allowed,false) THEN
        RAISE EXCEPTION 'ortak: reviewed job transition lacks a due claim, live lease, stop or audited retry' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER reviewed_export_job_guard BEFORE UPDATE ON reviewed_memory_export_jobs FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_job_guard();

CREATE FUNCTION ortak_reviewed_export_job_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NOT EXISTS(SELECT 1 FROM reviewed_memory_exports x JOIN reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
            WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id AND x.community_id=NEW.community_id
            AND x.xmin::text::bigint=txid_current()%4294967296 AND NEW.state='pending' AND NEW.attempt_count=0
            AND NEW.total_attempts=0 AND NEW.retry_version=0 AND NEW.last_error_code IS NULL
            AND NEW.idempotency_key='reviewed:'||NEW.action||':'||NEW.fact_id::text
            AND NEW.lease_token IS NULL AND ((NEW.action='withdraw' AND NEW.next_attempt_at=f.expires_at)
                OR (NEW.action='publish' AND NEW.next_attempt_at<=clock_timestamp()))) THEN
            RAISE EXCEPTION 'ortak: reviewed job requires atomic publication' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.retry_version<>OLD.retry_version THEN
        IF NOT EXISTS(SELECT 1 FROM reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id AND o.fact_id=NEW.fact_id
            AND o.action='retry_'||NEW.action AND o.result_version=NEW.retry_version AND o.xmin::text::bigint=txid_current()%4294967296) THEN
            RAISE EXCEPTION 'ortak: reviewed retry requires atomic human command' USING ERRCODE='check_violation';
        END IF;
    END IF;
    IF NEW.state='acknowledged' AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts r
        WHERE r.company_id=NEW.company_id AND r.fact_id=NEW.fact_id AND r.action=NEW.action AND r.request_hash=NEW.request_hash
          AND r.lease_token=NEW.lease_token AND r.total_attempts=NEW.total_attempts AND NEW.lease_expires_at>clock_timestamp()
          AND r.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed acknowledgement requires atomic live-lease receipt' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER reviewed_export_job_at_commit AFTER INSERT OR UPDATE ON reviewed_memory_export_jobs DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_job_at_commit();

CREATE FUNCTION ortak_reviewed_export_command_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.valid_before IS NOT NULL AND NEW.valid_before<=clock_timestamp() THEN
        RAISE EXCEPTION 'ortak: reviewed command authority expired' USING ERRCODE='serialization_failure';
    END IF;
    IF (NEW.action='publish' AND NOT EXISTS(SELECT 1 FROM reviewed_memory_exports x WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id
        AND x.operation_id=NEW.operation_id AND x.requested_by=NEW.actor_pubkey AND NEW.result_version=0 AND x.xmin::text::bigint=txid_current()%4294967296))
        OR (NEW.action<>'publish' AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_jobs j WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id
            AND 'retry_'||j.action=NEW.action AND j.retry_version=NEW.result_version AND j.xmin::text::bigint=txid_current()%4294967296)) THEN
        RAISE EXCEPTION 'ortak: reviewed command requires its atomic effect' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER reviewed_export_command_at_commit AFTER INSERT ON reviewed_memory_export_commands DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_command_at_commit();

CREATE FUNCTION ortak_reviewed_export_receipt_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM reviewed_memory_export_jobs j
        JOIN reviewed_memory_exports x ON x.company_id=j.company_id AND x.fact_id=j.fact_id
        JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id AND j.action=NEW.action AND j.community_id=NEW.community_id
        AND j.state='acknowledged' AND j.request_hash=NEW.request_hash AND t.binding_hash=NEW.binding_hash
        AND (NEW.content_hash=x.content_hash OR NEW.content_hash IS NULL AND NEW.action='withdraw'
            AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts p
                WHERE p.company_id=NEW.company_id AND p.fact_id=NEW.fact_id AND p.action='publish'))
        AND j.lease_token=NEW.lease_token AND j.total_attempts=NEW.total_attempts AND j.lease_expires_at>clock_timestamp()
        AND j.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed receipt requires its exact live job' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER reviewed_export_receipt_at_commit AFTER INSERT ON reviewed_memory_export_receipts DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_receipt_at_commit();

CREATE FUNCTION ortak_reviewed_export_view(company UUID,fact UUID) RETURNS JSONB LANGUAGE sql STABLE AS $$
    SELECT jsonb_build_object('fact_id',x.fact_id,'runtime_consumption_enabled',false,
        'publication',jsonb_build_object('state',p.state,'retry_version',p.retry_version,'attempt_count',p.attempt_count,
            'next_attempt_at',p.next_attempt_at,'error_code',p.last_error_code),
        'cleanup',jsonb_build_object('state',w.state,'retry_version',w.retry_version,'attempt_count',w.attempt_count,
            'next_attempt_at',w.next_attempt_at,'error_code',w.last_error_code),
        'erased_from_reviewed_store',coalesce(r.erased_from_reviewed_store,false))
    FROM reviewed_memory_exports x
    JOIN reviewed_memory_export_jobs p ON p.company_id=x.company_id AND p.fact_id=x.fact_id AND p.action='publish'
    JOIN reviewed_memory_export_jobs w ON w.company_id=x.company_id AND w.fact_id=x.fact_id AND w.action='withdraw'
    LEFT JOIN reviewed_memory_export_receipts r ON r.company_id=x.company_id AND r.fact_id=x.fact_id AND r.action='withdraw'
    WHERE x.company_id=company AND x.fact_id=fact
$$;

DO $$ DECLARE relation TEXT; BEGIN
    FOREACH relation IN ARRAY ARRAY['reviewed_memory_targets','reviewed_memory_exports','reviewed_memory_export_jobs','reviewed_memory_export_commands','reviewed_memory_export_receipts'] LOOP
        EXECUTE format('CREATE TRIGGER reviewed_export_no_delete BEFORE DELETE ON %I FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()',relation);
        EXECUTE format('CREATE TRIGGER reviewed_export_no_truncate BEFORE TRUNCATE ON %I FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()',relation);
        PERFORM attach_community_write_fence(relation);
    END LOOP;
    FOREACH relation IN ARRAY ARRAY['reviewed_memory_exports','reviewed_memory_export_commands','reviewed_memory_export_receipts'] LOOP
        EXECUTE format('CREATE TRIGGER reviewed_export_immutable BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()',relation);
    END LOOP;
END $$;
