-- Epoch pins permanently invalidate work predating a disable, including after re-enable.
ALTER TABLE employees ADD COLUMN lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(lifecycle_epoch>=0);
ALTER TABLE routing_recipients ADD COLUMN employee_lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(employee_lifecycle_epoch>=0);
ALTER TABLE runs ADD COLUMN employee_lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(employee_lifecycle_epoch>=0);
ALTER TABLE provisioning_operations ADD COLUMN employee_lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(employee_lifecycle_epoch>=0);
ALTER TABLE employee_configuration_drafts ADD COLUMN employee_lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(employee_lifecycle_epoch>=0),
 ADD COLUMN reenable BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE employee_management_commands ADD COLUMN employee_lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(employee_lifecycle_epoch>=0);
ALTER TABLE employee_management_commands DROP CONSTRAINT employee_management_commands_action_check;
ALTER TABLE employee_management_commands ADD CONSTRAINT employee_management_commands_action_check CHECK(action IN('adopt','update','retry','compensate','disable','reenable'));
ALTER TABLE employee_management_commands DROP CONSTRAINT employee_management_commands_check1;
ALTER TABLE employee_management_commands ADD CONSTRAINT employee_management_commands_configuration_required CHECK(action IN('compensate','disable') OR configuration IS NOT NULL);
ALTER TABLE employee_management_audit DROP CONSTRAINT employee_management_audit_action_check;
ALTER TABLE employee_management_audit ADD CONSTRAINT employee_management_audit_action_check CHECK(action IN('catalog','draft','adopt','update','retry','compensate','command','disable','reenable'));

CREATE TABLE employee_lifecycle_events (
 company_id UUID NOT NULL REFERENCES companies(id), id UUID NOT NULL DEFAULT gen_random_uuid(), employee_id TEXT NOT NULL,
 action TEXT NOT NULL CHECK(action IN('disable','reenable')), lifecycle_epoch BIGINT NOT NULL CHECK(lifecycle_epoch>0),
 command_id UUID, command_lease_token UUID, command_lease_expires_at TIMESTAMPTZ,
 previous_revision_id UUID, result_revision_id UUID, created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(company_id,id), UNIQUE(company_id,employee_id,lifecycle_epoch,action),
 FOREIGN KEY(company_id,employee_id) REFERENCES employees(company_id,id),
 FOREIGN KEY(company_id,command_id) REFERENCES employee_management_commands(company_id,id),
 CHECK(action='disable' OR (command_id IS NOT NULL AND result_revision_id IS NOT NULL)),
 CHECK((command_id IS NULL)=(command_lease_token IS NULL)),
 CHECK((command_id IS NULL)=(command_lease_expires_at IS NULL))
);
-- Upgrade barrier for employees already disabled before epochs existed. Their
-- old recipients/runs/operations keep zero; a later re-enable must never revive
-- them. This is migration attribution (NULL command), not an invented human
-- disable timestamp. Apply before the transition-only event/epoch guards.
UPDATE employees SET lifecycle_epoch=1 WHERE status='disabled' AND lifecycle_epoch=0;
INSERT INTO employee_lifecycle_events(company_id,employee_id,action,lifecycle_epoch,previous_revision_id,result_revision_id)
SELECT company_id,id,'disable',lifecycle_epoch,active_revision_id,active_revision_id
FROM employees WHERE status='disabled' AND lifecycle_epoch=1;

-- Events can originate only from the actual employee transition trigger. A raw
-- INSERT cannot forge the retained attribution or lease witness.
CREATE FUNCTION ortak_guard_lifecycle_event_insert() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF pg_trigger_depth()<>2 THEN
   RAISE EXCEPTION 'Lifecycle event requires employee transition' USING ERRCODE='insufficient_privilege';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER employee_lifecycle_event_transition BEFORE INSERT ON employee_lifecycle_events FOR EACH ROW EXECUTE FUNCTION ortak_guard_lifecycle_event_insert();
