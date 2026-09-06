-- REVIEW CANDIDATE ONLY. No migration number, execution or deployment authority.
-- Requires immutable 1..76 and the canonical JSON encoder from source75.
-- This file deliberately installs closed observation/command/target ports.
-- The companion authority fragment opens current-data observation/command
-- checks only. Private signed facade code is separate; retention integration
-- remains pending. Neither a JSON claim nor a hash opens namespace ownership.
-- There is NO project FK, alteration of legacy memory tables, runtime selector,
-- snapshot version, direct Honcho call, automatic extraction or private store.

CREATE FUNCTION ortak_employee_memory_timestamp(value TIMESTAMPTZ)
RETURNS TEXT LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT CASE WHEN value >= TIMESTAMPTZ '1970-01-01 00:00:00+00'
        AND value < TIMESTAMPTZ '10000-01-01 00:00:00+00'
        THEN to_char(value AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END
$$;

-- Root-approved first policy: an authenticated current non-agent human shares
-- only their OWN decided plaintext Office event. Both human and employee must
-- currently belong to source and destination. Relationship human=that actor.
-- Source partition, author, signature/tags/content evidence, current Office
-- identity, configured caller ceilings and both channel deadlines are resolved
-- centrally, never supplied as authorization. Stream + canonical private 1:1
-- are permitted only when the reviewed resolver supports their exact audience;
-- encrypted 1059, groups, undecided/missing/changed source remain refused.
-- Intentionally zero rows until that independent canonical resolver is ready.
CREATE FUNCTION ortak_employee_memory_observation(
    company UUID, employee TEXT, actor BYTEA, source_id BYTEA,
    source_created_at TIMESTAMPTZ, destination_channel UUID,
    memory_kind TEXT, relationship_human BYTEA
) RETURNS TABLE(community_id UUID, source_channel_id UUID,
    source_author_public_key BYTEA, source_evidence_hash BYTEA,
    employee_revision_id UUID, employee_lifecycle_epoch BIGINT,
    observed_at TIMESTAMPTZ, valid_before TIMESTAMPTZ)
LANGUAGE sql STABLE PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT NULL::uuid,NULL::uuid,NULL::bytea,NULL::bytea,NULL::uuid,
        NULL::bigint,NULL::timestamptz,NULL::timestamptz WHERE false
$$;

-- The private Principal-only employee_memory facade performs real NIP-98 and
-- deployment-capability/employee-ceiling authorization. This predicate is only
-- a fail-closed current-data port until the authority fragment is concatenated.
-- SQL credential holders are trusted application operators: matching actor,
-- hashes or auth_event_id are NOT signature or deployment-grant authentication.
CREATE FUNCTION ortak_employee_memory_command_current(
    company UUID, employee TEXT, actor BYTEA, action TEXT
) RETURNS BOOLEAN LANGUAGE sql STABLE PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$ SELECT false $$;

-- Separate from generic EmployeeExperience/Relationship and project receipts.
-- The controller/worker must prove the explicit owned employee namespace,
-- creation receipt + actual I/O witness and reviewed-employee/1 capability.
-- No health-only, ambient provider, generic peer or request-supplied grant.
CREATE FUNCTION ortak_employee_memory_target_authorized(
    company UUID, employee TEXT, deployment UUID, namespace_bytes BYTEA,
    binding JSONB, creation_receipt JSONB, revision UUID, lifecycle BIGINT,
    destination UUID, valid_until TIMESTAMPTZ
) RETURNS BOOLEAN LANGUAGE sql STABLE PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$ SELECT false $$;

CREATE TABLE employee_memory_channel_authorities (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    employee_id TEXT NOT NULL,
    channel_id UUID NOT NULL CHECK(channel_id<>'00000000-0000-0000-0000-000000000000'),
    epoch BIGINT NOT NULL DEFAULT 0 CHECK(epoch>=0),
    reason TEXT NOT NULL DEFAULT 'registered'
        CHECK(reason IN('registered','source_changed','audience_changed','identity_changed','scope_closed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,community_id,employee_id,channel_id),
    FOREIGN KEY(company_id,employee_id) REFERENCES employees(company_id,id),
    CHECK(company_id<>'00000000-0000-0000-0000-000000000000'
        AND community_id<>'00000000-0000-0000-0000-000000000000'),
    CHECK(changed_at>=created_at)
);
CREATE INDEX employee_memory_authority_community
    ON employee_memory_channel_authorities(community_id,channel_id,company_id,employee_id);

-- Registration follows a successful canonical observation and does not grant
-- source review. Shared Office fence first, then community/company cap locks,
-- then sorted source/destination rows. Each scope is retained permanently.
CREATE FUNCTION ortak_employee_memory_authority_guard() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (to_jsonb(NEW)-'epoch'-'reason'-'changed_at') IS DISTINCT FROM
            (to_jsonb(OLD)-'epoch'-'reason'-'changed_at') OR OLD.epoch=9223372036854775807
            OR NEW.epoch<>OLD.epoch+1 OR NEW.reason='registered' THEN
            RAISE EXCEPTION 'employee memory authority only advances' USING ERRCODE='check_violation';
        END IF;
        NEW.changed_at=clock_timestamp();
        RETURN NEW;
    END IF;
    PERFORM ortak_lock_office_authority(NEW.company_id);
    IF NEW.epoch<>0 OR NEW.reason<>'registered' OR NOT EXISTS(
        SELECT 1 FROM companies c JOIN office_company_bindings b ON b.company_id=c.id
        JOIN communities cm ON cm.id=b.community_id
        JOIN employees e ON e.company_id=c.id AND e.id=NEW.employee_id
        JOIN channels ch ON ch.community_id=cm.id AND ch.id=NEW.channel_id
        WHERE c.id=NEW.company_id AND cm.id=NEW.community_id AND c.status='active'
            AND cm.deletion_state='active' AND cm.deleted_at IS NULL AND e.status='active'
            AND ch.archived_at IS NULL AND ch.deleted_at IS NULL
            AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>clock_timestamp())) THEN
        RAISE EXCEPTION 'employee memory scope is not current' USING ERRCODE='check_violation';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-community-registration:'||NEW.community_id::text,0))
        OR NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-company-registration:'||NEW.company_id::text,0)) THEN
        RAISE EXCEPTION 'employee memory registration busy' USING ERRCODE='serialization_failure';
    END IF;
    IF (SELECT count(*) FROM employee_memory_channel_authorities WHERE company_id=NEW.company_id)>=128
        OR (SELECT count(*) FROM employee_memory_channel_authorities WHERE community_id=NEW.community_id)>=256 THEN
        RAISE EXCEPTION 'retained employee memory scope cap reached' USING ERRCODE='program_limit_exceeded';
    END IF;
    NEW.created_at=clock_timestamp(); NEW.changed_at=NEW.created_at;
    RETURN NEW;
END $$;
CREATE TRIGGER employee_memory_authority_guard BEFORE INSERT OR UPDATE
    ON employee_memory_channel_authorities FOR EACH ROW
    EXECUTE FUNCTION ortak_employee_memory_authority_guard();

CREATE FUNCTION ortak_register_employee_memory_authorities(
    company UUID, community UUID, employee TEXT, source_channel UUID, destination_channel UUID
) RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE channel UUID;
BEGIN
    PERFORM ortak_lock_office_authority(company);
    IF current_setting('transaction_isolation')<>'read committed'
        OR company IS NULL OR community IS NULL OR employee IS NULL
        OR source_channel IS NULL OR destination_channel IS NULL THEN
        RAISE EXCEPTION 'employee memory registration requires current scoped transaction'
            USING ERRCODE='invalid_transaction_state';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-community-registration:'||community::text,0))
        OR NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-company-registration:'||company::text,0)) THEN
        RAISE EXCEPTION 'employee memory registration busy' USING ERRCODE='serialization_failure';
    END IF;
    FOR channel IN SELECT DISTINCT v FROM unnest(ARRAY[source_channel,destination_channel]) v ORDER BY v LOOP
        -- No rebind/reset of retained keys; INSERT guard independently checks caps.
        PERFORM 1 FROM employee_memory_channel_authorities a WHERE a.company_id=company
            AND a.community_id=community AND a.employee_id=employee AND a.channel_id=channel FOR SHARE;
        IF NOT FOUND THEN
            INSERT INTO employee_memory_channel_authorities(company_id,community_id,employee_id,channel_id)
                VALUES(company,community,employee,channel);
        END IF;
    END LOOP;
END $$;

CREATE TABLE employee_reviewed_memory_facts (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    id UUID NOT NULL CHECK(id<>'00000000-0000-0000-0000-000000000000'),
    employee_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN('experience','relationship')),
    human_public_key BYTEA CHECK(octet_length(human_public_key)=32),
    destination_channel_id UUID NOT NULL,
    source_channel_id UUID NOT NULL,
    source_event_id BYTEA NOT NULL CHECK(octet_length(source_event_id)=32),
    source_event_created_at TIMESTAMPTZ NOT NULL,
    source_author_public_key BYTEA NOT NULL CHECK(octet_length(source_author_public_key)=32),
    source_evidence_hash BYTEA NOT NULL CHECK(octet_length(source_evidence_hash)=32),
    audience_bytes BYTEA NOT NULL CHECK(octet_length(audience_bytes) BETWEEN 1 AND 2048),
    audience_hash BYTEA NOT NULL CHECK(octet_length(audience_hash)=32 AND audience_hash=sha256(audience_bytes)),
    source_hash BYTEA NOT NULL CHECK(octet_length(source_hash)=32),
    provenance_bytes BYTEA NOT NULL CHECK(octet_length(provenance_bytes) BETWEEN 1 AND 4096),
    sharing_hash BYTEA NOT NULL CHECK(octet_length(sharing_hash)=32 AND sharing_hash=sha256(provenance_bytes)),
    content TEXT NOT NULL CHECK(octet_length(content) BETWEEN 1 AND 4096 AND btrim(content)<>''),
    content_hash BYTEA NOT NULL CHECK(content_hash=sha256(convert_to(content,'UTF8'))),
    approved_by BYTEA NOT NULL CHECK(octet_length(approved_by)=32 AND approved_by=source_author_public_key),
    approval_id UUID NOT NULL CHECK(approval_id<>'00000000-0000-0000-0000-000000000000'),
    approved_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    expires_at TIMESTAMPTZ NOT NULL,
    version INTEGER NOT NULL DEFAULT 1 CHECK(version IN(1,2)),
    revoked_at TIMESTAMPTZ,
    revoked_by BYTEA CHECK(octet_length(revoked_by)=32),
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,community_id,id),
    UNIQUE(company_id,approved_by,approval_id),
    FOREIGN KEY(company_id,employee_id) REFERENCES employees(company_id,id),
    FOREIGN KEY(company_id,community_id,employee_id,source_channel_id)
        REFERENCES employee_memory_channel_authorities(company_id,community_id,employee_id,channel_id),
    FOREIGN KEY(company_id,community_id,employee_id,destination_channel_id)
        REFERENCES employee_memory_channel_authorities(company_id,community_id,employee_id,channel_id),
    CHECK((kind='experience' AND human_public_key IS NULL)
        OR (kind='relationship' AND human_public_key IS NOT NULL AND human_public_key=approved_by)),
    CHECK(ortak_employee_memory_timestamp(source_event_created_at) IS NOT NULL),
    CHECK(ortak_employee_memory_timestamp(approved_at) IS NOT NULL
        AND ortak_employee_memory_timestamp(expires_at) IS NOT NULL),
    CHECK(expires_at>approved_at AND expires_at<=approved_at+INTERVAL '2160 hours'),
    CHECK((version=1 AND revoked_at IS NULL AND revoked_by IS NULL)
        OR (version=2 AND revoked_at IS NOT NULL AND revoked_by IS NOT NULL
            AND revoked_by=approved_by AND revoked_at>=approved_at))
);
CREATE INDEX employee_reviewed_memory_list ON employee_reviewed_memory_facts
    (company_id,employee_id,destination_channel_id,id);
