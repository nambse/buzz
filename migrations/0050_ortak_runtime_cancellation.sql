-- Durable adapter stop acknowledgements. Local terminal run state alone is
-- insufficient: a lost start acknowledgement must still be stopped by run key.
CREATE TABLE runtime_cancellations (
    company_id UUID NOT NULL REFERENCES companies(id),
    run_id UUID NOT NULL,
    reason TEXT NOT NULL CHECK (reason IN ('office_revoked', 'human_requested')),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'acknowledged', 'failed')),
    attempt_count INT NOT NULL DEFAULT 0 CHECK (attempt_count BETWEEN 0 AND 20),
    max_attempts INT NOT NULL DEFAULT 20 CHECK (max_attempts BETWEEN 1 AND 20),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK (last_error_code ~ '^[a-z][a-z0-9_.]{0,63}$'),
    acknowledged_at TIMESTAMPTZ,
    PRIMARY KEY (company_id, run_id),
    FOREIGN KEY (company_id, run_id) REFERENCES runs(company_id, id),
    CHECK (attempt_count <= max_attempts),
    CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL)),
    CHECK ((state = 'acknowledged') = (acknowledged_at IS NOT NULL)),
    CHECK (state = 'pending' OR lease_token IS NULL)
);
CREATE INDEX idx_runtime_cancellations_due
    ON runtime_cancellations (company_id, next_attempt_at, requested_at, run_id)
    WHERE state = 'pending';

CREATE FUNCTION ortak_runtime_cancellation_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id <> OLD.company_id OR NEW.run_id <> OLD.run_id
       OR NEW.reason <> OLD.reason OR NEW.requested_at <> OLD.requested_at
       OR NEW.max_attempts <> OLD.max_attempts OR NEW.attempt_count < OLD.attempt_count
       OR OLD.state <> 'pending'
    THEN
        RAISE EXCEPTION 'ortak: cancellation attribution is immutable and terminal state is final'
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_runtime_cancellations_guard BEFORE UPDATE ON runtime_cancellations
    FOR EACH ROW EXECUTE FUNCTION ortak_runtime_cancellation_guard();
CREATE TRIGGER trg_runtime_cancellations_no_delete BEFORE DELETE ON runtime_cancellations
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