CREATE TRIGGER employee_lifecycle_events_immutable BEFORE UPDATE OR DELETE ON employee_lifecycle_events FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER employee_lifecycle_events_no_truncate BEFORE TRUNCATE ON employee_lifecycle_events FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_pin_employee_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE epoch BIGINT;
BEGIN
 IF TG_OP='UPDATE' THEN
   IF NEW.employee_lifecycle_epoch IS DISTINCT FROM OLD.employee_lifecycle_epoch THEN
     RAISE EXCEPTION 'Employee lifecycle pin is immutable' USING ERRCODE='check_violation';
   END IF;
   RETURN NEW;
 END IF;
 PERFORM ortak_lock_office_authority(NEW.company_id);
 IF TG_TABLE_NAME='runs' THEN
 IF NEW.routing_decision_id IS NOT NULL THEN
   SELECT employee_lifecycle_epoch INTO epoch FROM routing_recipients WHERE company_id=NEW.company_id
     AND routing_decision_id=NEW.routing_decision_id AND employee_id=NEW.employee_id;
   IF epoch IS NULL THEN RAISE EXCEPTION 'Office lifecycle recipient missing' USING ERRCODE='check_violation'; END IF;
 END IF;
 ELSE
   SELECT lifecycle_epoch INTO epoch FROM employees WHERE company_id=NEW.company_id AND id=NEW.employee_id;
 END IF;
 IF TG_TABLE_NAME='runs' THEN
 IF epoch IS NULL THEN SELECT lifecycle_epoch INTO epoch FROM employees WHERE company_id=NEW.company_id AND id=NEW.employee_id; END IF;
 END IF;
 NEW.employee_lifecycle_epoch:=coalesce(epoch,0);
 RETURN NEW;
END $$;
CREATE TRIGGER lifecycle_pin_recipient BEFORE INSERT OR UPDATE ON routing_recipients FOR EACH ROW EXECUTE FUNCTION ortak_pin_employee_lifecycle();
CREATE TRIGGER lifecycle_pin_run BEFORE INSERT OR UPDATE ON runs FOR EACH ROW EXECUTE FUNCTION ortak_pin_employee_lifecycle();
CREATE TRIGGER lifecycle_pin_operation BEFORE INSERT OR UPDATE ON provisioning_operations FOR EACH ROW EXECUTE FUNCTION ortak_pin_employee_lifecycle();

-- No late snapshot/admission can refresh an old epoch into current authority.
CREATE FUNCTION ortak_check_run_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='UPDATE' AND NEW.office_admission_token IS NOT DISTINCT FROM OLD.office_admission_token
    AND NEW.office_admission_generation IS NOT DISTINCT FROM OLD.office_admission_generation
    AND NEW.office_admission_valid_before IS NOT DISTINCT FROM OLD.office_admission_valid_before
    AND NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token
    AND NEW.work_admission_generation IS NOT DISTINCT FROM OLD.work_admission_generation THEN RETURN NEW; END IF;
 IF NOT EXISTS(SELECT 1 FROM employees WHERE company_id=NEW.company_id AND id=NEW.employee_id
     AND status='active' AND lifecycle_epoch=NEW.employee_lifecycle_epoch) THEN
   RAISE EXCEPTION 'Employee lifecycle admission changed' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER lifecycle_run_admission AFTER INSERT OR UPDATE ON runs DEFERRABLE INITIALLY DEFERRED
 FOR EACH ROW EXECUTE FUNCTION ortak_check_run_lifecycle();

-- Old interrupted ordinary CLI operations may be retained/compensated, but may
-- not start another adapter step or activate after a disable/re-enable cycle.
CREATE FUNCTION ortak_check_provisioning_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE operation provisioning_operations%ROWTYPE; needs_admission BOOLEAN;
BEGIN
 IF TG_TABLE_NAME='provisioning_operations' THEN operation:=NEW;
   needs_admission:=NEW.status IN('running','succeeded') AND (TG_OP='INSERT' OR OLD.status<>'succeeded');
 ELSE
   SELECT * INTO operation FROM provisioning_operations WHERE company_id=NEW.company_id AND id=NEW.operation_id;
   needs_admission:=NEW.state IN('running','succeeded') AND operation.status<>'compensating';
 END IF;
 IF needs_admission AND EXISTS(SELECT 1 FROM employees WHERE company_id=operation.company_id AND id=operation.employee_id
      AND lifecycle_epoch<>operation.employee_lifecycle_epoch) THEN
   RAISE EXCEPTION 'Provisioning lifecycle epoch changed' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER lifecycle_provisioning_operation BEFORE INSERT OR UPDATE ON provisioning_operations FOR EACH ROW EXECUTE FUNCTION ortak_check_provisioning_lifecycle();
CREATE TRIGGER lifecycle_provisioning_step BEFORE INSERT OR UPDATE ON provisioning_operation_steps FOR EACH ROW EXECUTE FUNCTION ortak_check_provisioning_lifecycle();

