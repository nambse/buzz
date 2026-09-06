-- Reviewed conversation memory: immutable approval, retained scope epochs,
-- and exclusion from legacy project-only consumers. Runtime snapshot v4 and
-- conversation publication are separate later additions.

-- D4 source fragment; not an applied migration. Root assembles additive 75.
-- Depends only on immutable 1-74 tables and pgcrypto. No data writes.
-- This is the SQL counterpart of postgres/conversation_memory/{query,resolve}.rs
-- and memory/conversation/wire.rs. Caller ceilings remain in the Rust facade.

-- Compact UTF-8 JSON for the deliberately small conversation wire vocabulary.
-- Object keys use bytewise lexical order; array order and strings are exact.
-- PostgreSQL JSONB::text is used only for closed scalar values, never objects
-- or arrays. to_json(text) matches serde_json string quoting, including control
-- escapes, while retaining Unicode rather than ASCII-escaping it.
-- 524288 accommodates the worst-case JSON escaping of 65536 source bytes plus
-- 16384 encoded tag bytes. Invalid/deeper/oversized values return SQL NULL.
CREATE FUNCTION ortak_conversation_json75(value JSONB, nesting INTEGER DEFAULT 0)
RETURNS TEXT LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE
SET search_path = pg_catalog, public, pg_temp AS $$
DECLARE
    member RECORD;
    encoded TEXT;
    result TEXT;
    separator TEXT := '';
