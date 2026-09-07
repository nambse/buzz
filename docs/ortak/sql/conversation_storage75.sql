-- D4 storage SOURCE FRAGMENT; not a numbered migration or deployed schema.
-- Root assembles the additive migration after 74. Apply the independently
-- reviewed ortak_conversation_source_observation(...) definition before use.
-- No conversation export/runtime selection or snapshot-v4 admission is added.
-- Before serving, root must exclude conversation facts from the legacy 69/71
-- project predicates and wire the separately reviewed current-use boundary.

ALTER TABLE reviewed_memory_facts
    ADD COLUMN audience_kind TEXT NOT NULL DEFAULT 'project'
        CHECK (audience_kind IN ('project', 'conversation'));
-- The existing 66 fact guard compares every non-revocation column on UPDATE,
-- so this field is immutable without widening the permitted version transition.

CREATE TABLE conversation_memory_authorities (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    project_id UUID NOT NULL,
    channel_id UUID NOT NULL CHECK (channel_id <> '00000000-0000-0000-0000-000000000000'),
    epoch BIGINT NOT NULL DEFAULT 0 CHECK (epoch >= 0),
    last_change_reason TEXT NOT NULL DEFAULT 'registered'
        CHECK (last_change_reason IN ('registered', 'channel_changed',
            'membership_changed', 'project_changed', 'project_grant_changed',
            'event_changed', 'thread_changed', 'identity_changed', 'scope_closed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, project_id, channel_id),
    UNIQUE (company_id, community_id, project_id, channel_id),
    FOREIGN KEY (company_id, project_id) REFERENCES projects(company_id, id),
    CHECK (company_id <> '00000000-0000-0000-0000-000000000000'
        AND community_id <> '00000000-0000-0000-0000-000000000000'
        AND project_id <> '00000000-0000-0000-0000-000000000000'),
    CHECK (changed_at >= created_at)
);
CREATE INDEX idx_conversation_authority_channel
    ON conversation_memory_authorities(community_id, channel_id, company_id, project_id);

-- This checks scope identity, not a human grant, employee, source or permission
-- to consume memory. Those belong to the observation/admission boundaries.
CREATE FUNCTION ortak_conversation_scope_current(
    company UUID, community UUID, project UUID, channel UUID
) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM companies co
        JOIN office_company_bindings office ON office.company_id=co.id
            AND office.community_id=community
        JOIN communities cm ON cm.id=office.community_id
        JOIN projects p ON p.company_id=co.id AND p.id=project
        JOIN project_api_bindings b ON b.company_id=p.company_id AND b.project_id=p.id
            AND b.community_id=cm.id AND b.channel_id=channel
        JOIN channels ch ON ch.community_id=cm.id AND ch.id=b.channel_id
        WHERE co.id=company AND co.status='active' AND p.status='active'
            AND cm.deletion_state='active' AND cm.deleted_at IS NULL
            AND ch.channel_type='stream' AND ch.archived_at IS NULL AND ch.deleted_at IS NULL
            AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>clock_timestamp())
    )
$$;