CREATE OR REPLACE FUNCTION ortak_management_guard(target UUID, command UUID, token UUID, operation UUID) RETURNS VOID
LANGUAGE plpgsql VOLATILE AS $$
DECLARE c employee_management_commands%ROWTYPE; op provisioning_operations%ROWTYPE; current_revision UUID; current_status TEXT; current_epoch BIGINT;
BEGIN
 PERFORM set_config('lock_timeout','500ms',true);
 PERFORM set_config('statement_timeout','2s',true);
 PERFORM ortak_lock_office_authority(target);
 -- Read attribution before taking policy -> command locks. Immutable columns
 -- cannot change while the policy is checked.
 SELECT * INTO c FROM employee_management_commands WHERE company_id=target AND id=command;
 IF NOT FOUND OR NOT ortak_management_actor_allowed(target,c.actor,c.policy_fingerprint,c.employee_id,c.channel_ids) THEN
   RAISE EXCEPTION 'Management authority refused' USING ERRCODE='insufficient_privilege';
 END IF;
 SELECT * INTO c FROM employee_management_commands WHERE company_id=target AND id=command FOR UPDATE;
 IF c.status<>'running' OR c.lease_token IS DISTINCT FROM token OR c.lease_expires_at<=clock_timestamp() THEN
   RAISE EXCEPTION 'Management lease refused' USING ERRCODE='insufficient_privilege';
 END IF;
 IF c.operation_id IS NULL AND c.configuration IS NOT NULL THEN
   SELECT * INTO op FROM provisioning_operations WHERE company_id=target AND employee_id=c.employee_id AND idempotency_key=c.configuration->>'operation_key';
   IF FOUND THEN
     IF op.manifest IS DISTINCT FROM c.configuration->'manifest' OR op.mode IS DISTINCT FROM c.configuration->>'mode' OR op.dry_run THEN
       RAISE EXCEPTION 'Management operation mismatch' USING ERRCODE='check_violation';
     END IF;
     UPDATE employee_management_commands SET operation_id=op.id WHERE company_id=target AND id=command;
     c.operation_id:=op.id;
   END IF;
 END IF;
 IF operation IS NOT NULL AND c.operation_id IS DISTINCT FROM operation THEN
   RAISE EXCEPTION 'Management operation mismatch' USING ERRCODE='check_violation';
 END IF;
 IF c.operation_id IS NOT NULL THEN
   SELECT * INTO op FROM provisioning_operations WHERE company_id=target AND id=c.operation_id;
   IF NOT FOUND OR op.employee_id<>c.employee_id OR (c.action<>'compensate' AND
     (op.employee_lifecycle_epoch<>c.employee_lifecycle_epoch OR op.manifest IS DISTINCT FROM c.configuration->'manifest' OR op.mode IS DISTINCT FROM c.configuration->>'mode'
      OR op.idempotency_key IS DISTINCT FROM c.configuration->>'operation_key' OR op.dry_run)) THEN
     RAISE EXCEPTION 'Management operation scope mismatch' USING ERRCODE='check_violation';
   END IF;
 END IF;
 IF c.action<>'compensate' THEN
   SELECT active_revision_id,status,lifecycle_epoch INTO current_revision,current_status,current_epoch FROM employees WHERE company_id=target AND id=c.employee_id FOR SHARE;
   IF coalesce(current_epoch,0)<>c.employee_lifecycle_epoch OR (current_status='disabled' AND c.action NOT IN('reenable','disable')) OR (c.action='reenable' AND current_status<>'disabled' AND NOT EXISTS(SELECT 1 FROM provisioning_operations done WHERE done.company_id=target AND done.id=c.operation_id AND done.result_revision_id=current_revision AND done.status='succeeded')) OR (current_revision IS DISTINCT FROM c.expected_revision_id AND NOT EXISTS(
     SELECT 1 FROM provisioning_operations o WHERE o.company_id=target AND o.id=c.operation_id AND o.result_revision_id=current_revision AND o.status='succeeded')) THEN
     RAISE EXCEPTION 'Management revision superseded' USING ERRCODE='check_violation';
   END IF;
 END IF;
 PERFORM set_config('ortak.management_command',command::text,true);
 PERFORM set_config('ortak.management_token',token::text,true);
END $$;


