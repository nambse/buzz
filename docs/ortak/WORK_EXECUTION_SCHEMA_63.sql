-- PROPOSAL ONLY: root assigns/integrates migration numbering and desired schema.
-- E2: human-authorized Work execution, immutable text deliverable, human review.
CREATE TABLE work_authority_generations (
    company_id UUID NOT NULL REFERENCES companies(id),
    project_id UUID NOT NULL,
    generation BIGINT NOT NULL DEFAULT 0 CHECK (generation >= 0),
    PRIMARY KEY(company_id,project_id),
    FOREIGN KEY(company_id,project_id) REFERENCES projects(company_id,id)
);
INSERT INTO work_authority_generations(company_id,project_id) SELECT company_id,id FROM projects;
CREATE FUNCTION ortak_work_generation_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' OR (NEW.company_id,NEW.project_id) IS DISTINCT FROM (OLD.company_id,OLD.project_id)
       OR NEW.generation<>OLD.generation+1 THEN
        RAISE EXCEPTION 'ortak: Work generation only advances' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_generation_guard BEFORE UPDATE OR DELETE ON work_authority_generations
FOR EACH ROW EXECUTE FUNCTION ortak_work_generation_guard();
CREATE TRIGGER work_generation_no_truncate BEFORE TRUNCATE ON work_authority_generations
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_advance_work_authority() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; project UUID;
BEGIN
    company:=NEW.company_id;
    IF TG_TABLE_NAME='projects' THEN project:=NEW.id;
    ELSIF TG_TABLE_NAME IN ('work_items','project_access_grants') THEN project:=NEW.project_id;
    ELSE SELECT project_id INTO project FROM work_items WHERE company_id=company AND id=NEW.work_item_id;
    END IF;
    INSERT INTO work_authority_generations(company_id,project_id) VALUES(company,project)
    ON CONFLICT(company_id,project_id) DO UPDATE SET generation=work_authority_generations.generation+1;
    RETURN NEW;
END $$;
CREATE TRIGGER work_authority_projects AFTER INSERT OR UPDATE ON projects
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_grants AFTER INSERT OR UPDATE ON project_access_grants
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_items AFTER INSERT OR UPDATE ON work_items
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_assignments AFTER INSERT OR UPDATE ON work_assignments
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_dependencies AFTER INSERT ON work_dependencies
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_criteria AFTER INSERT OR UPDATE ON work_acceptance_criteria
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_approvals AFTER INSERT OR UPDATE ON work_approvals
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();

-- Domain commands lock project then item before child mutations. Direct writers
-- must obey the same parent authority; NOWAIT refuses reversed child lock order.
CREATE FUNCTION ortak_work_child_authority_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE parent_project UUID;
BEGIN
    IF TG_TABLE_NAME='work_assignments' THEN
        IF TG_OP='UPDATE' AND (NEW.company_id,NEW.work_item_id,NEW.employee_id) IS DISTINCT FROM (OLD.company_id,OLD.work_item_id,OLD.employee_id) THEN
            RAISE EXCEPTION 'ortak: Work assignment identity is immutable' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
    END IF;
    SELECT project_id INTO parent_project FROM work_items WHERE company_id=NEW.company_id AND id=NEW.work_item_id;
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=parent_project FOR SHARE NOWAIT;
    PERFORM 1 FROM work_items WHERE company_id=NEW.company_id AND id=NEW.work_item_id FOR UPDATE NOWAIT;
    IF NOT FOUND THEN RAISE EXCEPTION 'ortak: Work authority parent is missing' USING ERRCODE='foreign_key_violation'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_assignment_authority_guard BEFORE INSERT OR UPDATE ON work_assignments
    FOR EACH ROW EXECUTE FUNCTION ortak_work_child_authority_guard();
CREATE TRIGGER work_dependency_authority_guard BEFORE INSERT ON work_dependencies
    FOR EACH ROW EXECUTE FUNCTION ortak_work_child_authority_guard();
CREATE TRIGGER work_criterion_authority_guard BEFORE INSERT OR UPDATE ON work_acceptance_criteria
    FOR EACH ROW EXECUTE FUNCTION ortak_work_child_authority_guard();
CREATE TRIGGER work_approval_authority_guard BEFORE INSERT OR UPDATE ON work_approvals
    FOR EACH ROW EXECUTE FUNCTION ortak_work_child_authority_guard();

