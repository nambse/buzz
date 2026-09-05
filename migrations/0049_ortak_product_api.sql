-- Private MVP product request audit and supervised cancellation queue.
-- Purely additive company-scoped schema; never stores signed auth JSON or keys.

CREATE TABLE ortak_api_audit (
    company_id UUID NOT NULL REFERENCES companies(id),
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    actor_pubkey TEXT NOT NULL CHECK (actor_pubkey ~ '^[0-9a-f]{64}$'),
    auth_event_id BYTEA NOT NULL CHECK (octet_length(auth_event_id) = 32),
    action TEXT NOT NULL CHECK (action IN ('access', 'read_runs', 'read_run', 'read_events', 'read_employees', 'read_employee', 'cancel_run')),
    outcome TEXT NOT NULL CHECK (outcome IN ('denied', 'not_found', 'requested', 'already_requested', 'already_terminal')),
    -- Requested identifier only. No FK: denied and nonexistent targets must
    -- be auditable without looking up another company's row.
    requested_run_id UUID,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, id)
);
CREATE INDEX idx_ortak_api_audit_time ON ortak_api_audit (company_id, recorded_at DESC, id DESC);
CREATE TRIGGER trg_ortak_api_audit_immutable BEFORE UPDATE OR DELETE ON ortak_api_audit
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TABLE run_cancel_requests (
    company_id UUID NOT NULL REFERENCES companies(id),
    run_id UUID NOT NULL,
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    requested_by TEXT NOT NULL CHECK (requested_by ~ '^[0-9a-f]{64}$'),
    auth_event_id BYTEA NOT NULL CHECK (octet_length(auth_event_id) = 32),
    reason_code TEXT NOT NULL DEFAULT 'human_requested' CHECK (reason_code = 'human_requested'),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- pending is a request, never a claim that Hermes has stopped.
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'acknowledged', 'failed')),
    attempts INT NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 20),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK (last_error_code ~ '^[a-z][a-z0-9_.]{0,63}$'),
    acknowledged_at TIMESTAMPTZ,
    PRIMARY KEY (company_id, run_id),
    UNIQUE (company_id, id),
    FOREIGN KEY (company_id, run_id) REFERENCES runs(company_id, id),
    CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL)),
    CHECK ((status = 'acknowledged') = (acknowledged_at IS NOT NULL)),
    CHECK (status = 'pending' OR lease_token IS NULL)
);
CREATE INDEX idx_run_cancel_requests_due ON run_cancel_requests (company_id, next_attempt_at, requested_at)
    WHERE status = 'pending';

CREATE FUNCTION ortak_cancel_request_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id <> OLD.company_id OR NEW.run_id <> OLD.run_id OR NEW.id <> OLD.id
       OR NEW.requested_by <> OLD.requested_by OR NEW.auth_event_id <> OLD.auth_event_id
       OR NEW.reason_code <> OLD.reason_code OR NEW.requested_at <> OLD.requested_at
       OR NEW.attempts < OLD.attempts OR OLD.status <> 'pending'
    THEN
        RAISE EXCEPTION 'ortak: cancellation request attribution is immutable and terminal state is final'
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_run_cancel_requests_guard BEFORE UPDATE ON run_cancel_requests
    FOR EACH ROW EXECUTE FUNCTION ortak_cancel_request_guard();
CREATE TRIGGER trg_run_cancel_requests_no_delete BEFORE DELETE ON run_cancel_requests
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
