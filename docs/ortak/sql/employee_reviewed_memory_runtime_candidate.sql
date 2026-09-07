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