ALTER TABLE runs ADD COLUMN work_admission_generation BIGINT CHECK(work_admission_generation>=0),
    ADD COLUMN work_admission_token UUID,
    ADD CONSTRAINT runs_work_admission_pair CHECK((work_admission_generation IS NULL)=(work_admission_token IS NULL)),
    ADD CONSTRAINT runs_work_origin_exclusive CHECK(work_item_id IS NULL OR
        (routing_decision_id IS NULL AND message_id IS NULL AND root_message_id IS NULL));

CREATE TABLE work_executions (
    company_id UUID NOT NULL REFERENCES companies(id),
    run_id UUID NOT NULL,
    project_id UUID NOT NULL,
    work_item_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    employee_revision_id UUID NOT NULL,
    requested_by TEXT NOT NULL CHECK(requested_by ~ '^[0-9a-f]{64}$'),
    operation_id UUID NOT NULL,
    requested_version BIGINT NOT NULL CHECK(requested_version>=1),
    execution_version BIGINT NOT NULL CHECK(execution_version=requested_version+1),
    definition_bytes BYTEA NOT NULL CHECK(octet_length(definition_bytes) BETWEEN 1 AND 32768),
    definition_hash BYTEA NOT NULL CHECK(octet_length(definition_hash)=32 AND definition_hash=sha256(definition_bytes)),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    reconciled_at TIMESTAMPTZ,
    result_code TEXT CHECK(result_code ~ '^[a-z][a-z0-9_]{0,63}$'),
    PRIMARY KEY(company_id,run_id),
    UNIQUE(company_id,requested_by,operation_id),
    UNIQUE(company_id,work_item_id,requested_version),
    FOREIGN KEY(company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY(company_id,project_id,work_item_id) REFERENCES work_items(company_id,project_id,id),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id),
    FOREIGN KEY(company_id,requested_by,operation_id) REFERENCES work_api_operations(company_id,actor_pubkey,operation_id)
        DEFERRABLE INITIALLY DEFERRED,
    CHECK((reconciled_at IS NULL)=(result_code IS NULL))
);
CREATE UNIQUE INDEX idx_work_execution_active ON work_executions(company_id,work_item_id) WHERE reconciled_at IS NULL;
CREATE INDEX idx_work_execution_item ON work_executions(company_id,work_item_id,requested_at DESC,run_id);
CREATE FUNCTION ortak_work_execution_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (to_jsonb(NEW)-'reconciled_at'-'result_code') IS DISTINCT FROM (to_jsonb(OLD)-'reconciled_at'-'result_code')
       OR OLD.reconciled_at IS NOT NULL OR NEW.reconciled_at IS NULL
       OR NOT EXISTS(SELECT 1 FROM runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND status IN('completed','failed','cancelled')) THEN
        RAISE EXCEPTION 'ortak: Work execution pins its request and only closes once' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_execution_guard BEFORE UPDATE ON work_executions FOR EACH ROW EXECUTE FUNCTION ortak_work_execution_guard();
CREATE TRIGGER work_execution_no_delete BEFORE DELETE ON work_executions FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER work_execution_no_truncate BEFORE TRUNCATE ON work_executions FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

ALTER TABLE outbox DROP CONSTRAINT outbox_kind_check;
ALTER TABLE outbox ADD CONSTRAINT outbox_kind_check CHECK(kind IN('run_dispatch','work_run_dispatch','office_publish')),
    ADD CONSTRAINT outbox_work_dispatch_shape CHECK(kind<>'work_run_dispatch' OR
        (run_id IS NOT NULL AND employee_id IS NOT NULL AND routing_decision_id IS NULL));
CREATE UNIQUE INDEX idx_outbox_work_dispatch ON outbox(company_id,run_id) WHERE kind='work_run_dispatch';
ALTER TABLE runtime_cancellations DROP CONSTRAINT runtime_cancellations_reason_check;
ALTER TABLE runtime_cancellations ADD CONSTRAINT runtime_cancellations_reason_check
    CHECK(reason IN('office_revoked','human_requested','work_revoked'));

