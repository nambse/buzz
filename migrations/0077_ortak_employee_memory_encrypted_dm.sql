-- Ortak77: reviewed employee memory, protected two-party DM and routing notifications.
-- Additive to immutable1..76. Explicit runtime/pair/native selections remain required.
-- The source-fragment comments below describe development provenance; this numbered
-- assembly is the deployment contract. No provider key or activation is installed.

-- Source: docs/ortak/sql/employee_reviewed_memory_candidate.sql
-- SHA256: 40b7fa092678b82488a1f837efd424006231a5d6f96a7b718a178b0f905c75fc
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

-- Source: docs/ortak/sql/employee_reviewed_memory_authority_candidate.sql
-- SHA256: e942095e716b6a53fdf4c1900dd92e659cf3cc6e38d927cf69cb8d59d83a780b
-- SOURCE ONLY: concatenate AFTER employee_reviewed_memory_candidate.sql.
-- No numbered migration, SQL execution or deployment in this source slice.
-- Replaces canonical observation and current-data command checks. Real signed
-- authorization lives in the private server employee_memory facade; target
-- ownership/protocol remains the earlier independently closed port.
-- Current-table reads cannot verify a NIP-98 signature or server-configured
-- HumanGrant ceiling, and this file deliberately invents no SQL request GUC.

-- Exact private evidence preimage for the v1 employee source contract. The
-- existing Rust EmployeeMemorySourceV1 accepts its digest, not the raw source.
-- All keys are lexical, compact UTF8; content/tags preserve bytes/order. The
-- format and author_public_key spelling differ deliberately from conversation75.
-- This helper proves structure/encoding only, not event authenticity or access.
CREATE FUNCTION ortak_employee_memory_evidence_bytes(
    company UUID, community UUID, channel UUID, event_id BYTEA,
    event_created_at TIMESTAMPTZ, author BYTEA, event_kind INTEGER,
    signature BYTEA, tags JSONB, content TEXT
) RETURNS BYTEA LANGUAGE plpgsql IMMUTABLE SECURITY INVOKER PARALLEL SAFE
SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE tag JSONB; part JSONB; encoded TEXT;
BEGIN
    IF company IS NULL OR community IS NULL OR channel IS NULL
        OR company='00000000-0000-0000-0000-000000000000'::uuid
        OR community='00000000-0000-0000-0000-000000000000'::uuid
        OR channel='00000000-0000-0000-0000-000000000000'::uuid
        OR event_id IS NULL OR octet_length(event_id)<>32
        OR public.ortak_employee_memory_timestamp(event_created_at) IS NULL
        OR author IS NULL OR octet_length(author)<>32
        OR event_kind IS NULL OR event_kind NOT IN(9,40002)
        OR signature IS NULL OR octet_length(signature)<>64
        OR tags IS NULL OR jsonb_typeof(tags)<>'array' OR octet_length(tags::text)>16384
        OR content IS NULL OR octet_length(content)>65536 THEN RETURN NULL; END IF;
    FOR tag IN SELECT value FROM jsonb_array_elements(tags) LOOP
        IF jsonb_typeof(tag)<>'array' THEN RETURN NULL; END IF;
        FOR part IN SELECT value FROM jsonb_array_elements(tag) LOOP
            IF jsonb_typeof(part)<>'string' THEN RETURN NULL; END IF;
        END LOOP;
    END LOOP;
    encoded=public.ortak_conversation_json75(jsonb_build_object(
        'author_public_key',encode(author,'hex'),'channel_id',channel,
        'community_id',community,'company_id',company,'content',content,
        'event_created_at',public.ortak_employee_memory_timestamp(event_created_at),
        'event_id',encode(event_id,'hex'),'format','ortak-reviewed-employee-evidence/1',
        'kind',event_kind,'sig',encode(signature,'hex'),'tags',tags));
    IF encoded IS NULL OR octet_length(encoded)>524288 THEN RETURN NULL; END IF;
    RETURN convert_to(encoded,'UTF8');
END $$;

-- Read observation, NOT approval, source-sharing permission or run authority.
-- Actual caller: server-authenticated actor + current explicit employee AND
-- both channel ceilings, under a caller-owned bounded transaction/deadline.
-- Durable callers acquire the shared Office fence in a SEPARATE statement
-- before this STABLE one-snapshot read, keep it through commit, and recheck
-- returned valid_before with clock_timestamp(). No lock is acquired here.
CREATE OR REPLACE FUNCTION ortak_employee_memory_observation(
    company UUID, employee TEXT, actor BYTEA, source_id BYTEA,
    source_created_at TIMESTAMPTZ, destination_channel UUID,
    memory_kind TEXT, relationship_human BYTEA
) RETURNS TABLE(community_id UUID, source_channel_id UUID,
    source_author_public_key BYTEA, source_evidence_hash BYTEA,
    employee_revision_id UUID, employee_lifecycle_epoch BIGINT,
    observed_at TIMESTAMPTZ, valid_before TIMESTAMPTZ)
LANGUAGE plpgsql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE node RECORD; first_node RECORD; count_nodes INTEGER:=0;
    seen BYTEA[]:=ARRAY[]::bytea[]; expected_parent BYTEA;
    expected_parent_at TIMESTAMPTZ; expected_depth INTEGER;
    expected_root BYTEA; expected_root_at TIMESTAMPTZ;
    resolved_root BYTEA; resolved_root_at TIMESTAMPTZ;
    tag JSONB; part JSONB; marker TEXT; reference_id BYTEA;
    claimed_root BYTEA; claimed_parent BYTEA; effective_depth INTEGER;
    evidence BYTEA;
BEGIN
    IF company IS NULL OR company='00000000-0000-0000-0000-000000000000'::uuid
        OR employee IS NULL OR employee COLLATE "C" !~ '^[a-z0-9][a-z0-9_-]{0,63}$'
        OR octet_length(employee) NOT BETWEEN 1 AND 64
        OR actor IS NULL OR octet_length(actor)<>32
        OR source_id IS NULL OR octet_length(source_id)<>32
        OR public.ortak_employee_memory_timestamp(source_created_at) IS NULL
        OR destination_channel IS NULL
        OR destination_channel='00000000-0000-0000-0000-000000000000'::uuid
        OR memory_kind IS NULL OR memory_kind NOT IN('experience','relationship')
        OR (memory_kind='experience' AND relationship_human IS NOT NULL)
        OR (memory_kind='relationship' AND relationship_human IS DISTINCT FROM actor) THEN RETURN; END IF;

    FOR node IN
      WITH RECURSIVE selection AS MATERIALIZED (
        SELECT ob.community_id,i.channel_id,i.event_created_at,i.event_kind,i.author_pubkey,
            e.active_revision_id,e.lifecycle_epoch,b.public_key AS employee_key,
            b.valid_until AS identity_valid_before,statement_timestamp() AS observed_at
        FROM public.companies co
        JOIN public.office_company_bindings ob ON ob.company_id=co.id
        JOIN public.communities cm ON cm.id=ob.community_id
            AND cm.deletion_state='active' AND cm.deleted_at IS NULL
        JOIN public.employees e ON e.company_id=co.id AND e.id=$2 AND e.status='active'
        JOIN public.employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
        JOIN public.employee_office_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id
            AND encode(b.public_key,'hex')=r.manifest#>>'{office,public_key}'
            AND b.signer_ref=r.manifest#>>'{office,signer_ref}' AND b.verified_at IS NOT NULL
            AND b.valid_from<=statement_timestamp()
            AND (b.valid_until IS NULL OR b.valid_until>statement_timestamp())
        JOIN public.office_inbox i ON i.company_id=co.id AND i.event_id=$4
            AND i.event_created_at=$5 AND i.state='decided' AND i.author_pubkey=$3 AND i.event_kind IN(9,40002)
        WHERE co.id=$1 AND co.status='active' AND e.lifecycle_epoch>=0 AND b.public_key<>$3
            AND octet_length(b.public_key)=32
            AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=$3
                AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
            AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings eb WHERE eb.company_id=co.id AND eb.public_key=$3)
            AND NOT EXISTS(SELECT 1 FROM public.channel_members bot WHERE bot.community_id=cm.id AND bot.pubkey=$3 AND bot.role='bot')
            AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=b.public_key
                AND u.deactivated_at IS NOT NULL)
      ), accepted_channels AS MATERIALIZED (
        SELECT ch.id,ch.ttl_deadline
        FROM selection s JOIN public.channels ch ON ch.community_id=s.community_id AND ch.id IN(s.channel_id,$6)
        JOIN public.channel_members human_member ON human_member.community_id=ch.community_id
            AND human_member.channel_id=ch.id AND human_member.pubkey=$3
            AND human_member.removed_at IS NULL AND human_member.role<>'bot'
        JOIN public.channel_members employee_member ON employee_member.community_id=ch.community_id
            AND employee_member.channel_id=ch.id AND employee_member.pubkey=s.employee_key AND employee_member.removed_at IS NULL
        WHERE ch.archived_at IS NULL AND ch.deleted_at IS NULL
            AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>statement_timestamp())
            AND (ch.channel_type='stream' OR (
                ch.channel_type='dm' AND ch.visibility='private'
                -- Same binary sorted retained-pair recipe as direct_channel_on.
                -- Both exact keys already have current rows above; counting ALL
                -- retained rows (including removed) refuses a third/replaced key.
                AND ch.participant_hash=sha256(CASE WHEN $3<s.employee_key
                    THEN $3||s.employee_key ELSE s.employee_key||$3 END)
                AND (SELECT count(*) FROM (SELECT m.pubkey FROM public.channel_members m
                    WHERE m.community_id=ch.community_id AND m.channel_id=ch.id ORDER BY m.pubkey LIMIT 3) retained)=2))
      ), visible AS MATERIALIZED (
        SELECT s.*,least(src.ttl_deadline,dst.ttl_deadline,s.identity_valid_before) AS valid_before
        FROM selection s JOIN accepted_channels src ON src.id=s.channel_id
        JOIN accepted_channels dst ON dst.id=$6
      ), source AS MATERIALIZED (
        SELECT e.id,e.created_at,e.content,e.pubkey,e.kind,e.sig,v.*
        FROM visible v JOIN public.events e ON e.community_id=v.community_id
            AND e.id=$4 AND e.created_at=v.event_created_at
            AND e.channel_id=v.channel_id AND e.kind=v.event_kind AND e.pubkey=v.author_pubkey
        WHERE e.deleted_at IS NULL AND e.kind IN(9,40002) AND e.pubkey=$3
            AND octet_length(e.content)<=65536 AND octet_length(e.pubkey)=32 AND octet_length(e.sig)=64
      ), ancestry AS (
        SELECT 0 AS hop,e.id,e.created_at,
            CASE WHEN octet_length(e.tags::text)<=16384 THEN e.tags END AS tags,
            t.event_id IS NOT NULL AS metadata_present,t.channel_id AS metadata_channel,
            t.parent_event_id,t.parent_event_created_at,t.root_event_id,t.root_event_created_at,t.depth
        FROM source s JOIN public.events e ON e.community_id=s.community_id AND e.id=s.id AND e.created_at=s.created_at
        LEFT JOIN public.thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
        UNION ALL
        SELECT a.hop+1,e.id,e.created_at,
            CASE WHEN octet_length(e.tags::text)<=16384 THEN e.tags END,
            t.event_id IS NOT NULL,t.channel_id,t.parent_event_id,t.parent_event_created_at,
            t.root_event_id,t.root_event_created_at,t.depth
        FROM ancestry a JOIN public.events e ON e.community_id=(SELECT s.community_id FROM source s)
            AND e.id=a.parent_event_id AND e.created_at=a.parent_event_created_at
            AND e.channel_id=(SELECT s.channel_id FROM source s) AND e.deleted_at IS NULL AND e.kind IN(9,40002)
        LEFT JOIN public.thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
        WHERE a.hop<32
      )
      SELECT a.*,s.community_id,s.channel_id,s.active_revision_id,s.lifecycle_epoch,s.observed_at,s.valid_before,
        CASE WHEN a.hop=0 THEN s.content END AS source_content,
        CASE WHEN a.hop=0 THEN s.pubkey END AS source_author,
        CASE WHEN a.hop=0 THEN s.sig END AS source_signature,s.kind AS source_kind
      FROM ancestry a CROSS JOIN source s ORDER BY a.hop LIMIT 33
    LOOP
        IF node.hop <> count_nodes OR octet_length(node.id) <> 32
           OR node.id = ANY(seen)
           OR NOT isfinite(node.created_at)
           OR node.created_at < '1970-01-01 00:00:00+00'::timestamptz
           OR node.created_at >= '10000-01-01 00:00:00+00'::timestamptz
           OR node.tags IS NULL OR jsonb_typeof(node.tags) <> 'array' THEN RETURN; END IF;
        seen := array_append(seen,node.id);
        IF count_nodes=0 THEN
            first_node := node;
            IF node.community_id = '00000000-0000-0000-0000-000000000000'::uuid
               OR node.channel_id = '00000000-0000-0000-0000-000000000000'::uuid THEN RETURN; END IF;
        ELSE
            IF expected_parent IS DISTINCT FROM node.id
               OR expected_parent_at IS DISTINCT FROM node.created_at THEN RETURN; END IF;
        END IF;

        -- Vec<Vec<String>> parity: even non-e tags must be arrays of strings.
        claimed_root := NULL; claimed_parent := NULL;
        FOR tag IN SELECT t.value FROM jsonb_array_elements(node.tags) AS t(value) LOOP
            IF jsonb_typeof(tag) <> 'array' THEN RETURN; END IF;
            FOR part IN SELECT t.value FROM jsonb_array_elements(tag) AS t(value) LOOP
                IF jsonb_typeof(part) <> 'string' THEN RETURN; END IF;
            END LOOP;
            IF tag->>0 IS DISTINCT FROM 'e' THEN CONTINUE; END IF;
            IF jsonb_array_length(tag)<4 OR octet_length(tag->>1)<>64
               OR (tag->>1) COLLATE "C" !~ '^[0-9a-fA-F]{64}$' THEN RETURN; END IF;
            reference_id := decode(tag->>1,'hex');
            marker := tag->>3;
            CASE marker
            WHEN 'root' THEN
                IF claimed_root IS NOT NULL THEN RETURN; END IF;
                claimed_root := reference_id;
            WHEN 'reply' THEN
                IF claimed_parent IS NOT NULL THEN RETURN; END IF;
                claimed_parent := reference_id;
            WHEN 'mention' THEN CONTINUE;
            ELSE RETURN;
            END CASE;
        END LOOP;
        IF claimed_root IS NOT NULL AND claimed_parent IS NULL THEN RETURN; END IF;
        claimed_root := coalesce(claimed_root,claimed_parent);

        -- Both locator halves are required, including exact UTC partition time.
        IF (node.parent_event_id IS NULL) <> (node.parent_event_created_at IS NULL)
           OR (node.root_event_id IS NULL) <> (node.root_event_created_at IS NULL) THEN RETURN; END IF;
        IF node.parent_event_id IS NOT NULL AND (octet_length(node.parent_event_id)<>32
           OR NOT isfinite(node.parent_event_created_at)
           OR node.parent_event_created_at<'1970-01-01 00:00:00+00'::timestamptz
           OR node.parent_event_created_at>='10000-01-01 00:00:00+00'::timestamptz) THEN RETURN; END IF;
        IF node.root_event_id IS NOT NULL AND (octet_length(node.root_event_id)<>32
           OR NOT isfinite(node.root_event_created_at)
           OR node.root_event_created_at<'1970-01-01 00:00:00+00'::timestamptz
           OR node.root_event_created_at>='10000-01-01 00:00:00+00'::timestamptz) THEN RETURN; END IF;

        effective_depth := coalesce(node.depth,0);
        IF node.metadata_present THEN
            IF node.metadata_channel IS DISTINCT FROM first_node.channel_id THEN RETURN; END IF;
            IF node.parent_event_id IS NULL AND node.depth=0 AND claimed_parent IS NULL THEN
                IF node.root_event_id IS NOT NULL AND
                   (node.root_event_id IS DISTINCT FROM node.id OR node.root_event_created_at IS DISTINCT FROM node.created_at) THEN RETURN; END IF;
            ELSIF node.parent_event_id IS NOT NULL AND node.root_event_id IS NOT NULL
                  AND node.depth BETWEEN 1 AND 32
                  AND claimed_parent=node.parent_event_id AND claimed_root=node.root_event_id THEN
                NULL;
            ELSE RETURN;
            END IF;
        ELSIF node.parent_event_id IS NOT NULL OR node.root_event_id IS NOT NULL
              OR node.depth IS NOT NULL OR claimed_parent IS NOT NULL THEN RETURN;
        END IF;
        IF count_nodes>0 AND expected_depth IS DISTINCT FROM effective_depth THEN RETURN; END IF;
        IF node.parent_event_id IS NOT NULL THEN
            IF count_nodes=0 THEN
                expected_root := node.root_event_id;
                expected_root_at := node.root_event_created_at;
            ELSIF node.root_event_id IS DISTINCT FROM expected_root
                  OR node.root_event_created_at IS DISTINCT FROM expected_root_at THEN RETURN;
            END IF;
        ELSE
            IF expected_root IS NOT NULL AND (expected_root IS DISTINCT FROM node.id
               OR expected_root_at IS DISTINCT FROM node.created_at) THEN RETURN; END IF;
            resolved_root := node.id; resolved_root_at := node.created_at;
        END IF;
        expected_parent := node.parent_event_id;
        expected_parent_at := node.parent_event_created_at;
        expected_depth := effective_depth-1;
        count_nodes := count_nodes+1;
    END LOOP;
    -- A missing/deleted/cross-channel parent, cycle or 33rd edge cannot become
    -- a top-level fallback. Every nonterminal depth decreases to an actual root.
    IF count_nodes=0 OR expected_parent IS NOT NULL OR resolved_root IS NULL THEN RETURN; END IF;

    -- Exact original source locator, never the resolved ancestry root. The root
    -- above establishes consistency; it is not an employee audience field.
    evidence=public.ortak_employee_memory_evidence_bytes($1,first_node.community_id,
        first_node.channel_id,first_node.id,first_node.created_at,first_node.source_author,
        first_node.source_kind,first_node.source_signature,first_node.tags,first_node.source_content);
    IF evidence IS NULL OR first_node.created_at IS DISTINCT FROM $5
        OR first_node.source_author IS DISTINCT FROM $3 THEN RETURN; END IF;
    community_id=first_node.community_id;
    source_channel_id=first_node.channel_id;
    source_author_public_key=first_node.source_author;
    source_evidence_hash=sha256(evidence);
    employee_revision_id=first_node.active_revision_id;
    employee_lifecycle_epoch=first_node.lifecycle_epoch;
    observed_at=first_node.observed_at;
    valid_before=first_node.valid_before;
    -- Statement time pins one read snapshot; wall time can pass its deadline
    -- during a bounded ancestry walk. The final caller still checks at commit.
    IF valid_before IS NOT NULL AND valid_before<=clock_timestamp() THEN RETURN; END IF;
    RETURN NEXT;
END $$;

