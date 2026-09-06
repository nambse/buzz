-- The bridge owns OAuth values, real inference and its child containment proof.
-- This journal owns admission identity and recovery across CLI/API restarts.
CREATE TABLE provisioning_runtime_probes (
    company_id UUID NOT NULL,
    operation_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 20),
    probe_id UUID NOT NULL CHECK(probe_id<>'00000000-0000-0000-0000-000000000000'),
    bridge_origin TEXT NOT NULL CHECK(octet_length(bridge_origin)<=2048),
    bridge_token_env TEXT NOT NULL CHECK(bridge_token_env ~ '^[A-Za-z_][A-Za-z0-9_]{0,127}$'),
    state TEXT NOT NULL CHECK(state IN('running','succeeded','failed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    deadline TIMESTAMPTZ NOT NULL,
    contained_at TIMESTAMPTZ,
    error_code TEXT CHECK(error_code IN('probe_failed','probe_cancelled','probe_timeout',
        'probe_transport','probe_authority_changed','probe_unhealthy','probe_interrupted')),
    PRIMARY KEY(company_id,operation_id,generation),
    UNIQUE(company_id,probe_id),
    FOREIGN KEY(company_id,operation_id) REFERENCES provisioning_runner_selections(company_id,operation_id),
    CHECK(deadline>created_at AND deadline<=created_at+interval '90 seconds'),
    CHECK((state='running')=(contained_at IS NULL)),
    CHECK(contained_at IS NULL OR contained_at>=created_at),
    CHECK((state='failed')=(error_code IS NOT NULL))
);
-- New operations for the same employee must settle an older uncertain child.
CREATE UNIQUE INDEX provisioning_runtime_probe_one_running
    ON provisioning_runtime_probes(company_id,employee_id) WHERE state='running';

CREATE FUNCTION ortak_provisioning_runtime_probe_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE op provisioning_operations%ROWTYPE; prior INTEGER; epoch BIGINT; employee_status TEXT;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (to_jsonb(NEW)-ARRAY['state','contained_at','error_code']) IS DISTINCT FROM
           (to_jsonb(OLD)-ARRAY['state','contained_at','error_code'])
           OR OLD.state<>'running' OR NEW.state='running' OR NEW.contained_at>clock_timestamp() THEN
            RAISE EXCEPTION 'Runtime probe only permits one contained terminal receipt'
                USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        -- Revoked/expired operations can still retain successful cleanup. They
        -- cannot turn that cleanup into a current readiness witness.
        IF NEW.state='failed' THEN RETURN NEW; END IF;
    END IF;
    PERFORM ortak_lock_office_authority(NEW.company_id);
    IF NOT EXISTS(SELECT 1 FROM companies c JOIN office_company_bindings b ON b.company_id=c.id
        JOIN communities cm ON cm.id=b.community_id WHERE c.id=NEW.company_id AND c.status='active'
        AND cm.deletion_state='active' AND cm.deleted_at IS NULL) THEN
        RAISE EXCEPTION 'Runtime probe Office authority unavailable' USING ERRCODE='insufficient_privilege';
    END IF;
    SELECT * INTO op FROM provisioning_operations
        WHERE company_id=NEW.company_id AND id=NEW.operation_id FOR UPDATE NOWAIT;
    IF NOT FOUND OR op.employee_id<>NEW.employee_id OR op.dry_run
       OR op.status NOT IN('pending','running','failed')
       OR op.manifest->>'provisioning' IS DISTINCT FROM 'adopt'
       OR op.manifest#>>'{employee,runtime,adapter}' IS DISTINCT FROM 'hermes' THEN
        RAISE EXCEPTION 'Runtime probe operation unavailable' USING ERRCODE='check_violation';
    END IF;
    SELECT lifecycle_epoch,status INTO epoch,employee_status FROM employees
        WHERE company_id=NEW.company_id AND id=NEW.employee_id FOR SHARE;
    IF op.employee_lifecycle_epoch<>coalesce(epoch,0) OR (employee_status='disabled' AND NOT EXISTS(
        SELECT 1 FROM employee_management_commands c JOIN employees e
          ON e.company_id=c.company_id AND e.id=c.employee_id
        WHERE c.company_id=NEW.company_id AND c.id=nullif(current_setting('ortak.management_command',true),'')::uuid
          AND c.operation_id=op.id AND c.action='reenable' AND c.employee_lifecycle_epoch=e.lifecycle_epoch
          AND c.expected_revision_id IS NOT DISTINCT FROM e.active_revision_id)) THEN
        RAISE EXCEPTION 'Runtime probe lifecycle changed' USING ERRCODE='serialization_failure';
    END IF;
    IF TG_OP='INSERT' AND TG_WHEN='BEFORE' THEN
        SELECT coalesce(max(generation),0) INTO prior FROM provisioning_runtime_probes
            WHERE company_id=NEW.company_id AND operation_id=NEW.operation_id;
        IF NEW.generation<>prior+1 OR NEW.state<>'running' OR NEW.contained_at IS NOT NULL
           OR NEW.error_code IS NOT NULL OR NEW.created_at>clock_timestamp() OR NEW.deadline<=clock_timestamp() THEN
            RAISE EXCEPTION 'Runtime probe admission is not the next bounded attempt' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.deadline<=clock_timestamp() THEN
        RAISE EXCEPTION 'Runtime probe readiness expired before commit' USING ERRCODE='serialization_failure';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER provisioning_runtime_probe_guard BEFORE INSERT OR UPDATE ON provisioning_runtime_probes
    FOR EACH ROW EXECUTE FUNCTION ortak_provisioning_runtime_probe_guard();
CREATE TRIGGER provisioning_runtime_probe_no_delete BEFORE DELETE ON provisioning_runtime_probes
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER provisioning_runtime_probe_no_truncate BEFORE TRUNCATE ON provisioning_runtime_probes
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
-- Management admission/success cannot outlive the originating actor or lease.
-- Failed containment accounting remains possible after those rights disappear.
CREATE CONSTRAINT TRIGGER provisioning_runtime_probe_management_at_commit
    AFTER INSERT OR UPDATE ON provisioning_runtime_probes DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW WHEN(NEW.state<>'failed') EXECUTE FUNCTION ortak_management_operation_fence();
CREATE CONSTRAINT TRIGGER provisioning_runtime_probe_live_at_commit
    AFTER INSERT OR UPDATE ON provisioning_runtime_probes DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW WHEN(NEW.state<>'failed') EXECUTE FUNCTION ortak_provisioning_runtime_probe_guard();