-- Current original-approver/employee filtering and UUID pagination precede LIMIT.
CREATE INDEX employee_reviewed_memory_approver_list ON employee_reviewed_memory_facts
    (company_id,employee_id,approved_by,id);
CREATE INDEX employee_reviewed_memory_source ON employee_reviewed_memory_facts
    (community_id,source_event_id,source_event_created_at,company_id,employee_id);

-- Returned JSON is an intermediate only: compare the exact compact canonical
-- UTF8 preimage, not JSONB equality, against the private Rust pure constructors.
CREATE FUNCTION ortak_employee_memory_audience(f employee_reviewed_memory_facts)
RETURNS JSONB LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT jsonb_build_object('company_id',f.company_id,'employee_id',f.employee_id,
        'format','ortak-reviewed-employee-audience/1','kind',f.kind,
        'human_public_key',encode(f.human_public_key,'hex'),
        'destination_community_id',f.community_id,'destination_channel_id',f.destination_channel_id)
$$;
CREATE FUNCTION ortak_employee_memory_source(f employee_reviewed_memory_facts)
RETURNS JSONB LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT jsonb_build_object('community_id',f.community_id,'channel_id',f.source_channel_id,
        'event_id',encode(f.source_event_id,'hex'),
        'event_created_at',ortak_employee_memory_timestamp(f.source_event_created_at),
        'author_public_key',encode(f.source_author_public_key,'hex'),
        'evidence_hash',encode(f.source_evidence_hash,'hex'))
$$;
CREATE FUNCTION ortak_employee_memory_fact_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE audience JSONB; source JSONB; provenance JSONB;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF OLD.version<>1 OR NEW.version<>2 OR NEW.revoked_at IS NULL OR NEW.revoked_by IS DISTINCT FROM OLD.approved_by
            OR (to_jsonb(NEW)-'version'-'revoked_at'-'revoked_by') IS DISTINCT FROM
                (to_jsonb(OLD)-'version'-'revoked_at'-'revoked_by') THEN
            RAISE EXCEPTION 'employee memory fact only permits Stop' USING ERRCODE='check_violation';
        END IF;
        NEW.revoked_at=clock_timestamp(); RETURN NEW;
    END IF;
    IF NEW.version<>1 OR NEW.revoked_at IS NOT NULL OR NEW.revoked_by IS NOT NULL THEN
        RAISE EXCEPTION 'new employee memory fact must be approved' USING ERRCODE='check_violation';
    END IF;
    NEW.approved_at=clock_timestamp();
    audience=ortak_employee_memory_audience(NEW); source=ortak_employee_memory_source(NEW);
    provenance=jsonb_build_object('format','ortak-reviewed-employee-provenance/1',
        'audience',audience,'audience_hash',encode(NEW.audience_hash,'hex'),
        'source',source,'source_hash',encode(NEW.source_hash,'hex'),
        'approval',jsonb_build_object('format','ortak-reviewed-employee-sharing/1',
            'approval_id',NEW.approval_id,'approved_by',encode(NEW.approved_by,'hex'),
            'content_hash',encode(NEW.content_hash,'hex'),
            'expires_at',ortak_employee_memory_timestamp(NEW.expires_at)));
    IF NEW.audience_bytes IS DISTINCT FROM convert_to(ortak_conversation_json75(audience),'UTF8')
        OR NEW.source_hash IS DISTINCT FROM sha256(convert_to(ortak_conversation_json75(
            jsonb_build_object('audience_hash',encode(NEW.audience_hash,'hex'),
                'format','ortak-reviewed-employee-source/1','source',source)),'UTF8'))
        OR NEW.provenance_bytes IS DISTINCT FROM convert_to(ortak_conversation_json75(provenance),'UTF8') THEN
        RAISE EXCEPTION 'employee memory canonical bytes differ' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER employee_memory_fact_guard BEFORE INSERT OR UPDATE ON employee_reviewed_memory_facts
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_fact_guard();

