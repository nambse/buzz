-- Fresh external probes precede a bounded, sealed activation admission.
-- The repository explicitly defers this guard immediately before its final
-- success write and commits next. No network call runs in that transaction.
CREATE FUNCTION ortak_check_activation_admission_at_commit() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    activation_receipt JSONB;
    admission JSONB;
    activation_attempt INTEGER;
    issued_at TIMESTAMPTZ;
    expires_at TIMESTAMPTZ;
    checked_at TIMESTAMPTZ;
BEGIN
    IF TG_OP='UPDATE' AND OLD.status='succeeded'
        AND OLD.result_revision_id IS NOT DISTINCT FROM NEW.result_revision_id THEN
        RETURN NEW;
    END IF;
    SELECT s.result, s.attempt_count INTO activation_receipt, activation_attempt
    FROM provisioning_operation_steps s
    WHERE s.company_id=NEW.company_id AND s.operation_id=NEW.id
      AND s.step_index=9 AND s.step_name='activate_revision' AND s.state='succeeded'
      AND s.idempotency_key='provisioning:'||NEW.id::TEXT||':activate_revision'
    FOR SHARE;
    IF NOT FOUND OR activation_attempt<=0 THEN
        RAISE EXCEPTION 'ortak: successful activation requires its exact receipt'
            USING ERRCODE='40001';
    END IF;
    admission := activation_receipt->'admission';
    IF jsonb_typeof(admission) IS DISTINCT FROM 'object'
        OR admission->>'format' IS DISTINCT FROM 'ortak.activation/v1'
        OR admission->>'operation_id' IS DISTINCT FROM NEW.id::TEXT
        OR admission->>'employee_id' IS DISTINCT FROM NEW.employee_id
        OR activation_receipt->>'result_revision_id' IS DISTINCT FROM NEW.result_revision_id::TEXT
        OR jsonb_typeof(activation_receipt->'evidence') IS DISTINCT FROM 'object'
        OR jsonb_typeof(admission->'attempt_count') IS DISTINCT FROM 'number'
        OR (admission->>'attempt_count' ~ '^[1-9][0-9]{0,9}$') IS DISTINCT FROM true
        OR jsonb_typeof(admission->'manifest_fingerprint') IS DISTINCT FROM 'string'
        OR (admission->>'manifest_fingerprint' ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true
        OR jsonb_typeof(admission->'observed_at') IS DISTINCT FROM 'string'
        OR jsonb_typeof(admission->'valid_before') IS DISTINCT FROM 'string'
        OR length(admission->>'observed_at')>64 OR length(admission->>'valid_before')>64 THEN
        RAISE EXCEPTION 'ortak: activation admission correlation is invalid'
            USING ERRCODE='40001';
    END IF;
    IF (admission->>'attempt_count')::BIGINT<>activation_attempt THEN
        RAISE EXCEPTION 'ortak: activation admission attempt is stale'
            USING ERRCODE='40001';
    END IF;
    BEGIN
        issued_at := (admission->>'observed_at')::TIMESTAMPTZ;
        expires_at := (admission->>'valid_before')::TIMESTAMPTZ;
    EXCEPTION WHEN invalid_datetime_format OR datetime_field_overflow THEN
        RAISE EXCEPTION 'ortak: activation admission clock is invalid'
            USING ERRCODE='40001';
    END;
    checked_at := clock_timestamp();
    IF NOT isfinite(issued_at) OR NOT isfinite(expires_at)
        OR issued_at>checked_at OR expires_at<=checked_at
        OR expires_at<=issued_at OR expires_at-issued_at>interval '15 seconds' THEN
        RAISE EXCEPTION 'ortak: activation admission expired before commit'
            USING ERRCODE='40001';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM provisioning_operations o
        JOIN companies c ON c.id=o.company_id AND c.status='active'
        JOIN employees e ON e.company_id=o.company_id AND e.id=o.employee_id
            AND e.status='active' AND e.active_revision_id=o.result_revision_id
        JOIN employee_revisions r ON r.company_id=o.company_id AND r.employee_id=o.employee_id
            AND r.id=o.result_revision_id
        WHERE o.company_id=NEW.company_id AND o.id=NEW.id AND NOT o.dry_run
          AND o.status='succeeded' AND o.result_revision_id=NEW.result_revision_id
          AND o.employee_id=NEW.employee_id AND o.manifest_fingerprint=NEW.manifest_fingerprint
          AND r.manifest_fingerprint=decode(admission->>'manifest_fingerprint','hex')
          AND r.manifest->>'id'=o.employee_id AND r.manifest->>'status'='active'
          AND r.created_by='provisioning:'||o.id::TEXT
          AND EXISTS (SELECT 1 FROM employee_runtime_bindings b
              WHERE b.company_id=o.company_id AND b.revision_id=r.id
                AND b.employee_id=o.employee_id AND b.validated_at=issued_at)
          -- A refreshed same-key binding retains its original revision provenance.
          AND EXISTS (SELECT 1 FROM employee_office_bindings b
              WHERE b.company_id=o.company_id AND b.employee_id=o.employee_id
                AND b.public_key=decode(r.manifest->'office'->>'public_key','hex')
                AND b.signer_ref=r.manifest->'office'->>'signer_ref'
                AND b.verified_at=issued_at AND b.valid_from<=checked_at
                AND (b.valid_until IS NULL OR b.valid_until>checked_at))
          AND ((r.manifest->'memory') IS NULL OR r.manifest->'memory'='null'::JSONB
            OR EXISTS (SELECT 1 FROM employee_memory_bindings b
                WHERE b.company_id=o.company_id AND b.revision_id=r.id
                  AND b.employee_id=o.employee_id AND b.validated_at=issued_at))
    ) THEN
        RAISE EXCEPTION 'ortak: activation admission does not match committed authority'
            USING ERRCODE='40001';
    END IF;
    RETURN NEW;
