-- PROPOSAL ONLY: root integrates after 0063; never apply from product handlers.
-- No credential values. Complete prepared selections are private server data.
CREATE TABLE employee_management_policies (
 company_id UUID NOT NULL REFERENCES companies(id), public_key TEXT NOT NULL CHECK(public_key ~ '^[0-9a-f]{64}$'),
 fingerprint BYTEA NOT NULL CHECK(octet_length(fingerprint)=32), enabled BOOLEAN NOT NULL,
 employee_ids TEXT[] NOT NULL CHECK(cardinality(employee_ids) BETWEEN 1 AND 64),
 channel_ids UUID[] NOT NULL CHECK(cardinality(channel_ids) BETWEEN 1 AND 64),
 PRIMARY KEY(company_id,public_key)
);
CREATE TABLE prepared_employee_catalog (
 company_id UUID NOT NULL REFERENCES companies(id), id UUID NOT NULL, employee_id TEXT NOT NULL,
 label TEXT NOT NULL CHECK(octet_length(label) BETWEEN 1 AND 128), enabled BOOLEAN NOT NULL DEFAULT true,
 configuration JSONB NOT NULL CHECK(jsonb_typeof(configuration)='object' AND octet_length(configuration::text)<=65536),
 fingerprint BYTEA NOT NULL CHECK(octet_length(fingerprint)=32),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(company_id,id)
);
CREATE TABLE employee_configuration_drafts (
 company_id UUID NOT NULL REFERENCES companies(id), id UUID NOT NULL, employee_id TEXT NOT NULL,
 catalog_id UUID NOT NULL, actor TEXT NOT NULL, expected_revision_id UUID,
 configuration JSONB NOT NULL CHECK(jsonb_typeof(configuration)='object' AND octet_length(configuration::text)<=65536),
 fingerprint BYTEA NOT NULL CHECK(octet_length(fingerprint)=32),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(company_id,id),
 FOREIGN KEY(company_id,catalog_id) REFERENCES prepared_employee_catalog(company_id,id)
);
CREATE TABLE employee_management_commands (
 company_id UUID NOT NULL REFERENCES companies(id), id UUID NOT NULL, employee_id TEXT NOT NULL,
 actor TEXT NOT NULL CHECK(actor ~ '^[0-9a-f]{64}$'), auth_event_id BYTEA NOT NULL CHECK(octet_length(auth_event_id)=32),
 policy_fingerprint BYTEA NOT NULL CHECK(octet_length(policy_fingerprint)=32),
 policy_snapshot JSONB NOT NULL CHECK(jsonb_typeof(policy_snapshot)='object' AND octet_length(policy_snapshot::text)<=16384),
 action TEXT NOT NULL CHECK(action IN ('adopt','update','retry','compensate')),
 idempotency_key TEXT NOT NULL CHECK(octet_length(idempotency_key) BETWEEN 1 AND 128),
 request_fingerprint BYTEA NOT NULL CHECK(octet_length(request_fingerprint)=32),
 draft_id UUID, operation_id UUID, expected_revision_id UUID,
 configuration JSONB CHECK(configuration IS NULL OR (jsonb_typeof(configuration)='object' AND octet_length(configuration::text)<=65536)),
 channel_ids UUID[] NOT NULL CHECK(cardinality(channel_ids) BETWEEN 0 AND 64),
 status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','running','succeeded','failed','blocked')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
 next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 lease_token UUID, lease_expires_at TIMESTAMPTZ, error_code TEXT CHECK(error_code ~ '^[a-z][a-z0-9_]{0,63}$'),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(company_id,id), UNIQUE(company_id,idempotency_key),
 FOREIGN KEY(company_id,draft_id) REFERENCES employee_configuration_drafts(company_id,id),
 FOREIGN KEY(company_id,operation_id) REFERENCES provisioning_operations(company_id,id),
 CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),
 CHECK(action='compensate' OR configuration IS NOT NULL)
);
CREATE UNIQUE INDEX employee_management_one_pending ON employee_management_commands(company_id,employee_id)
 WHERE status IN ('pending','running');
CREATE INDEX employee_management_due ON employee_management_commands(company_id,next_attempt_at,id)
 WHERE status IN ('pending','running');