CREATE FUNCTION ortak_conversation_authority_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        RAISE EXCEPTION 'ortak: conversation authorities are retained'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF TG_OP='UPDATE' THEN
        IF (NEW.company_id,NEW.community_id,NEW.project_id,NEW.channel_id,NEW.created_at)
            IS DISTINCT FROM
            (OLD.company_id,OLD.community_id,OLD.project_id,OLD.channel_id,OLD.created_at)
            OR OLD.epoch=9223372036854775807 OR NEW.epoch<>OLD.epoch+1
            OR NEW.last_change_reason='registered' THEN
            RAISE EXCEPTION 'ortak: conversation authority only advances'
                USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        -- Mutation hooks own the Office/project fence and update sorted scope
        -- rows. Do not acquire Office exclusive here: project-grant writers use
        -- the existing project NOWAIT fence under signed shared-Office auth.
        NEW.changed_at=clock_timestamp();
        RETURN NEW;
    END IF;
    PERFORM ortak_lock_office_authority(NEW.company_id);
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=NEW.project_id
        FOR SHARE NOWAIT;
    IF NOT FOUND OR NEW.epoch<>0 OR NEW.last_change_reason<>'registered'
        OR NOT ortak_conversation_scope_current(
            NEW.company_id,NEW.community_id,NEW.project_id,NEW.channel_id) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration requires current identity'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    -- Both ceilings include retained/removed scopes. Community then company
    -- registration locks are always nonblocking and acquired in that order.
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-community-registration:'||NEW.community_id::text,0)) THEN
        RAISE EXCEPTION 'ortak: community conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-registration:'||NEW.company_id::text,0)) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    IF (SELECT count(*) FROM conversation_memory_authorities WHERE company_id=NEW.company_id)>=128 THEN
        RAISE EXCEPTION 'ortak: retained conversation scope limit reached'
            USING ERRCODE='program_limit_exceeded';
    END IF;
    IF (SELECT count(*) FROM conversation_memory_authorities WHERE community_id=NEW.community_id)>=256 THEN
        RAISE EXCEPTION 'ortak: retained community conversation scope limit reached'
            USING ERRCODE='program_limit_exceeded';
    END IF;
    NEW.created_at=clock_timestamp();
    NEW.changed_at=NEW.created_at;
    RETURN NEW;
END $$;
CREATE TRIGGER conversation_authority_guard
    BEFORE INSERT OR UPDATE OR DELETE ON conversation_memory_authorities
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_authority_guard();
CREATE TRIGGER conversation_authority_no_truncate
    BEFORE TRUNCATE ON conversation_memory_authorities
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
SELECT attach_community_write_fence('conversation_memory_authorities');

-- Called after the shared Office/project locks; never upgrades project SHARE.
-- Existing scopes still require current identity, but do not consume another
-- slot. The returned epoch is locked only for this transaction, not a cache.
CREATE FUNCTION ortak_register_conversation_authority(
    company UUID, community UUID, project UUID, channel UUID
) RETURNS BIGINT LANGUAGE plpgsql AS $$
DECLARE selected BIGINT;
BEGIN
    PERFORM ortak_lock_office_authority(company);
    PERFORM 1 FROM projects p WHERE p.company_id=company AND p.id=project FOR SHARE NOWAIT;
    IF NOT FOUND OR NOT ortak_conversation_scope_current(company,community,project,channel) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration requires current identity'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-community-registration:'||community::text,0)) THEN
        RAISE EXCEPTION 'ortak: community conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-registration:'||company::text,0)) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    SELECT a.epoch INTO selected FROM conversation_memory_authorities a
        WHERE a.company_id=company AND a.community_id=community
            AND a.project_id=project AND a.channel_id=channel FOR SHARE;
    IF FOUND THEN RETURN selected; END IF;
    -- A conflicting retained identity is an error, never a rebind/upsert.
    INSERT INTO conversation_memory_authorities(company_id,community_id,project_id,channel_id)
        VALUES(company,community,project,channel) RETURNING epoch INTO selected;
    RETURN selected;
END $$;

