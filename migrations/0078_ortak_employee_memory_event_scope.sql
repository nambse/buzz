-- Native read-state replacement cannot revoke unrelated employee memory.
-- Retain every schema77 source/membership/lifecycle/cleanup guard; only
-- event mutations outside the canonical plaintext vocabulary are neutral.

CREATE OR REPLACE FUNCTION ortak_employee_memory_epoch_mutation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE prior JSONB; proposed JSONB; kind TEXT:=TG_ARGV[0]; reason TEXT:=TG_ARGV[1];
    changed BOOLEAN:=TG_OP<>'UPDATE'; field TEXT; co UUID[]; cm UUID[];
    channels UUID[]; employee_keys TEXT[]; target UUID; keys JSONB; selected JSONB;
    old_identity JSONB; new_identity JSONB;
BEGIN
    IF TG_OP<>'INSERT' THEN prior=to_jsonb(OLD); END IF;
    IF TG_OP<>'DELETE' THEN proposed=to_jsonb(NEW); END IF;
    -- Only plaintext Office events can be a canonical employee-memory source
    -- or ancestor. Native NIP-RS (30078) replaces its old encrypted read-state
    -- payload by deletion; a NULL channel there is not a company-wide source
    -- revocation. Check BOTH sides so changing into or out of 9/40002 still
    -- retires the old use. The existing Office mutation fence remains intact.
    IF kind='event' AND NOT (
        coalesce((prior->>'kind')::integer IN (9,40002),false)
        OR coalesce((proposed->>'kind')::integer IN (9,40002),false)
    ) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
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