END
$$;

-- Successful activation is durable audit history. These guards do not acquire
-- parent locks from a step tuple, so they add no step->operation lock edge.
CREATE FUNCTION ortak_guard_activation_operation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.result_revision_id IS NOT NULL THEN
        IF TG_OP='DELETE' THEN
            RAISE EXCEPTION 'ortak: activated operation is immutable' USING ERRCODE='55000';
        END IF;
        IF NEW IS DISTINCT FROM OLD THEN
            RAISE EXCEPTION 'ortak: activated operation is immutable' USING ERRCODE='55000';
        END IF;
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END
$$;
CREATE FUNCTION ortak_guard_activation_receipt() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.step_name='activate_revision' AND OLD.state='succeeded' THEN
        IF TG_OP='DELETE' THEN
            RAISE EXCEPTION 'ortak: activation receipt is immutable' USING ERRCODE='55000';
        END IF;
        IF NEW IS DISTINCT FROM OLD THEN
            RAISE EXCEPTION 'ortak: activation receipt is immutable' USING ERRCODE='55000';
        END IF;
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER ortak_activation_operation_immutable BEFORE UPDATE OR DELETE ON provisioning_operations
FOR EACH ROW EXECUTE FUNCTION ortak_guard_activation_operation();
CREATE TRIGGER ortak_activation_receipt_immutable BEFORE UPDATE OR DELETE ON provisioning_operation_steps
FOR EACH ROW EXECUTE FUNCTION ortak_guard_activation_receipt();
CREATE TRIGGER ortak_activation_operation_no_truncate BEFORE TRUNCATE ON provisioning_operations
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER ortak_activation_receipt_no_truncate BEFORE TRUNCATE ON provisioning_operation_steps
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE CONSTRAINT TRIGGER ortak_activation_admission_at_commit AFTER INSERT OR UPDATE ON provisioning_operations
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
WHEN (NEW.status='succeeded' AND NOT NEW.dry_run AND NEW.result_revision_id IS NOT NULL)
EXECUTE FUNCTION ortak_check_activation_admission_at_commit();