CREATE TABLE reviewed_memory_conversation_audiences (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    project_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    channel_id UUID NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('channel','thread')),
    thread_root_event_id BYTEA CHECK (octet_length(thread_root_event_id)=32),
    thread_root_event_created_at TIMESTAMPTZ,
    source_event_id BYTEA NOT NULL CHECK (octet_length(source_event_id)=32),
    source_event_created_at TIMESTAMPTZ NOT NULL,
    audience_bytes BYTEA NOT NULL CHECK (octet_length(audience_bytes) BETWEEN 1 AND 2048),
    audience_hash BYTEA NOT NULL CHECK (octet_length(audience_hash)=32),
    source_evidence_hash BYTEA NOT NULL CHECK (octet_length(source_evidence_hash)=32),
    source_hash BYTEA NOT NULL CHECK (octet_length(source_hash)=32),
    provenance_bytes BYTEA NOT NULL CHECK (octet_length(provenance_bytes) BETWEEN 1 AND 4096),
    PRIMARY KEY (company_id,fact_id),
    FOREIGN KEY (company_id,fact_id) REFERENCES reviewed_memory_facts(company_id,id),
    FOREIGN KEY (company_id,project_id) REFERENCES projects(company_id,id),
    FOREIGN KEY (company_id,employee_id) REFERENCES employees(company_id,id),
    FOREIGN KEY (company_id,community_id,project_id,channel_id)
        REFERENCES conversation_memory_authorities(company_id,community_id,project_id,channel_id),
    CHECK ((kind='channel' AND thread_root_event_id IS NULL AND thread_root_event_created_at IS NULL)
        OR (kind='thread' AND thread_root_event_id IS NOT NULL AND thread_root_event_created_at IS NOT NULL)),
    CHECK (source_event_created_at >= TIMESTAMPTZ '1970-01-01 00:00:00+00'
        AND source_event_created_at < TIMESTAMPTZ '10000-01-01 00:00:00+00'),
    CHECK (thread_root_event_created_at IS NULL OR
        (thread_root_event_created_at >= TIMESTAMPTZ '1970-01-01 00:00:00+00'
         AND thread_root_event_created_at < TIMESTAMPTZ '10000-01-01 00:00:00+00')),
    CHECK (source_event_id IS DISTINCT FROM thread_root_event_id
        OR source_event_created_at=thread_root_event_created_at),
    CHECK (sha256(audience_bytes)=audience_hash),
    CHECK (source_hash=sha256(convert_to(
        '{"audience_hash":"'||encode(audience_hash,'hex')||
        '","format":"ortak-reviewed-conversation-source/1","source_evidence_hash":"'||
        encode(source_evidence_hash,'hex')||'"}','UTF8')))
);
CREATE INDEX idx_conversation_audience_source
    ON reviewed_memory_conversation_audiences(community_id,source_event_id,source_event_created_at,company_id,project_id);
CREATE INDEX idx_conversation_audience_root
    ON reviewed_memory_conversation_audiences(community_id,thread_root_event_id,thread_root_event_created_at,company_id,project_id)
    WHERE thread_root_event_id IS NOT NULL;
CREATE INDEX idx_conversation_audience_scope
    ON reviewed_memory_conversation_audiences(company_id,project_id,channel_id,employee_id,fact_id);
CREATE INDEX idx_conversation_audience_employee
    ON reviewed_memory_conversation_audiences(company_id,employee_id,project_id,channel_id);
CREATE TRIGGER conversation_audience_immutable
    BEFORE UPDATE OR DELETE ON reviewed_memory_conversation_audiences
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER conversation_audience_no_truncate
    BEFORE TRUNCATE ON reviewed_memory_conversation_audiences
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
SELECT attach_community_write_fence('reviewed_memory_conversation_audiences');

-- Run on INSERT only. A later Stop, expiry, grant loss or purge must not ask a
-- historical immutable audience to resolve against now-missing source rows.
CREATE FUNCTION ortak_conversation_fact_storage_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; fact UUID; f reviewed_memory_facts;
    a reviewed_memory_conversation_audiences; observed RECORD;