CREATE TABLE employee_reviewed_memory_operations (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    actor_public_key BYTEA NOT NULL CHECK(octet_length(actor_public_key)=32),
    operation_id UUID NOT NULL CHECK(operation_id<>'00000000-0000-0000-0000-000000000000'),
    fact_id UUID NOT NULL,
    action TEXT NOT NULL CHECK(action IN('approve','stop')),
    submitted_bytes BYTEA NOT NULL CHECK(octet_length(submitted_bytes) BETWEEN 1 AND 32768),
    submitted_hash BYTEA NOT NULL CHECK(submitted_hash=sha256(submitted_bytes)),
    result_version INTEGER NOT NULL CHECK((action='approve' AND result_version=1) OR (action='stop' AND result_version=2)),
    auth_event_id BYTEA NOT NULL CHECK(octet_length(auth_event_id)=32),
    valid_before TIMESTAMPTZ NOT NULL CHECK(ortak_employee_memory_timestamp(valid_before) IS NOT NULL),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,actor_public_key,operation_id),
    UNIQUE(company_id,fact_id,action),
    FOREIGN KEY(company_id,community_id,fact_id)
        REFERENCES employee_reviewed_memory_facts(company_id,community_id,id) DEFERRABLE INITIALLY DEFERRED
);
ALTER TABLE employee_reviewed_memory_facts ADD CONSTRAINT employee_memory_original_approval
    FOREIGN KEY(company_id,approved_by,approval_id)
    REFERENCES employee_reviewed_memory_operations(company_id,actor_public_key,operation_id)
    DEFERRABLE INITIALLY DEFERRED;

-- Exact submitted draft identity for replay BEFORE fresh source/expiry checks.
-- No client source digest, inferred root, source body or auto-edited text.
CREATE FUNCTION ortak_employee_memory_submission(
    f employee_reviewed_memory_facts, operation UUID, action TEXT
) RETURNS BYTEA LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT convert_to(ortak_conversation_json75(CASE action
        WHEN 'approve' THEN jsonb_build_object('format','ortak-reviewed-employee-command/1',
            'action',action,'operation_id',operation,'employee_id',f.employee_id,'kind',f.kind,
            'human_public_key',encode(f.human_public_key,'hex'),
            'source_event_id',encode(f.source_event_id,'hex'),
            'source_event_created_at',ortak_employee_memory_timestamp(f.source_event_created_at),
            'destination_channel_id',f.destination_channel_id,
            'expected_audience_hash',encode(f.audience_hash,'hex'),
            'content',f.content,'expires_at',ortak_employee_memory_timestamp(f.expires_at),'reviewed',true)
        WHEN 'stop' THEN jsonb_build_object('format','ortak-reviewed-employee-command/1',
            'action',action,'operation_id',operation,'fact_id',f.id,'expected_version',1)
        END),'UTF8')
$$;

CREATE FUNCTION ortak_employee_memory_fact_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE f employee_reviewed_memory_facts; o employee_reviewed_memory_operations;
    observation RECORD; selected_action TEXT;
BEGIN
    -- INSERT guards validate NEW creation only. Immutable historical receipts
    -- are not rechecked on later Stop, remote cleanup, or read/restore.
    SELECT * INTO STRICT f FROM employee_reviewed_memory_facts
        WHERE company_id=NEW.company_id AND id=NEW.id;
    PERFORM ortak_lock_office_authority(f.company_id);
    IF TG_OP='INSERT' THEN selected_action='approve'; ELSE selected_action='stop'; END IF;
    SELECT * INTO STRICT o FROM employee_reviewed_memory_operations op
        WHERE op.company_id=f.company_id AND op.fact_id=f.id AND op.action=selected_action
            AND op.xmin::text::bigint=txid_current()%4294967296;
    IF o.actor_public_key<>f.approved_by OR o.community_id<>f.community_id
        OR o.result_version<>f.version
        OR o.submitted_bytes IS DISTINCT FROM ortak_employee_memory_submission(f,o.operation_id,o.action)
        OR o.valid_before<=clock_timestamp()
        OR NOT coalesce(ortak_employee_memory_command_current(f.company_id,f.employee_id,
            o.actor_public_key,o.action),false) THEN
        RAISE EXCEPTION 'employee memory lacks its exact current atomic command' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='UPDATE' THEN RETURN NEW; END IF;
    IF o.operation_id<>f.approval_id THEN
        RAISE EXCEPTION 'employee memory approval identity mismatch' USING ERRCODE='check_violation';
    END IF;
    PERFORM ortak_lock_office_authority(f.company_id);
    PERFORM 1 FROM employee_memory_channel_authorities a WHERE a.company_id=f.company_id
        AND a.community_id=f.community_id AND a.employee_id=f.employee_id
        AND a.channel_id IN(f.source_channel_id,f.destination_channel_id)
        ORDER BY a.channel_id FOR SHARE;
    SELECT * INTO STRICT observation FROM ortak_employee_memory_observation(f.company_id,f.employee_id,
        f.approved_by,f.source_event_id,f.source_event_created_at,f.destination_channel_id,f.kind,f.human_public_key);
    IF (observation.community_id,observation.source_channel_id,observation.source_author_public_key,
        observation.source_evidence_hash) IS DISTINCT FROM
        (f.community_id,f.source_channel_id,f.source_author_public_key,f.source_evidence_hash)
        OR f.expires_at<=clock_timestamp()
        OR observation.observed_at IS NULL OR observation.observed_at>clock_timestamp()
        OR observation.employee_revision_id IS NULL OR observation.employee_lifecycle_epoch IS NULL
        OR NOT EXISTS(SELECT 1 FROM employees e WHERE e.company_id=f.company_id AND e.id=f.employee_id
            AND e.status='active' AND e.active_revision_id=observation.employee_revision_id
            AND e.lifecycle_epoch=observation.employee_lifecycle_epoch)
        OR (observation.valid_before IS NOT NULL AND
            (observation.valid_before<=clock_timestamp() OR f.expires_at>observation.valid_before)) THEN
        RAISE EXCEPTION 'employee memory source/sharing authority changed' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER employee_memory_fact_at_commit AFTER INSERT OR UPDATE
    ON employee_reviewed_memory_facts DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_fact_at_commit();