BEGIN
    IF nesting < 0 OR nesting > 4 OR octet_length(value::text) > 524288 THEN
        RETURN NULL;
    END IF;
    CASE jsonb_typeof(value)
    WHEN 'object' THEN
        result := '{';
        FOR member IN SELECT e.key, e.val FROM jsonb_each(value) AS e(key,val)
                      ORDER BY e.key COLLATE "C" LOOP
            encoded := public.ortak_conversation_json75(member.val, nesting + 1);
            IF encoded IS NULL THEN RETURN NULL; END IF;
            result := result || separator || to_json(member.key)::text || ':' || encoded;
            separator := ',';
        END LOOP;
        RETURN result || '}';
    WHEN 'array' THEN
        result := '[';
        FOR member IN SELECT e.val FROM jsonb_array_elements(value)
                      WITH ORDINALITY AS e(val,ordinal) ORDER BY e.ordinal LOOP
            encoded := public.ortak_conversation_json75(member.val, nesting + 1);
            IF encoded IS NULL THEN RETURN NULL; END IF;
            result := result || separator || encoded;
            separator := ',';
        END LOOP;
        RETURN result || ']';
    WHEN 'string' THEN RETURN to_json(value #>> '{}')::text;
    WHEN 'number' THEN
        -- These wires contain only the canonical event's int32 kind, never
        -- arbitrary floating-point values whose encoders could disagree.
        IF value::text !~ '^-?(0|[1-9][0-9]*)$' THEN RETURN NULL; END IF;
        RETURN value::text;
    WHEN 'boolean' THEN RETURN value::text;
    WHEN 'null' THEN RETURN 'null';
    ELSE RETURN NULL;
    END CASE;
END
$$;

-- One/zero row current-read observation, never an approval or retained epoch.
-- STABLE plus the one table-read statement below prevents mixing snapshots
-- while walking ancestry. No routing/delivery-chain root is consulted.
-- Callers doing durable work still need their Office/project/epoch fences and
-- final deadline check; current membership alone cannot prevent old-run revival.
CREATE FUNCTION ortak_conversation_source_observation(
    company UUID, project UUID, employee TEXT, human BYTEA,
    source_id BYTEA, audience_kind TEXT
) RETURNS TABLE(
    community_id UUID,
    channel_id UUID,
    source_event_created_at TIMESTAMPTZ,
    thread_root_event_id BYTEA,
    thread_root_event_created_at TIMESTAMPTZ,
    audience_bytes BYTEA,
    audience_hash BYTEA,
    source_evidence_hash BYTEA,
    source_hash BYTEA,
    provenance_bytes BYTEA,
    observed_at TIMESTAMPTZ,
    valid_before TIMESTAMPTZ
) LANGUAGE plpgsql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path = pg_catalog, public, pg_temp AS $$
DECLARE
    node RECORD;
    first_node RECORD;
    count_nodes INTEGER := 0;
    seen BYTEA[] := ARRAY[]::bytea[];
    expected_parent BYTEA;
    expected_parent_at TIMESTAMPTZ;
    expected_depth INTEGER;
    expected_root BYTEA;
    expected_root_at TIMESTAMPTZ;
    resolved_root BYTEA;
    resolved_root_at TIMESTAMPTZ;
    tag JSONB;
    part JSONB;
    marker TEXT;
    reference_id BYTEA;
    claimed_root BYTEA;
    claimed_parent BYTEA;
    effective_depth INTEGER;
    source_stamp TEXT;
    root_stamp TEXT;
    audience_wire JSONB;
    encoded TEXT;
BEGIN
    IF company IS NULL OR project IS NULL OR employee IS NULL OR human IS NULL
       OR source_id IS NULL OR audience_kind IS NULL
       OR company = '00000000-0000-0000-0000-000000000000'::uuid
       OR project = '00000000-0000-0000-0000-000000000000'::uuid
       OR octet_length(employee) NOT BETWEEN 1 AND 64
       OR employee COLLATE "C" !~ '^[a-z0-9][a-z0-9_-]{0,63}$'
       OR octet_length(human) <> 32 OR octet_length(source_id) <> 32
       OR audience_kind NOT IN ('channel','thread') THEN
        RETURN;
    END IF;

    FOR node IN
      WITH RECURSIVE visible AS MATERIALIZED (
        SELECT office.community_id, a.channel_id, statement_timestamp() AS observed_at,
               least(ch.ttl_deadline,b.valid_until) AS valid_before
        FROM public.companies co
        JOIN public.office_company_bindings office ON office.company_id=co.id
        JOIN public.communities cm ON cm.id=office.community_id
          AND cm.deleted_at IS NULL AND cm.deletion_state='active'
        JOIN public.projects p ON p.company_id=co.id AND p.id=$2 AND p.status='active'
        JOIN public.project_api_bindings a ON a.company_id=p.company_id AND a.project_id=p.id AND a.community_id=cm.id
        JOIN public.project_access_grants g ON g.company_id=p.company_id AND g.project_id=p.id
          AND g.actor_pubkey=encode($4,'hex') AND g.revoked_at IS NULL
        JOIN public.channels ch ON ch.community_id=cm.id AND ch.id=a.channel_id
          AND ch.channel_type='stream' AND ch.deleted_at IS NULL AND ch.archived_at IS NULL
          AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>statement_timestamp())
        JOIN public.channel_members human_member ON human_member.community_id=cm.id AND human_member.channel_id=ch.id
          AND human_member.pubkey=$4 AND human_member.removed_at IS NULL AND human_member.role<>'bot'
        JOIN public.employees emp ON emp.company_id=co.id AND emp.id=$3 AND emp.status='active'
        JOIN public.employee_revisions rev ON rev.company_id=emp.company_id AND rev.employee_id=emp.id AND rev.id=emp.active_revision_id
        JOIN public.employee_office_bindings b ON b.company_id=emp.company_id AND b.employee_id=emp.id
          AND encode(b.public_key,'hex')=rev.manifest #>> '{office,public_key}'
          AND b.signer_ref=rev.manifest #>> '{office,signer_ref}'
          AND b.verified_at IS NOT NULL AND b.valid_from<=statement_timestamp()
          AND (b.valid_until IS NULL OR b.valid_until>statement_timestamp())
        JOIN public.channel_members employee_member ON employee_member.community_id=cm.id AND employee_member.channel_id=ch.id
          AND employee_member.pubkey=b.public_key AND employee_member.removed_at IS NULL
        WHERE co.id=$1 AND co.status='active'
          AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=$4
            AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
          AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings eb WHERE eb.company_id=co.id AND eb.public_key=$4)
          AND NOT EXISTS(SELECT 1 FROM public.channel_members bot WHERE bot.community_id=cm.id AND bot.pubkey=$4 AND bot.role='bot')
          AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=b.public_key AND u.deactivated_at IS NOT NULL)
      ), source AS MATERIALIZED (
        SELECT e.id,e.created_at,e.content,e.pubkey,e.kind,e.sig,v.*
        FROM visible v JOIN public.office_inbox i ON i.company_id=$1 AND i.event_id=$5 AND i.state='decided'
          AND i.channel_id=v.channel_id
        JOIN public.events e ON e.community_id=v.community_id AND e.id=i.event_id AND e.created_at=i.event_created_at
          AND e.pubkey=i.author_pubkey AND e.kind=i.event_kind AND e.channel_id=i.channel_id
        WHERE e.kind IN(9,40002) AND e.deleted_at IS NULL AND octet_length(e.content)<=65536
          AND octet_length(e.pubkey)=32 AND octet_length(e.sig)=64
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
      SELECT a.*,s.community_id,s.channel_id,s.observed_at,s.valid_before,
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

    community_id := first_node.community_id;
    channel_id := first_node.channel_id;
    source_event_created_at := first_node.created_at;
    observed_at := first_node.observed_at;
    valid_before := first_node.valid_before;
    source_stamp := to_char(source_event_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"');
    IF audience_kind='thread' THEN
        thread_root_event_id := resolved_root;
        thread_root_event_created_at := resolved_root_at;
        root_stamp := to_char(resolved_root_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"');
    END IF;

    audience_wire := jsonb_build_object(
        'channel_id',channel_id,'community_id',community_id,'company_id',company,
        'employee_id',employee,'format','ortak-reviewed-conversation-audience/1',
        'kind',audience_kind,'project_id',project,'thread_root_event_created_at',root_stamp,
        'thread_root_event_id',encode(thread_root_event_id,'hex'));
    encoded := public.ortak_conversation_json75(audience_wire);
    IF encoded IS NULL THEN RETURN; END IF;
    audience_bytes := convert_to(encoded,'UTF8');
    IF octet_length(audience_bytes)>2048 THEN RETURN; END IF;
    audience_hash := public.digest(audience_bytes,'sha256');

    -- Exact SourceEvidence declaration order in Rust is lexical; tags and
    -- source content retain their original strings and array order. No body
    -- is returned, copied into provenance or replaced with a message:<id> hash.
    encoded := public.ortak_conversation_json75(jsonb_build_object(
        'author_pubkey',encode(first_node.source_author,'hex'),'channel_id',channel_id,
        'community_id',community_id,'company_id',company,'content',first_node.source_content,
        'event_created_at',source_stamp,'event_id',encode(source_id,'hex'),
        'format','ortak-reviewed-conversation-evidence/1','kind',first_node.source_kind,
        'sig',encode(first_node.source_signature,'hex'),'tags',first_node.tags));
    IF encoded IS NULL THEN RETURN; END IF;
    source_evidence_hash := public.digest(convert_to(encoded,'UTF8'),'sha256');
    encoded := public.ortak_conversation_json75(jsonb_build_object(
        'audience_hash',encode(audience_hash,'hex'),'format','ortak-reviewed-conversation-source/1',
        'source_evidence_hash',encode(source_evidence_hash,'hex')));
    IF encoded IS NULL THEN RETURN; END IF;
    source_hash := public.digest(convert_to(encoded,'UTF8'),'sha256');
    encoded := public.ortak_conversation_json75(jsonb_build_object(
        'audience',audience_wire,'audience_hash',encode(audience_hash,'hex'),
        'format','ortak-reviewed-conversation-provenance/1','source_event_created_at',source_stamp,
        'source_event_id',encode(source_id,'hex'),'source_evidence_hash',encode(source_evidence_hash,'hex'),
        'source_hash',encode(source_hash,'hex')));
    IF encoded IS NULL THEN RETURN; END IF;
    provenance_bytes := convert_to(encoded,'UTF8');
    IF octet_length(provenance_bytes)>4096 THEN RETURN; END IF;
    RETURN NEXT;
END
$$;

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

-- D4 SOURCE FRAGMENT, assembled by root after source75 + storage75.
-- No remote I/O, cleanup, per-run scan or mutation of retained use pins.
-- Storage registration retains <=128 scopes/company and <=256/community.
-- One row mutation has at most two old/new communities: inspect <=513 keys,
-- refuse >512 BEFORE updating anything, then advance in deterministic order.

CREATE INDEX idx_conversation_thread_parent_exact
    ON thread_metadata(community_id,parent_event_id,parent_event_created_at)
    WHERE parent_event_id IS NOT NULL;
CREATE INDEX idx_conversation_thread_root_exact
    ON thread_metadata(community_id,root_event_id,root_event_created_at)
    WHERE root_event_id IS NOT NULL;
CREATE INDEX idx_conversation_office_employee_keys
    ON employee_office_bindings(company_id,employee_id,public_key);

-- Neutral INSERT means precisely the same top-level identity as absent
-- metadata. Do NOT consult retained audiences for this Office-fence decision:
-- a concurrent first approval's audience may still be uncommitted/invisible.
CREATE FUNCTION ortak_conversation_thread_insert_neutral75(proposed JSONB)
RETURNS BOOLEAN LANGUAGE sql STABLE STRICT AS $$
    SELECT proposed->>'parent_event_id' IS NULL
      AND proposed->>'parent_event_created_at' IS NULL
      AND (proposed->>'depth')::integer=0
      AND ((proposed->>'root_event_id' IS NULL AND proposed->>'root_event_created_at' IS NULL)
        OR (proposed->>'root_event_id'=proposed->>'event_id'
          AND (proposed->>'root_event_created_at')::timestamptz=(proposed->>'event_created_at')::timestamptz))
      AND EXISTS(SELECT 1 FROM events e
        WHERE e.community_id=(proposed->>'community_id')::uuid
          AND e.id=(proposed->>'event_id')::bytea
          AND e.created_at=(proposed->>'event_created_at')::timestamptz
          AND e.channel_id=(proposed->>'channel_id')::uuid
          AND e.kind IN(9,40002) AND e.deleted_at IS NULL
          AND jsonb_typeof(e.tags)='array'
          AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(
            CASE WHEN jsonb_typeof(e.tags)='array' THEN e.tags ELSE '[]'::jsonb END) t(tag)
            WHERE jsonb_typeof(t.tag)<>'array'
              OR EXISTS(SELECT 1 FROM jsonb_array_elements(
                CASE WHEN jsonb_typeof(t.tag)='array' THEN t.tag ELSE '[]'::jsonb END) p(part)
                WHERE jsonb_typeof(p.part)<>'string')
              OR (t.tag->>0='e' AND (t.tag->>3 IS DISTINCT FROM 'mention'
                OR coalesce(t.tag->>1,'') COLLATE "C" !~ '^[0-9a-fA-F]{64}$'))))
$$;

-- Exact 48 body except the parentless metadata skip. The other skip cases,
-- lock ordering, try-lock failures and company/community mappings are retained.
CREATE OR REPLACE FUNCTION ortak_fence_office_mutation() RETURNS TRIGGER
LANGUAGE plpgsql VOLATILE AS $$
DECLARE
    previous JSONB := CASE WHEN TG_OP <> 'INSERT' THEN to_jsonb(OLD) END;
    proposed JSONB := CASE WHEN TG_OP <> 'DELETE' THEN to_jsonb(NEW) END;
    target UUID;
    target_company UUID;
    field TEXT;
    changed BOOLEAN := TG_OP <> 'UPDATE';
BEGIN
    IF TG_OP = 'UPDATE' THEN
        FOREACH field IN ARRAY TG_ARGV[1:TG_NARGS - 1] LOOP
            IF previous -> field IS DISTINCT FROM proposed -> field THEN
                changed := true;
                EXIT;
            END IF;
        END LOOP;
        IF NOT changed THEN RETURN NEW; END IF;
    END IF;
    IF TG_TABLE_NAME LIKE 'events%' AND TG_OP = 'INSERT' THEN RETURN NEW; END IF;
    IF TG_TABLE_NAME = 'thread_metadata' AND TG_OP = 'INSERT'
       AND ortak_conversation_thread_insert_neutral75(proposed) THEN RETURN NEW; END IF;
    IF TG_TABLE_NAME = 'runs' AND TG_OP = 'INSERT' THEN RETURN NEW; END IF;
    IF TG_TABLE_NAME = 'outbox'
       AND NOT (COALESCE(previous ->> 'kind' = 'office_publish'
                         AND previous ->> 'signed_event_id' IS NOT NULL, false)
                OR COALESCE(proposed ->> 'kind' = 'office_publish'
                            AND proposed ->> 'signed_event_id' IS NOT NULL, false)) THEN
        RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF TG_ARGV[0] IN ('community', 'binding', 'community_root') THEN
        FOR target IN
            SELECT DISTINCT value::UUID FROM (VALUES
                (previous ->> CASE WHEN TG_ARGV[0] = 'community_root' THEN 'id' ELSE 'community_id' END),
                (proposed ->> CASE WHEN TG_ARGV[0] = 'community_root' THEN 'id' ELSE 'community_id' END)
            ) AS scopes(value) WHERE value IS NOT NULL ORDER BY value::UUID
        LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_community_lock_key(target)) THEN
                RAISE EXCEPTION 'Office authority community mutation fence is busy'
                    USING ERRCODE = 'serialization_failure';
            END IF;
            SELECT company_id INTO target_company FROM office_company_bindings
             WHERE community_id = target;
            IF target_company IS NOT NULL THEN
                PERFORM ortak_advance_office_authority(target_company, TG_TABLE_NAME);
            END IF;
        END LOOP;
    END IF;
    IF TG_ARGV[0] IN ('company', 'binding', 'company_root') THEN
        FOR target IN
            SELECT DISTINCT value::UUID FROM (VALUES
                (previous ->> CASE WHEN TG_ARGV[0] = 'company_root' THEN 'id' ELSE 'company_id' END),
                (proposed ->> CASE WHEN TG_ARGV[0] = 'company_root' THEN 'id' ELSE 'company_id' END)
            ) AS scopes(value) WHERE value IS NOT NULL ORDER BY value::UUID
        LOOP
            PERFORM ortak_advance_office_authority(target, TG_TABLE_NAME);
        END LOOP;
    END IF;
    RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END
$$;
DROP TRIGGER ortak_office_authority_thread_metadata ON thread_metadata;
CREATE TRIGGER ortak_office_authority_thread_metadata BEFORE INSERT OR UPDATE OR DELETE ON thread_metadata
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community','community_id','event_id',
    'event_created_at','channel_id','parent_event_id','parent_event_created_at',
    'root_event_id','root_event_created_at','depth');