BEGIN
    company=NEW.company_id;
    IF TG_TABLE_NAME='reviewed_memory_facts' THEN
        fact=NEW.id;
    ELSE
        fact=NEW.fact_id;
    END IF;
    SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=company AND x.id=fact;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: conversation audience parent fact is missing' USING ERRCODE='check_violation';
    END IF;
    SELECT * INTO a FROM reviewed_memory_conversation_audiences x WHERE x.company_id=company AND x.fact_id=fact;
    IF f.audience_kind='project' THEN
        IF FOUND THEN
            RAISE EXCEPTION 'ortak: project facts cannot acquire a conversation audience' USING ERRCODE='check_violation';
        END IF;
        RETURN NEW;
    END IF;
    IF NOT FOUND OR f.audience_kind<>'conversation' OR f.source_artifact_id IS NOT NULL
        OR (a.company_id,a.community_id,a.project_id,a.employee_id,a.source_event_id)
            IS DISTINCT FROM (f.company_id,f.community_id,f.project_id,f.employee_id,f.source_message_id)
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_facts born
            WHERE born.company_id=company AND born.id=fact
                AND born.xmin::text::bigint=txid_current()%4294967296)
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_conversation_audiences born
            WHERE born.company_id=company AND born.fact_id=fact
                AND born.xmin::text::bigint=txid_current()%4294967296)
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_operations receipt
            WHERE receipt.company_id=f.company_id AND receipt.community_id=f.community_id
                AND receipt.fact_id=f.id AND receipt.project_id=f.project_id
                AND receipt.actor_pubkey=f.approved_by AND receipt.operation_id=f.promotion_operation_id
                AND receipt.action='promote' AND receipt.result_version=1
                AND receipt.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: conversation approval requires one atomic audience and promotion receipt'
            USING ERRCODE='check_violation';
    END IF;
    PERFORM ortak_lock_office_authority(company);
    PERFORM 1 FROM projects p WHERE p.company_id=company AND p.id=f.project_id FOR SHARE NOWAIT;
    PERFORM 1 FROM conversation_memory_authorities authority
        WHERE authority.company_id=a.company_id AND authority.community_id=a.community_id
            AND authority.project_id=a.project_id AND authority.channel_id=a.channel_id FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: conversation authority identity is missing' USING ERRCODE='check_violation';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM project_access_grants grant_row
        WHERE grant_row.company_id=f.company_id AND grant_row.project_id=f.project_id
            AND grant_row.actor_pubkey=f.approved_by AND grant_row.revoked_at IS NULL
            AND grant_row.role IN ('owner','reviewer')) THEN
        RAISE EXCEPTION 'ortak: conversation approval requires current project review authority'
            USING ERRCODE='check_violation';
    END IF;
    BEGIN
        SELECT * INTO STRICT observed FROM ortak_conversation_source_observation(
            f.company_id,f.project_id,f.employee_id,decode(f.approved_by,'hex'),
            f.source_message_id,a.kind);
    EXCEPTION WHEN NO_DATA_FOUND OR TOO_MANY_ROWS THEN
        RAISE EXCEPTION 'ortak: conversation approval source is no longer current'
            USING ERRCODE='check_violation';
    END;
    IF (a.community_id,a.channel_id,a.source_event_created_at,
        a.thread_root_event_id,a.thread_root_event_created_at,a.audience_bytes,
        a.audience_hash,a.source_evidence_hash,a.source_hash,a.provenance_bytes)
        IS DISTINCT FROM
        (observed.community_id,observed.channel_id,observed.source_event_created_at,
        observed.thread_root_event_id,observed.thread_root_event_created_at,observed.audience_bytes,
        observed.audience_hash,observed.source_evidence_hash,observed.source_hash,observed.provenance_bytes)
        OR f.expires_at<=clock_timestamp()
        OR (observed.valid_before IS NOT NULL AND
            (clock_timestamp()>=observed.valid_before OR f.expires_at>observed.valid_before)) THEN
        RAISE EXCEPTION 'ortak: conversation approval bytes or current deadline differ'
            USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER conversation_fact_storage_at_commit
    AFTER INSERT ON reviewed_memory_facts DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_fact_storage_at_commit();
CREATE CONSTRAINT TRIGGER conversation_audience_storage_at_commit
    AFTER INSERT ON reviewed_memory_conversation_audiences DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_fact_storage_at_commit();

ALTER TABLE reviewed_memory_targets
    ADD COLUMN conversation_consumption_enabled BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN conversation_channel_id UUID
        CHECK (conversation_channel_id IS NULL OR conversation_channel_id<>'00000000-0000-0000-0000-000000000000'),
    ADD COLUMN conversation_consumption_epoch BIGINT NOT NULL DEFAULT 0 CHECK (conversation_consumption_epoch>=0),
    ADD CONSTRAINT conversation_target_selection_shape CHECK (
        (NOT conversation_consumption_enabled OR conversation_channel_id IS NOT NULL)
        AND (conversation_channel_id IS NOT NULL OR conversation_consumption_epoch=0));

