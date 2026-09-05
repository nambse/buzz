-- Memory identity participates in the same generation as Office admission.
CREATE TRIGGER ortak_office_authority_memory_bindings BEFORE INSERT OR UPDATE OR DELETE ON employee_memory_bindings
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company','company_id','revision_id','employee_id','adapter','provisioning_mode','endpoint_ref','workspace','user_peer','employee_peer','options','validated_at');
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON employee_memory_bindings
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

-- Exact serialized pre-start input. The runtime owns validation and first-writer
-- admission; the database prevents retries from replacing an existing request.
CREATE TABLE run_context_snapshots (
    company_id UUID NOT NULL,
    run_id UUID NOT NULL,
    spec_bytes BYTEA NOT NULL CHECK (octet_length(spec_bytes) BETWEEN 1 AND 262144),
    spec_hash BYTEA NOT NULL CHECK (octet_length(spec_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id,run_id),
    FOREIGN KEY (company_id,run_id) REFERENCES runs(company_id,id)
);
CREATE TRIGGER trg_run_context_snapshot_immutable BEFORE UPDATE OR DELETE ON run_context_snapshots
FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- Acknowledged Office replies are the only automatic memory input. RunScratch
-- preserves the original run boundary; no project/global promotion is implied.
CREATE TABLE runtime_memory_writes (
    company_id UUID NOT NULL,
    run_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    employee_revision_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    outbox_id UUID NOT NULL,
    signed_event_id BYTEA NOT NULL CHECK (octet_length(signed_event_id)=32),
    binding JSONB NOT NULL CHECK (jsonb_typeof(binding)='object' AND octet_length(binding::text)<=32768),
    source_facts JSONB NOT NULL CHECK (jsonb_typeof(source_facts)='object' AND octet_length(source_facts::text)<=4096),
    content TEXT NOT NULL CHECK (octet_length(content) BETWEEN 1 AND 32768 AND btrim(content)<>''),
    recorded_at TIMESTAMPTZ NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (idempotency_key='office-output:'||run_id::text),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','acknowledged','failed')),
    attempt_count INT NOT NULL DEFAULT 0 CHECK (attempt_count BETWEEN 0 AND 20),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK (last_error_code ~ '^[a-z][a-z0-9_.]{0,63}$'),
    admission_generation BIGINT CHECK (admission_generation>=0),
    admission_valid_before TIMESTAMPTZ,
    admission_token UUID,
    receipt JSONB CHECK (jsonb_typeof(receipt)='object' AND octet_length(receipt::text)<=4096),
    acknowledged_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id,run_id),
    UNIQUE (company_id,outbox_id),
    FOREIGN KEY (company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY (company_id,outbox_id) REFERENCES outbox(company_id,id),
    FOREIGN KEY (company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id),
    CHECK ((lease_token IS NULL)=(lease_expires_at IS NULL)),
    CHECK (state='pending' OR lease_token IS NULL),
    CHECK ((admission_generation IS NULL)=(admission_token IS NULL)),
    CHECK ((state='acknowledged')=(receipt IS NOT NULL)),
    CHECK ((state='acknowledged')=(acknowledged_at IS NOT NULL))
);
CREATE INDEX idx_runtime_memory_writes_due ON runtime_memory_writes
    (company_id,next_attempt_at,created_at,run_id) WHERE state='pending';

CREATE FUNCTION ortak_memory_write_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.state<>'pending' OR NEW.attempt_count<OLD.attempt_count OR
       ROW(NEW.company_id,NEW.run_id,NEW.employee_id,NEW.employee_revision_id,NEW.channel_id,
           NEW.outbox_id,NEW.signed_event_id,NEW.binding,NEW.source_facts,NEW.content,
           NEW.recorded_at,NEW.idempotency_key,NEW.created_at)
       IS DISTINCT FROM
       ROW(OLD.company_id,OLD.run_id,OLD.employee_id,OLD.employee_revision_id,OLD.channel_id,
           OLD.outbox_id,OLD.signed_event_id,OLD.binding,OLD.source_facts,OLD.content,
           OLD.recorded_at,OLD.idempotency_key,OLD.created_at) THEN
        RAISE EXCEPTION 'ortak: memory request and terminal receipt are immutable'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_memory_write_guard BEFORE UPDATE ON runtime_memory_writes
FOR EACH ROW EXECUTE FUNCTION ortak_memory_write_guard();
CREATE TRIGGER trg_memory_write_no_delete BEFORE DELETE ON runtime_memory_writes
FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE FUNCTION ortak_insert_acknowledged_memory_write(target_company UUID,target_outbox UUID)
RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    -- FK insertion would otherwise wait for run FOR UPDATE while the caller
    -- holds outbox. Cancellation takes run before outbox. Refuse immediately
    -- on that inversion; the pending delivery lease remains safely retryable.
    PERFORM r.id FROM outbox o
    JOIN runtime_office_outputs j ON j.company_id=o.company_id AND j.outbox_id=o.id
    JOIN runs r ON r.company_id=j.company_id AND r.id=j.run_id
    WHERE o.company_id=target_company AND o.id=target_outbox AND o.kind='office_publish'
      AND o.state='delivered' AND j.state='enqueued' AND r.status='completed'
      AND r.delivery_intent IN ('reply','channel')
    FOR KEY SHARE OF r NOWAIT;
    INSERT INTO runtime_memory_writes(company_id,run_id,employee_id,employee_revision_id,channel_id,
        outbox_id,signed_event_id,binding,source_facts,content,recorded_at,idempotency_key)
    SELECT r.company_id,r.id,r.employee_id,r.employee_revision_id,i.channel_id,
        o.id,o.signed_event_id,rev.manifest->'memory',j.source_facts,j.draft_content,
        o.delivered_at,'office-output:'||r.id::text
    FROM outbox o JOIN runtime_office_outputs j ON j.company_id=o.company_id AND j.outbox_id=o.id
    JOIN runs r ON r.company_id=j.company_id AND r.id=j.run_id
    JOIN employee_revisions rev ON rev.company_id=r.company_id AND rev.employee_id=r.employee_id AND rev.id=r.employee_revision_id
    JOIN office_inbox i ON i.company_id=r.company_id AND i.event_id=r.message_id
    WHERE o.company_id=target_company AND o.id=target_outbox AND o.kind='office_publish'
      AND o.state='delivered' AND o.signed_event_id IS NOT NULL AND o.signed_event_bytes IS NOT NULL
      AND o.run_id=r.id AND j.state='enqueued' AND r.status='completed'
      AND r.delivery_intent IN ('reply','channel') AND i.channel_id IS NOT NULL
      AND jsonb_typeof(rev.manifest->'memory')='object'
      AND NOT EXISTS (SELECT 1 FROM runtime_cancellations x WHERE x.company_id=r.company_id AND x.run_id=r.id)
      AND NOT EXISTS (SELECT 1 FROM run_cancel_requests x WHERE x.company_id=r.company_id AND x.run_id=r.id)
    ON CONFLICT (company_id,run_id) DO NOTHING;
END;
$$;
CREATE FUNCTION ortak_schedule_acknowledged_memory_write() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.kind='office_publish' AND NEW.state='delivered' AND OLD.state<>'delivered' THEN
        -- A NOWAIT FK-parent check prevents waiting in outbox→run order.
        -- Claim/prepare later revalidate current authority.
        PERFORM ortak_insert_acknowledged_memory_write(NEW.company_id,NEW.id);
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_outbox_schedule_memory_write AFTER UPDATE OF state ON outbox
FOR EACH ROW EXECUTE FUNCTION ortak_schedule_acknowledged_memory_write();

CREATE FUNCTION ortak_check_memory_write_authority() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.admission_token IS NOT NULL AND (TG_OP='INSERT' OR
       ROW(NEW.admission_token,NEW.admission_generation,NEW.admission_valid_before)
       IS DISTINCT FROM ROW(OLD.admission_token,OLD.admission_generation,OLD.admission_valid_before)) THEN
        IF NEW.admission_generation IS DISTINCT FROM ortak_lock_office_authority(NEW.company_id)
           OR (NEW.admission_valid_before IS NOT NULL AND clock_timestamp()>=NEW.admission_valid_before) THEN
            RAISE EXCEPTION 'ortak: memory write admission changed or expired' USING ERRCODE='serialization_failure';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
CREATE CONSTRAINT TRIGGER trg_memory_write_authority AFTER INSERT OR UPDATE ON runtime_memory_writes
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_memory_write_authority();

-- Existing acknowledged completions receive the same deterministic job. This
-- function also supports desired-state reconciliation without duplicate work.
SELECT ortak_insert_acknowledged_memory_write(company_id,id)
FROM outbox WHERE kind='office_publish' AND state='delivered';