-- The 73 channel trigger, including participant_hash/ttl_seconds/ttl_deadline,
-- remains untouched.

CREATE FUNCTION ortak_advance_conversation_scopes75(
    companies UUID[], communities UUID[], channels UUID[], projects UUID[],
    employees TEXT[], public_keys BYTEA[], selection TEXT, reason TEXT,
    office_fence BOOLEAN
) RETURNS VOID LANGUAGE plpgsql VOLATILE AS $$
DECLARE target UUID; project_key RECORD; keys JSONB; selected JSONB; key_hex TEXT[];
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Conversation authority requires READ COMMITTED isolation'
            USING ERRCODE='invalid_transaction_state';
    END IF;
    IF companies IS NULL OR communities IS NULL OR channels IS NULL OR projects IS NULL
       OR employees IS NULL OR public_keys IS NULL OR selection IS NULL OR reason IS NULL OR office_fence IS NULL
       OR selection NOT IN('scope','channel','membership','project','identity','employee')
       OR reason NOT IN('channel_changed','membership_changed','project_changed','project_grant_changed',
            'event_changed','thread_changed','identity_changed','scope_closed')
       OR cardinality(companies)>2 OR cardinality(communities)>2 OR cardinality(channels)>2
       OR cardinality(projects)>2 OR cardinality(employees)>2 OR cardinality(public_keys)>2 THEN
        RAISE EXCEPTION 'Conversation mutation selection is invalid' USING ERRCODE='check_violation';
    END IF;
    IF office_fence THEN
        -- Acquire discovery's absent-row fence BEFORE selecting retained keys.
        -- Registration holds the matching Office shared locks. Try-locks retain
        -- 48's reverse-order refusal when the mutating tuple is already locked.
        FOR target IN SELECT DISTINCT v FROM unnest(communities) v ORDER BY v LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_community_lock_key(target)) THEN
                RAISE EXCEPTION 'Conversation community mutation is busy' USING ERRCODE='serialization_failure';
            END IF;
        END LOOP;
        FOR target IN SELECT DISTINCT v FROM unnest(companies) v ORDER BY v LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
                RAISE EXCEPTION 'Conversation company mutation is busy' USING ERRCODE='serialization_failure';
            END IF;
        END LOOP;
    ELSE
        -- Project archive/binding/grant writers can run under signed shared
        -- Office authentication. NEVER upgrade that Office lock. Project row
        -- locking blocks both grant races and newly registered scope phantoms.
        IF selection<>'project' OR cardinality(companies)=0 OR cardinality(projects)=0 THEN
            RAISE EXCEPTION 'Conversation project mutation selection is invalid' USING ERRCODE='check_violation';
        END IF;
        FOR project_key IN SELECT DISTINCT c AS company_id,p AS project_id
            FROM unnest(companies) c CROSS JOIN unnest(projects) p ORDER BY c,p LOOP
            PERFORM 1 FROM public.projects p WHERE p.company_id=project_key.company_id
                AND p.id=project_key.project_id FOR UPDATE NOWAIT;
        END LOOP;
    END IF;
    SELECT coalesce(array_agg(encode(v,'hex')),ARRAY[]::text[]) INTO key_hex FROM unnest(public_keys) v;
    SELECT coalesce(jsonb_agg(to_jsonb(candidate) ORDER BY candidate.company_id,candidate.project_id,candidate.channel_id),'[]'::jsonb)
      INTO keys FROM (
        SELECT a.company_id,a.project_id,a.channel_id
        FROM conversation_memory_authorities a
        JOIN public.communities cm ON cm.id=a.community_id AND cm.deletion_state='active' AND cm.deleted_at IS NULL
        WHERE (CASE WHEN cardinality(companies)>0 THEN a.company_id=ANY(companies)
                    ELSE a.community_id=ANY(communities) END)
          AND (selection='scope'
            OR (selection='project' AND a.project_id=ANY(projects))
            OR (selection IN('channel','membership') AND a.channel_id=ANY(channels))
            OR (selection IN('identity','employee','membership') AND (
                EXISTS(SELECT 1 FROM channel_members m WHERE m.community_id=a.community_id AND m.channel_id=a.channel_id
                    AND m.pubkey=ANY(public_keys))
                OR EXISTS(SELECT 1 FROM project_access_grants g WHERE g.company_id=a.company_id
                    AND g.project_id=a.project_id AND g.actor_pubkey=ANY(key_hex))
                OR EXISTS(SELECT 1 FROM reviewed_memory_conversation_audiences f
                    WHERE f.company_id=a.company_id AND f.project_id=a.project_id AND f.channel_id=a.channel_id
                      AND (f.employee_id=ANY(employees) OR EXISTS(SELECT 1 FROM employee_office_bindings b
                        WHERE b.company_id=f.company_id AND b.employee_id=f.employee_id AND b.public_key=ANY(public_keys))))
                OR EXISTS(SELECT 1 FROM reviewed_memory_targets t
                    WHERE t.company_id=a.company_id AND t.project_id=a.project_id AND t.conversation_channel_id=a.channel_id
                      AND (t.employee_id=ANY(employees) OR EXISTS(SELECT 1 FROM employee_office_bindings b
                        WHERE b.company_id=t.company_id AND b.employee_id=t.employee_id AND b.public_key=ANY(public_keys))))
                OR EXISTS(SELECT 1 FROM employee_office_bindings b JOIN channel_members m
                    ON m.community_id=a.community_id AND m.channel_id=a.channel_id AND m.pubkey=b.public_key
                    WHERE b.company_id=a.company_id AND b.employee_id=ANY(employees)))))
        ORDER BY a.company_id,a.project_id,a.channel_id LIMIT 513
      ) candidate;
    IF jsonb_array_length(keys)>512 THEN
        RAISE EXCEPTION 'Conversation mutation exceeds retained scope bound' USING ERRCODE='program_limit_exceeded';
    END IF;
    -- Retained mappings, not only current office_company_bindings, determine
    -- company identity. Closed communities were retired BEFORE their first
    -- close; later mutations leave that epoch/reason intact. They must not
    -- demand an old deletion lease or bypass the universal community fence.
    IF office_fence THEN
        FOR target IN SELECT DISTINCT (v->>'company_id')::uuid FROM jsonb_array_elements(keys) v ORDER BY 1 LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
                RAISE EXCEPTION 'Conversation retained company mutation is busy' USING ERRCODE='serialization_failure';
            END IF;
        END LOOP;
    END IF;
    FOR selected IN SELECT v FROM jsonb_array_elements(keys) v LOOP
        PERFORM 1 FROM public.projects p WHERE p.company_id=(selected->>'company_id')::uuid
            AND p.id=(selected->>'project_id')::uuid FOR SHARE NOWAIT;
        PERFORM 1 FROM conversation_memory_authorities a
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.project_id=(selected->>'project_id')::uuid
                AND a.channel_id=(selected->>'channel_id')::uuid FOR UPDATE NOWAIT;
        UPDATE conversation_memory_authorities a SET epoch=a.epoch+1,last_change_reason=reason
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.project_id=(selected->>'project_id')::uuid
                AND a.channel_id=(selected->>'channel_id')::uuid;
    END LOOP;