CREATE TABLE artifacts (
    company_id UUID NOT NULL REFERENCES companies(id),
    id UUID NOT NULL,
    project_id UUID NOT NULL,
    work_item_id UUID NOT NULL,
    run_id UUID NOT NULL,
    terminal_sequence BIGINT NOT NULL,
    employee_id TEXT NOT NULL,
    employee_revision_id UUID NOT NULL,
    media_type TEXT NOT NULL DEFAULT 'text/plain; charset=utf-8' CHECK(media_type='text/plain; charset=utf-8'),
    content_bytes BYTEA NOT NULL CHECK(octet_length(content_bytes) BETWEEN 1 AND 32768),
    content_hash BYTEA NOT NULL CHECK(octet_length(content_hash)=32 AND content_hash=sha256(content_bytes)),
    size_bytes INT NOT NULL CHECK(size_bytes=octet_length(content_bytes)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,run_id),
    UNIQUE(company_id,work_item_id,id),
    FOREIGN KEY(company_id,project_id,work_item_id) REFERENCES work_items(company_id,project_id,id),
    FOREIGN KEY(company_id,run_id) REFERENCES work_executions(company_id,run_id),
    FOREIGN KEY(company_id,run_id,terminal_sequence) REFERENCES run_events(company_id,run_id,sequence),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id)
);
CREATE TRIGGER artifacts_immutable BEFORE UPDATE OR DELETE ON artifacts FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER artifacts_no_truncate BEFORE TRUNCATE ON artifacts FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
ALTER TABLE work_attachments ADD COLUMN artifact_id UUID,
    ADD CONSTRAINT work_attachment_artifact_fk FOREIGN KEY(company_id,work_item_id,artifact_id) REFERENCES artifacts(company_id,work_item_id,id),
    ADD CONSTRAINT work_attachment_artifact_shape CHECK((kind='artifact')=(artifact_id IS NOT NULL));
ALTER TABLE work_attachments DROP CONSTRAINT work_attachments_kind_check;
ALTER TABLE work_attachments ADD CONSTRAINT work_attachments_kind_check CHECK(kind IN('office_message','routing_decision','run','artifact'));
CREATE UNIQUE INDEX idx_work_attachments_artifact ON work_attachments(company_id,work_item_id,artifact_id) WHERE artifact_id IS NOT NULL;

CREATE TABLE runtime_work_outputs (
    company_id UUID NOT NULL REFERENCES companies(id),
    run_id UUID NOT NULL,
    terminal_sequence BIGINT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','materialized','failed')),
    artifact_id UUID,
    attempt_count INT NOT NULL DEFAULT 0 CHECK(attempt_count BETWEEN 0 AND 20),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK(last_error_code ~ '^[a-z][a-z0-9_]{0,63}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    completed_at TIMESTAMPTZ,
    PRIMARY KEY(company_id,run_id),
    FOREIGN KEY(company_id,run_id) REFERENCES work_executions(company_id,run_id),
    FOREIGN KEY(company_id,run_id,terminal_sequence) REFERENCES run_events(company_id,run_id,sequence),
    FOREIGN KEY(company_id,artifact_id) REFERENCES artifacts(company_id,id),
    CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),
    CHECK((state='pending')=(completed_at IS NULL)),
    CHECK(state='pending' OR lease_token IS NULL),
    CHECK((state='materialized')=(artifact_id IS NOT NULL)),
    CHECK(state<>'failed' OR last_error_code IS NOT NULL)
);
CREATE INDEX idx_runtime_work_outputs_due ON runtime_work_outputs(company_id,next_attempt_at,created_at,run_id) WHERE state='pending';
CREATE FUNCTION ortak_work_output_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.company_id,NEW.run_id,NEW.terminal_sequence,NEW.created_at) IS DISTINCT FROM
       (OLD.company_id,OLD.run_id,OLD.terminal_sequence,OLD.created_at)
       OR NEW.attempt_count<OLD.attempt_count OR OLD.state<>'pending' THEN
        RAISE EXCEPTION 'ortak: Work output attribution is immutable and terminal state is final' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_output_guard BEFORE UPDATE ON runtime_work_outputs FOR EACH ROW EXECUTE FUNCTION ortak_work_output_guard();