-- Existing DB administration can always disable an employee; its automatic
-- retained event is marked by a NULL command rather than inventing a human.
-- Re-enable is exclusively a fresh, sealed management activation.
CREATE FUNCTION ortak_guard_employee_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE command UUID; token UUID; selected employee_management_commands%ROWTYPE;
BEGIN
 IF NEW.lifecycle_epoch<>OLD.lifecycle_epoch THEN
   RAISE EXCEPTION 'Lifecycle epoch advances only with disable' USING ERRCODE='check_violation';
 END IF;
 IF NEW.status='disabled' AND OLD.status<>'disabled' THEN
   NEW.lifecycle_epoch:=OLD.lifecycle_epoch+1;
   command:=nullif(current_setting('ortak.management_command',true),'')::uuid;
   IF command IS NOT NULL THEN
     token:=nullif(current_setting('ortak.management_token',true),'')::uuid;
     SELECT * INTO selected FROM employee_management_commands WHERE company_id=NEW.company_id AND id=command;
     IF selected.action IS DISTINCT FROM 'disable' OR selected.employee_id<>NEW.id OR selected.status<>'running'
        OR selected.lease_token IS DISTINCT FROM token OR selected.lease_expires_at<=clock_timestamp()
        OR selected.expected_revision_id IS DISTINCT FROM OLD.active_revision_id OR selected.employee_lifecycle_epoch<>OLD.lifecycle_epoch THEN
       RAISE EXCEPTION 'Disable intent changed' USING ERRCODE='insufficient_privilege';
     END IF;
   END IF;
   INSERT INTO employee_lifecycle_events(company_id,employee_id,action,lifecycle_epoch,command_id,command_lease_token,command_lease_expires_at,previous_revision_id,result_revision_id)
   VALUES(NEW.company_id,NEW.id,'disable',NEW.lifecycle_epoch,command,selected.lease_token,selected.lease_expires_at,OLD.active_revision_id,NEW.active_revision_id);
 ELSIF OLD.status='disabled' AND (NEW.status<>'disabled' OR NEW.active_revision_id IS DISTINCT FROM OLD.active_revision_id) THEN
   command:=nullif(current_setting('ortak.management_command',true),'')::uuid;
   token:=nullif(current_setting('ortak.management_token',true),'')::uuid;
   IF command IS NULL OR token IS NULL THEN RAISE EXCEPTION 'Re-enable requires sealed activation' USING ERRCODE='insufficient_privilege'; END IF;
   SELECT * INTO selected FROM employee_management_commands WHERE company_id=NEW.company_id AND id=command;
   IF selected.action IS DISTINCT FROM 'reenable' OR selected.employee_id<>NEW.id OR selected.status<>'running'
      OR selected.lease_token IS DISTINCT FROM token OR selected.lease_expires_at<=clock_timestamp()
      OR selected.expected_revision_id IS DISTINCT FROM OLD.active_revision_id OR selected.employee_lifecycle_epoch<>OLD.lifecycle_epoch
      OR NEW.status<>'active' OR NEW.active_revision_id IS NULL OR NEW.active_revision_id IS NOT DISTINCT FROM OLD.active_revision_id
      OR NOT EXISTS(SELECT 1 FROM employee_revisions r WHERE r.company_id=NEW.company_id AND r.employee_id=NEW.id AND r.id=NEW.active_revision_id
         AND r.created_by='provisioning:'||selected.operation_id::text AND r.xmin::text::bigint=txid_current()%4294967296) THEN
     RAISE EXCEPTION 'Re-enable intent changed' USING ERRCODE='insufficient_privilege';
   END IF;
   INSERT INTO employee_lifecycle_events(company_id,employee_id,action,lifecycle_epoch,command_id,command_lease_token,command_lease_expires_at,previous_revision_id,result_revision_id)
   VALUES(NEW.company_id,NEW.id,'reenable',NEW.lifecycle_epoch,command,selected.lease_token,selected.lease_expires_at,OLD.active_revision_id,NEW.active_revision_id);
 END IF;
 RETURN NEW;
END $$;
-- After the existing Office mutation trigger, which acquires exclusive authority.
CREATE TRIGGER ortak_z_employee_lifecycle BEFORE UPDATE ON employees FOR EACH ROW EXECUTE FUNCTION ortak_guard_employee_lifecycle();