CREATE FUNCTION ortak_employee_memory_operation_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
        WHERE f.company_id=NEW.company_id AND f.community_id=NEW.community_id AND f.id=NEW.fact_id
            AND f.approved_by=NEW.actor_public_key AND f.version=NEW.result_version
            AND (NEW.action='stop' OR f.approval_id=NEW.operation_id)
            AND NEW.submitted_bytes=ortak_employee_memory_submission(f,NEW.operation_id,NEW.action)
            AND f.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'employee memory receipt lacks its atomic effect' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER employee_memory_operation_at_commit AFTER INSERT
    ON employee_reviewed_memory_operations DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_operation_at_commit();

-- Separate employee-owned remote journal; structurally follows 69, with
-- domain-separated keys, exact sharing bytes and no project namespace.
CREATE TABLE employee_reviewed_memory_targets (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    id UUID NOT NULL DEFAULT gen_random_uuid() CHECK(id<>'00000000-0000-0000-0000-000000000000'),
    destination_channel_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    deployment_id UUID NOT NULL CHECK(deployment_id<>'00000000-0000-0000-0000-000000000000'),
    namespace_bytes BYTEA NOT NULL CHECK(octet_length(namespace_bytes) BETWEEN 1 AND 2048),
    namespace_hash BYTEA NOT NULL CHECK(namespace_hash=sha256(namespace_bytes)),
    protocol TEXT NOT NULL CHECK(protocol='reviewed-employee/1'),
    binding JSONB NOT NULL CHECK(jsonb_typeof(binding)='object' AND octet_length(binding::text)<=8192),
    creation_receipt JSONB NOT NULL CHECK(jsonb_typeof(creation_receipt)='object' AND octet_length(creation_receipt::text)<=16384),
    binding_hash BYTEA NOT NULL CHECK(octet_length(binding_hash)=32),
    employee_revision_id UUID NOT NULL,
    employee_lifecycle_epoch BIGINT NOT NULL CHECK(employee_lifecycle_epoch>=0),
    enabled BOOLEAN NOT NULL DEFAULT false,
    consumption_epoch BIGINT NOT NULL DEFAULT 0 CHECK(consumption_epoch>=0),
    valid_until TIMESTAMPTZ NOT NULL CHECK(ortak_employee_memory_timestamp(valid_until) IS NOT NULL),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,destination_channel_id,employee_id,deployment_id,binding_hash),
    FOREIGN KEY(company_id,community_id,employee_id,destination_channel_id)
        REFERENCES employee_memory_channel_authorities(company_id,community_id,employee_id,channel_id),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id),
    CHECK(coalesce(creation_receipt->>'company_id'=company_id::text AND creation_receipt->>'employee_id'=employee_id
        AND creation_receipt->>'deployment_id'=deployment_id::text AND creation_receipt->'binding'=binding
        AND creation_receipt->>'protocol'=protocol
        AND creation_receipt->>'namespace_hash'=encode(namespace_hash,'hex')
        AND creation_receipt->>'request_hash' ~ '^[0-9a-f]{64}$' AND jsonb_typeof(creation_receipt->'native_ids')='object',false))
);

CREATE TABLE employee_reviewed_memory_exports (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    destination_channel_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    target_id UUID NOT NULL,
    employee_revision_id UUID NOT NULL,
    employee_lifecycle_epoch BIGINT NOT NULL CHECK(employee_lifecycle_epoch>=0),
    content_hash BYTEA NOT NULL CHECK(octet_length(content_hash)=32),
    source_hash BYTEA NOT NULL CHECK(octet_length(source_hash)=32),
    sharing_hash BYTEA NOT NULL CHECK(octet_length(sharing_hash)=32),
    requested_by TEXT NOT NULL CHECK(requested_by ~ '^[0-9a-f]{64}$'),
    operation_id UUID NOT NULL CHECK(operation_id<>'00000000-0000-0000-0000-000000000000'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,fact_id),
    FOREIGN KEY(company_id,fact_id) REFERENCES employee_reviewed_memory_facts(company_id,id),
    FOREIGN KEY(company_id,community_id,employee_id,destination_channel_id)
        REFERENCES employee_memory_channel_authorities(company_id,community_id,employee_id,channel_id),
    FOREIGN KEY(company_id,target_id) REFERENCES employee_reviewed_memory_targets(company_id,id),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id)
);

-- Two stable operations suffice: scheduled withdrawal also handles expiry and
-- may precede an uncertain publication. Distinct expiry/withdraw keys would race
-- for the extension's one irreversible withdrawal identity.
CREATE TABLE employee_reviewed_memory_export_jobs (
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
    FOREIGN KEY(company_id,fact_id) REFERENCES employee_reviewed_memory_exports(company_id,fact_id),
    CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),
    CHECK(total_attempts>=attempt_count AND total_attempts<=20*(retry_version+1)),
    CHECK(state<>'failed' OR lease_token IS NULL)
);
CREATE INDEX employee_reviewed_memory_export_due ON employee_reviewed_memory_export_jobs(company_id,next_attempt_at,fact_id,action)
    WHERE state='pending';

CREATE TABLE employee_reviewed_memory_export_commands (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    actor_pubkey TEXT NOT NULL CHECK(actor_pubkey ~ '^[0-9a-f]{64}$'),
    operation_id UUID NOT NULL CHECK(operation_id<>'00000000-0000-0000-0000-000000000000'),
    fact_id UUID NOT NULL,
    action TEXT NOT NULL CHECK(action IN('publish','retry_publish','retry_withdraw')),
    request_hash BYTEA NOT NULL CHECK(octet_length(request_hash)=32),
    result_version INTEGER NOT NULL CHECK((action='publish' AND result_version=0)
        OR (action IN('retry_publish','retry_withdraw') AND result_version BETWEEN 1 AND 8)),
    auth_event_id BYTEA NOT NULL CHECK(octet_length(auth_event_id)=32),
    valid_before TIMESTAMPTZ NOT NULL CHECK(ortak_employee_memory_timestamp(valid_before) IS NOT NULL),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,actor_pubkey,operation_id),
    UNIQUE(company_id,fact_id,action,result_version),
    FOREIGN KEY(company_id,fact_id) REFERENCES employee_reviewed_memory_exports(company_id,fact_id) DEFERRABLE INITIALLY DEFERRED
);
ALTER TABLE employee_reviewed_memory_exports ADD CONSTRAINT employee_reviewed_export_instruction
    FOREIGN KEY(company_id,requested_by,operation_id)
    REFERENCES employee_reviewed_memory_export_commands(company_id,actor_pubkey,operation_id) DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE employee_reviewed_memory_export_receipts (
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
    FOREIGN KEY(company_id,fact_id,action) REFERENCES employee_reviewed_memory_export_jobs(company_id,fact_id,action),
    CHECK(erased_from_reviewed_store=(tombstone_at IS NOT NULL)),
    CHECK((remote_status='active')=(NOT erased_from_reviewed_store)),
    CHECK(action<>'withdraw' OR (erased_from_reviewed_store AND remote_status<>'active'))
);