CREATE TABLE employee_management_audit (
 company_id UUID NOT NULL REFERENCES companies(id), id UUID NOT NULL DEFAULT gen_random_uuid(),
 actor TEXT NOT NULL, auth_event_id BYTEA NOT NULL CHECK(octet_length(auth_event_id)=32),
 employee_id TEXT, command_id UUID, action TEXT NOT NULL CHECK(action IN ('catalog','draft','adopt','update','retry','compensate','command')),
 outcome TEXT NOT NULL CHECK(outcome IN ('accepted','read','denied','not_found','conflict','replayed')),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(company_id,id)
);
CREATE FUNCTION ortak_management_immutable() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP IN ('DELETE','TRUNCATE') THEN RAISE EXCEPTION 'Management history is retained' USING ERRCODE='check_violation'; END IF;
 IF TG_TABLE_NAME='prepared_employee_catalog' AND (to_jsonb(NEW)-'enabled')=(to_jsonb(OLD)-'enabled') THEN RETURN NEW; END IF;
 IF TG_TABLE_NAME='employee_management_commands' AND
   (to_jsonb(NEW)-ARRAY['operation_id','status','attempts','next_attempt_at','lease_token','lease_expires_at','error_code','updated_at'])=
   (to_jsonb(OLD)-ARRAY['operation_id','status','attempts','next_attempt_at','lease_token','lease_expires_at','error_code','updated_at'])
   AND (OLD.operation_id IS NULL OR NEW.operation_id IS NOT DISTINCT FROM OLD.operation_id)
   AND NEW.attempts>=OLD.attempts
   AND (OLD.status IN ('pending','running') OR NEW=OLD) THEN RETURN NEW; END IF;
 RAISE EXCEPTION 'Management selection is immutable' USING ERRCODE='check_violation';
END $$;
CREATE TRIGGER prepared_employee_catalog_immutable BEFORE UPDATE OR DELETE ON prepared_employee_catalog FOR EACH ROW EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_configuration_drafts_immutable BEFORE UPDATE OR DELETE ON employee_configuration_drafts FOR EACH ROW EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_management_commands_immutable BEFORE UPDATE OR DELETE ON employee_management_commands FOR EACH ROW EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_management_audit_immutable BEFORE UPDATE OR DELETE ON employee_management_audit FOR EACH ROW EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER prepared_employee_catalog_no_truncate BEFORE TRUNCATE ON prepared_employee_catalog FOR EACH STATEMENT EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_configuration_drafts_no_truncate BEFORE TRUNCATE ON employee_configuration_drafts FOR EACH STATEMENT EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_management_commands_no_truncate BEFORE TRUNCATE ON employee_management_commands FOR EACH STATEMENT EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_management_audit_no_truncate BEFORE TRUNCATE ON employee_management_audit FOR EACH STATEMENT EXECUTE FUNCTION ortak_management_immutable();

-- Caller holds Office authority before policy/command/employee/operation locks.
-- Shared policy locks prevent an old worker committing after policy replacement.
CREATE FUNCTION ortak_management_actor_allowed(target UUID, actor_key TEXT, policy_hash BYTEA, employee TEXT, channels UUID[]) RETURNS BOOLEAN
LANGUAGE plpgsql VOLATILE AS $$
DECLARE p employee_management_policies%ROWTYPE; community UUID; key_bytes BYTEA;
BEGIN
 SELECT * INTO p FROM employee_management_policies WHERE company_id=target AND public_key=actor_key FOR SHARE;
 IF NOT FOUND OR NOT p.enabled OR p.fingerprint<>policy_hash OR NOT(employee=ANY(p.employee_ids)) OR NOT(channels<@p.channel_ids) THEN RETURN false; END IF;
 SELECT b.community_id INTO community FROM office_company_bindings b JOIN companies c ON c.id=b.company_id
 JOIN communities cm ON cm.id=b.community_id WHERE b.company_id=target AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL;
 IF community IS NULL THEN RETURN false; END IF;
 key_bytes:=decode(actor_key,'hex');
 IF NOT(EXISTS(SELECT 1 FROM relay_members WHERE community_id=community AND pubkey=actor_key)
     OR EXISTS(SELECT 1 FROM channel_members WHERE community_id=community AND pubkey=key_bytes AND removed_at IS NULL))
   OR EXISTS(SELECT 1 FROM users WHERE community_id=community AND pubkey=key_bytes AND (deactivated_at IS NOT NULL OR agent_type IS NOT NULL OR agent_owner_pubkey IS NOT NULL))
   OR EXISTS(SELECT 1 FROM employee_office_bindings WHERE company_id=target AND public_key=key_bytes)
   OR EXISTS(SELECT 1 FROM channel_members WHERE community_id=community AND pubkey=key_bytes AND role='bot') THEN RETURN false; END IF;
 RETURN NOT EXISTS(SELECT 1 FROM unnest(channels) selected(id) WHERE NOT EXISTS(
   SELECT 1 FROM channels c WHERE c.community_id=community AND c.id=selected.id AND c.deleted_at IS NULL
   AND c.channel_type::text='stream' AND (c.visibility::text='open' OR EXISTS(
     SELECT 1 FROM channel_members m WHERE m.community_id=community AND m.channel_id=c.id AND m.pubkey=key_bytes AND m.removed_at IS NULL))));