-- Still CLOSED, by design. Existing server/auth.rs verifies NIP-98 signature,
-- Host + URL + method + payload hash + time window, enforces replay through
-- the configured replay store and loads HumanGrant from private server config.
-- Principal.grant.employee_ids/channel_ids are not authoritative SQL rows.
-- HumanGrant::Role is Reader/Operator (read/cancel), with separate project and
-- provisioning flags; none is an existing employee-memory sharing capability.
-- work::authorized builds ApiWorkPrincipal from those server-owned values;
-- its project reviewer role is not a grant for this genuine employee scope.
-- Authentication lives in the private Principal-only employee_memory facade:
-- genuine NIP-98, current deployment capability, employee/channel ceilings and
-- original-approver recovery. This SECURITY INVOKER predicate checks only current
-- relational facts under the caller's prior Office fence. It does NOT authenticate
-- a SQL-credential holder, a caller-supplied actor, auth_event_id, hash or GUC.
-- Historical receipts are not revalidated as current commands on read/restore.
CREATE OR REPLACE FUNCTION ortak_employee_memory_command_current(
    company UUID, employee TEXT, actor BYTEA, action TEXT
) RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT coalesce($1 IS NOT NULL AND $2 IS NOT NULL AND octet_length($3)=32
        AND $4 IN('approve','stop','publish','retry_publish','retry_withdraw')
        AND EXISTS(
            SELECT 1 FROM public.companies co
            JOIN public.office_company_bindings b ON b.company_id=co.id
            JOIN public.communities cm ON cm.id=b.community_id
            JOIN public.employees e ON e.company_id=co.id AND e.id=$2
            WHERE co.id=$1 AND co.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL
                AND ($4 IN('stop','retry_withdraw') OR (e.status='active' AND e.active_revision_id IS NOT NULL))
                AND (EXISTS(SELECT 1 FROM public.relay_members rm WHERE rm.community_id=cm.id AND rm.pubkey=encode($3,'hex'))
                    OR EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=cm.id AND m.pubkey=$3 AND m.removed_at IS NULL))
                AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=$3
                    AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
                AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings eb WHERE eb.company_id=co.id AND eb.public_key=$3)
                AND NOT EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=cm.id AND m.pubkey=$3 AND m.role='bot')
        ),false)
$$;

-- Canonical source/authority integration requirements (no executor in this file):
-- * Actor must be authenticated, current configured employee ceiling contains
--   employee, and source + destination are in the current configured channel
--   ceiling. The resolver cannot substitute membership for these explicit grants.
-- * READ COMMITTED, finite caller statement/lock timeout, shared Office fence
--   acquired in a prior statement; observation + scope registration + immutable
--   effect/receipt + final clock/currentness check share that transaction.
-- * The storage candidate advances source/destination epochs on channel TTL,
--   membership removal/restoration/bot classification, canonical source/thread/
--   decided inbox mutation, Office/memory identity and employee lifecycle changes.
--   Model-only revision with unchanged Office/memory/lifecycle preserves identity.
-- * Time-only source/destination TTL and Office binding expiry are carried in
--   valid_before, capped into the new fact's expiry and rechecked with DB clock.
--   A historical valid_before, hash, revision or epoch is never a lock or grant.
-- * Old source partition IDs, audience/provenance bytes and operation receipts
--   remain readable as structure after source loss; this function returns zero
--   rows then. It does not authorize disclosure of retained content or cleanup.
-- * Root's execution gate must bind the real signed facade and SQL observation, plus
--   exact pure vectors in employee_reviewed_memory_authority_vectors.json.
--   Pure existing cc-author/bb-approver claim vectors stay unchanged and must
--   fail this stricter own-source policy; structural validity is not sharing.

-- Source: docs/ortak/sql/employee_reviewed_memory_protocol_candidate.sql
-- SHA256: a3dc4ebe89423291f9ead5ae59f4071c4594d32dd5978b0ac7b64439649bbcae
-- SOURCE ONLY, unnumbered. Assemble after the storage, canonical source and
-- authority candidates. No deployed protocol or runtime namespace is enabled.
-- The trusted application boundary is explicit: SQL checks current relational
-- facts and immutable metadata, not authenticity against SQL-credential holders.
-- The only initial registration caller accepts a private adapter-minted witness
-- after one synthetic write/read/confirmed cleanup. No GUC or caller JSON can
-- mint that Rust value. Current refresh uses read-only original ownership.

ALTER TABLE employee_reviewed_memory_targets ADD COLUMN registration_receipt JSONB NOT NULL
    CHECK(jsonb_typeof(registration_receipt)='object' AND octet_length(registration_receipt::text)<=4096);

CREATE OR REPLACE FUNCTION ortak_employee_memory_target_authorized(
    company UUID, employee TEXT, deployment UUID, namespace_bytes BYTEA,
    binding JSONB, creation_receipt JSONB, revision UUID, lifecycle BIGINT,
    destination UUID, valid_until TIMESTAMPTZ
) RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT coalesce($3 IS NOT NULL AND $3<>'00000000-0000-0000-0000-000000000000'::uuid
        AND $10>clock_timestamp() AND public.ortak_employee_memory_timestamp($10) IS NOT NULL
        AND $6->>'company_id'=$1::text AND $6->>'employee_id'=$2 AND $6->>'deployment_id'=$3::text
        AND $6->'binding'=$5 AND $6->>'protocol'='reviewed-employee/1'
        AND $6->>'namespace_hash'=encode(sha256($4),'hex')
        AND $6->>'request_hash' ~ '^[0-9a-f]{64}$' AND jsonb_typeof($6->'native_ids')='object'
        AND EXISTS(SELECT 1 FROM public.companies co
            JOIN public.office_company_bindings ob ON ob.company_id=co.id
            JOIN public.communities cm ON cm.id=ob.community_id
            JOIN public.employees e ON e.company_id=co.id AND e.id=$2
            JOIN public.employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
            JOIN public.employee_memory_bindings mb ON mb.company_id=e.company_id AND mb.employee_id=e.id AND mb.revision_id=r.id
            JOIN public.employee_office_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id
                AND encode(b.public_key,'hex')=r.manifest#>>'{office,public_key}' AND b.signer_ref=r.manifest#>>'{office,signer_ref}'
            JOIN public.channels ch ON ch.community_id=cm.id AND ch.id=$9
            JOIN public.channel_members member ON member.community_id=cm.id AND member.channel_id=ch.id
                AND member.pubkey=b.public_key AND member.removed_at IS NULL
            WHERE co.id=$1 AND co.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL
                AND e.status='active' AND r.id=$7 AND e.lifecycle_epoch=$8 AND mb.validated_at IS NOT NULL
                AND b.verified_at IS NOT NULL AND b.valid_from<=clock_timestamp()
                AND (b.valid_until IS NULL OR b.valid_until>clock_timestamp())
                AND $5=r.manifest->'memory' AND $5=jsonb_build_object('adapter',mb.adapter,'endpoint_ref',mb.endpoint_ref,
                    'workspace',mb.workspace,'user_peer',mb.user_peer,'employee_peer',mb.employee_peer,'options',mb.options)
                AND $5->>'adapter'='honcho' AND $5->'options'='{}'::jsonb
                AND ch.archived_at IS NULL AND ch.deleted_at IS NULL AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>clock_timestamp())
                AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=b.public_key AND u.deactivated_at IS NOT NULL)
                AND (ch.channel_type='stream' OR (ch.channel_type='dm' AND ch.visibility='private'
                    AND (SELECT count(*) FROM (SELECT m.pubkey FROM public.channel_members m WHERE m.community_id=cm.id AND m.channel_id=ch.id LIMIT 3) all_members)=2
                    AND EXISTS(SELECT 1 FROM public.channel_members h WHERE h.community_id=cm.id AND h.channel_id=ch.id
                        AND h.pubkey<>b.public_key AND h.removed_at IS NULL AND h.role<>'bot'
                        AND ch.participant_hash=sha256(CASE WHEN h.pubkey<b.public_key THEN h.pubkey||b.public_key ELSE b.public_key||h.pubkey END)
                        AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings eb WHERE eb.company_id=co.id AND eb.public_key=h.pubkey)
                        AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=h.pubkey
                            AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL)))))
        ),false)
$$;

CREATE OR REPLACE FUNCTION ortak_employee_memory_target_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE expected_namespace BYTEA; expected_binding BYTEA; registration JSONB; diagnostic JSONB;
    observed TIMESTAMPTZ; cleanup_hash TEXT;
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    expected_namespace=convert_to(ortak_conversation_json75(jsonb_build_object(
        'format','ortak-reviewed-employee-namespace/1','company_id',NEW.company_id,'employee_id',NEW.employee_id)),'UTF8');
    expected_binding=sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
        'binding',NEW.binding,'namespace_hash',encode(NEW.namespace_hash,'hex'),'protocol',NEW.protocol)),'UTF8'));
    IF NEW.namespace_bytes IS DISTINCT FROM expected_namespace OR NEW.binding_hash IS DISTINCT FROM expected_binding THEN
        RAISE EXCEPTION 'employee memory target namespace differs' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='INSERT' THEN
        registration=NEW.registration_receipt; diagnostic=registration->'diagnostic';
        IF jsonb_typeof(registration)<>'object' OR (SELECT count(*) FROM jsonb_object_keys(registration))<>3
            OR registration->>'format' IS DISTINCT FROM 'ortak-employee-namespace-registration/1'
            OR jsonb_typeof(diagnostic)<>'object' OR (SELECT count(*) FROM jsonb_object_keys(diagnostic))<>8
            OR diagnostic->>'operation_id' IS NULL OR diagnostic->>'employee_revision_id' IS DISTINCT FROM NEW.employee_revision_id::text
            OR diagnostic->>'employee_lifecycle_epoch' IS DISTINCT FROM NEW.employee_lifecycle_epoch::text
            OR diagnostic->>'erased' IS DISTINCT FROM 'true'
            OR NOT coalesce(diagnostic->>'challenge_hash' ~ '^[0-9a-f]{64}$',false)
            OR NOT coalesce(diagnostic->>'write_request_hash' ~ '^[0-9a-f]{64}$',false)
            OR NOT coalesce(diagnostic->>'withdraw_request_hash' ~ '^[0-9a-f]{64}$',false)
            OR diagnostic->>'tombstone_at' IS NULL OR registration->>'validated_at' IS NULL THEN
            RAISE EXCEPTION 'employee namespace registration metadata invalid' USING ERRCODE='check_violation';
        END IF;
        observed=(registration->>'validated_at')::timestamptz;
        IF (diagnostic->>'operation_id')::uuid='00000000-0000-0000-0000-000000000000'::uuid
            OR ortak_employee_memory_timestamp(observed) IS DISTINCT FROM registration->>'validated_at'
            OR ortak_employee_memory_timestamp((diagnostic->>'tombstone_at')::timestamptz) IS NULL
            OR observed>clock_timestamp()+interval '5 seconds' OR observed<=clock_timestamp()-interval '55 seconds'
            OR NEW.valid_until<=clock_timestamp() OR NEW.valid_until>observed+interval '90 days'
            OR NEW.consumption_epoch<>0 THEN
            RAISE EXCEPTION 'employee namespace initial witness expired or selection invalid' USING ERRCODE='check_violation';
        END IF;
        cleanup_hash=encode(sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
            'format','ortak-reviewed-employee-diagnostic-withdraw/1','operation_id',(diagnostic->>'operation_id')::uuid,
            'namespace_hash',encode(NEW.namespace_hash,'hex'),'binding_hash',encode(NEW.binding_hash,'hex'),
            'employee_revision_id',NEW.employee_revision_id,'employee_lifecycle_epoch',NEW.employee_lifecycle_epoch,
            'challenge_hash',diagnostic->>'challenge_hash')),'UTF8')),'hex');
        IF diagnostic->>'withdraw_request_hash' IS DISTINCT FROM cleanup_hash THEN
            RAISE EXCEPTION 'employee namespace cleanup commitment differs' USING ERRCODE='check_violation';
        END IF;
    ELSE
        -- Includes registration receipt and original selection expiry. A model
        -- refresh cannot create ownership, renew an expired selection or rewrite
        -- the original I/O evidence. Explicit future renewal is a separate API.
        IF (to_jsonb(NEW)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'updated_at'-'consumption_epoch')
            IS DISTINCT FROM (to_jsonb(OLD)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'updated_at'-'consumption_epoch')
            OR NEW.consumption_epoch<>OLD.consumption_epoch THEN
            RAISE EXCEPTION 'employee memory target identity is immutable' USING ERRCODE='check_violation';
        END IF;
        IF (NEW.enabled,NEW.employee_lifecycle_epoch) IS DISTINCT FROM (OLD.enabled,OLD.employee_lifecycle_epoch) THEN
            IF OLD.consumption_epoch=9223372036854775807 THEN
                RAISE EXCEPTION 'employee memory target epoch exhausted' USING ERRCODE='program_limit_exceeded';
            END IF;
            NEW.consumption_epoch=OLD.consumption_epoch+1;
        END IF;
    END IF;
    IF (TG_OP='INSERT' OR NEW.enabled) AND NOT coalesce(ortak_employee_memory_target_authorized(
        NEW.company_id,NEW.employee_id,NEW.deployment_id,NEW.namespace_bytes,NEW.binding,NEW.creation_receipt,
        NEW.employee_revision_id,NEW.employee_lifecycle_epoch,NEW.destination_channel_id,NEW.valid_until),false) THEN
        RAISE EXCEPTION 'employee namespace current binding unavailable' USING ERRCODE='check_violation';
    END IF;
    NEW.updated_at=clock_timestamp(); RETURN NEW;
END $$;

-- Expiry hides text but is not erasure. Only a confirmed tombstone proves this
-- remote store's text deletion. Identify the old two-column CHECK exactly; do
-- not drop an unknown receipt/lease/content guard during candidate assembly.
DO $$ DECLARE guard_name TEXT; found_count INTEGER; expected SMALLINT[];
BEGIN
    SELECT array_agg(attnum ORDER BY attnum) INTO expected FROM pg_attribute
        WHERE attrelid='employee_reviewed_memory_export_receipts'::regclass AND attname IN('remote_status','erased_from_reviewed_store');
    SELECT count(*),min(conname) INTO found_count,guard_name FROM pg_constraint c
        WHERE c.conrelid='employee_reviewed_memory_export_receipts'::regclass AND c.contype='c'
            AND (SELECT array_agg(k ORDER BY k) FROM unnest(c.conkey) k)=expected;
    IF found_count<>1 THEN RAISE EXCEPTION 'expected exact employee receipt state constraint'; END IF;
    EXECUTE format('ALTER TABLE employee_reviewed_memory_export_receipts DROP CONSTRAINT %I',guard_name);
END $$;
ALTER TABLE employee_reviewed_memory_export_receipts ADD CONSTRAINT employee_reviewed_receipt_erasure_state
    CHECK((remote_status='withdrawn')=erased_from_reviewed_store);

-- Inventory remains the original eight central tables; this adds one immutable
-- target column and replaces two candidate function bodies + one state CHECK.
-- No employee fact is exposed through legacy project/conversation paths. No
-- runtime requester/use resolver, runtime enable flag, or encrypted input exists.

-- Source: docs/ortak/sql/encrypted_dm_jobs.sql
-- SHA256: a09b4fb550d549fc3adf6abd41c8a722db526fe221c4df8e9c44ace770c43f58
-- Unnumbered encrypted-DM prerequisite. No activation/normalizer/run changes.
-- Root must assemble with confidential persistence, expiry/deletion inventory
-- and final admission before enabling any consumer. Depends on immutable 1..76.

CREATE TABLE encrypted_dm_selections (
    company_id UUID NOT NULL REFERENCES companies(id),
    selection_id UUID NOT NULL CHECK(selection_id<>'00000000-0000-0000-0000-000000000000'),
    community_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    human_public_key BYTEA NOT NULL CHECK(octet_length(human_public_key)=32),
    employee_public_key BYTEA NOT NULL CHECK(octet_length(employee_public_key)=32),
    office_binding_id UUID NOT NULL,
    key_version BIGINT NOT NULL CHECK(key_version>=0),
    decrypt_ref TEXT NOT NULL CHECK(ortak_is_credential_ref(decrypt_ref)),
    purpose TEXT NOT NULL DEFAULT 'dm_decrypt' CHECK(purpose='dm_decrypt'),
    enabled BOOLEAN NOT NULL DEFAULT false,
    generation BIGINT NOT NULL DEFAULT 1 CHECK(generation>0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    enabled_at TIMESTAMPTZ,
    PRIMARY KEY(company_id,selection_id),
    CHECK(human_public_key<>employee_public_key),
    CHECK(NOT enabled OR enabled_at IS NOT NULL)
);
-- Outer1059 identifies a recipient, not its human or conversation. This initial
-- explicit one-human mode refuses ambiguity instead of guessing. Retained rows
-- are not overwritten, and this partial index does not preclude future multipair.
CREATE UNIQUE INDEX encrypted_dm_one_enabled_pair
 ON encrypted_dm_selections(company_id,employee_id) WHERE enabled;
SELECT attach_community_write_fence('encrypted_dm_selections');

CREATE FUNCTION ortak_encrypted_dm_pair_current(s encrypted_dm_selections)
RETURNS BOOLEAN LANGUAGE SQL VOLATILE STRICT
SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(
  SELECT 1 FROM public.office_company_bindings cb
  JOIN public.companies co ON co.id=cb.company_id AND co.status='active'
  JOIN public.communities cm ON cm.id=cb.community_id AND cm.deletion_state='active' AND cm.deleted_at IS NULL
  JOIN public.channels ch ON ch.community_id=cm.id AND ch.id=s.channel_id
  JOIN public.employees e ON e.company_id=co.id AND e.id=s.employee_id AND e.status='active'
  JOIN public.employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
  JOIN public.employee_office_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id AND b.id=s.office_binding_id
  WHERE cb.company_id=s.company_id AND cb.community_id=s.community_id
    AND ch.channel_type='dm' AND ch.visibility='private'
    AND ch.archived_at IS NULL AND ch.deleted_at IS NULL AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>clock_timestamp())
    AND b.public_key=s.employee_public_key AND b.signer_ref=s.decrypt_ref
    AND b.verified_at IS NOT NULL AND b.valid_from<=clock_timestamp()
    AND (b.valid_until IS NULL OR b.valid_until>clock_timestamp())
    AND r.manifest#>>'{office,public_key}'=encode(s.employee_public_key,'hex')
    AND r.manifest#>>'{office,signer_ref}'=s.decrypt_ref
    AND ch.participant_hash=public.digest(
        least(s.human_public_key,s.employee_public_key)||greatest(s.human_public_key,s.employee_public_key),'sha256')
    AND (SELECT count(*) FROM (SELECT 1 FROM public.channel_members m
        WHERE m.community_id=s.community_id AND m.channel_id=s.channel_id LIMIT 3) members)=2
    AND EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=s.community_id AND m.channel_id=s.channel_id AND m.pubkey=s.human_public_key AND m.removed_at IS NULL)
    AND EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=s.community_id AND m.channel_id=s.channel_id AND m.pubkey=s.employee_public_key AND m.removed_at IS NULL)
    AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings other WHERE other.company_id=s.company_id AND other.public_key=s.human_public_key)
    AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=s.community_id AND u.pubkey=s.human_public_key
        AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
    AND NOT EXISTS(SELECT 1 FROM public.channel_members m WHERE m.community_id=s.community_id AND m.pubkey=s.human_public_key AND m.role='bot')
    AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=s.community_id AND u.pubkey=s.employee_public_key AND u.deactivated_at IS NOT NULL)
 )
$$;