CREATE FUNCTION ortak_employee_memory_target_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE expected_namespace BYTEA; expected_binding BYTEA;
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    expected_namespace=convert_to(ortak_conversation_json75(jsonb_build_object(
        'format','ortak-reviewed-employee-namespace/1',
        'company_id',NEW.company_id,'employee_id',NEW.employee_id)),'UTF8');
    expected_binding=sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
        'binding',NEW.binding,'namespace_hash',encode(NEW.namespace_hash,'hex'),
        'protocol',NEW.protocol)),'UTF8'));
    IF NEW.namespace_bytes IS DISTINCT FROM expected_namespace
        OR NEW.binding_hash IS DISTINCT FROM expected_binding THEN
        RAISE EXCEPTION 'employee memory target namespace differs' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='INSERT' THEN
        IF NEW.consumption_epoch<>0 THEN
            RAISE EXCEPTION 'employee memory target epoch must start at zero' USING ERRCODE='check_violation';
        END IF;
    ELSE
        IF (to_jsonb(NEW)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at'-'consumption_epoch')
            IS DISTINCT FROM
            (to_jsonb(OLD)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at'-'consumption_epoch')
            OR NEW.consumption_epoch<>OLD.consumption_epoch THEN
            RAISE EXCEPTION 'employee memory target identity is immutable' USING ERRCODE='check_violation';
        END IF;
        -- Model-only revisions with the identical owned binding keep identity.
        -- Disable/enable, lifecycle turnover, or renewing a lapsed witness retire
        -- old use pins; an expired lease/witness is never containment proof.
        IF (NEW.enabled,NEW.employee_lifecycle_epoch) IS DISTINCT FROM
            (OLD.enabled,OLD.employee_lifecycle_epoch)
            OR NEW.enabled AND OLD.valid_until<=clock_timestamp() THEN
            IF OLD.consumption_epoch=9223372036854775807 THEN
                RAISE EXCEPTION 'employee memory target epoch exhausted' USING ERRCODE='program_limit_exceeded';
            END IF;
            NEW.consumption_epoch=OLD.consumption_epoch+1;
        END IF;
    END IF;
    IF NEW.enabled AND (NEW.valid_until<=clock_timestamp()
        OR NEW.valid_until>clock_timestamp()+INTERVAL '60 seconds'
        OR NOT coalesce(ortak_employee_memory_target_authorized(NEW.company_id,NEW.employee_id,
            NEW.deployment_id,NEW.namespace_bytes,NEW.binding,NEW.creation_receipt,
            NEW.employee_revision_id,NEW.employee_lifecycle_epoch,
            NEW.destination_channel_id,NEW.valid_until),false)) THEN
        RAISE EXCEPTION 'employee memory target lacks current owned protocol witness' USING ERRCODE='check_violation';
    END IF;
    -- Even disabled initial rows require genuine creation ownership. They are
    -- not a way to smuggle a project receipt into later cleanup.
    IF TG_OP='INSERT' AND NOT coalesce(ortak_employee_memory_target_authorized(
        NEW.company_id,NEW.employee_id,NEW.deployment_id,NEW.namespace_bytes,
        NEW.binding,NEW.creation_receipt,NEW.employee_revision_id,NEW.employee_lifecycle_epoch,
        NEW.destination_channel_id,NEW.valid_until),false) THEN
        RAISE EXCEPTION 'employee namespace ownership unavailable' USING ERRCODE='check_violation';
    END IF;
    NEW.updated_at=clock_timestamp(); RETURN NEW;
END $$;
CREATE TRIGGER employee_memory_target_guard BEFORE INSERT OR UPDATE ON employee_reviewed_memory_targets
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_target_guard();

-- Publication eligibility only. Runtime disclosure requires a future actual
-- run origin/human resolver and frozen source/destination/target epoch pins.
-- Do not use this boolean as a runtime permission or an unlocked snapshot.
CREATE FUNCTION ortak_employee_reviewed_export_eligible(company UUID, fact UUID, target UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
        JOIN employee_reviewed_memory_targets t ON t.company_id=f.company_id AND t.id=target
            AND t.community_id=f.community_id AND t.employee_id=f.employee_id
            AND t.destination_channel_id=f.destination_channel_id
        JOIN employees e ON e.company_id=f.company_id AND e.id=f.employee_id
        JOIN employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
        JOIN employee_memory_bindings mb ON mb.company_id=e.company_id AND mb.employee_id=e.id AND mb.revision_id=r.id
        CROSS JOIN LATERAL ortak_employee_memory_observation(f.company_id,f.employee_id,f.approved_by,
            f.source_event_id,f.source_event_created_at,f.destination_channel_id,f.kind,f.human_public_key) o
        WHERE f.company_id=company AND f.id=fact AND f.version=1 AND f.expires_at>clock_timestamp()
            AND e.status='active' AND t.enabled AND t.valid_until>clock_timestamp()
            AND t.employee_revision_id=r.id AND t.employee_lifecycle_epoch=e.lifecycle_epoch
            AND o.employee_revision_id=r.id AND o.employee_lifecycle_epoch=e.lifecycle_epoch
            AND mb.validated_at IS NOT NULL AND t.binding=r.manifest->'memory'
            AND t.binding=jsonb_build_object('adapter',mb.adapter,'endpoint_ref',mb.endpoint_ref,
                'workspace',mb.workspace,'user_peer',mb.user_peer,'employee_peer',mb.employee_peer,'options',mb.options)
            AND o.community_id=f.community_id AND o.source_channel_id=f.source_channel_id
            AND o.source_author_public_key=f.source_author_public_key AND o.source_evidence_hash=f.source_evidence_hash
            AND o.observed_at IS NOT NULL AND o.observed_at<=clock_timestamp()
            AND (o.valid_before IS NULL OR o.valid_before>clock_timestamp()))
$$;

CREATE FUNCTION ortak_employee_reviewed_request_hash(company UUID, fact UUID, action TEXT)
RETURNS BYTEA LANGUAGE sql STABLE AS $$
    SELECT CASE WHEN action IN('publish','withdraw') THEN sha256(convert_to(ortak_conversation_json75(
        jsonb_build_object('format','ortak-reviewed-employee-remote-request/1','action',action,
            'company_id',x.company_id,'employee_id',x.employee_id,'fact_id',x.fact_id,'target_id',x.target_id,
            'namespace_hash',encode(t.namespace_hash,'hex'),'binding_hash',encode(t.binding_hash,'hex'),
            'content_hash',encode(x.content_hash,'hex'),'source_hash',encode(x.source_hash,'hex'),
            'sharing_hash',encode(x.sharing_hash,'hex'))),'UTF8')) END
    FROM employee_reviewed_memory_exports x JOIN employee_reviewed_memory_targets t
        ON t.company_id=x.company_id AND t.id=x.target_id
    WHERE x.company_id=company AND x.fact_id=fact
$$;

CREATE FUNCTION ortak_employee_reviewed_export_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE f employee_reviewed_memory_facts;
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    SELECT * INTO STRICT f FROM employee_reviewed_memory_facts
        WHERE company_id=NEW.company_id AND id=NEW.fact_id;
    PERFORM 1 FROM employee_memory_channel_authorities a WHERE a.company_id=f.company_id
        AND a.community_id=f.community_id AND a.employee_id=f.employee_id
        AND a.channel_id IN(f.source_channel_id,f.destination_channel_id) ORDER BY a.channel_id FOR SHARE;
    PERFORM 1 FROM employee_reviewed_memory_facts v WHERE v.company_id=f.company_id AND v.id=f.id FOR SHARE;
    PERFORM 1 FROM employee_reviewed_memory_targets t WHERE t.company_id=f.company_id AND t.id=NEW.target_id FOR SHARE;
    IF (NEW.community_id,NEW.employee_id,NEW.destination_channel_id,NEW.content_hash,NEW.source_hash,NEW.sharing_hash)
        IS DISTINCT FROM (f.community_id,f.employee_id,f.destination_channel_id,f.content_hash,f.source_hash,f.sharing_hash)
        OR NEW.requested_by<>encode(f.approved_by,'hex')
        OR NOT ortak_employee_reviewed_export_eligible(NEW.company_id,NEW.fact_id,NEW.target_id)
        OR NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_targets t WHERE t.company_id=NEW.company_id AND t.id=NEW.target_id
            AND t.employee_revision_id=NEW.employee_revision_id AND t.employee_lifecycle_epoch=NEW.employee_lifecycle_epoch)
        OR NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id
            AND o.fact_id=NEW.fact_id AND o.actor_pubkey=NEW.requested_by AND o.operation_id=NEW.operation_id
            AND o.action='publish' AND o.result_version=0 AND o.xmin::text::bigint=txid_current()%4294967296)
        OR (SELECT count(*) FROM employee_reviewed_memory_export_jobs j WHERE j.company_id=NEW.company_id
            AND j.fact_id=NEW.fact_id AND j.state='pending' AND j.attempt_count=0
            AND j.xmin::text::bigint=txid_current()%4294967296)<>2 THEN
        RAISE EXCEPTION 'employee memory publication requires current fact and atomic command/jobs' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER employee_reviewed_export_at_commit AFTER INSERT ON employee_reviewed_memory_exports
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_at_commit();

