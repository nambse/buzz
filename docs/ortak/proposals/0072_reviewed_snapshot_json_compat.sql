-- Proposal 72: restore valid JSON scratch compatibility after migration 71.
-- Additive function replacement only. Do not rewrite frozen snapshot bytes,
-- retained uses, original migrations, or current reviewed authority functions.
-- Root owns migration, desired schema and exact-function reconciler integration.

-- PostgreSQL json preserves escaped NUL; jsonb cannot represent it. For scratch
-- comparison only, encode NUL as SOH+STX and SOH as SOH+SOH. The prefix encoding is
-- injective, including adjacent controls and literal backslash-u sequences.
-- A JSON control byte must be a Unicode escape. The negative lookbehind and
-- paired-backslash prefix match true escapes, never an escaped backslash.
-- This does not change stored bytes or the actual runtime input.
CREATE FUNCTION ortak_snapshot_scratch_jsonb(value JSON) RETURNS JSONB
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $function$
    SELECT regexp_replace(
        regexp_replace(value::text,
            $pattern$(?<!\\)((?:\\\\)*)\\u0001$pattern$,
            $replacement$\1\\u0001\\u0001$replacement$,'g'),
        $pattern$(?<!\\)((?:\\\\)*)\\u0000$pattern$,
        $replacement$\1\\u0001\\u0002$replacement$,'g')::jsonb
$function$;

CREATE OR REPLACE FUNCTION ortak_reviewed_snapshot_consistent() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; run UUID; wire JSONB; used_count INTEGER; record JSONB; pin JSONB; i INTEGER=0; scratch_count INTEGER; total_bytes INTEGER=0; rendered JSONB; u run_reviewed_memory_uses; f reviewed_memory_facts;
BEGIN
    company=NEW.company_id; run=NEW.run_id;
    -- Even PostgreSQL json field access may unescape unrelated NUL values.
    -- Encode the whole comparison document before performing any field access.
    SELECT ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json) INTO wire FROM run_context_snapshots s WHERE s.company_id=company AND s.run_id=run;
    SELECT count(*) INTO used_count FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run;
    IF wire IS NULL THEN RAISE EXCEPTION 'ortak: reviewed snapshot missing' USING ERRCODE='check_violation'; END IF;
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
