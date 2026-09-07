-- Integration proposal for the next root-owned migration and desired schema.
-- Adapter journal only: activation/revision/domain state remains control-owned.
CREATE TABLE office_identity_profiles (
    company_id UUID NOT NULL REFERENCES companies(id),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 256
                                      AND idempotency_key ~ '^[A-Za-z0-9:_.-]+$'),
    community_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    event_id BYTEA NOT NULL CHECK (octet_length(event_id)=32),
    signed_event_bytes BYTEA NOT NULL CHECK (octet_length(signed_event_bytes) BETWEEN 1 AND 16384),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    acknowledged_at TIMESTAMPTZ,
    PRIMARY KEY (company_id,idempotency_key),
    FOREIGN KEY (company_id,employee_id) REFERENCES employees(company_id,id),
    FOREIGN KEY (company_id,idempotency_key)
        REFERENCES provisioning_operation_steps(company_id,idempotency_key),
    CHECK (acknowledged_at IS NULL OR acknowledged_at>=created_at)
);

CREATE FUNCTION ortak_office_profile_receipt_immutable() RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP IN ('DELETE','TRUNCATE') THEN
        RAISE EXCEPTION 'Office profile receipts are immutable' USING ERRCODE='check_violation';
    END IF;
    IF (to_jsonb(NEW)-'acknowledged_at') IS DISTINCT FROM (to_jsonb(OLD)-'acknowledged_at')
       OR (OLD.acknowledged_at IS NOT NULL AND NEW.acknowledged_at IS DISTINCT FROM OLD.acknowledged_at) THEN
        RAISE EXCEPTION 'Office profile receipt bytes are immutable' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_office_identity_profiles_immutable
    BEFORE UPDATE OR DELETE ON office_identity_profiles
    FOR EACH ROW EXECUTE FUNCTION ortak_office_profile_receipt_immutable();

CREATE TRIGGER trg_office_identity_profiles_no_truncate
    BEFORE TRUNCATE ON office_identity_profiles
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_office_profile_receipt_immutable();