CREATE FUNCTION ortak_encrypted_dm_selection_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF TG_OP='DELETE' THEN
  RAISE EXCEPTION 'Encrypted DM selection is retained' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' THEN
  IF (to_jsonb(NEW)-ARRAY['enabled','generation','changed_at','enabled_at']) IS DISTINCT FROM
     (to_jsonb(OLD)-ARRAY['enabled','generation','changed_at','enabled_at'])
     OR NEW.generation<>OLD.generation OR NEW.changed_at<>OLD.changed_at
     OR NEW.enabled_at IS DISTINCT FROM OLD.enabled_at THEN
   RAISE EXCEPTION 'Encrypted DM selection identity is immutable' USING ERRCODE='check_violation';
  END IF;
  IF NEW.enabled=OLD.enabled THEN RETURN OLD; END IF;
 END IF;
 -- Config changes are Office mutations. Try-lock fails rather than upgrading
 -- across another signed reader; no caller holds this fence through crypto.
 PERFORM public.ortak_advance_office_authority(NEW.company_id,'encrypted_dm_selections');
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 IF TG_OP='INSERT' THEN
  IF NEW.generation<>1 OR (SELECT count(*) FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id)>=128 THEN
   RAISE EXCEPTION 'Encrypted DM retained selection bound' USING ERRCODE='check_violation';
  END IF;
  NEW.created_at:=clock_timestamp();
 ELSE NEW.generation:=OLD.generation+1;
 END IF;
 IF (TG_OP='INSERT' OR NEW.enabled) AND NOT public.ortak_encrypted_dm_pair_current(NEW) THEN
  RAISE EXCEPTION 'Encrypted DM selected pair unavailable' USING ERRCODE='check_violation';
 END IF;
 NEW.changed_at:=clock_timestamp();
 IF NEW.enabled THEN NEW.enabled_at:=NEW.changed_at; END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER encrypted_dm_selection_guard BEFORE INSERT OR UPDATE OR DELETE ON encrypted_dm_selections
FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_selection_guard();

CREATE FUNCTION ortak_encrypted_dm_selection_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE current_row public.encrypted_dm_selections;
BEGIN
 SELECT * INTO current_row FROM public.encrypted_dm_selections
  WHERE company_id=NEW.company_id AND selection_id=NEW.selection_id;
 IF current_row.enabled AND NOT public.ortak_encrypted_dm_pair_current(current_row) THEN
  RAISE EXCEPTION 'Encrypted DM selection expired before commit' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER encrypted_dm_selection_current_at_commit AFTER INSERT OR UPDATE ON encrypted_dm_selections
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_selection_commit_guard();

-- Reconstruct signed outer bytes from server columns only. Explicit partition,
-- pending untouched inbox, one p, strict cipher/tag bounds; no plaintext input.
CREATE FUNCTION ortak_encrypted_dm_outer(target UUID, community UUID, source BYTEA, at_time TIMESTAMPTZ, recipient BYTEA)
RETURNS BYTEA LANGUAGE plpgsql VOLATILE STRICT
SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE ev RECORD; canonical TEXT;
BEGIN
 SELECT e.id,e.pubkey,e.created_at,e.kind,e.tags,e.content,e.sig INTO ev
 FROM public.office_inbox i JOIN public.events e
  ON e.community_id=community AND e.id=i.event_id AND e.created_at=i.event_created_at
 WHERE i.company_id=target AND i.event_id=source AND i.event_created_at=at_time
  AND i.event_kind=1059 AND e.kind=1059 AND e.channel_id IS NULL AND i.channel_id IS NULL
  AND i.author_pubkey=e.pubkey AND e.deleted_at IS NULL
  AND i.state='pending' AND i.claim_generation=0 AND i.attempt_count=0 AND i.finalized_at IS NULL
  AND e.created_at>=timestamptz '1970-01-01 00:00:00+00' AND e.created_at<timestamptz '10000-01-01 00:00:00+00'
  AND date_trunc('second',e.created_at)=e.created_at
  AND octet_length(e.pubkey)=32 AND octet_length(e.sig)=64
  AND octet_length(e.content) BETWEEN 132 AND 60000 AND e.content~'^[A-Za-z0-9+/]*={0,2}$'
  AND octet_length(e.tags::text)<=256 AND e.tags=jsonb_build_array(jsonb_build_array('p',encode(recipient,'hex')));
 IF NOT FOUND THEN RETURN NULL; END IF;
 canonical:=public.ortak_conversation_json75(jsonb_build_object(
  'id',encode(ev.id,'hex'),'pubkey',encode(ev.pubkey,'hex'),'created_at',extract(epoch FROM ev.created_at)::bigint,
  'kind',1059,'tags',ev.tags,'content',ev.content,'sig',encode(ev.sig,'hex')));
 IF canonical IS NULL OR octet_length(canonical)>65536 THEN RETURN NULL; END IF;
 RETURN convert_to(canonical,'UTF8');
END
$$;

CREATE TABLE encrypted_dm_decrypt_jobs (
 company_id UUID NOT NULL REFERENCES companies(id), community_id UUID NOT NULL,
 source_id BYTEA NOT NULL CHECK(octet_length(source_id)=32), source_created_at TIMESTAMPTZ NOT NULL,
 source_author BYTEA NOT NULL CHECK(octet_length(source_author)=32), source_hash BYTEA NOT NULL CHECK(octet_length(source_hash)=32),
 source_received_at TIMESTAMPTZ NOT NULL, selection_id UUID NOT NULL, selection_generation BIGINT NOT NULL CHECK(selection_generation>0),
 employee_id TEXT NOT NULL, employee_revision_id UUID NOT NULL, employee_lifecycle_epoch BIGINT NOT NULL CHECK(employee_lifecycle_epoch>=0),
 office_generation BIGINT NOT NULL CHECK(office_generation>=0), valid_before TIMESTAMPTZ NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), deadline TIMESTAMPTZ NOT NULL,
 state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','claimed','verified','failed','cancelled')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
 claim_generation BIGINT NOT NULL DEFAULT 0 CHECK(claim_generation BETWEEN 0 AND 3),
 claim_token UUID, worker_id UUID, claimed_at TIMESTAMPTZ, claim_expires_at TIMESTAMPTZ, crypto_deadline TIMESTAMPTZ,
 next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), terminal_at TIMESTAMPTZ,
 error_code TEXT CHECK(error_code IN('material_unavailable','crypto_invalid','authority_changed','source_unavailable','deadline_exceeded','attempts_exhausted','cancelled')),
 seal_id BYTEA CHECK(octet_length(seal_id)=32), seal_created_at TIMESTAMPTZ,
 rumor_id BYTEA CHECK(octet_length(rumor_id)=32), rumor_created_at TIMESTAMPTZ,
 rumor_hash BYTEA CHECK(octet_length(rumor_hash)=32), reply_to BYTEA CHECK(octet_length(reply_to)=32), verified_at TIMESTAMPTZ,
 PRIMARY KEY(company_id,source_id),
 FOREIGN KEY(company_id,selection_id) REFERENCES encrypted_dm_selections(company_id,selection_id),
 CHECK(isfinite(deadline) AND isfinite(valid_before) AND deadline>source_received_at AND deadline<=source_received_at+interval '120 seconds' AND valid_before<=deadline),
 CHECK(isfinite(next_attempt_at) AND next_attempt_at<=deadline+interval '5 seconds'),
 CHECK(claim_generation=attempts),
 CHECK((state IN('claimed','verified'))=(claim_token IS NOT NULL)),
 CHECK((claim_token IS NULL)=(worker_id IS NULL) AND (claim_token IS NULL)=(claimed_at IS NULL)
  AND (claim_token IS NULL)=(claim_expires_at IS NULL) AND (claim_token IS NULL)=(crypto_deadline IS NULL)),
 CHECK(claim_token IS NULL OR (claim_token<>'00000000-0000-0000-0000-000000000000' AND worker_id<>'00000000-0000-0000-0000-000000000000')),
 CHECK(claim_token IS NULL OR (claimed_at<crypto_deadline AND crypto_deadline<=claimed_at+interval '5 seconds'
  AND crypto_deadline<=claim_expires_at AND claim_expires_at<=claimed_at+interval '30 seconds' AND claim_expires_at<=valid_before)),
 CHECK((state IN('failed','cancelled'))=(terminal_at IS NOT NULL)),
 CHECK((verified_at IS NULL)=(rumor_id IS NULL) AND (verified_at IS NULL)=(seal_id IS NULL)
  AND (verified_at IS NULL)=(seal_created_at IS NULL) AND (verified_at IS NULL)=(rumor_created_at IS NULL)
  AND (verified_at IS NULL)=(rumor_hash IS NULL)),
 CHECK(verified_at IS NOT NULL OR reply_to IS NULL),
 CHECK(seal_created_at IS NULL OR (seal_created_at>=timestamptz '1970-01-01 00:00:00+00'
  AND seal_created_at<timestamptz '10000-01-01 00:00:00+00' AND date_trunc('second',seal_created_at)=seal_created_at)),
 CHECK(rumor_created_at IS NULL OR (rumor_created_at>=timestamptz '1970-01-01 00:00:00+00'
  AND rumor_created_at<timestamptz '10000-01-01 00:00:00+00' AND date_trunc('second',rumor_created_at)=rumor_created_at)),
 CHECK(state<>'verified' OR verified_at IS NOT NULL)
);
CREATE INDEX encrypted_dm_jobs_due ON encrypted_dm_decrypt_jobs(company_id,next_attempt_at,source_received_at,source_id)
 WHERE state IN('pending','claimed','verified');
CREATE INDEX encrypted_dm_jobs_live ON encrypted_dm_decrypt_jobs(company_id,claim_expires_at)
 WHERE state IN('claimed','verified');
CREATE INDEX encrypted_dm_verified_rumor ON encrypted_dm_decrypt_jobs(company_id,employee_id,rumor_id)
 WHERE verified_at IS NOT NULL;
SELECT attach_community_write_fence('encrypted_dm_decrypt_jobs');

-- Replaced only by the later protected-admission fragment. This prerequisite
-- remains usable without that table; no job is consumed by verification alone.
CREATE FUNCTION ortak_encrypted_dm_job_consumed(company UUID,source BYTEA)
RETURNS BOOLEAN LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT false
$$;

CREATE FUNCTION ortak_encrypted_dm_job_current(j encrypted_dm_decrypt_jobs)
RETURNS BOOLEAN LANGUAGE SQL VOLATILE STRICT
SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM public.encrypted_dm_selections s
  JOIN public.employees e ON e.company_id=s.company_id AND e.id=s.employee_id
  JOIN public.office_routing_cohorts c ON c.company_id=s.company_id AND c.community_id=s.community_id AND c.state='enabled'
  JOIN public.office_routing_channels ch ON ch.company_id=c.company_id AND ch.community_id=c.community_id AND ch.channel_id=s.channel_id
  JOIN public.office_routing_employees ce ON ce.company_id=c.company_id AND ce.employee_id=e.id
  JOIN public.office_inbox i ON i.company_id=j.company_id AND i.event_id=j.source_id
  WHERE s.company_id=j.company_id AND s.selection_id=j.selection_id AND s.community_id=j.community_id
   AND s.enabled AND s.generation=j.selection_generation AND s.employee_id=j.employee_id
   AND e.status='active' AND e.active_revision_id=j.employee_revision_id AND e.lifecycle_epoch=j.employee_lifecycle_epoch
   AND i.received_at=j.source_received_at AND i.received_at>=s.enabled_at AND i.author_pubkey=j.source_author
   AND clock_timestamp()<j.valid_before
   AND coalesce((SELECT generation FROM public.office_authority_generations g WHERE g.company_id=j.company_id),0)=j.office_generation
   AND public.ortak_encrypted_dm_pair_current(s)
   AND public.digest(public.ortak_encrypted_dm_outer(j.company_id,j.community_id,j.source_id,j.source_created_at,s.employee_public_key),'sha256')=j.source_hash)
$$;

CREATE FUNCTION ortak_encrypted_dm_job_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE fresh BOOLEAN:=false;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Encrypted DM job is retained' USING ERRCODE='check_violation'; END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.verified_at IS NOT NULL OR NEW.error_code IS NOT NULL THEN
   RAISE EXCEPTION 'Encrypted DM job initial state' USING ERRCODE='check_violation';
  END IF;
  fresh:=true;
 ELSE
  IF (to_jsonb(NEW)-ARRAY['state','attempts','claim_generation','claim_token','worker_id','claimed_at','claim_expires_at','crypto_deadline','next_attempt_at','terminal_at','error_code','seal_id','seal_created_at','rumor_id','rumor_created_at','rumor_hash','reply_to','verified_at']) IS DISTINCT FROM
     (to_jsonb(OLD)-ARRAY['state','attempts','claim_generation','claim_token','worker_id','claimed_at','claim_expires_at','crypto_deadline','next_attempt_at','terminal_at','error_code','seal_id','seal_created_at','rumor_id','rumor_created_at','rumor_hash','reply_to','verified_at']) THEN
   RAISE EXCEPTION 'Encrypted DM job source is immutable' USING ERRCODE='check_violation';
  END IF;
  IF OLD.state IN('failed','cancelled') THEN
   IF NEW IS DISTINCT FROM OLD THEN RAISE EXCEPTION 'Encrypted DM terminal job retained' USING ERRCODE='check_violation'; END IF;
   RETURN OLD;
  END IF;
  IF OLD.verified_at IS NOT NULL AND
   (NEW.seal_id,NEW.seal_created_at,NEW.rumor_id,NEW.rumor_created_at,NEW.rumor_hash,NEW.reply_to,NEW.verified_at) IS DISTINCT FROM
   (OLD.seal_id,OLD.seal_created_at,OLD.rumor_id,OLD.rumor_created_at,OLD.rumor_hash,OLD.reply_to,OLD.verified_at) THEN
   RAISE EXCEPTION 'Encrypted DM verified metadata is immutable' USING ERRCODE='check_violation';
  END IF;
  IF OLD.verified_at IS NULL AND NEW.verified_at IS NOT NULL
    AND NOT(OLD.state='claimed' AND NEW.state='verified') THEN
   RAISE EXCEPTION 'Encrypted DM metadata requires current verification' USING ERRCODE='check_violation';
  END IF;
  -- Identical in-budget receipt replay has no new effect and cannot renew a
  -- token or deadline. Deferred current checks still apply to the result row.
  IF OLD.state='verified' AND NEW IS NOT DISTINCT FROM OLD THEN RETURN OLD; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.claim_generation=OLD.claim_generation+1 AND NEW.state='claimed'
   AND (OLD.state='pending' OR OLD.claim_expires_at+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)<=clock_timestamp()) AND OLD.next_attempt_at<=clock_timestamp()
   AND NEW.claim_token IS NOT NULL AND NEW.claim_token IS DISTINCT FROM OLD.claim_token THEN fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.claim_generation=OLD.claim_generation THEN
   IF NEW.state='verified' AND OLD.state='claimed' AND OLD.crypto_deadline>clock_timestamp()
    AND (OLD.verified_at IS NOT NULL OR NEW.verified_at>=OLD.claimed_at) AND NEW.verified_at<=clock_timestamp()
    AND (NEW.claim_token,NEW.worker_id,NEW.claimed_at,NEW.claim_expires_at,NEW.crypto_deadline) IS NOT DISTINCT FROM
        (OLD.claim_token,OLD.worker_id,OLD.claimed_at,OLD.claim_expires_at,OLD.crypto_deadline) THEN fresh:=true;
   ELSIF NEW.state IN('failed','cancelled') AND NEW.error_code IS NOT NULL THEN NULL;
   ELSIF NEW.state='pending' AND OLD.state IN('claimed','verified') AND OLD.claim_expires_at>clock_timestamp()
    AND NEW.error_code='material_unavailable' AND OLD.attempts<3
    AND NEW.next_attempt_at>=statement_timestamp()+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END) THEN NULL;
   ELSE RAISE EXCEPTION 'Encrypted DM job transition refused' USING ERRCODE='check_violation';
   END IF;
  ELSE RAISE EXCEPTION 'Encrypted DM claim generation refused' USING ERRCODE='check_violation';
  END IF;
 END IF;
 IF fresh THEN
  PERFORM public.ortak_lock_office_authority(NEW.company_id);
  PERFORM 1 FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id AND selection_id=NEW.selection_id FOR SHARE;
  -- Inbox claim-state changes deliberately do not advance Office generation.
  -- Retain its row lock through commit as well as comparing canonical facts.
  PERFORM 1 FROM public.office_inbox WHERE company_id=NEW.company_id AND event_id=NEW.source_id FOR SHARE;
  IF NEW.state='claimed' THEN
   IF NOT pg_try_advisory_xact_lock(hashtextextended('ortak-encrypted-dm-claims:'||NEW.company_id::text,0))
     OR NEW.claimed_at>clock_timestamp() OR NEW.crypto_deadline<=clock_timestamp()
     OR (SELECT count(*) FROM public.encrypted_dm_decrypt_jobs j WHERE j.company_id=NEW.company_id
          AND j.source_id<>NEW.source_id AND j.state IN('claimed','verified') AND NOT public.ortak_encrypted_dm_job_consumed(j.company_id,j.source_id) AND j.claim_expires_at>clock_timestamp())>=2 THEN
    RAISE EXCEPTION 'Encrypted DM finite claim slot unavailable' USING ERRCODE='serialization_failure';
   END IF;
  END IF;
  IF NOT public.ortak_encrypted_dm_job_current(NEW) THEN
   RAISE EXCEPTION 'Encrypted DM job authority changed' USING ERRCODE='serialization_failure';
  END IF;
  IF NEW.state='verified' AND NEW.reply_to IS NOT NULL AND NOT EXISTS(
    SELECT 1 FROM public.encrypted_dm_decrypt_jobs previous
    JOIN public.encrypted_dm_selections p ON p.company_id=previous.company_id AND p.selection_id=previous.selection_id
    JOIN public.encrypted_dm_selections s ON s.company_id=NEW.company_id AND s.selection_id=NEW.selection_id
    WHERE previous.company_id=NEW.company_id AND previous.employee_id=NEW.employee_id
      AND previous.rumor_id=NEW.reply_to AND previous.verified_at IS NOT NULL AND previous.source_id<>NEW.source_id
      AND (p.community_id,p.channel_id,p.human_public_key,p.employee_public_key)=(s.community_id,s.channel_id,s.human_public_key,s.employee_public_key)) THEN
   RAISE EXCEPTION 'Encrypted DM reply lacks same-pair verified provenance' USING ERRCODE='check_violation';
  END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER encrypted_dm_job_guard BEFORE INSERT OR UPDATE OR DELETE ON encrypted_dm_decrypt_jobs
FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_job_guard();

CREATE FUNCTION ortak_encrypted_dm_job_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE current_row public.encrypted_dm_decrypt_jobs;
BEGIN
 SELECT * INTO current_row FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
 IF current_row.state IN('pending','claimed','verified') AND
  (TG_OP='INSERT' OR NEW.state='verified' OR NEW.attempts>OLD.attempts) THEN
  PERFORM public.ortak_lock_office_authority(NEW.company_id);
  IF NOT public.ortak_encrypted_dm_job_current(current_row)
   OR (current_row.state IN('claimed','verified') AND clock_timestamp()>=current_row.crypto_deadline) THEN
   RAISE EXCEPTION 'Encrypted DM job expired before commit' USING ERRCODE='serialization_failure';
  END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER encrypted_dm_job_current_at_commit AFTER INSERT OR UPDATE ON encrypted_dm_decrypt_jobs
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_job_commit_guard();

-- Both retained ledgers reject bulk erasure as well as row deletion.
CREATE TRIGGER encrypted_dm_selections_no_truncate BEFORE TRUNCATE ON encrypted_dm_selections
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER encrypted_dm_decrypt_jobs_no_truncate BEFORE TRUNCATE ON encrypted_dm_decrypt_jobs
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

-- Source: docs/ortak/sql/encrypted_dm_admission.sql
-- SHA256: dbc5fc2e0207c81646b6a653a409185149f52953a038152533962ee8a8ece93d
-- Unnumbered, unactivated protected admission. Assemble AFTER encrypted_dm_jobs.
-- No ordinary 1059 normalizer or dispatch consumer is enabled by this fragment.
-- Shared Office fence precedes selection, verified job, rumor, inbox and run.
-- The conservative v1 epoch is the carried Office generation: any relevant
-- Office mutation retires old authority permanently, including remove/restore.
-- No renewal or remote cleanup policy is introduced here.