-- Narrow replacement of 71's target guard: retain its original project epoch
-- transition, immutable binding/receipt identity and <=60s advertisement bound.
CREATE OR REPLACE FUNCTION ortak_reviewed_target_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE selecting BOOLEAN;
BEGIN
    IF TG_OP='UPDATE' AND
        (to_jsonb(NEW)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at'
            -'runtime_consumption_enabled'-'consumption_epoch'-'conversation_channel_id'
            -'conversation_consumption_enabled'-'conversation_consumption_epoch')
        IS DISTINCT FROM
        (to_jsonb(OLD)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at'
            -'runtime_consumption_enabled'-'consumption_epoch'-'conversation_channel_id'
            -'conversation_consumption_enabled'-'conversation_consumption_epoch') THEN
        RAISE EXCEPTION 'ortak: reviewed target identity is immutable' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='INSERT' THEN
        IF NEW.consumption_epoch<>0 OR NEW.conversation_consumption_epoch<>0 THEN
            RAISE EXCEPTION 'ortak: invalid initial consumption epoch' USING ERRCODE='check_violation';
        END IF;
        selecting=NEW.conversation_channel_id IS NOT NULL;
    ELSE
        IF NEW.consumption_epoch<>OLD.consumption_epoch
            OR NEW.conversation_consumption_epoch<>OLD.conversation_consumption_epoch THEN
            RAISE EXCEPTION 'ortak: consumption epochs are server derived' USING ERRCODE='check_violation';
        END IF;
        IF OLD.conversation_channel_id IS NOT NULL
            AND NEW.conversation_channel_id IS DISTINCT FROM OLD.conversation_channel_id THEN
            RAISE EXCEPTION 'ortak: conversation target channel is immutable' USING ERRCODE='check_violation';
        END IF;
        IF OLD.runtime_consumption_enabled AND NOT NEW.runtime_consumption_enabled THEN
            NEW.consumption_epoch=OLD.consumption_epoch+1;
        END IF;
        IF OLD.conversation_consumption_enabled AND NOT NEW.conversation_consumption_enabled THEN
            NEW.conversation_consumption_epoch=OLD.conversation_consumption_epoch+1;
        END IF;
        selecting=OLD.conversation_channel_id IS NULL AND NEW.conversation_channel_id IS NOT NULL;
    END IF;
    IF NEW.enabled AND (NEW.valid_until<=clock_timestamp() OR NEW.valid_until>clock_timestamp()+INTERVAL '60 seconds') THEN
        RAISE EXCEPTION 'ortak: reviewed target witness must be short and live' USING ERRCODE='check_violation';
    END IF;
    -- A disable-only advertisement must still work after source/identity loss.
    -- In particular the existing advertise transaction briefly sets enabled=false
    -- before refreshing its selected rows. That is not conversation opt-out and
    -- must not advance its separate epoch or fail a stale-scope current check.
    IF selecting OR (NEW.conversation_consumption_enabled AND NEW.enabled) THEN
        PERFORM ortak_lock_office_authority(NEW.company_id);
        PERFORM 1 FROM projects p WHERE p.company_id=NEW.company_id AND p.id=NEW.project_id FOR SHARE NOWAIT;
        PERFORM 1 FROM conversation_memory_authorities authority
            WHERE authority.company_id=NEW.company_id AND authority.community_id=NEW.community_id
                AND authority.project_id=NEW.project_id AND authority.channel_id=NEW.conversation_channel_id FOR SHARE;
        IF NOT FOUND OR NOT ortak_conversation_scope_current(
                NEW.company_id,NEW.community_id,NEW.project_id,NEW.conversation_channel_id)
            OR NOT EXISTS (SELECT 1 FROM employees e
                JOIN employee_revisions rev ON rev.company_id=e.company_id AND rev.employee_id=e.id AND rev.id=e.active_revision_id
                JOIN employee_memory_bindings memory ON memory.company_id=e.company_id AND memory.employee_id=e.id AND memory.revision_id=e.active_revision_id
                WHERE e.company_id=NEW.company_id AND e.id=NEW.employee_id AND e.status='active'
                    AND NEW.employee_revision_id=e.active_revision_id AND NEW.employee_lifecycle_epoch=e.lifecycle_epoch
                    AND NEW.binding=rev.manifest->'memory' AND memory.validated_at IS NOT NULL
                    AND NEW.binding=jsonb_build_object('adapter',memory.adapter,'endpoint_ref',memory.endpoint_ref,
                        'workspace',memory.workspace,'user_peer',memory.user_peer,'employee_peer',memory.employee_peer,'options',memory.options))
            OR (NEW.conversation_consumption_enabled AND (NOT NEW.enabled OR NEW.valid_until<=clock_timestamp())) THEN
            RAISE EXCEPTION 'ortak: conversation target requires current selected scope and binding'
                USING ERRCODE='check_violation';
        END IF;
    END IF;
    RETURN NEW;