CREATE FUNCTION ortak_check_lifecycle_activation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.command_id IS NOT NULL AND (NEW.command_lease_expires_at<=clock_timestamp() OR NOT EXISTS(
   SELECT 1 FROM employee_management_commands c WHERE c.company_id=NEW.company_id AND c.id=NEW.command_id
     AND c.employee_id=NEW.employee_id AND c.action=NEW.action
     AND c.expected_revision_id IS NOT DISTINCT FROM NEW.previous_revision_id
     AND c.employee_lifecycle_epoch=NEW.lifecycle_epoch-CASE WHEN NEW.action='disable' THEN 1 ELSE 0 END
     AND ortak_management_actor_allowed(c.company_id,c.actor,c.policy_fingerprint,c.employee_id,c.channel_ids)
     AND ((NEW.action='disable' AND c.status='succeeded' AND c.lease_token IS NULL AND c.lease_expires_at IS NULL)
       OR (NEW.action='reenable' AND c.status='running' AND c.lease_token=NEW.command_lease_token
         AND c.lease_expires_at=NEW.command_lease_expires_at)))) THEN
   RAISE EXCEPTION 'Lifecycle lease must remain valid at commit' USING ERRCODE='insufficient_privilege';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM employees e WHERE e.company_id=NEW.company_id AND e.id=NEW.employee_id
     AND e.lifecycle_epoch=NEW.lifecycle_epoch AND e.active_revision_id IS NOT DISTINCT FROM NEW.result_revision_id
     AND e.status=CASE WHEN NEW.action='disable' THEN 'disabled' ELSE 'active' END
     AND e.xmin::text::bigint=txid_current()%4294967296) THEN
   RAISE EXCEPTION 'Lifecycle transition must commit atomically' USING ERRCODE='serialization_failure';
 END IF;
 IF NEW.action='reenable' AND NOT EXISTS(SELECT 1 FROM employee_management_commands c
    JOIN provisioning_operations o ON o.company_id=c.company_id AND o.id=c.operation_id
    JOIN employees e ON e.company_id=c.company_id AND e.id=c.employee_id
    WHERE c.company_id=NEW.company_id AND c.id=NEW.command_id AND c.action='reenable'
    AND c.employee_id=NEW.employee_id AND c.employee_lifecycle_epoch=NEW.lifecycle_epoch
    AND o.status='succeeded' AND NOT o.dry_run AND o.mode='update'
    AND o.employee_lifecycle_epoch=NEW.lifecycle_epoch AND o.result_revision_id=NEW.result_revision_id
    AND e.status='active' AND e.active_revision_id=NEW.result_revision_id AND e.lifecycle_epoch=NEW.lifecycle_epoch) THEN
   RAISE EXCEPTION 'Re-enable activation must commit atomically' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER lifecycle_activation_at_commit AFTER INSERT ON employee_lifecycle_events DEFERRABLE INITIALLY DEFERRED
 FOR EACH ROW EXECUTE FUNCTION ortak_check_lifecycle_activation();

-- The shared Office fence covers these final effect commits. Terminal failure
-- receipts are still writable after revocation; they are retained accounting.
CREATE FUNCTION ortak_check_output_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE effect BOOLEAN;
BEGIN
 IF TG_TABLE_NAME='runtime_work_outputs' THEN effect:=NEW.state='materialized';
 ELSIF TG_TABLE_NAME='runtime_office_outputs' THEN effect:=NEW.state='enqueued';
 ELSIF TG_TABLE_NAME='runtime_memory_writes' THEN effect:=NEW.state='pending' AND NEW.admission_token IS NOT NULL;
   IF TG_OP='UPDATE' AND NEW.admission_token IS NOT DISTINCT FROM OLD.admission_token THEN effect:=false; END IF;
 ELSE effect:=true;
 END IF;
 IF effect AND NOT EXISTS(SELECT 1 FROM runs r JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id
   WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND e.status='active' AND e.lifecycle_epoch=r.employee_lifecycle_epoch) THEN
   RAISE EXCEPTION 'Output lifecycle epoch changed' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER lifecycle_work_output_at_commit AFTER INSERT OR UPDATE ON runtime_work_outputs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_output_lifecycle();
CREATE CONSTRAINT TRIGGER lifecycle_artifact_at_commit AFTER INSERT ON artifacts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_output_lifecycle();
CREATE CONSTRAINT TRIGGER lifecycle_office_output_at_commit AFTER INSERT OR UPDATE ON runtime_office_outputs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_output_lifecycle();
CREATE CONSTRAINT TRIGGER lifecycle_memory_output_at_commit AFTER INSERT OR UPDATE ON runtime_memory_writes DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_output_lifecycle();