END
$$;

CREATE FUNCTION ortak_conversation_epoch_mutation75() RETURNS TRIGGER LANGUAGE plpgsql VOLATILE AS $$
DECLARE
    previous JSONB := CASE WHEN TG_OP<>'INSERT' THEN to_jsonb(OLD) END;
    proposed JSONB := CASE WHEN TG_OP<>'DELETE' THEN to_jsonb(NEW) END;
    fields TEXT[]; field TEXT; changed BOOLEAN := TG_OP<>'UPDATE';
    companies UUID[]; communities UUID[]; channels UUID[]; projects UUID[];
    employees TEXT[]; public_keys BYTEA[];
    kind TEXT := TG_ARGV[0]; selection TEXT; reason TEXT; office_fence BOOLEAN := true;
    old_manifest JSONB; new_manifest JSONB;
BEGIN
    CASE kind
    WHEN 'channel' THEN fields:=ARRAY['community_id','id','channel_type','visibility','archived_at','deleted_at','participant_hash','ttl_seconds','ttl_deadline']; selection:='channel'; reason:='channel_changed';
    WHEN 'membership' THEN fields:=ARRAY['community_id','channel_id','pubkey','role','removed_at']; selection:='membership'; reason:='membership_changed';
    WHEN 'event' THEN fields:=ARRAY['community_id','id','created_at','pubkey','kind','tags','content','sig','channel_id','deleted_at']; selection:='channel'; reason:='event_changed';
    WHEN 'thread' THEN fields:=ARRAY['community_id','event_id','event_created_at','channel_id','parent_event_id','parent_event_created_at','root_event_id','root_event_created_at','depth']; selection:='channel'; reason:='thread_changed';
    WHEN 'inbox' THEN fields:=ARRAY['company_id','event_id','event_created_at','event_kind','author_pubkey','channel_id','state']; selection:='channel'; reason:='event_changed';
    WHEN 'project' THEN fields:=ARRAY['company_id','id','status','archived_at']; selection:='project'; reason:='project_changed'; office_fence:=false;
    WHEN 'project_binding' THEN fields:=ARRAY['company_id','project_id','community_id','channel_id']; selection:='project'; reason:='project_changed'; office_fence:=false;
    WHEN 'grant' THEN fields:=ARRAY['company_id','project_id','actor_pubkey','role','revoked_at']; selection:='project'; reason:='project_grant_changed'; office_fence:=false;
    WHEN 'user' THEN fields:=ARRAY['community_id','pubkey','agent_type','agent_owner_pubkey','deactivated_at']; selection:='identity'; reason:='identity_changed';
    WHEN 'employee' THEN fields:=ARRAY['company_id','id','status','active_revision_id']; selection:='employee'; reason:='identity_changed';
    WHEN 'office_identity' THEN fields:=ARRAY['company_id','employee_id','public_key','signer_ref','valid_from','valid_until']; selection:='employee'; reason:='identity_changed';
    WHEN 'memory_identity' THEN fields:=ARRAY['company_id','employee_id','revision_id','adapter','endpoint_ref','workspace','user_peer','employee_peer','options']; selection:='employee'; reason:='identity_changed';
    WHEN 'company' THEN fields:=ARRAY['id','status']; selection:='scope'; reason:='scope_closed';
    WHEN 'community' THEN fields:=ARRAY['id','deletion_state','deletion_fence_generation','deleted_at']; selection:='scope'; reason:='scope_closed';
    WHEN 'company_binding' THEN fields:=ARRAY['company_id','community_id']; selection:='scope'; reason:='scope_closed';
    ELSE RAISE EXCEPTION 'Conversation mutation kind is invalid' USING ERRCODE='check_violation';
    END CASE;
    IF TG_OP='UPDATE' THEN
        FOREACH field IN ARRAY fields LOOP
            IF previous->field IS DISTINCT FROM proposed->field THEN changed:=true; EXIT; END IF;
        END LOOP;
        IF kind='office_identity' THEN changed:=changed OR ((previous->>'verified_at' IS NULL)<>(proposed->>'verified_at' IS NULL)); END IF;
        IF kind='memory_identity' THEN changed:=changed OR ((previous->>'validated_at' IS NULL)<>(proposed->>'validated_at' IS NULL)); END IF;
        IF NOT changed THEN RETURN NEW; END IF;
    END IF;
    IF kind='community' AND coalesce(previous->>'deletion_state','')<>'active' THEN
        -- The first transition out of active retired every scope under the
        -- Office exclusive fence. Later closure stages and return to active
        -- never lower that epoch, so no old use can revive. Run this hook
        -- BEFORE the first close while the universal write fence allows it.
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='thread' AND TG_OP='INSERT' THEN
        IF ortak_conversation_thread_insert_neutral75(proposed) THEN RETURN NEW; END IF;
        -- Exclude the just-inserted metadata row from its own reference proof.
        -- Child/root indexes cover a channel fact whose canonical root is not
        -- retained in its channel-wide audience. New unrelated replies have
        -- neither retained anchors nor existing descendants and do not bump.
        IF NOT EXISTS(SELECT 1 FROM reviewed_memory_conversation_audiences a
            WHERE a.community_id=(proposed->>'community_id')::uuid AND
              ((a.source_event_id=(proposed->>'event_id')::bytea AND a.source_event_created_at=(proposed->>'event_created_at')::timestamptz)
                OR (a.thread_root_event_id=(proposed->>'event_id')::bytea AND a.thread_root_event_created_at=(proposed->>'event_created_at')::timestamptz)))
           AND NOT EXISTS(SELECT 1 FROM thread_metadata t WHERE t.community_id=(proposed->>'community_id')::uuid
             AND (t.event_id,t.event_created_at) IS DISTINCT FROM ((proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz)
             AND ((t.parent_event_id=(proposed->>'event_id')::bytea AND t.parent_event_created_at=(proposed->>'event_created_at')::timestamptz)
               OR (t.root_event_id=(proposed->>'event_id')::bytea AND t.root_event_created_at=(proposed->>'event_created_at')::timestamptz))) THEN RETURN NEW; END IF;
    END IF;
    IF kind='inbox' AND coalesce(previous->>'state','')<>'decided'
       AND NOT EXISTS(SELECT 1 FROM reviewed_memory_conversation_audiences a
         WHERE (a.company_id,a.source_event_id,a.source_event_created_at) IN (
           ((previous->>'company_id')::uuid,(previous->>'event_id')::bytea,(previous->>'event_created_at')::timestamptz),
           ((proposed->>'company_id')::uuid,(proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='employee' AND TG_OP='UPDATE' AND previous->>'company_id'=proposed->>'company_id'
       AND previous->>'id'=proposed->>'id' AND previous->>'status'=proposed->>'status' THEN
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO old_manifest
            FROM employee_revisions r WHERE r.company_id=(previous->>'company_id')::uuid AND r.employee_id=previous->>'id'
                AND r.id=(previous->>'active_revision_id')::uuid;
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO new_manifest
            FROM employee_revisions r WHERE r.company_id=(proposed->>'company_id')::uuid AND r.employee_id=proposed->>'id'
                AND r.id=(proposed->>'active_revision_id')::uuid;
        IF old_manifest IS NOT NULL AND old_manifest IS NOT DISTINCT FROM new_manifest THEN RETURN NEW; END IF;
    END IF;
    IF kind='memory_identity' AND NOT EXISTS(SELECT 1 FROM public.employees e
        WHERE (e.company_id,e.id,e.active_revision_id) IN (
          ((previous->>'company_id')::uuid,previous->>'employee_id',(previous->>'revision_id')::uuid),
          ((proposed->>'company_id')::uuid,proposed->>'employee_id',(proposed->>'revision_id')::uuid))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='user' AND TG_OP IN('INSERT','DELETE')
       AND coalesce(proposed,previous)->>'agent_type' IS NULL
       AND coalesce(proposed,previous)->>'agent_owner_pubkey' IS NULL
       AND coalesce(proposed,previous)->>'deactivated_at' IS NULL THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    -- Ordinary first joins cannot invalidate a previously authorized reader;
    -- bot insertion is different: the resolver treats that key as automated
    -- across its entire community, including other retained channels/projects.
    IF kind='membership' AND TG_OP='INSERT' AND proposed->>'role'<>'bot' THEN RETURN NEW; END IF;

    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO companies FROM (VALUES
        (previous->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END),
        (proposed->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO communities FROM (VALUES
        (previous->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END),
        (proposed->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO channels FROM (VALUES
        (previous->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END),
        (proposed->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO projects FROM (VALUES
        (previous->>CASE WHEN kind='project' THEN 'id' ELSE 'project_id' END),
        (proposed->>CASE WHEN kind='project' THEN 'id' ELSE 'project_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v),ARRAY[]::text[]) INTO employees FROM (VALUES
        (previous->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END),
        (proposed->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::bytea),ARRAY[]::bytea[]) INTO public_keys FROM (VALUES
        (previous->>CASE WHEN kind='office_identity' THEN 'public_key' ELSE 'pubkey' END),
        (proposed->>CASE WHEN kind='office_identity' THEN 'public_key' ELSE 'pubkey' END)) t(v) WHERE v IS NOT NULL;
    IF kind='membership' AND coalesce(previous->>'role','')<>'bot' AND coalesce(proposed->>'role','')<>'bot' THEN
        public_keys:=ARRAY[]::bytea[];
    END IF;
    PERFORM ortak_advance_conversation_scopes75(companies,communities,channels,projects,employees,public_keys,selection,reason,office_fence);
    RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
END
$$;

-- AFTER hooks run only after the retained 48/54/73 mutation guards acquired
-- their fences; the helper also covers absent mappings and newly watched data.
-- Community closure is the explicit BEFORE exception, alphabetically after
-- ortak_office_authority_communities and before the universal fence closes.
CREATE TRIGGER conversation_epoch_channels AFTER INSERT OR UPDATE OR DELETE ON channels FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('channel');
CREATE TRIGGER conversation_epoch_members AFTER INSERT OR UPDATE OR DELETE ON channel_members FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('membership');
CREATE TRIGGER conversation_epoch_events AFTER UPDATE OR DELETE ON events FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('event');
CREATE TRIGGER conversation_epoch_threads AFTER INSERT OR UPDATE OR DELETE ON thread_metadata FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('thread');
CREATE TRIGGER conversation_epoch_inbox AFTER UPDATE OR DELETE ON office_inbox FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('inbox');
CREATE TRIGGER conversation_epoch_projects AFTER UPDATE OR DELETE ON projects FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('project');
CREATE TRIGGER conversation_epoch_project_bindings AFTER INSERT OR UPDATE OR DELETE ON project_api_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('project_binding');
CREATE TRIGGER conversation_epoch_grants AFTER INSERT OR UPDATE OR DELETE ON project_access_grants FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('grant');
CREATE TRIGGER conversation_epoch_users AFTER INSERT OR UPDATE OR DELETE ON users FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('user');
CREATE TRIGGER conversation_epoch_employees AFTER INSERT OR UPDATE OR DELETE ON employees FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('employee');
CREATE TRIGGER conversation_epoch_office_identities AFTER INSERT OR UPDATE OR DELETE ON employee_office_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('office_identity');
CREATE TRIGGER conversation_epoch_memory_identities AFTER INSERT OR UPDATE OR DELETE ON employee_memory_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('memory_identity');
CREATE TRIGGER conversation_epoch_companies AFTER UPDATE OR DELETE ON companies FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('company');
CREATE TRIGGER ortak_z_conversation_epoch_communities BEFORE UPDATE OR DELETE ON communities FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('community');
CREATE TRIGGER conversation_epoch_company_bindings AFTER INSERT OR UPDATE OR DELETE ON office_company_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('company_binding');

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
