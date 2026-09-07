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
