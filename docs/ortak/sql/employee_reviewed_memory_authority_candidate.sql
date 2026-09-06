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
