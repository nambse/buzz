-- Real prepared-resource provisioning and durable Office profile publication.
CREATE TABLE office_identity_profiles (
    company_id UUID NOT NULL REFERENCES companies(id),
    idempotency_key TEXT NOT NULL CHECK (idempotency_key ~ '^[A-Za-z0-9:_.-]{1,256}$'),
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

-- Freeze all public adapter/receipt selections separately from the manifest.
-- A retry cannot replace original native ownership or diagnostic provenance.
CREATE TABLE provisioning_runner_selections (
    company_id UUID NOT NULL,
    operation_id UUID NOT NULL,
    configuration_fingerprint BYTEA NOT NULL CHECK (octet_length(configuration_fingerprint)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, operation_id),
    FOREIGN KEY (company_id, operation_id) REFERENCES provisioning_operations(company_id, id)
);

CREATE FUNCTION ortak_provisioning_selection_immutable() RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'Provisioning runner selections are immutable' USING ERRCODE='check_violation';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_provisioning_runner_selections_immutable
    BEFORE UPDATE OR DELETE ON provisioning_runner_selections
    FOR EACH ROW EXECUTE FUNCTION ortak_provisioning_selection_immutable();

CREATE TRIGGER trg_office_identity_profiles_no_truncate
    BEFORE TRUNCATE ON office_identity_profiles
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_office_profile_receipt_immutable();

CREATE TRIGGER trg_provisioning_runner_selections_no_truncate
    BEFORE TRUNCATE ON provisioning_runner_selections
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_provisioning_selection_immutable();