ALTER TABLE runs ADD COLUMN payload_mode TEXT NOT NULL DEFAULT 'ordinary'
 CHECK(payload_mode IN('ordinary','confidential_dm_v1'));

CREATE FUNCTION ortak_confidential_runtime_binding(company UUID,revision UUID)
RETURNS JSONB LANGUAGE SQL VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT jsonb_build_object('adapter',b.adapter,'profile_ref',b.profile_ref,'model',b.model,
   'workspace_ref',b.workspace_ref,'credential_refs',b.credential_refs,'options',b.options)
 FROM public.employee_runtime_bindings b JOIN public.employee_revisions r
 ON r.company_id=b.company_id AND r.id=b.revision_id AND r.employee_id=b.employee_id
 WHERE b.company_id=company AND b.revision_id=revision AND b.validated_at IS NOT NULL
 AND r.manifest->'runtime'=jsonb_build_object('adapter',b.adapter,'profile_ref',b.profile_ref,'model',b.model,
   'workspace_ref',b.workspace_ref,'credential_refs',b.credential_refs,'options',b.options)
 AND r.manifest->'permissions'='{"allowed_tools":[],"allowed_workspaces":[],"allowed_networks":[],"approval_required":[]}'::jsonb
 AND r.manifest#>'{routing,enabled}'='true'::jsonb
$$;

CREATE FUNCTION ortak_confidential_dm_run_id(company UUID,source BYTEA)
RETURNS UUID LANGUAGE SQL IMMUTABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT substr(encode(public.digest(convert_to('ortak-confidential-run-id/1:'||company::text||':'||encode(source,'hex'),'UTF8'),'sha256'),'hex'),1,32)::uuid
 WHERE octet_length(source)=32
$$;

CREATE FUNCTION ortak_confidential_dm_source(company UUID,source BYTEA)
RETURNS BYTEA LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT convert_to(public.ortak_conversation_json75(jsonb_build_object(
  'format','ortak-confidential-dm-source/1','company_id',j.company_id,'community_id',j.community_id,
  'conversation_id',s.channel_id,'employee_id',j.employee_id,
  'employee_public_key',encode(s.employee_public_key,'hex'),'human_public_key',encode(s.human_public_key,'hex'),
  'office_binding_id',s.office_binding_id,'key_version',s.key_version::text,
  'outer_event_id',encode(j.source_id,'hex'),'outer_event_created_at',to_char(j.source_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'outer_json_sha256',encode(j.source_hash,'hex'),'seal_event_id',encode(j.seal_id,'hex'),
  'seal_event_created_at',to_char(j.seal_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'rumor_event_id',encode(j.rumor_id,'hex'),'rumor_event_created_at',to_char(j.rumor_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'rumor_json_sha256',encode(j.rumor_hash,'hex'),'reply_rumor_id',encode(j.reply_to,'hex'))),'UTF8')
 FROM public.encrypted_dm_decrypt_jobs j JOIN public.encrypted_dm_selections s USING(company_id,selection_id)
 WHERE j.company_id=company AND j.source_id=source AND j.verified_at IS NOT NULL
$$;

CREATE FUNCTION ortak_confidential_dm_identity(company UUID,source BYTEA,run UUID,key UUID)
RETURNS BYTEA LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT convert_to(public.ortak_conversation_json75(jsonb_build_object(
  'authority_epoch',j.office_generation::text,'company_id',j.company_id,'community_id',j.community_id,
  'conversation_id',s.channel_id,'employee_id',j.employee_id,'employee_lifecycle_epoch',j.employee_lifecycle_epoch::text,
  'employee_public_key',encode(s.employee_public_key,'hex'),'employee_revision_id',j.employee_revision_id,
  'human_public_key',encode(s.human_public_key,'hex'),'key_id',key,'key_version',s.key_version::text,
  'office_binding_id',s.office_binding_id,'rumor_id',encode(j.rumor_id,'hex'),'run_id',run,
  'source_evidence_hash',encode(public.digest(public.ortak_confidential_dm_source(company,source),'sha256'),'hex'),
  'source_outer_created_at',to_char(j.source_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  'source_outer_id',encode(j.source_id,'hex'))),'UTF8')
 FROM public.encrypted_dm_decrypt_jobs j JOIN public.encrypted_dm_selections s USING(company_id,selection_id)
 WHERE j.company_id=company AND j.source_id=source AND j.verified_at IS NOT NULL
 AND run=public.ortak_confidential_dm_run_id(company,source)
 AND key<>'00000000-0000-0000-0000-000000000000'
$$;

CREATE TABLE confidential_runs (
 company_id UUID NOT NULL REFERENCES companies(id),community_id UUID NOT NULL,run_id UUID NOT NULL,
 source_id BYTEA NOT NULL CHECK(octet_length(source_id)=32),selection_id UUID NOT NULL,
 employee_id TEXT NOT NULL,human_public_key BYTEA NOT NULL CHECK(octet_length(human_public_key)=32),
 rumor_id BYTEA NOT NULL CHECK(octet_length(rumor_id)=32),key_id UUID NOT NULL,
 identity_bytes BYTEA NOT NULL CHECK(octet_length(identity_bytes) BETWEEN 1 AND 2048),
 source_bytes BYTEA NOT NULL CHECK(octet_length(source_bytes) BETWEEN 1 AND 4096),
 wrapped_key BYTEA NOT NULL CHECK(octet_length(wrapped_key) BETWEEN 1 AND 12288),
 start_key TEXT NOT NULL CHECK(start_key='ortak-run:'||company_id::text||':'||run_id::text),
 admitted_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 admission_deadline TIMESTAMPTZ NOT NULL,execution_deadline TIMESTAMPTZ NOT NULL,
 claim_generation BIGINT NOT NULL CHECK(claim_generation BETWEEN 1 AND 3),claim_token UUID NOT NULL,claim_worker UUID NOT NULL,
 PRIMARY KEY(company_id,run_id), UNIQUE(company_id,source_id), UNIQUE(company_id,key_id),
 -- Independent of wrapper, Office key version, pair re-enable and model revision.
 UNIQUE(company_id,employee_id,human_public_key,rumor_id),
 FOREIGN KEY(company_id,run_id) REFERENCES runs(company_id,id),
 FOREIGN KEY(company_id,source_id) REFERENCES encrypted_dm_decrypt_jobs(company_id,source_id),
 FOREIGN KEY(company_id,selection_id) REFERENCES encrypted_dm_selections(company_id,selection_id),
 CHECK(isfinite(admitted_at) AND isfinite(admission_deadline) AND isfinite(execution_deadline)
  AND admission_deadline>admitted_at AND execution_deadline>admitted_at AND execution_deadline<=admitted_at+interval '10 minutes')
);
SELECT attach_community_write_fence('confidential_runs');

CREATE TABLE confidential_run_payloads (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 purpose TEXT NOT NULL CHECK(purpose IN('snapshot','runtime_event','reply_draft')),
 ordinal INTEGER NOT NULL,
 envelope_bytes BYTEA NOT NULL CHECK(octet_length(envelope_bytes) BETWEEN 1 AND 98304),
 nonce BYTEA NOT NULL CHECK(octet_length(nonce)=12),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(company_id,run_id,purpose,ordinal),UNIQUE(company_id,run_id,purpose,nonce),
 FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
 CHECK((purpose IN('snapshot','reply_draft') AND ordinal=0) OR (purpose='runtime_event' AND ordinal BETWEEN 1 AND 512))
);
SELECT attach_community_write_fence('confidential_run_payloads');

-- Each verified outer is consumed once, including alternative wrappers. This
-- immutable row is the terminal job receipt; the job's original lease remains
-- historical evidence and can never be reclaimed after consumption.
CREATE TABLE confidential_dm_receipts (
 company_id UUID NOT NULL,community_id UUID NOT NULL,source_id BYTEA NOT NULL,
 run_id UUID NOT NULL,duplicate_rumor BOOLEAN NOT NULL,
 claim_generation BIGINT NOT NULL,claim_token UUID NOT NULL,claim_worker UUID NOT NULL,
 committed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(company_id,source_id),
 FOREIGN KEY(company_id,source_id) REFERENCES encrypted_dm_decrypt_jobs(company_id,source_id),
 FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id)
);
SELECT attach_community_write_fence('confidential_dm_receipts');

CREATE OR REPLACE FUNCTION ortak_encrypted_dm_job_consumed(company UUID,source BYTEA)
RETURNS BOOLEAN LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=company AND source_id=source)
$$;

-- Separate outbox: ordinary runtime startup cannot claim this row by accident.
-- The later confidential worker owns these finite leases. No consumer is wired.
CREATE TABLE confidential_run_dispatches (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','delivered','failed','cancelled')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
 generation BIGINT NOT NULL DEFAULT 0 CHECK(generation=attempts),
 lease_token UUID,lease_expires_at TIMESTAMPTZ,
 next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 error_code TEXT CHECK(error_code IN('unavailable','authority_changed','deadline_exceeded','cancelled')),
 finished_at TIMESTAMPTZ,
 PRIMARY KEY(company_id,run_id),FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
 CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),
 CHECK((state<>'pending')=(finished_at IS NOT NULL)),CHECK(state='pending' OR lease_token IS NULL),
 CHECK(isfinite(next_attempt_at) AND (lease_expires_at IS NULL OR isfinite(lease_expires_at)))
);
SELECT attach_community_write_fence('confidential_run_dispatches');

-- Canonical stored outer after finalization. The real ephemeral author, NULL
-- channel, exact partition and signed ciphertext remain unchanged in both rows.
CREATE FUNCTION ortak_confidential_dm_current(company UUID,run UUID)
RETURNS BOOLEAN LANGUAGE SQL VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM public.confidential_runs c
 JOIN public.runs r ON r.company_id=c.company_id AND r.id=c.run_id
 JOIN public.encrypted_dm_decrypt_jobs j ON j.company_id=c.company_id AND j.source_id=c.source_id
 JOIN public.encrypted_dm_selections s ON s.company_id=j.company_id AND s.selection_id=j.selection_id
 JOIN public.employees e ON e.company_id=j.company_id AND e.id=j.employee_id
 JOIN public.office_routing_cohorts co ON co.company_id=c.company_id AND co.community_id=c.community_id AND co.state='enabled'
 JOIN public.office_routing_channels ch ON ch.company_id=co.company_id AND ch.community_id=co.community_id AND ch.channel_id=s.channel_id
 JOIN public.office_routing_employees ce ON ce.company_id=co.company_id AND ce.employee_id=e.id
 JOIN public.office_inbox i ON i.company_id=c.company_id AND i.event_id=j.source_id
 JOIN public.events ev ON ev.community_id=c.community_id AND ev.id=j.source_id AND ev.created_at=j.source_created_at
 WHERE c.company_id=company AND c.run_id=run AND r.payload_mode='confidential_dm_v1'
 AND r.status IN('queued','running','waiting','completed') AND r.work_item_id IS NULL
 AND r.employee_id=j.employee_id AND r.employee_revision_id=j.employee_revision_id AND r.employee_lifecycle_epoch=j.employee_lifecycle_epoch
 AND r.message_id=j.source_id AND r.root_message_id=j.source_id
 AND s.enabled AND s.generation=j.selection_generation AND s.community_id=c.community_id
 AND e.status='active' AND e.active_revision_id=j.employee_revision_id AND e.lifecycle_epoch=j.employee_lifecycle_epoch
 AND public.ortak_encrypted_dm_pair_current(s) AND public.ortak_confidential_runtime_binding(company,j.employee_revision_id) IS NOT NULL
 AND coalesce((SELECT generation FROM public.office_authority_generations WHERE company_id=company),0)=j.office_generation
 AND clock_timestamp()<c.execution_deadline
 AND i.state='decided' AND i.event_kind=1059 AND i.channel_id IS NULL
 AND i.event_created_at=j.source_created_at AND i.author_pubkey=j.source_author AND i.received_at=j.source_received_at
 AND ev.kind=1059 AND ev.channel_id IS NULL AND ev.pubkey=j.source_author AND ev.deleted_at IS NULL
 AND ev.tags=jsonb_build_array(jsonb_build_array('p',encode(s.employee_public_key,'hex')))
 AND octet_length(ev.content) BETWEEN 132 AND 60000 AND octet_length(ev.tags::text)<=256
 AND public.digest(convert_to(public.ortak_conversation_json75(jsonb_build_object(
 'id',encode(ev.id,'hex'),'pubkey',encode(ev.pubkey,'hex'),'created_at',extract(epoch FROM ev.created_at)::bigint,
 'kind',1059,'tags',ev.tags,'content',ev.content,'sig',encode(ev.sig,'hex'))),'UTF8'),'sha256')=j.source_hash
 AND NOT EXISTS(SELECT 1 FROM public.runtime_cancellations stop WHERE stop.company_id=company AND stop.run_id=run))
$$;

CREATE FUNCTION ortak_lock_confidential_dm(company UUID,run UUID) RETURNS BOOLEAN
LANGUAGE plpgsql VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE target RECORD;
BEGIN
 PERFORM public.ortak_lock_office_authority(company);
 SELECT selection_id,source_id INTO target FROM public.confidential_runs WHERE company_id=company AND run_id=run;
 IF NOT FOUND THEN RETURN false; END IF;
 PERFORM 1 FROM public.encrypted_dm_selections WHERE company_id=company AND selection_id=target.selection_id FOR SHARE;
 PERFORM 1 FROM public.encrypted_dm_decrypt_jobs WHERE company_id=company AND source_id=target.source_id FOR SHARE;
 PERFORM 1 FROM public.office_inbox WHERE company_id=company AND event_id=target.source_id FOR SHARE;
 RETURN public.ortak_confidential_dm_current(company,run);
END
$$;

CREATE FUNCTION ortak_confidential_payload_valid(bytes BYTEA,identity BYTEA,purpose TEXT,ordinal INTEGER)
RETURNS BOOLEAN LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE wire JSONB; header JSONB; size INTEGER; nonce BYTEA; cipher BYTEA; maximum INTEGER;
BEGIN
 IF octet_length(bytes)>98304 OR octet_length(identity)>2048 THEN RETURN false; END IF;
 wire:=convert_from(bytes,'UTF8')::jsonb;
 IF jsonb_typeof(wire) IS DISTINCT FROM 'object' OR NOT wire ?& ARRAY['ciphertext','header','nonce'] OR wire-ARRAY['ciphertext','header','nonce']<>'{}'::jsonb
  OR convert_to(public.ortak_conversation_json75(wire),'UTF8')<>bytes THEN RETURN false; END IF;
 header:=wire->'header';
 IF jsonb_typeof(header) IS DISTINCT FROM 'object' OR NOT header ?& ARRAY['algorithm','format','identity','ordinal','plaintext_bytes','purpose'] OR header-ARRAY['algorithm','format','identity','ordinal','plaintext_bytes','purpose']<>'{}'::jsonb
  OR header->>'algorithm' IS DISTINCT FROM 'A256GCM' OR header->>'format' IS DISTINCT FROM 'ortak-confidential-payload/1'
  OR header->>'purpose' IS DISTINCT FROM purpose OR header->'ordinal' IS DISTINCT FROM to_jsonb(ordinal)
  OR convert_to(public.ortak_conversation_json75(header->'identity'),'UTF8') IS DISTINCT FROM identity
  OR jsonb_typeof(header->'plaintext_bytes')<>'number' THEN RETURN false; END IF;
 maximum:=CASE purpose WHEN 'snapshot' THEN 49152 WHEN 'runtime_event' THEN 32768 WHEN 'reply_draft' THEN 16384 END;
 IF maximum IS NULL OR (purpose='runtime_event' AND ordinal NOT BETWEEN 1 AND 512)
  OR (purpose<>'runtime_event' AND ordinal<>0) THEN RETURN false; END IF;
 IF (header->>'plaintext_bytes')!~'^(0|[1-9][0-9]{0,5})$' THEN RETURN false; END IF;
 size:=(header->>'plaintext_bytes')::integer;
 IF size>maximum OR jsonb_typeof(wire->'nonce')<>'string' OR length(wire->>'nonce')<>16
  OR jsonb_typeof(wire->'ciphertext')<>'string' OR length(wire->>'ciphertext')>65560 THEN RETURN false; END IF;
 nonce:=decode(wire->>'nonce','base64'); cipher:=decode(wire->>'ciphertext','base64');
 RETURN octet_length(nonce)=12 AND octet_length(cipher)=size+16
  AND replace(encode(nonce,'base64'),E'\n','')=wire->>'nonce'
  AND replace(encode(cipher,'base64'),E'\n','')=wire->>'ciphertext';
END
$$;

CREATE FUNCTION ortak_confidential_run_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE j public.encrypted_dm_decrypt_jobs; s public.encrypted_dm_selections; r public.runs; wrapped JSONB; cipher BYTEA;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Confidential run bytes are immutable' USING ERRCODE='check_violation'; END IF;
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id AND selection_id=NEW.selection_id FOR SHARE;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id FOR UPDATE;
 SELECT * INTO STRICT r FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id;
 IF j.state<>'verified' OR j.claim_expires_at<=clock_timestamp() OR NOT public.ortak_encrypted_dm_job_current(j)
  OR (j.claim_generation,j.claim_token,j.worker_id) IS DISTINCT FROM (NEW.claim_generation,NEW.claim_token,NEW.claim_worker)
  OR (j.community_id,j.selection_id,j.employee_id,j.rumor_id,s.human_public_key) IS DISTINCT FROM
     (NEW.community_id,NEW.selection_id,NEW.employee_id,NEW.rumor_id,NEW.human_public_key)
  OR r.payload_mode<>'confidential_dm_v1' OR r.status<>'queued' OR r.runtime_run_ref IS NOT NULL
  OR (r.employee_id,r.employee_revision_id,r.employee_lifecycle_epoch,r.message_id,r.root_message_id) IS DISTINCT FROM
     (j.employee_id,j.employee_revision_id,j.employee_lifecycle_epoch,j.source_id,j.source_id)
  OR r.work_item_id IS NOT NULL OR public.ortak_confidential_runtime_binding(NEW.company_id,j.employee_revision_id) IS NULL
  OR NEW.run_id<>public.ortak_confidential_dm_run_id(NEW.company_id,NEW.source_id)
  OR NEW.identity_bytes IS DISTINCT FROM public.ortak_confidential_dm_identity(NEW.company_id,NEW.source_id,NEW.run_id,NEW.key_id)
  OR NEW.source_bytes IS DISTINCT FROM public.ortak_confidential_dm_source(NEW.company_id,NEW.source_id)
  OR NEW.admission_deadline IS DISTINCT FROM j.claim_expires_at THEN
  RAISE EXCEPTION 'Confidential run requires exact current verified claim' USING ERRCODE='check_violation';
 END IF;
 NEW.admitted_at:=clock_timestamp();
 IF NEW.execution_deadline>NEW.admitted_at+interval '10 minutes' THEN
  RAISE EXCEPTION 'Confidential execution deadline exceeds bound' USING ERRCODE='check_violation';
 END IF;
 wrapped:=convert_from(NEW.wrapped_key,'UTF8')::jsonb;
 IF jsonb_typeof(wrapped) IS DISTINCT FROM 'object' OR NOT wrapped ?& ARRAY['ciphertext','format','identity','purpose','signer_ref'] OR wrapped-ARRAY['ciphertext','format','identity','purpose','signer_ref']<>'{}'::jsonb
  OR convert_to(public.ortak_conversation_json75(wrapped),'UTF8')<>NEW.wrapped_key
  OR wrapped->>'format' IS DISTINCT FROM 'ortak-confidential-key-envelope/1'
  OR wrapped->>'purpose' IS DISTINCT FROM 'confidential_master'
  OR convert_to(wrapped->>'identity','UTF8') IS DISTINCT FROM NEW.identity_bytes
  OR wrapped->>'signer_ref' IS DISTINCT FROM s.decrypt_ref
  OR jsonb_typeof(wrapped->'ciphertext') IS DISTINCT FROM 'string'
  OR length(wrapped->>'ciphertext') NOT BETWEEN 132 AND 8192 THEN
  RAISE EXCEPTION 'Confidential wrapped key identity differs' USING ERRCODE='check_violation';
 END IF;
 cipher:=decode(wrapped->>'ciphertext','base64');
 IF octet_length(cipher)<99 OR get_byte(cipher,0)<>2 OR replace(encode(cipher,'base64'),E'\n','')<>wrapped->>'ciphertext' THEN
  RAISE EXCEPTION 'Confidential wrapped key encoding differs' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_run_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_runs
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_guard();