CREATE TRIGGER work_output_no_delete BEFORE DELETE ON runtime_work_outputs FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER work_output_no_truncate BEFORE TRUNCATE ON runtime_work_outputs FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE OR REPLACE FUNCTION ortak_schedule_completed_office_output() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.work_item_id IS NULL AND NEW.routing_decision_id IS NOT NULL
       AND NEW.status='completed' AND NEW.delivery_intent IN('reply','channel') THEN
        INSERT INTO runtime_office_outputs(company_id,run_id) VALUES(NEW.company_id,NEW.id)
        ON CONFLICT(company_id,run_id) DO NOTHING;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE FUNCTION ortak_schedule_work_output() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.event_type IN('run.completed','run.failed','run.cancelled') AND EXISTS(
        SELECT 1 FROM work_executions WHERE company_id=NEW.company_id AND run_id=NEW.run_id) THEN
        INSERT INTO runtime_work_outputs(company_id,run_id,terminal_sequence) VALUES(NEW.company_id,NEW.run_id,NEW.sequence)
        ON CONFLICT(company_id,run_id) DO NOTHING;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_output_schedule AFTER INSERT ON run_events FOR EACH ROW EXECUTE FUNCTION ortak_schedule_work_output();

CREATE FUNCTION ortak_check_work_execution_request() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE definition JSONB;
BEGIN
    definition:=convert_from(NEW.definition_bytes,'UTF8')::jsonb;
    IF NOT EXISTS(
        SELECT 1 FROM runs r JOIN work_items w ON w.company_id=r.company_id AND w.id=r.work_item_id
        JOIN work_item_history h ON h.company_id=w.company_id AND h.work_item_id=w.id AND h.version=NEW.execution_version
        JOIN work_api_operations o ON o.company_id=NEW.company_id AND o.actor_pubkey=NEW.requested_by AND o.operation_id=NEW.operation_id
        JOIN outbox ticket ON ticket.company_id=r.company_id AND ticket.run_id=r.id AND ticket.kind='work_run_dispatch'
        JOIN work_attachments attachment ON attachment.company_id=r.company_id AND attachment.work_item_id=w.id AND attachment.run_id=r.id
        WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.work_item_id=NEW.work_item_id
        AND r.employee_id=NEW.employee_id AND r.employee_revision_id=NEW.employee_revision_id
        AND r.routing_decision_id IS NULL AND r.message_id IS NULL AND r.root_message_id IS NULL
        AND ticket.employee_id=NEW.employee_id AND ticket.routing_decision_id IS NULL
        AND w.project_id=NEW.project_id AND w.version=NEW.execution_version AND w.state='in_progress'
        AND h.event_type='work.execution_requested' AND h.actor_type='human' AND h.actor_id=NEW.requested_by
        AND h.payload->>'run_id'=NEW.run_id::text AND h.payload->>'employee_id'=NEW.employee_id
        AND o.action='mutate_work_item' AND o.project_id=NEW.project_id AND o.work_item_id=NEW.work_item_id
        AND o.result_version=NEW.execution_version
        AND o.request_hash=sha256(convert_to(format('["start_execution","%s",%s,"%s"]',NEW.work_item_id,NEW.requested_version,NEW.employee_id),'UTF8'))
        AND h.xmin::text::bigint=txid_current()%4294967296 AND o.xmin::text::bigint=txid_current()%4294967296
        AND definition->>'type'='work_item' AND definition->>'work_item_id'=w.id::text
        AND definition->>'project_id'=w.project_id::text AND definition->>'title'=w.title AND definition->>'description'=w.description
        AND definition->'acceptance_criteria'=coalesce((SELECT jsonb_agg(jsonb_build_object('id',cr.id,'text',cr.text) ORDER BY cr.position)
            FROM work_acceptance_criteria cr WHERE cr.company_id=w.company_id AND cr.work_item_id=w.id),'[]'::jsonb)
    ) THEN
        RAISE EXCEPTION 'ortak: Work execution requires its atomic human request, definition and run provenance'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER work_execution_request_at_commit AFTER INSERT ON work_executions
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_work_execution_request();

CREATE FUNCTION ortak_work_run_identity_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (OLD.work_item_id IS NOT NULL OR NEW.work_item_id IS NOT NULL) AND
        (NEW.company_id,NEW.id,NEW.work_item_id,NEW.employee_id,NEW.employee_revision_id,NEW.runtime_adapter,
         NEW.routing_decision_id,NEW.message_id,NEW.root_message_id,NEW.queued_at)
        IS DISTINCT FROM
        (OLD.company_id,OLD.id,OLD.work_item_id,OLD.employee_id,OLD.employee_revision_id,OLD.runtime_adapter,
         OLD.routing_decision_id,OLD.message_id,OLD.root_message_id,OLD.queued_at) THEN
        RAISE EXCEPTION 'ortak: Work run origin and configuration pins are immutable' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_run_identity BEFORE UPDATE ON runs FOR EACH ROW EXECUTE FUNCTION ortak_work_run_identity_guard();

