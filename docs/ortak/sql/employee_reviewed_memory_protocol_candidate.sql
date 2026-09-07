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