CREATE FUNCTION ortak_confidential_payload_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; initial BOOLEAN; prior INTEGER;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Confidential ciphertext is immutable' USING ERRCODE='check_violation'; END IF;
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id;
 initial:=NEW.purpose='snapshot' AND NOT EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=c.company_id AND source_id=c.source_id);
 IF initial THEN
  IF NOT EXISTS(SELECT 1 FROM public.encrypted_dm_decrypt_jobs j WHERE j.company_id=c.company_id AND j.source_id=c.source_id
   AND j.state='verified' AND j.claim_token=c.claim_token AND j.claim_expires_at>clock_timestamp() AND public.ortak_encrypted_dm_job_current(j)) THEN
   RAISE EXCEPTION 'Confidential initial snapshot claim expired' USING ERRCODE='check_violation';
  END IF;
 ELSIF NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential payload authority retired' USING ERRCODE='check_violation';
 END IF;
 -- Serialize event ordinals without any plaintext parser or per-run unbounded scan.
 PERFORM 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
 IF NEW.community_id<>c.community_id OR NOT public.ortak_confidential_payload_valid(NEW.envelope_bytes,c.identity_bytes,NEW.purpose,NEW.ordinal)
  OR NEW.nonce IS DISTINCT FROM decode(convert_from(NEW.envelope_bytes,'UTF8')::jsonb->>'nonce','base64') THEN
  RAISE EXCEPTION 'Confidential payload wire differs' USING ERRCODE='check_violation';
 END IF;
 IF NEW.purpose='runtime_event' THEN
  IF NOT EXISTS(SELECT 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND status IN('queued','running','waiting')) THEN
   RAISE EXCEPTION 'Confidential runtime event follows terminal run' USING ERRCODE='check_violation';
  END IF;
  SELECT coalesce(max(ordinal),0) INTO prior FROM public.confidential_run_payloads
   WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND purpose='runtime_event';
  IF NEW.ordinal<>prior+1 THEN RAISE EXCEPTION 'Confidential event sequence gap' USING ERRCODE='check_violation'; END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_payload_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_run_payloads
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_payload_guard();

CREATE FUNCTION ortak_confidential_receipt_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE j public.encrypted_dm_decrypt_jobs; c public.confidential_runs; s public.encrypted_dm_selections;
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION 'Confidential receipt is retained' USING ERRCODE='check_violation'; END IF;
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 SELECT selection_id INTO j.selection_id FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
 SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id AND selection_id=j.selection_id FOR SHARE;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id FOR UPDATE;
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id;
 IF j.state<>'verified' OR j.claim_expires_at<=clock_timestamp() OR NOT public.ortak_encrypted_dm_job_current(j)
  OR (j.claim_generation,j.claim_token,j.worker_id) IS DISTINCT FROM (NEW.claim_generation,NEW.claim_token,NEW.claim_worker)
  OR j.community_id<>NEW.community_id OR c.employee_id<>j.employee_id OR c.human_public_key<>s.human_public_key OR c.rumor_id<>j.rumor_id
  OR NEW.duplicate_rumor IS DISTINCT FROM (NEW.source_id<>c.source_id) THEN
  RAISE EXCEPTION 'Confidential receipt needs exact verified rumor' USING ERRCODE='check_violation';
 END IF;
 NEW.committed_at:=clock_timestamp();
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_receipt_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_dm_receipts
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_receipt_guard();

CREATE FUNCTION ortak_confidential_consumed_job() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=OLD.company_id AND source_id=OLD.source_id)
  AND NEW IS DISTINCT FROM OLD THEN
  RAISE EXCEPTION 'Consumed decrypt job cannot be reclaimed' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_consumed_job BEFORE UPDATE ON encrypted_dm_decrypt_jobs
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_consumed_job();

CREATE FUNCTION ortak_confidential_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; j public.encrypted_dm_decrypt_jobs; s public.encrypted_dm_selections; receipt public.confidential_dm_receipts;
BEGIN
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id;
 IF TG_TABLE_NAME='confidential_dm_receipts' THEN
  SELECT * INTO STRICT receipt FROM public.confidential_dm_receipts WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
  SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=NEW.company_id AND source_id=NEW.source_id;
  SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=j.company_id AND selection_id=j.selection_id;
  IF j.claim_expires_at<=clock_timestamp() OR j.valid_before<=clock_timestamp()
   OR coalesce((SELECT generation FROM public.office_authority_generations WHERE company_id=j.company_id),0)<>j.office_generation
   OR NOT s.enabled OR s.generation<>j.selection_generation OR NOT public.ortak_encrypted_dm_pair_current(s)
   OR NOT EXISTS(SELECT 1 FROM public.office_inbox i WHERE i.company_id=j.company_id AND i.event_id=j.source_id
       AND i.state=(CASE WHEN receipt.duplicate_rumor THEN 'dropped' ELSE 'decided' END) AND i.finalized_at IS NOT NULL) THEN
   RAISE EXCEPTION 'Confidential receipt authority expired before commit' USING ERRCODE='serialization_failure';
  END IF;
  IF receipt.duplicate_rumor THEN RETURN NEW; END IF;
 END IF;
 IF NOT public.ortak_confidential_dm_current(c.company_id,c.run_id) THEN
  RAISE EXCEPTION 'Confidential current authority expired before commit' USING ERRCODE='serialization_failure';
 END IF;
 IF TG_TABLE_NAME='confidential_runs' OR (TG_TABLE_NAME='confidential_run_payloads' AND to_jsonb(NEW)->>'purpose'='snapshot') THEN
  IF c.admission_deadline<=clock_timestamp()
   OR NOT EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=c.company_id AND source_id=c.source_id AND run_id=c.run_id AND NOT duplicate_rumor)
   OR NOT EXISTS(SELECT 1 FROM public.confidential_run_payloads WHERE company_id=c.company_id AND run_id=c.run_id AND purpose='snapshot' AND ordinal=0)
   OR NOT EXISTS(SELECT 1 FROM public.confidential_run_dispatches WHERE company_id=c.company_id AND run_id=c.run_id AND state='pending' AND attempts=0) THEN
   RAISE EXCEPTION 'Confidential admission is incomplete or expired' USING ERRCODE='serialization_failure';
  END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER confidential_run_at_commit AFTER INSERT ON confidential_runs
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard();
CREATE CONSTRAINT TRIGGER confidential_payload_at_commit AFTER INSERT ON confidential_run_payloads
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard();
CREATE CONSTRAINT TRIGGER confidential_receipt_at_commit AFTER INSERT ON confidential_dm_receipts
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard();

CREATE FUNCTION ortak_confidential_run_mode_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF TG_OP='UPDATE' AND NEW.payload_mode IS DISTINCT FROM OLD.payload_mode THEN
  RAISE EXCEPTION 'Run payload mode is immutable' USING ERRCODE='check_violation';
 END IF;
 IF NEW.payload_mode='ordinary' THEN RETURN NEW; END IF;
 IF NEW.work_item_id IS NOT NULL OR NEW.routing_decision_id IS NULL OR NEW.message_id IS NULL OR NEW.root_message_id<>NEW.message_id
  OR NEW.error_message IS NOT NULL OR (NEW.error_code IS NOT NULL AND NEW.error_code NOT IN('confidential_failed','confidential_cancelled'))
  OR (NEW.cancel_reason IS NOT NULL AND NEW.cancel_reason NOT IN('office_revoked','human_requested'))
  OR (NEW.runtime_run_ref IS NOT NULL AND NEW.runtime_run_ref!~'^[A-Za-z0-9][A-Za-z0-9:._/-]{0,255}$') THEN
  RAISE EXCEPTION 'Confidential run permits bounded metadata only' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND
  (to_jsonb(NEW)-ARRAY['status','runtime_run_ref','started_at','finished_at','updated_at','delivery_intent','cancel_reason','error_code']) IS DISTINCT FROM
  (to_jsonb(OLD)-ARRAY['status','runtime_run_ref','started_at','finished_at','updated_at','delivery_intent','cancel_reason','error_code']) THEN
  RAISE EXCEPTION 'Confidential run authority is immutable' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND OLD.status IN('completed','failed','cancelled') AND NEW.status<>OLD.status THEN
  RAISE EXCEPTION 'Confidential terminal status cannot revive' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND OLD.runtime_run_ref IS NOT NULL AND NEW.runtime_run_ref IS DISTINCT FROM OLD.runtime_run_ref THEN
  RAISE EXCEPTION 'Confidential start correlation cannot change' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND NEW.status IS DISTINCT FROM OLD.status AND NEW.status IN('running','waiting','completed')
  AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.id) THEN
  RAISE EXCEPTION 'Confidential fresh execution authority retired' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_run_mode_guard BEFORE INSERT OR UPDATE ON runs
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_mode_guard();

-- SQL backstop for every retained ordinary content/effect sink. Existing guards
-- still run for ordinary rows and their legacy snapshot/NUL contracts are intact.
CREATE FUNCTION ortak_confidential_reject_ordinary() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM public.runs r WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.payload_mode='confidential_dm_v1') THEN
  RAISE EXCEPTION 'Confidential run cannot use an ordinary content path' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_no_ordinary_snapshot BEFORE INSERT OR UPDATE ON run_context_snapshots FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_ordinary_events BEFORE INSERT OR UPDATE ON run_events FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_ordinary_office BEFORE INSERT OR UPDATE ON runtime_office_outputs FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_ordinary_work BEFORE INSERT OR UPDATE ON runtime_work_outputs FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_ordinary_memory BEFORE INSERT OR UPDATE ON runtime_memory_writes FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_reviewed_use BEFORE INSERT OR UPDATE ON run_reviewed_memory_uses FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_workspace_use BEFORE INSERT OR UPDATE ON run_workspace_uses FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_workspace_action BEFORE INSERT OR UPDATE ON workspace_tool_actions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_workspace_receipt BEFORE INSERT OR UPDATE ON workspace_tool_receipts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_workspace_reader BEFORE INSERT OR UPDATE ON workspace_reader_executions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_work_execution BEFORE INSERT OR UPDATE ON work_executions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_artifact BEFORE INSERT OR UPDATE ON artifacts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_work_attachment BEFORE INSERT OR UPDATE ON work_attachments FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();
CREATE TRIGGER confidential_no_ordinary_outbox BEFORE INSERT OR UPDATE ON outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE FUNCTION ortak_confidential_dispatch_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE deadline TIMESTAMPTZ; fresh BOOLEAN:=false;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Confidential dispatch is retained' USING ERRCODE='check_violation'; END IF;
 SELECT execution_deadline INTO STRICT deadline FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.generation<>0 OR NEW.lease_token IS NOT NULL OR NEW.error_code IS NOT NULL THEN
   RAISE EXCEPTION 'Confidential dispatch initial state' USING ERRCODE='check_violation';
  END IF;
 ELSE
  IF (NEW.company_id,NEW.community_id,NEW.run_id) IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.run_id)
   OR OLD.state<>'pending' THEN RAISE EXCEPTION 'Confidential dispatch identity or terminal result changed' USING ERRCODE='check_violation'; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.generation=OLD.generation+1 AND NEW.state='pending'
   AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token
   AND OLD.next_attempt_at<=clock_timestamp() AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)<=clock_timestamp())
   AND NEW.lease_expires_at>clock_timestamp() AND NEW.lease_expires_at<=least(deadline,clock_timestamp()+interval '30 seconds') THEN
   fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.generation=OLD.generation AND NEW.lease_token IS NULL THEN
   -- Exact lease accounting remains possible after source/Office revocation.
   -- A delivered result requires a retained start reference; it grants no start.
   IF NEW.state='delivered' AND (OLD.lease_expires_at<=clock_timestamp() OR OLD.lease_token IS NULL
      OR NOT EXISTS(SELECT 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND runtime_run_ref IS NOT NULL)) THEN
    RAISE EXCEPTION 'Confidential delivery needs retained start receipt' USING ERRCODE='check_violation';
   ELSIF NEW.state='pending' AND (NEW.error_code<>'unavailable' OR OLD.lease_token IS NULL OR OLD.lease_expires_at<=clock_timestamp()
     OR NEW.attempts>=3 OR NEW.next_attempt_at<statement_timestamp()+(CASE WHEN NEW.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)) THEN
    RAISE EXCEPTION 'Confidential retry is not bounded lease accounting' USING ERRCODE='check_violation';
   END IF;
  ELSE RAISE EXCEPTION 'Confidential dispatch lease transition refused' USING ERRCODE='check_violation';
  END IF;
 END IF;
 IF NEW.next_attempt_at>deadline+interval '5 seconds' THEN RAISE EXCEPTION 'Confidential retry deadline exceeded' USING ERRCODE='check_violation'; END IF;
 IF fresh AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential dispatch authority retired' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_dispatch_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_run_dispatches
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_dispatch_guard();

CREATE FUNCTION ortak_confidential_dispatch_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL THEN
  IF NEW.lease_expires_at<=clock_timestamp() OR NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.run_id) THEN
   RAISE EXCEPTION 'Confidential dispatch expired before commit' USING ERRCODE='serialization_failure';
  END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER confidential_dispatch_at_commit AFTER UPDATE ON confidential_run_dispatches
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_dispatch_commit_guard();

CREATE FUNCTION ortak_commit_confidential_dm(company UUID,source BYTEA,run UUID,key UUID,identity BYTEA,wrapped BYTEA,snapshot BYTEA,nonce BYTEA)
RETURNS TABLE(committed_run_id UUID,duplicate_rumor BOOLEAN)
LANGUAGE plpgsql VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE j public.encrypted_dm_decrypt_jobs; s public.encrypted_dm_selections; old public.confidential_runs;
 decision UUID; input_hash BYTEA; policy_hash TEXT; binding JSONB;
BEGIN
 PERFORM public.ortak_lock_office_authority(company);
 SELECT selection_id INTO j.selection_id FROM public.encrypted_dm_decrypt_jobs WHERE company_id=company AND source_id=source;
 SELECT * INTO STRICT s FROM public.encrypted_dm_selections WHERE company_id=company AND selection_id=j.selection_id FOR SHARE;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=company AND source_id=source FOR UPDATE;
 IF j.state<>'verified' OR j.claim_expires_at<=clock_timestamp() OR NOT public.ortak_encrypted_dm_job_current(j)
  OR identity IS DISTINCT FROM public.ortak_confidential_dm_identity(company,source,run,key) THEN
  RAISE EXCEPTION 'Confidential verified commit claim changed' USING ERRCODE='check_violation';
 END IF;
 -- Serialize absent rumor discovery before allocating any run/chain. The hash
 -- lock is collision-conservative; the full unique tuple is the authority.
 IF NOT pg_try_advisory_xact_lock(hashtextextended('ortak-confidential-rumor:'||company::text||':'||j.employee_id||':'||encode(s.human_public_key,'hex')||':'||encode(j.rumor_id,'hex'),0)) THEN
  RAISE EXCEPTION 'Confidential rumor commit busy' USING ERRCODE='serialization_failure';
 END IF;
 PERFORM 1 FROM public.office_inbox WHERE company_id=company AND event_id=source FOR UPDATE;
 IF NOT public.ortak_encrypted_dm_job_current(j) THEN RAISE EXCEPTION 'Confidential source claimed elsewhere' USING ERRCODE='serialization_failure'; END IF;
 SELECT * INTO old FROM public.confidential_runs WHERE company_id=company AND employee_id=j.employee_id AND human_public_key=s.human_public_key AND rumor_id=j.rumor_id;
 IF FOUND THEN
  INSERT INTO public.confidential_dm_receipts(company_id,community_id,source_id,run_id,duplicate_rumor,claim_generation,claim_token,claim_worker)
   VALUES(company,j.community_id,source,old.run_id,true,j.claim_generation,j.claim_token,j.worker_id);
  UPDATE public.office_inbox SET state='dropped',finalized_at=clock_timestamp(),last_error=NULL WHERE company_id=company AND event_id=source;
  RETURN QUERY SELECT old.run_id,true; RETURN;
 END IF;
 binding:=public.ortak_confidential_runtime_binding(company,j.employee_revision_id);
 IF binding IS NULL THEN RAISE EXCEPTION 'Confidential selected policy is not empty' USING ERRCODE='check_violation'; END IF;
 decision:=run; input_hash:=public.digest(public.ortak_confidential_dm_source(company,source),'sha256');
 policy_hash:='sha256:'||encode(public.digest(convert_to('ortak-confidential-dm-direct/1','UTF8'),'sha256'),'hex');
 INSERT INTO public.delivery_chains(company_id,root_message_id,policy_version,policy_fingerprint,max_hops,max_wakes,hop_count,wake_count)
  VALUES(company,source,'confidential_dm_v1',policy_hash,1,1,0,0);
 INSERT INTO public.routing_decisions(company_id,id,message_id,root_message_id,inbox_claim_generation,origin_type,origin_id,mode,summary_reason,
  policy_version,policy_fingerprint,input_hash,candidate_revision_ids,wake_count,hop_consumed,chain_hop_count,chain_wake_count,
  office_authority_generation,office_authority_valid_before,office_input_hash)
 VALUES(company,decision,source,source,0,'human',encode(s.human_public_key,'hex'),'deterministic','direct_message',
  'confidential_dm_v1',policy_hash,input_hash,jsonb_build_array(j.employee_revision_id),1,true,1,1,j.office_generation,j.claim_expires_at,j.source_hash);
 INSERT INTO public.routing_recipients(company_id,routing_decision_id,employee_id,position,action,reason,employee_revision_id,employee_lifecycle_epoch)
  VALUES(company,decision,j.employee_id,0,'wake','direct_message',j.employee_revision_id,j.employee_lifecycle_epoch);
 INSERT INTO public.delivery_chain_visits(company_id,root_message_id,employee_id,routing_decision_id,recipient_action,batch_hop)
  VALUES(company,source,j.employee_id,decision,'wake',1);
 UPDATE public.delivery_chains SET hop_count=1,wake_count=1,updated_at=clock_timestamp() WHERE company_id=company AND root_message_id=source;
 INSERT INTO public.runs(company_id,id,employee_id,employee_revision_id,routing_decision_id,message_id,root_message_id,runtime_adapter,
  payload_mode,employee_lifecycle_epoch,office_admission_generation,office_admission_valid_before,office_admission_token)
 VALUES(company,run,j.employee_id,j.employee_revision_id,decision,source,source,binding->>'adapter','confidential_dm_v1',j.employee_lifecycle_epoch,
  j.office_generation,j.claim_expires_at,j.claim_token);
 INSERT INTO public.confidential_runs(company_id,community_id,run_id,source_id,selection_id,employee_id,human_public_key,rumor_id,key_id,
  identity_bytes,source_bytes,wrapped_key,start_key,admission_deadline,execution_deadline,claim_generation,claim_token,claim_worker)
 VALUES(company,j.community_id,run,source,s.selection_id,j.employee_id,s.human_public_key,j.rumor_id,key,identity,
  public.ortak_confidential_dm_source(company,source),wrapped,'ortak-run:'||company::text||':'||run::text,j.claim_expires_at,clock_timestamp()+interval '10 minutes',j.claim_generation,j.claim_token,j.worker_id);
 INSERT INTO public.confidential_run_payloads(company_id,community_id,run_id,purpose,ordinal,envelope_bytes,nonce)
 VALUES(company,j.community_id,run,'snapshot',0,snapshot,nonce);
 INSERT INTO public.confidential_run_dispatches(company_id,community_id,run_id) VALUES(company,j.community_id,run);
 INSERT INTO public.confidential_dm_receipts(company_id,community_id,source_id,run_id,duplicate_rumor,claim_generation,claim_token,claim_worker)
 VALUES(company,j.community_id,source,run,false,j.claim_generation,j.claim_token,j.worker_id);
 UPDATE public.office_inbox SET state='decided',finalized_at=clock_timestamp(),last_error=NULL WHERE company_id=company AND event_id=source;
 RETURN QUERY SELECT run,false;