-- Stable two-job journal and lease/receipt transitions adapted from immutable69.
-- Acknowledgements are historical facts, not current source-access checks.
CREATE FUNCTION ortak_employee_reviewed_export_stop() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    UPDATE employee_reviewed_memory_export_jobs SET next_attempt_at=least(next_attempt_at,NEW.revoked_at),updated_at=clock_timestamp()
        WHERE company_id=NEW.company_id AND fact_id=NEW.id AND action='withdraw' AND state='pending';
    RETURN NEW;
END $$;
CREATE TRIGGER employee_reviewed_export_stop AFTER UPDATE ON employee_reviewed_memory_facts FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_stop();

CREATE FUNCTION ortak_employee_reviewed_export_job_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
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
                AND EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f WHERE f.company_id=NEW.company_id AND f.id=NEW.fact_id
                    AND f.revoked_at IS NOT NULL AND NEW.next_attempt_at=least(OLD.next_attempt_at,f.revoked_at)
                    AND f.xmin::text::bigint=txid_current()%4294967296);
            IF NOT coalesce(allowed,false) THEN
                allowed:=OLD.attempt_count=0 AND OLD.lease_token IS NULL
                    AND NEW.lease_token IS NULL AND NEW.last_error_code IS NOT DISTINCT FROM OLD.last_error_code
                    AND NEW.next_attempt_at<=clock_timestamp()
                    AND EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x
                        WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id
                        AND NOT ortak_employee_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id));
            END IF;
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
CREATE TRIGGER employee_reviewed_export_job_guard BEFORE UPDATE ON employee_reviewed_memory_export_jobs FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_job_guard();

CREATE FUNCTION ortak_employee_reviewed_export_job_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x JOIN employee_reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
            WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id AND x.community_id=NEW.community_id
            AND x.xmin::text::bigint=txid_current()%4294967296 AND NEW.state='pending' AND NEW.attempt_count=0
            AND NEW.total_attempts=0 AND NEW.retry_version=0 AND NEW.last_error_code IS NULL
            AND NEW.idempotency_key='employee-reviewed:'||NEW.action||':'||NEW.company_id::text||':'||NEW.fact_id::text
            AND NEW.request_hash=ortak_employee_reviewed_request_hash(NEW.company_id,NEW.fact_id,NEW.action)
            AND NEW.lease_token IS NULL AND ((NEW.action='withdraw' AND NEW.next_attempt_at=f.expires_at)
                OR (NEW.action='publish' AND NEW.next_attempt_at<=clock_timestamp()))) THEN
            RAISE EXCEPTION 'ortak: reviewed job requires atomic publication' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.retry_version<>OLD.retry_version THEN
        IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id AND o.fact_id=NEW.fact_id
            AND o.action='retry_'||NEW.action AND o.result_version=NEW.retry_version AND o.xmin::text::bigint=txid_current()%4294967296) THEN
            RAISE EXCEPTION 'ortak: reviewed retry requires atomic human command' USING ERRCODE='check_violation';
        END IF;
    END IF;
    IF NEW.state='acknowledged' AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_receipts r
        WHERE r.company_id=NEW.company_id AND r.fact_id=NEW.fact_id AND r.action=NEW.action AND r.request_hash=NEW.request_hash
          AND r.community_id=NEW.community_id AND r.lease_token=NEW.lease_token AND r.total_attempts=NEW.total_attempts AND NEW.lease_expires_at>clock_timestamp()
          AND r.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed acknowledgement requires atomic live-lease receipt' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER employee_reviewed_export_job_at_commit AFTER INSERT OR UPDATE ON employee_reviewed_memory_export_jobs DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_job_at_commit();

CREATE FUNCTION ortak_employee_reviewed_export_command_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE f employee_reviewed_memory_facts; expected BYTEA;
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    SELECT * INTO STRICT f FROM employee_reviewed_memory_facts
        WHERE company_id=NEW.company_id AND id=NEW.fact_id;
    expected=sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
        'format','ortak-reviewed-employee-export-command/1','operation_id',NEW.operation_id,
        'fact_id',NEW.fact_id,'action',NEW.action,
        'expected_version',CASE WHEN NEW.action='publish' THEN 1 ELSE NEW.result_version-1 END)),'UTF8'));
    IF NEW.actor_pubkey<>encode(f.approved_by,'hex') OR NEW.community_id<>f.community_id
        OR NEW.request_hash IS DISTINCT FROM expected OR NEW.valid_before IS NULL
        OR NOT coalesce(ortak_employee_memory_command_current(f.company_id,f.employee_id,
            decode(NEW.actor_pubkey,'hex'),NEW.action),false) THEN
        RAISE EXCEPTION 'employee export command lacks current/recovery data authority' USING ERRCODE='check_violation';
    END IF;
    IF NEW.action='retry_publish' AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x
        WHERE x.company_id=f.company_id AND x.fact_id=f.id
            AND ortak_employee_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id)) THEN
        RAISE EXCEPTION 'employee publication retry is no longer eligible' USING ERRCODE='check_violation';
    END IF;
    IF NEW.valid_before IS NOT NULL AND NEW.valid_before<=clock_timestamp() THEN
        RAISE EXCEPTION 'ortak: reviewed command authority expired' USING ERRCODE='serialization_failure';
    END IF;
    IF (NEW.action='publish' AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id
        AND x.operation_id=NEW.operation_id AND x.requested_by=NEW.actor_pubkey AND NEW.result_version=0 AND x.xmin::text::bigint=txid_current()%4294967296))
        OR (NEW.action<>'publish' AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_jobs j WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id
            AND 'retry_'||j.action=NEW.action AND j.retry_version=NEW.result_version AND j.xmin::text::bigint=txid_current()%4294967296)) THEN
        RAISE EXCEPTION 'ortak: reviewed command requires its atomic effect' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER employee_reviewed_export_command_at_commit AFTER INSERT ON employee_reviewed_memory_export_commands DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_command_at_commit();