END $$;

ALTER TABLE run_reviewed_memory_uses
    ADD COLUMN conversation_audience_hash BYTEA CHECK (octet_length(conversation_audience_hash)=32),
    ADD COLUMN conversation_authority_epoch BIGINT CHECK (conversation_authority_epoch>=0),
    ADD COLUMN conversation_consumption_epoch BIGINT CHECK (conversation_consumption_epoch>=0),
    ADD CONSTRAINT conversation_use_pin_shape CHECK (
        (conversation_audience_hash IS NULL AND conversation_authority_epoch IS NULL AND conversation_consumption_epoch IS NULL)
        OR (conversation_audience_hash IS NOT NULL AND conversation_authority_epoch IS NOT NULL
            AND conversation_consumption_epoch IS NOT NULL AND consumption_epoch=0));

-- Retained pin consistency only. This does not replace 71/72 current-use,
-- snapshot/admission guards, allocate v4, or permit a conversation runtime use.
CREATE FUNCTION ortak_conversation_use_storage_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE f reviewed_memory_facts; a reviewed_memory_conversation_audiences;
BEGIN
    SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=NEW.company_id AND x.id=NEW.fact_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: reviewed use fact is missing' USING ERRCODE='check_violation';
    END IF;
    IF f.audience_kind='project' THEN
        IF NEW.conversation_audience_hash IS NOT NULL OR NEW.conversation_authority_epoch IS NOT NULL
            OR NEW.conversation_consumption_epoch IS NOT NULL THEN
            RAISE EXCEPTION 'ortak: project use cannot carry conversation pins' USING ERRCODE='check_violation';
        END IF;
        RETURN NEW;
    END IF;
    SELECT * INTO a FROM reviewed_memory_conversation_audiences x WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id;
    IF NOT FOUND OR NEW.consumption_epoch<>0 OR NEW.conversation_audience_hash IS DISTINCT FROM a.audience_hash
        OR NEW.conversation_authority_epoch IS NULL OR NEW.conversation_consumption_epoch IS NULL
        OR NEW.community_id<>a.community_id OR NEW.source_hash<>a.source_hash
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_targets target
            WHERE target.company_id=NEW.company_id AND target.id=NEW.target_id
                AND target.community_id=a.community_id AND target.project_id=a.project_id
                AND target.employee_id=a.employee_id AND target.conversation_channel_id=a.channel_id) THEN
        RAISE EXCEPTION 'ortak: reviewed conversation use storage pins differ' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER conversation_use_storage_at_commit
    AFTER INSERT ON run_reviewed_memory_uses DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_use_storage_at_commit();

-- Root assembly dependencies outside this storage fragment:
-- * scoped authority mutation hooks/indexed affected-scope lookup;
-- * explicit legacy project-kind exclusions in 66/69/71 consumers;
-- * conversation source-hash/export dispatch only after separate approval;
-- * reviewed v4 current-use/snapshot/origin/epoch checks before runtime use;
-- * exact+retained deletion inventory and universal fence parity for both new
--   relations, plus G version/table/added-column witnesses.
-- No cleanup columns: permission loss only retires scope epochs. Existing
-- explicit Stop/expiry and real 69 withdrawal receipts remain authoritative.