END
$$;

CREATE INDEX confidential_dispatch_due ON confidential_run_dispatches(company_id,next_attempt_at,run_id) WHERE state='pending';
CREATE INDEX confidential_runs_selection ON confidential_runs(company_id,selection_id,run_id);
CREATE TRIGGER confidential_runs_no_truncate BEFORE TRUNCATE ON confidential_runs FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER confidential_payloads_no_truncate BEFORE TRUNCATE ON confidential_run_payloads FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER confidential_receipts_no_truncate BEFORE TRUNCATE ON confidential_dm_receipts FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER confidential_dispatches_no_truncate BEFORE TRUNCATE ON confidential_run_dispatches FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_confidential_run_complete_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; j public.encrypted_dm_decrypt_jobs;
BEGIN
 IF NEW.payload_mode='ordinary' THEN RETURN NEW; END IF;
 SELECT * INTO c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.id;
 IF NOT FOUND THEN RAISE EXCEPTION 'Confidential run has no protected admission' USING ERRCODE='check_violation'; END IF;
 SELECT * INTO STRICT j FROM public.encrypted_dm_decrypt_jobs WHERE company_id=c.company_id AND source_id=c.source_id;
 IF NOT EXISTS(SELECT 1 FROM public.routing_decisions d
  JOIN public.routing_recipients rr ON rr.company_id=d.company_id AND rr.routing_decision_id=d.id AND rr.employee_id=j.employee_id
  JOIN public.delivery_chain_visits v ON v.company_id=d.company_id AND v.root_message_id=d.root_message_id AND v.employee_id=rr.employee_id AND v.routing_decision_id=d.id
  JOIN public.delivery_chains ch ON ch.company_id=d.company_id AND ch.root_message_id=d.root_message_id
  WHERE d.company_id=c.company_id AND d.id=NEW.routing_decision_id AND d.message_id=j.source_id AND d.root_message_id=j.source_id
   AND d.id=NEW.id AND d.mode='deterministic' AND d.summary_reason='direct_message'
   AND d.policy_version='confidential_dm_v1' AND d.inbox_claim_generation=0 AND d.origin_type='human' AND d.origin_id=encode(c.human_public_key,'hex')
   AND d.input_hash=public.digest(c.source_bytes,'sha256') AND d.office_input_hash=j.source_hash
   AND d.wake_count=1 AND d.hop_consumed AND d.chain_hop_count=1 AND d.chain_wake_count=1
   AND rr.action='wake' AND rr.employee_revision_id=j.employee_revision_id AND rr.employee_lifecycle_epoch=j.employee_lifecycle_epoch
   AND v.batch_hop=1 AND ch.hop_count=1 AND ch.wake_count=1 AND ch.max_hops=1 AND ch.max_wakes=1)
  OR NOT public.ortak_confidential_dm_current(c.company_id,c.run_id) THEN
  RAISE EXCEPTION 'Confidential admission routing provenance differs' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER confidential_run_complete_at_commit AFTER INSERT ON runs
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_complete_guard();

CREATE FUNCTION ortak_confidential_run_transition_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF NEW.payload_mode='confidential_dm_v1' AND NEW.status IS DISTINCT FROM OLD.status AND NEW.status IN('running','waiting','completed')
  AND NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.id) THEN
  RAISE EXCEPTION 'Confidential execution expired before commit' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER confidential_run_transition_at_commit AFTER UPDATE ON runs
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_transition_guard();

-- Preserve the migration53 trigger OID and ordinary claim semantics. A
-- confidential wake consumes its distinct verified decrypt lease, never a
-- fabricated ordinary inbox claim or a policy-name-only exception.
CREATE OR REPLACE FUNCTION ortak_check_routing_claim_expiry() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    current_claim RECORD;
BEGIN
    IF NEW.wake_count = 0 OR NEW.office_authority_generation IS NULL THEN
        RETURN NEW;
    END IF;
    IF EXISTS(SELECT 1 FROM public.runs r WHERE r.company_id=NEW.company_id
        AND r.id=NEW.id AND r.routing_decision_id=NEW.id AND r.payload_mode='confidential_dm_v1') THEN
        PERFORM public.ortak_lock_office_authority(NEW.company_id);
        IF NOT EXISTS(SELECT 1 FROM public.confidential_runs c
          JOIN public.runs r ON r.company_id=c.company_id AND r.id=c.run_id
          JOIN public.encrypted_dm_decrypt_jobs j ON j.company_id=c.company_id AND j.source_id=c.source_id
          JOIN public.confidential_dm_receipts receipt ON receipt.company_id=c.company_id AND receipt.source_id=c.source_id AND receipt.run_id=c.run_id
          JOIN public.office_inbox i ON i.company_id=c.company_id AND i.event_id=c.source_id
          WHERE c.company_id=NEW.company_id AND c.run_id=NEW.id AND c.source_id=NEW.message_id
            AND NEW.root_message_id=c.source_id AND NEW.inbox_claim_generation=0
            AND NEW.policy_version='confidential_dm_v1' AND NEW.mode='deterministic'
            AND NEW.origin_type='human' AND NEW.origin_id=encode(c.human_public_key,'hex')
            AND NEW.wake_count=1 AND NEW.hop_consumed
            AND NEW.office_authority_generation=j.office_generation
            AND NEW.office_authority_valid_before=j.claim_expires_at AND NEW.office_input_hash=j.source_hash
            AND NEW.input_hash=public.digest(c.source_bytes,'sha256')
            AND j.state='verified' AND j.claim_expires_at>clock_timestamp() AND j.valid_before>clock_timestamp()
            AND c.admission_deadline=j.claim_expires_at AND c.admission_deadline>clock_timestamp()
            AND (c.claim_generation,c.claim_token,c.claim_worker)=(j.claim_generation,j.claim_token,j.worker_id)
            AND (receipt.claim_generation,receipt.claim_token,receipt.claim_worker)=(j.claim_generation,j.claim_token,j.worker_id)
            AND NOT receipt.duplicate_rumor
            AND (r.employee_id,r.employee_revision_id,r.employee_lifecycle_epoch,r.office_admission_token)=
                (j.employee_id,j.employee_revision_id,j.employee_lifecycle_epoch,j.claim_token)
            AND i.state='decided' AND i.event_kind=1059 AND i.channel_id IS NULL
            AND i.event_created_at=j.source_created_at AND i.author_pubkey=j.source_author
            AND public.ortak_confidential_dm_current(c.company_id,c.run_id)) THEN
            RAISE EXCEPTION 'ortak: confidential decrypt claim changed or expired before commit'
                USING ERRCODE='serialization_failure';
        END IF;
        RETURN NEW;
    END IF;
    -- Unchanged migration53 ordinary inbox-claim branch.
    SELECT state, claim_generation, claim_expires_at INTO current_claim
    FROM office_inbox
    WHERE company_id = NEW.company_id AND event_id = NEW.message_id
    FOR UPDATE;
    IF NOT FOUND OR current_claim.state NOT IN ('claimed', 'decided')
       OR current_claim.claim_generation IS DISTINCT FROM NEW.inbox_claim_generation
       OR current_claim.claim_expires_at IS NULL
       OR clock_timestamp() >= current_claim.claim_expires_at THEN
        RAISE EXCEPTION 'ortak: waking routing claim changed or expired before commit'
            USING ERRCODE = 'serialization_failure';
    END IF;
    RETURN NEW;
END
$$;

-- Source: docs/ortak/sql/employee_reviewed_memory_runtime_candidate.sql
-- SHA256: 28f9bfa93aa8d20c399a183eeb7a7a325a695b1cbf0f0bbab69f3976b4c4236c
-- SOURCE ONLY. Assemble after employee storage, authority and protocol candidates.
-- No numbered migration, runtime opt-in or current deployment is implied.
-- SQL checks relational authority; trusted configured worker selection is separate.

ALTER TABLE employee_reviewed_memory_targets ADD COLUMN runtime_consumption_enabled BOOLEAN NOT NULL DEFAULT false;

CREATE FUNCTION ortak_employee_memory_run_origin(company UUID, run UUID, destination UUID)
RETURNS TABLE(origin_bytes BYTEA,observed_at TIMESTAMPTZ,valid_before TIMESTAMPTZ)
LANGUAGE sql STABLE AS $$
    WITH base AS MATERIALIZED (
        SELECT r.*, b.community_id,$3 AS destination_channel_id,active.manifest AS active_manifest,
            pinned.manifest AS pinned_manifest
        FROM runs r
        JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id
            AND e.status='active' AND e.lifecycle_epoch=r.employee_lifecycle_epoch
        JOIN employee_revisions pinned ON pinned.company_id=r.company_id
            AND pinned.employee_id=r.employee_id AND pinned.id=r.employee_revision_id
        JOIN employee_revisions active ON active.company_id=e.company_id
            AND active.employee_id=e.id AND active.id=e.active_revision_id
        JOIN office_company_bindings b ON b.company_id=r.company_id
        JOIN office_routing_cohorts cohort ON cohort.company_id=r.company_id
            AND cohort.community_id=b.community_id AND cohort.state='enabled'
        JOIN office_routing_channels ch ON ch.company_id=cohort.company_id
            AND ch.community_id=cohort.community_id AND ch.channel_id=$3
        JOIN office_routing_employees selected ON selected.company_id=r.company_id
            AND selected.employee_id=r.employee_id
        WHERE r.company_id=$1 AND r.id=$2
            AND coalesce(to_jsonb(r)->>'payload_mode','ordinary')='ordinary'
            AND pinned.manifest->'office'=active.manifest->'office'
            AND pinned.manifest->'memory'=active.manifest->'memory'
            AND NOT EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=r.company_id AND c.run_id=r.id)
            AND NOT EXISTS(SELECT 1 FROM run_cancel_requests c WHERE c.company_id=r.company_id AND c.run_id=r.id)
    ), origins AS (
        SELECT i.author_pubkey AS human,r.message_id AS source,i.event_created_at AS source_created_at,r.employee_id
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
            AND i.channel_id=r.destination_channel_id AND i.state='decided'
            AND d.origin_id=encode(i.author_pubkey,'hex')
        WHERE r.work_item_id IS NULL AND r.status IN('queued','running','waiting','completed')
        UNION ALL
        SELECT decode(x.requested_by,'hex'), w.source_message_id, input.event_created_at, r.employee_id
        FROM base r JOIN work_executions x ON x.company_id=r.company_id AND x.run_id=r.id
            AND x.work_item_id=r.work_item_id
            AND x.employee_id=r.employee_id AND x.employee_revision_id=r.employee_revision_id
        JOIN work_items w ON w.company_id=x.company_id AND w.project_id=x.project_id AND w.id=x.work_item_id
        JOIN project_api_bindings project_binding ON project_binding.company_id=x.company_id
            AND project_binding.project_id=x.project_id AND project_binding.community_id=r.community_id
            AND project_binding.channel_id=r.destination_channel_id
        JOIN office_inbox input ON input.company_id=x.company_id AND input.event_id=w.source_message_id
            AND input.author_pubkey=decode(x.requested_by,'hex') AND input.state='decided'
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
    SELECT convert_to(ortak_conversation_json75(jsonb_build_object(
        'format','ortak-reviewed-employee-run-origin/1','company_id',$1,
        'employee_id',o.employee_id,'destination_channel_id',$3,
        'requester_public_key',encode(o.human,'hex'),
        'source_authority_epoch',source_scope.epoch,'destination_authority_epoch',destination_scope.epoch,
        'source',jsonb_build_object('community_id',s.community_id,'channel_id',s.source_channel_id,
            'event_id',encode(o.source,'hex'),'event_created_at',ortak_employee_memory_timestamp(o.source_created_at),
            'author_public_key',encode(s.source_author_public_key,'hex'),
            'evidence_hash',encode(s.source_evidence_hash,'hex')))),'UTF8'),s.observed_at,s.valid_before
    FROM unique_origin o CROSS JOIN LATERAL
        ortak_employee_memory_observation($1,o.employee_id,o.human,o.source,o.source_created_at,$3,'experience',NULL) s
    JOIN employee_memory_channel_authorities source_scope ON source_scope.company_id=$1
        AND source_scope.community_id=s.community_id AND source_scope.employee_id=o.employee_id
        AND source_scope.channel_id=s.source_channel_id
    JOIN employee_memory_channel_authorities destination_scope ON destination_scope.company_id=$1
        AND destination_scope.community_id=s.community_id AND destination_scope.employee_id=o.employee_id
        AND destination_scope.channel_id=$3
    WHERE s.valid_before IS NULL OR s.valid_before>clock_timestamp()
$$;

-- Explicit opt-in changes retire previous use epochs; initial registration is closed.
CREATE OR REPLACE FUNCTION ortak_employee_memory_target_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE expected_namespace BYTEA; expected_binding BYTEA; registration JSONB; diagnostic JSONB;
    observed TIMESTAMPTZ; cleanup_hash TEXT; recovery_only BOOLEAN=false;
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    expected_namespace=convert_to(ortak_conversation_json75(jsonb_build_object(
        'format','ortak-reviewed-employee-namespace/1','company_id',NEW.company_id,'employee_id',NEW.employee_id)),'UTF8');
    expected_binding=sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
        'binding',NEW.binding,'namespace_hash',encode(NEW.namespace_hash,'hex'),'protocol',NEW.protocol)),'UTF8'));
    IF NEW.namespace_bytes IS DISTINCT FROM expected_namespace OR NEW.binding_hash IS DISTINCT FROM expected_binding THEN
        RAISE EXCEPTION 'employee memory target namespace differs' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='INSERT' THEN
        registration=NEW.registration_receipt; diagnostic=registration->'diagnostic';
        IF jsonb_typeof(registration)<>'object' OR (SELECT count(*) FROM jsonb_object_keys(registration))<>3
            OR registration->>'format' IS DISTINCT FROM 'ortak-employee-namespace-registration/1'
            OR jsonb_typeof(diagnostic)<>'object' OR (SELECT count(*) FROM jsonb_object_keys(diagnostic))<>8
            OR diagnostic->>'operation_id' IS NULL OR diagnostic->>'employee_revision_id' IS DISTINCT FROM NEW.employee_revision_id::text
            OR diagnostic->>'employee_lifecycle_epoch' IS DISTINCT FROM NEW.employee_lifecycle_epoch::text
            OR diagnostic->>'erased' IS DISTINCT FROM 'true'
            OR NOT coalesce(diagnostic->>'challenge_hash' ~ '^[0-9a-f]{64}$',false)
            OR NOT coalesce(diagnostic->>'write_request_hash' ~ '^[0-9a-f]{64}$',false)
            OR NOT coalesce(diagnostic->>'withdraw_request_hash' ~ '^[0-9a-f]{64}$',false)
            OR diagnostic->>'tombstone_at' IS NULL OR registration->>'validated_at' IS NULL THEN
            RAISE EXCEPTION 'employee namespace registration metadata invalid' USING ERRCODE='check_violation';
        END IF;
        observed=(registration->>'validated_at')::timestamptz;
        IF (diagnostic->>'operation_id')::uuid='00000000-0000-0000-0000-000000000000'::uuid
            OR ortak_employee_memory_timestamp(observed) IS DISTINCT FROM registration->>'validated_at'
            OR ortak_employee_memory_timestamp((diagnostic->>'tombstone_at')::timestamptz) IS NULL
            OR observed>clock_timestamp()+interval '5 seconds' OR observed<=clock_timestamp()-interval '55 seconds'
            OR NEW.valid_until<=clock_timestamp() OR NEW.valid_until>observed+interval '90 days'
            OR NEW.consumption_epoch<>0 OR NEW.runtime_consumption_enabled THEN
            RAISE EXCEPTION 'employee namespace initial witness expired or selection invalid' USING ERRCODE='check_violation';
        END IF;
        cleanup_hash=encode(sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
            'format','ortak-reviewed-employee-diagnostic-withdraw/1','operation_id',(diagnostic->>'operation_id')::uuid,
            'namespace_hash',encode(NEW.namespace_hash,'hex'),'binding_hash',encode(NEW.binding_hash,'hex'),
            'employee_revision_id',NEW.employee_revision_id,'employee_lifecycle_epoch',NEW.employee_lifecycle_epoch,
            'challenge_hash',diagnostic->>'challenge_hash')),'UTF8')),'hex');
        IF diagnostic->>'withdraw_request_hash' IS DISTINCT FROM cleanup_hash THEN
            RAISE EXCEPTION 'employee namespace cleanup commitment differs' USING ERRCODE='check_violation';
        END IF;
    ELSE
        recovery_only=OLD.runtime_consumption_enabled AND NOT NEW.runtime_consumption_enabled
            AND (to_jsonb(NEW)-'runtime_consumption_enabled'-'updated_at')=(to_jsonb(OLD)-'runtime_consumption_enabled'-'updated_at');
        -- Includes registration receipt and original selection expiry. A model
        -- refresh cannot create ownership, renew an expired selection or rewrite
        -- the original I/O evidence. Explicit future renewal is a separate API.
        IF (to_jsonb(NEW)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'runtime_consumption_enabled'-'updated_at'-'consumption_epoch')
            IS DISTINCT FROM (to_jsonb(OLD)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'runtime_consumption_enabled'-'updated_at'-'consumption_epoch')
            OR NEW.consumption_epoch<>OLD.consumption_epoch THEN
            RAISE EXCEPTION 'employee memory target identity is immutable' USING ERRCODE='check_violation';
        END IF;
        IF (NEW.enabled,NEW.runtime_consumption_enabled,NEW.employee_lifecycle_epoch) IS DISTINCT FROM (OLD.enabled,OLD.runtime_consumption_enabled,OLD.employee_lifecycle_epoch) THEN
            IF OLD.consumption_epoch=9223372036854775807 THEN
                RAISE EXCEPTION 'employee memory target epoch exhausted' USING ERRCODE='program_limit_exceeded';
            END IF;
            NEW.consumption_epoch=OLD.consumption_epoch+1;
        END IF;
    END IF;
    IF NOT recovery_only AND (TG_OP='INSERT' OR NEW.enabled) AND NOT coalesce(ortak_employee_memory_target_authorized(
        NEW.company_id,NEW.employee_id,NEW.deployment_id,NEW.namespace_bytes,NEW.binding,NEW.creation_receipt,
        NEW.employee_revision_id,NEW.employee_lifecycle_epoch,NEW.destination_channel_id,NEW.valid_until),false) THEN
        RAISE EXCEPTION 'employee namespace current binding unavailable' USING ERRCODE='check_violation';
    END IF;
    NEW.updated_at=clock_timestamp(); RETURN NEW;
