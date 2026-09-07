-- Completion commits its publication job, including when the process crashes
-- before a worker can construct the canonical draft.
CREATE TABLE runtime_office_outputs (
    company_id UUID NOT NULL REFERENCES companies(id),
    run_id UUID NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','enqueued','failed')),
    attempt_count INT NOT NULL DEFAULT 0 CHECK (attempt_count BETWEEN 0 AND 20),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK (last_error_code ~ '^[a-z][a-z0-9_.]{0,63}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    draft_kind INT CHECK (draft_kind IN (9,40002)),
    draft_tags JSONB CHECK (jsonb_typeof(draft_tags)='array' AND octet_length(draft_tags::text)<=32768),
    draft_content TEXT CHECK (octet_length(draft_content) BETWEEN 1 AND 32768 AND btrim(draft_content)<>''),
    draft_created_at TIMESTAMPTZ,
    source_facts JSONB CHECK (jsonb_typeof(source_facts)='object' AND octet_length(source_facts::text)<=4096),
    office_authority_generation BIGINT CHECK (office_authority_generation>=0),
    office_authority_valid_before TIMESTAMPTZ,
    office_authority_token UUID,
    outbox_id UUID,
    enqueued_at TIMESTAMPTZ,
    PRIMARY KEY (company_id,run_id),
    FOREIGN KEY (company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY (company_id,outbox_id) REFERENCES outbox(company_id,id),
    CHECK ((lease_token IS NULL)=(lease_expires_at IS NULL)),
    CHECK (state='pending' OR lease_token IS NULL),
    CHECK ((state='enqueued')=(outbox_id IS NOT NULL)),
    CHECK ((state='enqueued')=(enqueued_at IS NOT NULL)),
    CHECK ((draft_kind IS NULL AND draft_tags IS NULL AND draft_content IS NULL
            AND draft_created_at IS NULL AND source_facts IS NULL AND office_authority_generation IS NULL
            AND office_authority_valid_before IS NULL AND office_authority_token IS NULL)
        OR (draft_kind IS NOT NULL AND draft_tags IS NOT NULL AND draft_content IS NOT NULL
            AND draft_created_at IS NOT NULL AND source_facts IS NOT NULL AND office_authority_generation IS NOT NULL
            AND office_authority_token IS NOT NULL)),
    CHECK (state<>'enqueued' OR draft_kind IS NOT NULL)
);
CREATE INDEX idx_runtime_office_outputs_due ON runtime_office_outputs
    (company_id,next_attempt_at,created_at,run_id) WHERE state='pending';

CREATE FUNCTION ortak_runtime_office_output_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id<>OLD.company_id OR NEW.run_id<>OLD.run_id OR NEW.created_at<>OLD.created_at
       OR NEW.attempt_count<OLD.attempt_count OR OLD.state<>'pending'
       OR (NEW.state='enqueued' AND NEW.office_authority_token IS NOT DISTINCT FROM OLD.office_authority_token)
       OR (OLD.draft_kind IS NOT NULL AND
           ROW(NEW.draft_kind,NEW.draft_tags,NEW.draft_content,NEW.draft_created_at,NEW.source_facts)
           IS DISTINCT FROM ROW(OLD.draft_kind,OLD.draft_tags,OLD.draft_content,OLD.draft_created_at,OLD.source_facts))
    THEN
        RAISE EXCEPTION 'ortak: Office output draft is immutable and terminal job state is final'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_runtime_office_output_guard BEFORE UPDATE ON runtime_office_outputs
    FOR EACH ROW EXECUTE FUNCTION ortak_runtime_office_output_guard();
CREATE TRIGGER trg_runtime_office_output_no_delete BEFORE DELETE ON runtime_office_outputs
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE FUNCTION ortak_schedule_completed_office_output() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.status='completed' AND NEW.delivery_intent IN ('reply','channel') THEN
        INSERT INTO runtime_office_outputs(company_id,run_id) VALUES (NEW.company_id,NEW.id)
        ON CONFLICT (company_id,run_id) DO NOTHING;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_runs_schedule_office_output AFTER INSERT OR UPDATE OF status,delivery_intent ON runs
    FOR EACH ROW EXECUTE FUNCTION ortak_schedule_completed_office_output();

CREATE FUNCTION ortak_check_office_output_authority() RETURNS TRIGGER AS $$
DECLARE current_generation BIGINT;
BEGIN
    IF NEW.draft_kind IS NULL OR (TG_OP='UPDATE' AND
       ROW(NEW.office_authority_token,NEW.office_authority_generation,NEW.office_authority_valid_before)
       IS NOT DISTINCT FROM ROW(OLD.office_authority_token,OLD.office_authority_generation,OLD.office_authority_valid_before)) THEN
        RETURN NEW;
    END IF;
    current_generation:=ortak_lock_office_authority(NEW.company_id);
    IF NEW.office_authority_generation IS DISTINCT FROM current_generation
       OR (NEW.office_authority_valid_before IS NOT NULL
           AND clock_timestamp()>=NEW.office_authority_valid_before) THEN
        RAISE EXCEPTION 'ortak: Office output authority changed or expired' USING ERRCODE='serialization_failure';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE CONSTRAINT TRIGGER trg_runtime_office_output_authority AFTER INSERT OR UPDATE ON runtime_office_outputs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_office_output_authority();

INSERT INTO runtime_office_outputs(company_id,run_id)
SELECT company_id,id FROM runs WHERE status='completed' AND delivery_intent IN ('reply','channel')
ON CONFLICT (company_id,run_id) DO NOTHING;