CREATE FUNCTION ortak_check_run_work_authority() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE current_run runs%ROWTYPE;
BEGIN
    -- INSERT can precede the one final admission UPDATE in the same transaction.
    SELECT * INTO current_run FROM runs WHERE company_id=NEW.company_id AND id=NEW.id;
    IF current_run.work_item_id IS NULL THEN
        IF current_run.work_admission_generation IS NOT NULL THEN
            RAISE EXCEPTION 'ortak: Work admission requires Work origin' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP='UPDATE' AND NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token
        AND NEW.work_admission_generation IS NOT DISTINCT FROM OLD.work_admission_generation THEN RETURN NEW; END IF;
    IF NOT EXISTS(SELECT 1 FROM work_executions x
        JOIN work_items w ON w.company_id=x.company_id AND w.id=x.work_item_id
        JOIN projects p ON p.company_id=x.company_id AND p.id=x.project_id
        JOIN work_authority_generations g ON g.company_id=x.company_id AND g.project_id=x.project_id
        JOIN project_access_grants a ON a.company_id=x.company_id AND a.project_id=x.project_id AND a.actor_pubkey=x.requested_by
        JOIN work_assignments assignment ON assignment.company_id=x.company_id AND assignment.work_item_id=x.work_item_id AND assignment.employee_id=x.employee_id
        WHERE x.company_id=current_run.company_id AND x.run_id=current_run.id AND x.work_item_id=current_run.work_item_id
        AND x.employee_id=current_run.employee_id AND x.employee_revision_id=current_run.employee_revision_id
        AND g.generation=current_run.work_admission_generation AND current_run.work_admission_token IS NOT NULL
        AND p.status='active' AND w.state='in_progress' AND w.version=x.execution_version
        AND a.role IN('owner','contributor') AND a.revoked_at IS NULL
        AND assignment.status='active' AND assignment.role IN('owner','contributor')) THEN
        RAISE EXCEPTION 'ortak: Work admission changed before commit' USING ERRCODE='serialization_failure';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER work_run_admission_at_commit AFTER INSERT OR UPDATE ON runs
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_run_work_authority();

