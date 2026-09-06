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