END $$;

CREATE FUNCTION ortak_employee_reviewed_runtime_eligible(company UUID, run UUID, fact UUID, target UUID,
    source_epoch BIGINT,destination_epoch BIGINT,target_epoch BIGINT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
        JOIN runs r ON r.company_id=f.company_id AND r.id=$2 AND r.employee_id=f.employee_id
        JOIN employees e ON e.company_id=f.company_id AND e.id=f.employee_id AND e.status='active'
            AND e.lifecycle_epoch=r.employee_lifecycle_epoch
        JOIN employee_reviewed_memory_exports x ON x.company_id=f.company_id AND x.fact_id=f.id
            AND x.employee_id=f.employee_id AND x.community_id=f.community_id
            AND x.destination_channel_id=f.destination_channel_id
            AND x.content_hash=f.content_hash AND x.source_hash=f.source_hash AND x.sharing_hash=f.sharing_hash
        JOIN employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
            AND t.employee_id=f.employee_id AND t.community_id=f.community_id AND t.destination_channel_id=f.destination_channel_id
        JOIN employee_reviewed_memory_export_receipts ack ON ack.company_id=x.company_id AND ack.fact_id=x.fact_id
            AND ack.action='publish' AND ack.remote_status='active' AND NOT ack.erased_from_reviewed_store
            AND ack.binding_hash=t.binding_hash AND ack.content_hash=f.content_hash
        JOIN employee_memory_channel_authorities source_scope ON source_scope.company_id=f.company_id
            AND source_scope.community_id=f.community_id AND source_scope.employee_id=f.employee_id
            AND source_scope.channel_id=f.source_channel_id AND source_scope.epoch=$5
        JOIN employee_memory_channel_authorities destination_scope ON destination_scope.company_id=f.company_id
            AND destination_scope.community_id=f.community_id AND destination_scope.employee_id=f.employee_id
            AND destination_scope.channel_id=f.destination_channel_id AND destination_scope.epoch=$6
        CROSS JOIN LATERAL ortak_employee_memory_run_origin($1,$2,f.destination_channel_id) run_origin
        CROSS JOIN LATERAL (SELECT convert_from(run_origin.origin_bytes,'UTF8')::jsonb AS value) origin
        CROSS JOIN LATERAL ortak_employee_memory_observation(f.company_id,f.employee_id,f.approved_by,
            f.source_event_id,f.source_event_created_at,f.destination_channel_id,f.kind,f.human_public_key) observed
        WHERE f.company_id=$1 AND f.id=$3 AND t.id=$4 AND f.version=1 AND f.revoked_at IS NULL
            AND f.expires_at>clock_timestamp() AND t.enabled AND t.runtime_consumption_enabled
            AND t.consumption_epoch=$7 AND t.employee_lifecycle_epoch=e.lifecycle_epoch
            AND coalesce(to_jsonb(r)->>'payload_mode','ordinary')='ordinary'
            -- A model-only change keeps namespace identity. Use still requires
            -- the exact current memory binding and unchanged lifecycle/expiry.
            AND ortak_employee_memory_target_authorized(t.company_id,t.employee_id,t.deployment_id,t.namespace_bytes,
                t.binding,t.creation_receipt,e.active_revision_id,e.lifecycle_epoch,t.destination_channel_id,t.valid_until)
            AND observed.community_id=f.community_id AND observed.source_channel_id=f.source_channel_id
            AND observed.source_author_public_key=f.source_author_public_key AND observed.source_evidence_hash=f.source_evidence_hash
            AND observed.employee_revision_id=e.active_revision_id AND observed.employee_lifecycle_epoch=e.lifecycle_epoch
            AND (observed.valid_before IS NULL OR observed.valid_before>clock_timestamp())
            AND (run_origin.valid_before IS NULL OR run_origin.valid_before>clock_timestamp())
            AND (f.kind='experience' OR f.kind='relationship'
                AND origin.value->>'requester_public_key'=encode(f.human_public_key,'hex'))
            AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_receipts stop
                WHERE stop.company_id=f.company_id AND stop.fact_id=f.id AND stop.action='withdraw'))
$$;

-- Exact immutable namespace-specific uses. Ordinal is global within the v5
-- reviewed union; the deferred snapshot guard excludes cross-table collisions.
CREATE TABLE run_employee_reviewed_memory_uses (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 7),
    fact_id UUID NOT NULL,
    target_id UUID NOT NULL,
    fact_version BIGINT NOT NULL CHECK(fact_version=1),
    content_hash BYTEA NOT NULL CHECK(octet_length(content_hash)=32),
    source_hash BYTEA NOT NULL CHECK(octet_length(source_hash)=32),
    sharing_hash BYTEA NOT NULL CHECK(octet_length(sharing_hash)=32),
    audience_hash BYTEA NOT NULL CHECK(octet_length(audience_hash)=32),
    binding_hash BYTEA NOT NULL CHECK(octet_length(binding_hash)=32),
    namespace_hash BYTEA NOT NULL CHECK(octet_length(namespace_hash)=32),
    approval_id UUID NOT NULL,
    approved_by TEXT NOT NULL CHECK(approved_by ~ '^[0-9a-f]{64}$'),
    expires_at TIMESTAMPTZ NOT NULL,
    source_authority_epoch BIGINT NOT NULL CHECK(source_authority_epoch>=0),
    destination_authority_epoch BIGINT NOT NULL CHECK(destination_authority_epoch>=0),
    consumption_epoch BIGINT NOT NULL CHECK(consumption_epoch>=0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id,ordinal),
    UNIQUE(company_id,run_id,fact_id),
    FOREIGN KEY(company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY(company_id,run_id) REFERENCES run_context_snapshots(company_id,run_id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(company_id,fact_id) REFERENCES employee_reviewed_memory_facts(company_id,id),
    FOREIGN KEY(company_id,fact_id) REFERENCES employee_reviewed_memory_exports(company_id,fact_id),
    FOREIGN KEY(company_id,target_id) REFERENCES employee_reviewed_memory_targets(company_id,id)
);
CREATE INDEX employee_memory_use_fact ON run_employee_reviewed_memory_uses(company_id,fact_id,run_id);
CREATE INDEX employee_memory_use_expiry ON run_employee_reviewed_memory_uses(company_id,expires_at,run_id);
CREATE TRIGGER employee_memory_use_immutable BEFORE UPDATE OR DELETE ON run_employee_reviewed_memory_uses
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER employee_memory_use_no_truncate BEFORE TRUNCATE ON run_employee_reviewed_memory_uses
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
SELECT attach_community_write_fence('run_employee_reviewed_memory_uses');

-- Independent ordinary-payload guard works before or after the confidential
-- candidate is assembled. Missing payload_mode means immutable76 ordinary;
-- an explicitly confidential row never becomes eligible through that fallback.
CREATE FUNCTION ortak_employee_use_ordinary() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM runs r WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id
        AND coalesce(to_jsonb(r)->>'payload_mode','ordinary')='ordinary') THEN
        RAISE EXCEPTION 'employee memory requires ordinary run' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER employee_memory_use_ordinary BEFORE INSERT ON run_employee_reviewed_memory_uses
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_use_ordinary();

CREATE FUNCTION ortak_run_employee_memory_current(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT NOT EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u
        LEFT JOIN employee_reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
        LEFT JOIN employee_reviewed_memory_targets t ON t.company_id=u.company_id AND t.id=u.target_id
        LEFT JOIN run_context_snapshots s ON s.company_id=u.company_id AND s.run_id=u.run_id
        WHERE u.company_id=$1 AND u.run_id=$2 AND (
            f.id IS NULL OR t.id IS NULL OR s.run_id IS NULL OR f.community_id IS DISTINCT FROM u.community_id
            OR f.version IS DISTINCT FROM u.fact_version OR f.approval_id IS DISTINCT FROM u.approval_id
            OR encode(f.approved_by,'hex') IS DISTINCT FROM u.approved_by OR f.expires_at IS DISTINCT FROM u.expires_at
            OR f.content_hash IS DISTINCT FROM u.content_hash OR f.source_hash IS DISTINCT FROM u.source_hash
            OR f.sharing_hash IS DISTINCT FROM u.sharing_hash OR f.audience_hash IS DISTINCT FROM u.audience_hash
            OR t.binding_hash IS DISTINCT FROM u.binding_hash OR t.namespace_hash IS DISTINCT FROM u.namespace_hash
            OR NOT coalesce(ortak_employee_reviewed_runtime_eligible($1,$2,u.fact_id,u.target_id,
                u.source_authority_epoch,u.destination_authority_epoch,u.consumption_epoch),false)
            OR NOT EXISTS(SELECT 1 FROM ortak_employee_memory_run_origin($1,$2,f.destination_channel_id) origin
                WHERE ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json)#>'{employee,origin}'
                    =ortak_snapshot_scratch_jsonb(to_json(convert_from(origin.origin_bytes,'UTF8'))))))
$$;

-- Existing guard OIDs remain intact. Legacy-only rows retain their existing branches.
CREATE OR REPLACE FUNCTION ortak_run_reviewed_memory_current(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT ortak_run_employee_memory_current(company,run) AND NOT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u
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
    PERFORM a.channel_id FROM employee_memory_channel_authorities a WHERE a.company_id=company
        AND EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u
            JOIN employee_reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
            CROSS JOIN LATERAL ortak_employee_memory_run_origin(company,run,f.destination_channel_id) origin
            WHERE u.company_id=company AND u.run_id=run AND f.employee_id=a.employee_id
                AND f.community_id=a.community_id AND (a.channel_id IN(f.source_channel_id,f.destination_channel_id)
                    OR a.channel_id=(convert_from(origin.origin_bytes,'UTF8')::jsonb#>>'{source,channel_id}')::uuid))
        ORDER BY a.employee_id,a.channel_id FOR SHARE OF a NOWAIT;
    PERFORM f.id FROM reviewed_memory_facts f JOIN run_reviewed_memory_uses u ON u.company_id=f.company_id AND u.fact_id=f.id
        WHERE u.company_id=company AND u.run_id=run ORDER BY f.id FOR SHARE OF f NOWAIT;
    PERFORM t.id FROM reviewed_memory_targets t WHERE t.company_id=company AND EXISTS
        (SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=company AND u.run_id=run AND u.target_id=t.id)
        ORDER BY t.id FOR SHARE OF t NOWAIT;
    PERFORM f.id FROM employee_reviewed_memory_facts f JOIN run_employee_reviewed_memory_uses u
        ON u.company_id=f.company_id AND u.fact_id=f.id WHERE u.company_id=company AND u.run_id=run
        ORDER BY f.id FOR SHARE OF f NOWAIT;
    PERFORM t.id FROM employee_reviewed_memory_targets t WHERE t.company_id=company AND EXISTS
        (SELECT 1 FROM run_employee_reviewed_memory_uses u WHERE u.company_id=company AND u.run_id=run AND u.target_id=t.id)
        ORDER BY t.id FOR SHARE OF t NOWAIT;
    RETURN ortak_run_reviewed_memory_current(company,run);
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_run_admission() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE selected_run UUID; conversation BOOLEAN;
BEGIN
    IF TG_TABLE_NAME='runs' THEN selected_run=NEW.id; ELSE selected_run=NEW.run_id; END IF;
    SELECT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
        AND u.run_id=selected_run AND u.conversation_audience_hash IS NOT NULL) OR EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u
        WHERE u.company_id=NEW.company_id AND u.run_id=selected_run) INTO conversation;
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

CREATE OR REPLACE FUNCTION ortak_conversation_effect_admission76() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE effect BOOLEAN=false; previous JSONB; proposed JSONB;
BEGIN
    IF NOT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
        AND u.run_id=NEW.run_id AND u.conversation_audience_hash IS NOT NULL)
        AND NOT EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
            AND u.run_id=NEW.run_id) THEN RETURN NEW; END IF;
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
CREATE FUNCTION ortak_employee_snapshot_v5(company UUID, run UUID, wire JSONB)
RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE
    r runs; revision employee_revisions; work work_executions;
    selected_project UUID; origin RECORD; context JSONB; record JSONB; pin JSONB;
    wrapped JSONB; rendered JSONB; expected_pin JSONB; expected_record JSONB;
    u run_reviewed_memory_uses; f reviewed_memory_facts; a reviewed_memory_conversation_audiences;
    eu run_employee_reviewed_memory_uses; ef employee_reviewed_memory_facts; selected_destination UUID; employees INTEGER=0;
    previous_priority INTEGER=-1; previous_employee UUID; priority INTEGER;
    used_count INTEGER; scratch_count INTEGER; i INTEGER=0; conversations INTEGER=0;
    reviewed_bytes INTEGER=0; total_bytes INTEGER=0; content TEXT; seen UUID[]=ARRAY[]::uuid[];
BEGIN
    SELECT * INTO r FROM runs x WHERE x.company_id=company AND x.id=run;
    SELECT * INTO revision FROM employee_revisions x WHERE x.company_id=company
        AND x.employee_id=r.employee_id AND x.id=r.employee_revision_id;
    context=wire->'employee';
    IF r.id IS NULL OR revision.id IS NULL OR r.status NOT IN('queued','running','waiting')
        OR wire->'version' IS DISTINCT FROM '5'::jsonb
        OR wire ? 'reviewed' OR wire ? 'conversation'
        OR coalesce(to_jsonb(r)->>'payload_mode','ordinary')<>'ordinary' OR jsonb_typeof(context) IS DISTINCT FROM 'object'
        OR (context-'origin'-'conversation_origin'-'records'-'truncated')<>'{}'::jsonb
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
    SELECT (SELECT count(*) FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run)
        +(SELECT count(*) FROM run_employee_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run) INTO used_count;
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
    SELECT min(fact.destination_channel_id::text)::uuid INTO selected_destination
        FROM run_employee_reviewed_memory_uses used JOIN employee_reviewed_memory_facts fact
            ON fact.company_id=used.company_id AND fact.id=used.fact_id
        WHERE used.company_id=company AND used.run_id=run
        HAVING count(DISTINCT fact.destination_channel_id)=1;
    SELECT * INTO origin FROM ortak_employee_memory_run_origin(company,run,selected_destination);
    IF NOT FOUND OR context->'origin' IS DISTINCT FROM
        ortak_snapshot_scratch_jsonb(to_json(convert_from(origin.origin_bytes,'UTF8'))) THEN
        RAISE EXCEPTION 'employee snapshot actual origin differs' USING ERRCODE='check_violation';
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
        IF work.run_id IS NULL OR (selected_project IS NOT NULL AND work.project_id<>selected_project)
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
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',(used_count+i)::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type','run_scratch_memory','trust','untrusted_data','record',record)
            OR octet_length(wire#>>ARRAY['spec','context','memory_context',(used_count+i)::text])>8192
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
        IF wrapped->>'scope'='employee' THEN
            SELECT * INTO eu FROM run_employee_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run AND x.ordinal=i;
            SELECT * INTO ef FROM employee_reviewed_memory_facts x WHERE x.company_id=company AND x.id=eu.fact_id;
            priority=CASE WHEN ef.kind='relationship' THEN 0 ELSE 1 END;
            IF eu.run_id IS NULL OR ef.id IS NULL OR ef.destination_channel_id<>selected_destination
                OR eu.fact_id=ANY(seen) OR i<>employees OR priority<previous_priority
                OR (priority=previous_priority AND eu.fact_id<=previous_employee)
                OR NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_targets target WHERE target.company_id=company
                    AND target.id=eu.target_id AND ortak_snapshot_scratch_jsonb(target.binding::json)=wire->'memory_binding') THEN
                RAISE EXCEPTION 'employee snapshot retained identity or order differs' USING ERRCODE='check_violation';
            END IF;
            seen=array_append(seen,eu.fact_id); previous_priority=priority; previous_employee=eu.fact_id;
            expected_pin=jsonb_build_object('fact_id',eu.fact_id,'target_id',eu.target_id,'fact_version',eu.fact_version,
                'content_hash',encode(eu.content_hash,'hex'),'source_hash',encode(eu.source_hash,'hex'),
                'sharing_hash',encode(eu.sharing_hash,'hex'),'audience_hash',encode(eu.audience_hash,'hex'),
                'binding_hash',encode(eu.binding_hash,'hex'),'namespace_hash',encode(eu.namespace_hash,'hex'),
                'approval_id',eu.approval_id,'approved_by',eu.approved_by,'expires_at',pin->>'expires_at',
                'source_authority_epoch',eu.source_authority_epoch,'destination_authority_epoch',eu.destination_authority_epoch,
                'consumption_epoch',eu.consumption_epoch);
            expected_record=jsonb_build_object('pin',expected_pin,'content',ef.content,'provenance',convert_from(ef.provenance_bytes,'UTF8'));
            IF record IS DISTINCT FROM ortak_snapshot_scratch_jsonb(expected_record::json)
                OR wrapped IS DISTINCT FROM jsonb_build_object('scope','employee','record',record)
                OR (pin->>'expires_at')::timestamptz IS DISTINCT FROM eu.expires_at THEN
                RAISE EXCEPTION 'employee snapshot retained bytes differ' USING ERRCODE='check_violation';
            END IF;
            employees=employees+1; reviewed_bytes=reviewed_bytes+octet_length(ef.content);
        ELSE
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
        reviewed_bytes=reviewed_bytes+octet_length(f.content);
        END IF;
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',i::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type',CASE WHEN wrapped->>'scope'='project'
                THEN 'reviewed_project_memory' WHEN wrapped->>'scope'='employee' THEN 'reviewed_employee_memory' ELSE 'reviewed_conversation_memory' END,'trust','untrusted_data','record',record)
            OR octet_length(wire#>>ARRAY['spec','context','memory_context',i::text])>8192 THEN
            RAISE EXCEPTION 'ortak: conversation rendered bytes differ' USING ERRCODE='check_violation';
        END IF;
        i=i+1;
    END LOOP;
    IF conversations>0 THEN
        SELECT * INTO origin FROM ortak_conversation_run_origin(company,run,selected_project);
        IF NOT FOUND OR context->'conversation_origin' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(
            jsonb_build_object('requester_public_key',encode(origin.requester_public_key,'hex'),
                'provenance',convert_from(origin.provenance_bytes,'UTF8'))::json) THEN
            RAISE EXCEPTION 'employee mixed conversation origin differs' USING ERRCODE='check_violation';
        END IF;
    ELSIF context ? 'conversation_origin' THEN
        RAISE EXCEPTION 'employee context has unused conversation origin' USING ERRCODE='check_violation';
    END IF;
    IF employees=0 OR reviewed_bytes>8192 OR total_bytes+reviewed_bytes>16384
        OR NOT ortak_run_reviewed_memory_current(company,run) THEN
        RAISE EXCEPTION 'ortak: conversation budget or current authority differs' USING ERRCODE='check_violation';
    END IF;
END $$;

CREATE OR REPLACE FUNCTION ortak_reviewed_snapshot_consistent() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; run UUID; wire JSONB; used_count INTEGER; record JSONB; pin JSONB; i INTEGER=0; scratch_count INTEGER; total_bytes INTEGER=0; rendered JSONB; u run_reviewed_memory_uses; f reviewed_memory_facts;
BEGIN
    company=NEW.company_id; run=NEW.run_id;
    -- Even PostgreSQL json field access may unescape unrelated NUL values.
    -- Encode the whole comparison document before performing any field access.
    SELECT ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json) INTO wire FROM run_context_snapshots s WHERE s.company_id=company AND s.run_id=run;
    SELECT count(*) INTO used_count FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run;
    IF wire IS NULL THEN RAISE EXCEPTION 'ortak: reviewed snapshot missing' USING ERRCODE='check_violation'; END IF;
    IF wire->'version'='5'::jsonb THEN
        PERFORM ortak_employee_snapshot_v5(company,run,wire);
        RETURN NEW;
    END IF;
    IF wire ? 'employee' OR EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses employee_use
        WHERE employee_use.company_id=company AND employee_use.run_id=run) THEN
        RAISE EXCEPTION 'legacy snapshot cannot carry employee context' USING ERRCODE='check_violation';
    END IF;
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