CREATE FUNCTION ortak_check_work_output_provenance() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; run UUID; job runtime_work_outputs%ROWTYPE; final_turn JSONB; final_text TEXT; fragments BIGINT; payload_bytes BIGINT; truncated BOOLEAN;
BEGIN
    company:=NEW.company_id;
    IF TG_TABLE_NAME='artifacts' THEN run:=NEW.run_id;
    ELSE run:=NEW.run_id;
    END IF;
    SELECT * INTO job FROM runtime_work_outputs WHERE company_id=company AND run_id=run;
    IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM runs r JOIN run_events ev ON ev.company_id=r.company_id AND ev.run_id=r.id
        WHERE r.company_id=company AND r.id=run AND ev.sequence=job.terminal_sequence
        AND ((r.status='completed' AND ev.event_type='run.completed') OR (r.status='failed' AND ev.event_type='run.failed')
            OR (r.status='cancelled' AND ev.event_type='run.cancelled')))
    THEN RAISE EXCEPTION 'ortak: Work output requires canonical terminal provenance' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF job.state='materialized' THEN
        SELECT payload->'turn' INTO final_turn FROM run_events WHERE company_id=company AND run_id=run
            AND sequence<job.terminal_sequence AND event_type='assistant.delta' ORDER BY sequence DESC LIMIT 1;
        SELECT count(*),coalesce(sum(octet_length(payload::text)),0),
            bool_or(NOT coalesce(
                payload->>'event_type'='assistant.delta'
                AND jsonb_typeof(payload->'turn')='number'
                AND (payload->>'turn') ~ '^(0|[1-9][0-9]{0,9})$'
                AND (payload->>'turn')::numeric<=4294967295
                AND jsonb_typeof(payload->'delta')='object'
                AND jsonb_typeof(payload->'delta'->'text')='string'
                AND (NOT (payload->'delta' ? 'truncated') OR payload->'delta'->'truncated'='false'::jsonb)
                AND (payload->'delta'->'original_bytes' IS NULL OR payload->'delta'->'original_bytes'='null'::jsonb)
                AND (payload->'delta'->'original_sha256' IS NULL OR payload->'delta'->'original_sha256'='null'::jsonb),false))
            INTO fragments,payload_bytes,truncated FROM run_events
            WHERE company_id=company AND run_id=run AND sequence<job.terminal_sequence
            AND event_type='assistant.delta' AND payload->'turn'=final_turn;
        IF fragments=0 OR fragments>4096 OR payload_bytes>1048576 OR truncated THEN
            RAISE EXCEPTION 'ortak: Work artifact requires a complete bounded final turn' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        SELECT string_agg(payload->'delta'->>'text','' ORDER BY sequence) INTO final_text FROM run_events
            WHERE company_id=company AND run_id=run AND sequence<job.terminal_sequence
            AND event_type='assistant.delta' AND payload->'turn'=final_turn;
        IF final_text IS NULL OR btrim(final_text,U&'\0009\000A\000B\000C\000D\0020\0085\00A0\1680\2000\2001\2002\2003\2004\2005\2006\2007\2008\2009\200A\2028\2029\202F\205F\3000')='' OR octet_length(final_text)>32768 THEN
            RAISE EXCEPTION 'ortak: Work artifact final text is empty or oversized' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        IF NOT EXISTS(SELECT 1 FROM artifacts art
            JOIN work_executions x ON x.company_id=art.company_id AND x.run_id=art.run_id
            JOIN runs r ON r.company_id=x.company_id AND r.id=x.run_id
            JOIN work_items w ON w.company_id=art.company_id AND w.id=art.work_item_id
            JOIN work_item_history h ON h.company_id=w.company_id AND h.work_item_id=w.id AND h.version=x.execution_version+1
            JOIN work_attachments attachment ON attachment.company_id=w.company_id AND attachment.work_item_id=w.id AND attachment.artifact_id=art.id
            WHERE art.company_id=company AND art.id=job.artifact_id AND art.run_id=run AND art.terminal_sequence=job.terminal_sequence
            AND art.project_id=x.project_id AND art.work_item_id=x.work_item_id
            AND art.content_bytes=convert_to(final_text,'UTF8')
            AND art.employee_id=x.employee_id AND art.employee_revision_id=x.employee_revision_id
            AND r.status='completed' AND r.delivery_intent='silent' AND w.state='review' AND w.version=x.execution_version+1
            AND h.event_type='work.execution_result_ready' AND h.actor_type='system' AND h.actor_id IS NULL
            AND h.payload->>'artifact_id'=art.id::text AND h.payload->>'run_id'=run::text
            AND h.xmin::text::bigint=txid_current()%4294967296 AND art.xmin::text::bigint=txid_current()%4294967296
            AND w.xmin::text::bigint=txid_current()%4294967296 AND attachment.xmin::text::bigint=txid_current()%4294967296
            AND x.result_code='result_ready' AND x.reconciled_at IS NOT NULL
            AND NOT EXISTS(SELECT 1 FROM work_acceptance_criteria cr WHERE cr.company_id=w.company_id AND cr.work_item_id=w.id AND cr.status<>'pending')
            AND NOT EXISTS(SELECT 1 FROM work_approvals ap WHERE ap.company_id=w.company_id AND ap.work_item_id=w.id AND ap.status<>'pending'))
        THEN RAISE EXCEPTION 'ortak: Work deliverable and review must commit atomically without human decisions' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
    ELSIF TG_TABLE_NAME='artifacts' THEN
        RAISE EXCEPTION 'ortak: artifacts require their materialized Work output receipt' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER work_output_provenance_at_commit AFTER INSERT OR UPDATE ON runtime_work_outputs
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_work_output_provenance();
CREATE CONSTRAINT TRIGGER artifact_provenance_at_commit AFTER INSERT ON artifacts
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_work_output_provenance();

-- Work ACL changes and late output receipts wake existing authenticated streams.
CREATE TRIGGER trg_activity_work_authority AFTER INSERT OR UPDATE ON work_authority_generations
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('');
CREATE TRIGGER trg_activity_work_outputs AFTER INSERT OR UPDATE OF state,artifact_id,last_error_code ON runtime_work_outputs
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