CREATE FUNCTION ortak_employee_reviewed_export_receipt_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_jobs j
        JOIN employee_reviewed_memory_exports x ON x.company_id=j.company_id AND x.fact_id=j.fact_id
        JOIN employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id AND j.action=NEW.action AND j.community_id=NEW.community_id
        AND j.state='acknowledged' AND j.request_hash=NEW.request_hash AND t.binding_hash=NEW.binding_hash
        AND (NEW.content_hash=x.content_hash OR NEW.content_hash IS NULL AND NEW.action='withdraw'
            AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_receipts p
                WHERE p.company_id=NEW.company_id AND p.fact_id=NEW.fact_id AND p.action='publish'))
        AND j.lease_token=NEW.lease_token AND j.total_attempts=NEW.total_attempts AND j.lease_expires_at>clock_timestamp()
        AND j.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed receipt requires its exact live job' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER employee_reviewed_export_receipt_at_commit AFTER INSERT ON employee_reviewed_memory_export_receipts DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_receipt_at_commit();

-- Called by a future bounded worker scan of initial scheduled withdrawals on
-- current permission/target loss. One fact per transaction; no unbounded source
-- mutation -> exports fanout. The retained initial job already exists, including
-- when publication was never ACKed. Normal retry backoff is not reset.
CREATE FUNCTION ortak_employee_memory_schedule_cleanup(company UUID, fact UUID)
RETURNS BOOLEAN LANGUAGE plpgsql AS $$
DECLARE affected INTEGER;
BEGIN
    PERFORM ortak_lock_office_authority(company);
    UPDATE employee_reviewed_memory_export_jobs j SET next_attempt_at=clock_timestamp(),updated_at=clock_timestamp()
        WHERE j.company_id=company AND j.fact_id=fact AND j.action='withdraw'
            AND j.state='pending' AND j.attempt_count=0 AND j.lease_token IS NULL
            AND j.next_attempt_at>clock_timestamp()
            AND EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x WHERE x.company_id=j.company_id
                AND x.fact_id=j.fact_id AND NOT ortak_employee_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id));
    GET DIAGNOSTICS affected=ROW_COUNT;
    RETURN affected=1;
END $$;

-- Retained epoch mutation uses the same Office absent-row fence as source75.
-- Source events/threads and ordinary members select their exact old/new channel;
-- bot identity/member changes select the whole community because human-vs-agent
-- classification is community-wide. Company/employee changes remain scoped.
-- <=128 retained keys/company, <=256/community. At most two old/new keys on
-- either axis => <=768 union rows; refuse the 769th BEFORE any epoch writes.
-- Only source/destination scope rows change; no fact/receipt bytes or use pins.
CREATE FUNCTION ortak_employee_memory_epoch_mutation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE prior JSONB; proposed JSONB; kind TEXT:=TG_ARGV[0]; reason TEXT:=TG_ARGV[1];
    changed BOOLEAN:=TG_OP<>'UPDATE'; field TEXT; co UUID[]; cm UUID[];
    channels UUID[]; employee_keys TEXT[]; target UUID; keys JSONB; selected JSONB;
    old_identity JSONB; new_identity JSONB;