END $$;

CREATE FUNCTION ortak_management_guard(target UUID, command UUID, token UUID, operation UUID) RETURNS VOID
LANGUAGE plpgsql VOLATILE AS $$
DECLARE c employee_management_commands%ROWTYPE; op provisioning_operations%ROWTYPE; current_revision UUID; current_status TEXT;
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
     (op.manifest IS DISTINCT FROM c.configuration->'manifest' OR op.mode IS DISTINCT FROM c.configuration->>'mode'
      OR op.idempotency_key IS DISTINCT FROM c.configuration->>'operation_key' OR op.dry_run)) THEN
     RAISE EXCEPTION 'Management operation scope mismatch' USING ERRCODE='check_violation';
   END IF;
 END IF;
 IF c.action<>'compensate' THEN
   SELECT active_revision_id,status INTO current_revision,current_status FROM employees WHERE company_id=target AND id=c.employee_id FOR SHARE;
   IF current_status='disabled' OR (current_revision IS DISTINCT FROM c.expected_revision_id AND NOT EXISTS(
     SELECT 1 FROM provisioning_operations o WHERE o.company_id=target AND o.id=c.operation_id AND o.result_revision_id=current_revision AND o.status='succeeded')) THEN
     RAISE EXCEPTION 'Management revision superseded' USING ERRCODE='check_violation';
   END IF;
 END IF;
 PERFORM set_config('ortak.management_command',command::text,true);
 PERFORM set_config('ortak.management_token',token::text,true);
END $$;

-- Direct CLI replays cannot bypass the delegated actor/lease for a managed
-- operation. Deferred validation also catches lease expiry before final commit.
CREATE FUNCTION ortak_management_operation_fence() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE target UUID; operation UUID; selected UUID; token UUID; managed BOOLEAN;
BEGIN
 target:=NEW.company_id;
 IF TG_TABLE_NAME='provisioning_operations' THEN operation:=NEW.id; ELSE operation:=NEW.operation_id; END IF;
 SELECT EXISTS(SELECT 1 FROM employee_management_commands c JOIN provisioning_operations o ON o.company_id=c.company_id
   AND (o.id=c.operation_id OR o.idempotency_key=c.configuration->>'operation_key')
   WHERE c.company_id=target AND o.id=operation) INTO managed;
 IF NOT managed THEN RETURN NEW; END IF;
 selected:=nullif(current_setting('ortak.management_command',true),'')::uuid;
 token:=nullif(current_setting('ortak.management_token',true),'')::uuid;
 IF selected IS NULL OR token IS NULL THEN RAISE EXCEPTION 'Managed operation requires its executor' USING ERRCODE='insufficient_privilege'; END IF;
 PERFORM ortak_management_guard(target,selected,token,operation);
 RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER employee_management_operation_at_commit AFTER INSERT OR UPDATE ON provisioning_operations
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_management_operation_fence();
CREATE CONSTRAINT TRIGGER employee_management_step_at_commit AFTER INSERT OR UPDATE ON provisioning_operation_steps
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_management_operation_fence();