CREATE CONSTRAINT TRIGGER employee_memory_snapshot_at_commit AFTER INSERT ON run_employee_reviewed_memory_uses
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_snapshot_consistent();

-- Source: docs/ortak/sql/encrypted_dm_execution.sql
-- SHA256: 633d6f752a7b06e006e092b3a17520930a8abd14295a9c094cf2b4aa3d19abe5
-- Unnumbered and unactivated. Requires encrypted_dm_jobs + admission fragments.
-- All payload bytes remain in the protected store. No ordinary run event,
-- snapshot, output, memory write or Office HTTP publication is introduced.

-- Preserve the immutable63 ordinary completion behavior and its existing OID.
-- Confidential completion is materialized only in the protected reply tables.
CREATE OR REPLACE FUNCTION ortak_schedule_completed_office_output() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.payload_mode='confidential_dm_v1' THEN RETURN NEW; END IF;
    IF NEW.work_item_id IS NULL AND NEW.routing_decision_id IS NOT NULL
       AND NEW.status='completed' AND NEW.delivery_intent IN('reply','channel') THEN
        INSERT INTO runtime_office_outputs(company_id,run_id) VALUES(NEW.company_id,NEW.id)
        ON CONFLICT(company_id,run_id) DO NOTHING;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TABLE confidential_execution_leases (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 state TEXT NOT NULL DEFAULT 'observing' CHECK(state IN('observing','sealing','cancelling','complete','stopped','unconfirmed')),
 generation BIGINT NOT NULL DEFAULT 0 CHECK(generation BETWEEN 0 AND 128),
 failures INTEGER NOT NULL DEFAULT 0 CHECK(failures BETWEEN 0 AND 3),
 cancel_attempts INTEGER NOT NULL DEFAULT 0 CHECK(cancel_attempts BETWEEN 0 AND 3),
 lease_token UUID,lease_expires_at TIMESTAMPTZ,next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 error_code TEXT CHECK(error_code IN('unavailable','authority_changed','protocol','deadline_exceeded','cancelled')),
 finished_at TIMESTAMPTZ,
 PRIMARY KEY(company_id,run_id),FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
 CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),
 CHECK(state IN('observing','sealing','cancelling') OR lease_token IS NULL),
 CHECK((state IN('complete','stopped','unconfirmed'))=(finished_at IS NOT NULL)),
 CHECK(isfinite(next_attempt_at) AND (lease_expires_at IS NULL OR isfinite(lease_expires_at)))
);
SELECT attach_community_write_fence('confidential_execution_leases');
CREATE INDEX confidential_execution_due ON confidential_execution_leases(company_id,next_attempt_at,run_id)
 WHERE state IN('observing','sealing','cancelling');

-- The authenticated time is copied beside the exact original envelope, then
-- checked against its inner time while opening under current authority.
CREATE TABLE confidential_event_receipts (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 512),
 purpose TEXT NOT NULL DEFAULT 'runtime_event' CHECK(purpose='runtime_event'),
 occurred_at TIMESTAMPTZ NOT NULL CHECK(isfinite(occurred_at)),
 PRIMARY KEY(company_id,run_id,ordinal),
 FOREIGN KEY(company_id,run_id,purpose,ordinal) REFERENCES confidential_run_payloads(company_id,run_id,purpose,ordinal)
);
SELECT attach_community_write_fence('confidential_event_receipts');

CREATE TABLE confidential_reply_bundles (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 rumor_id BYTEA NOT NULL CHECK(octet_length(rumor_id)=32),rumor_hash BYTEA NOT NULL CHECK(octet_length(rumor_hash)=32),
 recipient_id BYTEA NOT NULL CHECK(octet_length(recipient_id)=32),history_id BYTEA NOT NULL CHECK(octet_length(history_id)=32),
 recipient_bytes BYTEA NOT NULL CHECK(octet_length(recipient_bytes) BETWEEN 1 AND 65536),
 history_bytes BYTEA NOT NULL CHECK(octet_length(history_bytes) BETWEEN 1 AND 65536),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(company_id,run_id),UNIQUE(company_id,recipient_id),UNIQUE(company_id,history_id),
 FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
 CHECK(recipient_id<>history_id)
);
SELECT attach_community_write_fence('confidential_reply_bundles');
CREATE TABLE confidential_reply_outbox (
 company_id UUID NOT NULL,community_id UUID NOT NULL,run_id UUID NOT NULL,
 copy INTEGER NOT NULL CHECK(copy IN(0,1)),
 state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','acked','failed','retired')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),generation BIGINT NOT NULL DEFAULT 0 CHECK(generation=attempts),
 lease_token UUID,lease_expires_at TIMESTAMPTZ,next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 error_code TEXT CHECK(error_code IN('unavailable','authority_changed','deadline_exceeded')),
 acknowledged_at TIMESTAMPTZ,finished_at TIMESTAMPTZ,
 PRIMARY KEY(company_id,run_id,copy),FOREIGN KEY(company_id,run_id) REFERENCES confidential_reply_bundles(company_id,run_id),
 CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),CHECK(state='pending' OR lease_token IS NULL),
 CHECK((state<>'pending')=(finished_at IS NOT NULL)),CHECK((state='acked')=(acknowledged_at IS NOT NULL)),
 CHECK(isfinite(next_attempt_at) AND (lease_expires_at IS NULL OR isfinite(lease_expires_at)))
);
SELECT attach_community_write_fence('confidential_reply_outbox');
CREATE INDEX confidential_reply_due ON confidential_reply_outbox(company_id,next_attempt_at,run_id,copy) WHERE state='pending';

CREATE FUNCTION ortak_confidential_execution_immutable() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN RAISE EXCEPTION 'Confidential execution history is retained' USING ERRCODE='check_violation'; END
$$;
CREATE TRIGGER confidential_event_immutable BEFORE UPDATE OR DELETE ON confidential_event_receipts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_reply_immutable BEFORE UPDATE OR DELETE ON confidential_reply_bundles FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_execution_retain BEFORE DELETE ON confidential_execution_leases FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_outbox_retain BEFORE DELETE ON confidential_reply_outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_event_no_truncate BEFORE TRUNCATE ON confidential_event_receipts FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_reply_no_truncate BEFORE TRUNCATE ON confidential_reply_bundles FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_execution_no_truncate BEFORE TRUNCATE ON confidential_execution_leases FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();
CREATE TRIGGER confidential_outbox_no_truncate BEFORE TRUNCATE ON confidential_reply_outbox FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();

CREATE FUNCTION ortak_confidential_execution_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE c public.confidential_runs; fresh BOOLEAN:=false;
BEGIN
 SELECT * INTO STRICT c FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id;
 IF TG_OP='INSERT' THEN
  IF NEW.state NOT IN('observing','cancelling') OR NEW.generation<>0 OR NEW.failures<>0 OR NEW.cancel_attempts<>0 OR NEW.lease_token IS NOT NULL THEN
   RAISE EXCEPTION 'Invalid confidential supervision admission' USING ERRCODE='check_violation'; END IF;
  fresh:=NEW.state='observing';
 ELSE
  IF (NEW.company_id,NEW.community_id,NEW.run_id) IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.run_id)
    OR OLD.state IN('stopped','unconfirmed') OR (OLD.state='complete' AND NOT (NEW.state='cancelling'
        AND NEW.generation=OLD.generation AND NEW.lease_token IS NULL
        AND EXISTS(SELECT 1 FROM public.runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND state='pending'))) THEN
   RAISE EXCEPTION 'Confidential supervision cannot revive' USING ERRCODE='check_violation'; END IF;
  IF NEW.generation=OLD.generation+1 AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token THEN
   IF NEW.state<>OLD.state OR OLD.next_attempt_at>clock_timestamp()
    OR (OLD.lease_expires_at IS NOT NULL AND OLD.lease_expires_at+interval '5 seconds'>clock_timestamp())
    OR NEW.lease_expires_at<=clock_timestamp() OR NEW.lease_expires_at>clock_timestamp()+interval '30 seconds'
    OR NEW.failures<>OLD.failures OR NEW.cancel_attempts<>OLD.cancel_attempts+(CASE WHEN NEW.state='cancelling' THEN 1 ELSE 0 END) THEN
    RAISE EXCEPTION 'Confidential supervision lease refused' USING ERRCODE='check_violation'; END IF;
   fresh:=NEW.state IN('observing','sealing');
  ELSIF NEW.generation=OLD.generation AND NEW.lease_token IS NULL THEN
   IF NEW.cancel_attempts<>OLD.cancel_attempts OR (NEW.state=OLD.state AND NEW.state IN('observing','sealing')
      AND (OLD.lease_token IS NULL OR OLD.lease_expires_at<=clock_timestamp()
       OR NEW.next_attempt_at<statement_timestamp()+interval '1 second'
       OR NEW.failures NOT IN(0,OLD.failures+1)))
    OR (OLD.state='cancelling' AND NEW.state NOT IN('cancelling','stopped','unconfirmed')) THEN
    RAISE EXCEPTION 'Confidential supervision settlement refused' USING ERRCODE='check_violation'; END IF;
  ELSE RAISE EXCEPTION 'Confidential supervision generation mismatch' USING ERRCODE='check_violation'; END IF;
 END IF;
 IF fresh AND (NEW.generation>=124 OR clock_timestamp()>=c.execution_deadline OR NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id)) THEN
  RAISE EXCEPTION 'Confidential supervision authority retired' USING ERRCODE='check_violation'; END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_execution_guard BEFORE INSERT OR UPDATE ON confidential_execution_leases FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_guard();

CREATE FUNCTION ortak_confidential_execution_commit() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE fresh BOOLEAN:=TG_OP='INSERT';
BEGIN
 IF NOT EXISTS(SELECT 1 FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id) THEN
  RAISE EXCEPTION 'Confidential execution community mismatch' USING ERRCODE='check_violation'; END IF;
 IF TG_TABLE_NAME='confidential_execution_leases' THEN
  fresh:=NEW.state IN('observing','sealing') AND (TG_OP='INSERT' OR NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL);
  IF NEW.state='stopped' AND NOT EXISTS(SELECT 1 FROM public.runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND state='acknowledged') THEN
   RAISE EXCEPTION 'Confidential stopped state lacks containment acknowledgement' USING ERRCODE='check_violation'; END IF;
  IF NEW.state IN('complete','sealing') AND NOT EXISTS(SELECT 1 FROM public.runs r
     WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.status='completed'
      AND ((r.delivery_intent='silent' AND NEW.state='complete'
          AND (SELECT count(*) FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id)=3)
       OR (r.delivery_intent='reply'
          AND (SELECT count(*) FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id)=4
          AND EXISTS(SELECT 1 FROM public.confidential_run_payloads WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND purpose='reply_draft' AND ordinal=0)
          AND (NEW.state='sealing' OR EXISTS(SELECT 1 FROM public.confidential_reply_bundles WHERE company_id=NEW.company_id AND run_id=NEW.run_id))))) THEN
   RAISE EXCEPTION 'Confidential terminal projection is incomplete' USING ERRCODE='check_violation'; END IF;
 ELSIF TG_TABLE_NAME='confidential_reply_outbox' AND TG_OP='UPDATE' THEN
  fresh:=NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL;
 END IF;
 IF fresh AND NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential execution authority expired at commit' USING ERRCODE='serialization_failure'; END IF;
 IF TG_TABLE_NAME='confidential_reply_bundles' THEN
  IF (SELECT count(*) FROM public.confidential_reply_outbox WHERE company_id=NEW.company_id AND run_id=NEW.run_id)<>2
   OR NOT EXISTS(SELECT 1 FROM public.confidential_run_payloads WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND purpose='reply_draft' AND ordinal=0) THEN
   RAISE EXCEPTION 'Confidential reply freeze is incomplete' USING ERRCODE='check_violation'; END IF;
 END IF;
 IF TG_TABLE_NAME='confidential_run_payloads' THEN
  IF NEW.purpose='runtime_event' AND NOT EXISTS(SELECT 1 FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND ordinal=NEW.ordinal) THEN
   RAISE EXCEPTION 'Confidential event time receipt absent' USING ERRCODE='check_violation'; END IF;
 END IF;
 RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER confidential_execution_at_commit AFTER INSERT OR UPDATE ON confidential_execution_leases DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();
CREATE CONSTRAINT TRIGGER confidential_event_at_commit AFTER INSERT ON confidential_event_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();
CREATE CONSTRAINT TRIGGER confidential_reply_at_commit AFTER INSERT ON confidential_reply_bundles DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();
CREATE CONSTRAINT TRIGGER confidential_outbox_at_commit AFTER INSERT OR UPDATE ON confidential_reply_outbox DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();
CREATE CONSTRAINT TRIGGER confidential_event_payload_at_commit AFTER INSERT ON confidential_run_payloads DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();

CREATE FUNCTION ortak_confidential_reply_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE identity JSONB; wire JSONB; bytes BYTEA; target TEXT; expected BYTEA; n INTEGER;
BEGIN
 IF NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) OR NOT EXISTS(SELECT 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND status='completed' AND delivery_intent='reply') THEN
  RAISE EXCEPTION 'Confidential reply has no current completion' USING ERRCODE='check_violation'; END IF;
 SELECT convert_from(identity_bytes,'UTF8')::jsonb INTO STRICT identity FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id;
 FOR n IN 0..1 LOOP
  bytes:=CASE n WHEN 0 THEN NEW.recipient_bytes ELSE NEW.history_bytes END;
  expected:=CASE n WHEN 0 THEN NEW.recipient_id ELSE NEW.history_id END;
  target:=identity->>(CASE n WHEN 0 THEN 'human_public_key' ELSE 'employee_public_key' END);
  wire:=convert_from(bytes,'UTF8')::jsonb;
  IF jsonb_typeof(wire)<>'object' OR NOT wire ?& ARRAY['id','pubkey','created_at','kind','tags','content','sig']
   OR wire-ARRAY['id','pubkey','created_at','kind','tags','content','sig']<>'{}'::jsonb
   OR wire->>'id' IS DISTINCT FROM encode(expected,'hex') OR wire->'kind' IS DISTINCT FROM '1059'::jsonb
   OR wire->'tags' IS DISTINCT FROM jsonb_build_array(jsonb_build_array('p',target))
   OR ((wire->>'pubkey')~'^[0-9a-f]{64}$') IS DISTINCT FROM true OR ((wire->>'sig')~'^[0-9a-f]{128}$') IS DISTINCT FROM true
   OR jsonb_typeof(wire->'created_at') IS DISTINCT FROM 'number'
   OR ((wire->>'created_at')~'^(0|[1-9][0-9]{0,11})$') IS DISTINCT FROM true
   OR jsonb_typeof(wire->'content') IS DISTINCT FROM 'string' OR octet_length(wire->>'content') NOT BETWEEN 132 AND 60000 THEN
   RAISE EXCEPTION 'Confidential reply copy mismatch' USING ERRCODE='check_violation'; END IF;
 END LOOP;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_reply_guard BEFORE INSERT ON confidential_reply_bundles FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reply_guard();

CREATE FUNCTION ortak_confidential_reply_lease_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE deadline TIMESTAMPTZ; fresh BOOLEAN:=false;
BEGIN
 SELECT c.execution_deadline INTO STRICT deadline FROM public.confidential_runs c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id AND c.community_id=NEW.community_id;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.generation<>0 OR NEW.lease_token IS NOT NULL THEN RAISE EXCEPTION 'Invalid confidential output admission' USING ERRCODE='check_violation'; END IF;
 ELSE
  IF (NEW.company_id,NEW.community_id,NEW.run_id,NEW.copy) IS DISTINCT FROM(OLD.company_id,OLD.community_id,OLD.run_id,OLD.copy) OR OLD.state<>'pending' THEN
   RAISE EXCEPTION 'Confidential output identity or terminal result changed' USING ERRCODE='check_violation'; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.generation=OLD.generation+1 AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token THEN
   IF NEW.state<>'pending' OR OLD.next_attempt_at>clock_timestamp()
    OR (OLD.lease_expires_at IS NOT NULL AND OLD.lease_expires_at+interval '5 seconds'>clock_timestamp())
    OR NEW.lease_expires_at<=clock_timestamp() OR NEW.lease_expires_at>least(deadline,clock_timestamp()+interval '30 seconds') THEN
    RAISE EXCEPTION 'Confidential output lease refused' USING ERRCODE='check_violation'; END IF;fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.generation=OLD.generation AND NEW.lease_token IS NULL THEN
   -- A known ACK for the unchanged locked owner is receipt-only after expiry.
   -- Pending retry still needs a live lease and cannot gain new authority here.
   IF NEW.state='acked' AND OLD.lease_token IS NULL
    OR NEW.state='pending' AND (OLD.lease_token IS NULL OR OLD.lease_expires_at<=clock_timestamp())
    OR NEW.state='pending' AND (NEW.attempts>=3 OR NEW.next_attempt_at<statement_timestamp()+interval '5 seconds') THEN
    RAISE EXCEPTION 'Confidential output settlement refused' USING ERRCODE='check_violation'; END IF;
  ELSE RAISE EXCEPTION 'Confidential output generation mismatch' USING ERRCODE='check_violation'; END IF;
 END IF;
 IF fresh AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential output authority retired' USING ERRCODE='check_violation'; END IF;
 RETURN NEW;
END
$$;
CREATE TRIGGER confidential_reply_lease_guard BEFORE INSERT OR UPDATE ON confidential_reply_outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reply_lease_guard();

-- Source: crates/ortak-server/src/routing_stream_schema.sql
-- SHA256: 3aded4d8fb9ebcf42b9fe0c94926b670b6a779b13b1615d27ca99c082d064da1
-- SOURCE FRAGMENT ONLY: root owns allocation, numbered migration and convergence.
-- Hints contain public scope IDs only. LISTEN precedes durable current reads;
-- a lost hint is repaired by the next bounded signed subscription, not a cursor.
CREATE FUNCTION ortak_routing_notify() RETURNS TRIGGER AS $$
DECLARE
    message TEXT;
BEGIN
    IF TG_TABLE_NAME = 'routing_decisions' THEN
        message := encode(NEW.message_id, 'hex');
    ELSIF TG_TABLE_NAME <> 'office_authority_generations' THEN
        RAISE EXCEPTION 'invalid routing notification source' USING ERRCODE='55000';
    END IF;
    PERFORM pg_notify('ortak_routing_v1', json_build_object(
        'company_id', NEW.company_id, 'message_id', message)::TEXT);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_routing_decisions_notify AFTER INSERT ON routing_decisions
    FOR EACH ROW EXECUTE FUNCTION ortak_routing_notify();
-- Existing canonical Office fences advance this row in the authority mutation
-- transaction, including membership, identity, community and source removal.
CREATE TRIGGER trg_routing_authority_notify AFTER INSERT OR UPDATE ON office_authority_generations
    FOR EACH ROW EXECUTE FUNCTION ortak_routing_notify();