BEGIN
    IF TG_OP<>'INSERT' THEN prior=to_jsonb(OLD); END IF;
    IF TG_OP<>'DELETE' THEN proposed=to_jsonb(NEW); END IF;
    IF TG_OP='UPDATE' THEN
        FOREACH field IN ARRAY TG_ARGV[2:TG_NARGS-1] LOOP
            IF prior->field IS DISTINCT FROM proposed->field THEN changed=true; EXIT; END IF;
        END LOOP;
        IF kind='office_identity' THEN
            changed=changed OR ((prior->>'verified_at' IS NULL)<>(proposed->>'verified_at' IS NULL));
        END IF;
        IF kind='memory_identity' THEN
            changed=changed OR ((prior->>'validated_at' IS NULL)<>(proposed->>'validated_at' IS NULL));
        END IF;
        IF NOT changed THEN RETURN NEW; END IF;
    END IF;
    IF kind='community' AND coalesce(prior->>'deletion_state','')<>'active' THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='thread' AND TG_OP='INSERT' THEN
        IF ortak_conversation_thread_insert_neutral75(proposed) THEN RETURN NEW; END IF;
        -- A new unrelated reply cannot revoke a running memory consumer while
        -- that consumer is delivering it. Restoration of a referenced anchor
        -- is different, and the existing parent/root indexes bound that lookup.
        IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
            WHERE f.community_id=(proposed->>'community_id')::uuid
                AND f.source_event_id=(proposed->>'event_id')::bytea
                AND f.source_event_created_at=(proposed->>'event_created_at')::timestamptz)
            AND NOT EXISTS(SELECT 1 FROM thread_metadata t
                WHERE t.community_id=(proposed->>'community_id')::uuid
                    AND (t.event_id,t.event_created_at) IS DISTINCT FROM
                        ((proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz)
                    AND ((t.parent_event_id=(proposed->>'event_id')::bytea
                        AND t.parent_event_created_at=(proposed->>'event_created_at')::timestamptz)
                        OR (t.root_event_id=(proposed->>'event_id')::bytea
                        AND t.root_event_created_at=(proposed->>'event_created_at')::timestamptz))) THEN
            RETURN NEW;
        END IF;
    END IF;
    IF kind='inbox' AND coalesce(prior->>'state','')<>'decided'
        AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
            WHERE (f.company_id,f.source_event_id,f.source_event_created_at) IN(
                ((prior->>'company_id')::uuid,(prior->>'event_id')::bytea,(prior->>'event_created_at')::timestamptz),
                ((proposed->>'company_id')::uuid,(proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='user' AND TG_OP IN('INSERT','DELETE')
        AND coalesce(proposed,prior)->>'agent_type' IS NULL
        AND coalesce(proposed,prior)->>'agent_owner_pubkey' IS NULL
        AND coalesce(proposed,prior)->>'deactivated_at' IS NULL THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='employee' AND TG_OP='UPDATE'
        AND (prior->'company_id',prior->'id',prior->'status',prior->'lifecycle_epoch')
            IS NOT DISTINCT FROM
            (proposed->'company_id',proposed->'id',proposed->'status',proposed->'lifecycle_epoch') THEN
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO old_identity
            FROM employee_revisions r WHERE r.company_id=(prior->>'company_id')::uuid
                AND r.employee_id=prior->>'id' AND r.id=(prior->>'active_revision_id')::uuid;
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO new_identity
            FROM employee_revisions r WHERE r.company_id=(proposed->>'company_id')::uuid
                AND r.employee_id=proposed->>'id' AND r.id=(proposed->>'active_revision_id')::uuid;
        IF old_identity IS NOT NULL AND old_identity IS NOT DISTINCT FROM new_identity THEN RETURN NEW; END IF;
    END IF;
    IF kind='memory_identity' AND NOT EXISTS(SELECT 1 FROM employees e
        WHERE (e.company_id,e.id,e.active_revision_id) IN(
            ((prior->>'company_id')::uuid,prior->>'employee_id',(prior->>'revision_id')::uuid),
            ((proposed->>'company_id')::uuid,proposed->>'employee_id',(proposed->>'revision_id')::uuid))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO co FROM (VALUES
        (prior->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END),
        (proposed->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO cm FROM (VALUES
        (prior->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END),
        (proposed->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO channels FROM (VALUES
        (prior->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END),
        (proposed->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v),ARRAY[]::text[]) INTO employee_keys FROM (VALUES
        (prior->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END),
        (proposed->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END)) t(v) WHERE v IS NOT NULL;
    IF kind='office_identity' THEN
        -- A new/removed employee key also changes the community-wide human
        -- classification of that key in other employees' approved sources.
        -- Retire bounded company scopes, not only the binding's employee.
        employee_keys=ARRAY[]::text[];
    END IF;
    IF kind='membership' AND (prior->>'role'='bot' OR proposed->>'role'='bot') THEN
        channels=ARRAY[]::uuid[];
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'employee memory authority requires READ COMMITTED' USING ERRCODE='invalid_transaction_state';
    END IF;
    -- Do not rely only on currently visible retained rows: a first registration
    -- may be in flight. These exclusive try-locks conflict with that shared read.
    FOR target IN SELECT unnest(cm) ORDER BY 1 LOOP
        IF NOT pg_try_advisory_xact_lock(ortak_office_community_lock_key(target)) THEN
            RAISE EXCEPTION 'employee memory community fence busy' USING ERRCODE='serialization_failure';
        END IF;
    END LOOP;
    FOR target IN SELECT unnest(co) ORDER BY 1 LOOP
        IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
            RAISE EXCEPTION 'employee memory company fence busy' USING ERRCODE='serialization_failure';
        END IF;
    END LOOP;
    SELECT coalesce(jsonb_agg(to_jsonb(k) ORDER BY company_id,community_id,employee_id,channel_id),'[]'::jsonb)
        INTO keys FROM (
            SELECT a.company_id,a.community_id,a.employee_id,a.channel_id
            FROM employee_memory_channel_authorities a JOIN communities c ON c.id=a.community_id
            WHERE (a.company_id=ANY(co) OR a.community_id=ANY(cm))
                AND (cardinality(channels)=0 OR a.channel_id=ANY(channels))
                AND (cardinality(employee_keys)=0 OR a.employee_id=ANY(employee_keys))
                AND c.deletion_state='active' AND c.deleted_at IS NULL
            ORDER BY a.company_id,a.community_id,a.employee_id,a.channel_id LIMIT 769
        ) k;
    IF jsonb_array_length(keys)>768 THEN
        RAISE EXCEPTION 'employee memory mutation scope cap exceeded' USING ERRCODE='program_limit_exceeded';
    END IF;
    FOR target IN SELECT DISTINCT (v->>'company_id')::uuid FROM jsonb_array_elements(keys) v ORDER BY 1 LOOP
        IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
            RAISE EXCEPTION 'retained employee memory company fence busy' USING ERRCODE='serialization_failure';
        END IF;
    END LOOP;
    FOR selected IN SELECT value FROM jsonb_array_elements(keys) LOOP
        PERFORM 1 FROM employee_memory_channel_authorities a
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.community_id=(selected->>'community_id')::uuid
                AND a.employee_id=selected->>'employee_id' AND a.channel_id=(selected->>'channel_id')::uuid
            FOR UPDATE NOWAIT;
        UPDATE employee_memory_channel_authorities a SET epoch=epoch+1,reason=TG_ARGV[1]
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.community_id=(selected->>'community_id')::uuid
                AND a.employee_id=selected->>'employee_id' AND a.channel_id=(selected->>'channel_id')::uuid;
    END LOOP;
    RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
END $$;

CREATE TRIGGER employee_memory_epoch_channels AFTER INSERT OR UPDATE OR DELETE ON channels
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('channel','audience_changed',
        'community_id','id','channel_type','visibility','archived_at','deleted_at','participant_hash','ttl_seconds','ttl_deadline');
CREATE TRIGGER employee_memory_epoch_members AFTER INSERT OR UPDATE OR DELETE ON channel_members
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('membership','audience_changed',
        'community_id','channel_id','pubkey','role','removed_at');
CREATE TRIGGER employee_memory_epoch_events AFTER UPDATE OR DELETE ON events
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('event','source_changed',
        'community_id','id','created_at','pubkey','kind','tags','content','sig','channel_id','deleted_at');
CREATE TRIGGER employee_memory_epoch_threads AFTER INSERT OR UPDATE OR DELETE ON thread_metadata
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('thread','source_changed',
        'community_id','event_id','event_created_at','channel_id','parent_event_id','parent_event_created_at',
        'root_event_id','root_event_created_at','depth');
CREATE TRIGGER employee_memory_epoch_inbox AFTER UPDATE OR DELETE ON office_inbox
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('inbox','source_changed',
        'company_id','event_id','event_created_at','event_kind','author_pubkey','channel_id','state');
CREATE TRIGGER employee_memory_epoch_users AFTER INSERT OR UPDATE OR DELETE ON users
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('user','identity_changed',
        'community_id','pubkey','agent_type','agent_owner_pubkey','deactivated_at');
CREATE TRIGGER employee_memory_epoch_employees AFTER INSERT OR UPDATE OR DELETE ON employees
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('employee','identity_changed',
        'company_id','id','status','active_revision_id','lifecycle_epoch');
CREATE TRIGGER employee_memory_epoch_office_identity AFTER INSERT OR UPDATE OR DELETE ON employee_office_bindings
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('office_identity','identity_changed',
        'company_id','employee_id','public_key','signer_ref','valid_from','valid_until');
CREATE TRIGGER employee_memory_epoch_memory_identity AFTER INSERT OR UPDATE OR DELETE ON employee_memory_bindings
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('memory_identity','identity_changed',
        'company_id','employee_id','revision_id','adapter','endpoint_ref','workspace','user_peer','employee_peer','options');
CREATE TRIGGER employee_memory_epoch_companies AFTER UPDATE OR DELETE ON companies
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('company','scope_closed','id','status');
-- Same ordering requirement as source75: advance while the community is active,
-- after the Office exclusive fence and before universal quiescing closes writes.
CREATE TRIGGER ortak_z_employee_memory_epoch_communities BEFORE UPDATE OR DELETE ON communities
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('community','scope_closed',
        'id','deletion_state','deletion_fence_generation','deleted_at');
CREATE TRIGGER employee_memory_epoch_company_bindings AFTER INSERT OR UPDATE OR DELETE ON office_company_bindings
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('company_binding','scope_closed','company_id','community_id');

-- Retained history: no table cascades from event/channel/member deletion.
-- Universal community fences are NOT bypassed for cleanup: canonical deletion
-- must require exact withdrawal ACKs and no uncertain publish leases BEFORE
-- quiescing. Backup may retain future scheduled withdrawals as obligations.
DO $$ DECLARE relation TEXT; BEGIN
    FOREACH relation IN ARRAY ARRAY['employee_memory_channel_authorities','employee_reviewed_memory_facts',
        'employee_reviewed_memory_operations','employee_reviewed_memory_targets','employee_reviewed_memory_exports',
        'employee_reviewed_memory_export_jobs','employee_reviewed_memory_export_commands',
        'employee_reviewed_memory_export_receipts'] LOOP
        EXECUTE format('CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON %I FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()',relation);
        EXECUTE format('CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON %I FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()',relation);
        PERFORM attach_community_write_fence(relation);
    END LOOP;
    FOREACH relation IN ARRAY ARRAY['employee_reviewed_memory_operations','employee_reviewed_memory_exports',
        'employee_reviewed_memory_export_commands','employee_reviewed_memory_export_receipts'] LOOP
        EXECUTE format('CREATE TRIGGER employee_memory_immutable BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()',relation);
    END LOOP;
END $$;
