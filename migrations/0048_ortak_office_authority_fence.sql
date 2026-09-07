-- Office authority is serialized with routing/admission, including absent rows.
-- The generation row is also a coalescing durable reconciliation signal: a run
-- whose admitted generation is older must be reauthorized or durably cancelled.
-- No trigger touches runs/outbox while holding an Office mutation row lock.
CREATE TABLE office_authority_generations (
    company_id UUID NOT NULL PRIMARY KEY REFERENCES companies(id),
    generation BIGINT NOT NULL CHECK (generation > 0),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    changed_table TEXT NOT NULL
);

ALTER TABLE routing_decisions
    ADD COLUMN office_authority_generation BIGINT CHECK (office_authority_generation >= 0),
    ADD COLUMN office_authority_valid_before TIMESTAMPTZ,
    ADD COLUMN office_input_hash BYTEA CHECK (octet_length(office_input_hash) = 32);

ALTER TABLE runs
    ADD COLUMN office_admission_generation BIGINT CHECK (office_admission_generation >= 0),
    ADD COLUMN office_admission_valid_before TIMESTAMPTZ,
    ADD COLUMN office_admission_token UUID,
    ADD CONSTRAINT runs_office_admission_token_required
        CHECK ((office_admission_generation IS NULL) = (office_admission_token IS NULL));

-- Domain prefixes isolate this protocol from the retained community deletion
-- fence. Hash collisions conservatively serialize unrelated companies.
CREATE FUNCTION ortak_office_company_lock_key(target UUID) RETURNS BIGINT
LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT hashtextextended('ortak-office-company-v1:' || target::text, 0)
$$;
CREATE FUNCTION ortak_office_community_lock_key(target UUID) RETURNS BIGINT
LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT hashtextextended('ortak-office-community-v1:' || target::text, 0)
$$;

-- Call before taking inbox/root/run/outbox row locks. Keep the transaction
-- short and READ COMMITTED; each SELECT below gets a fresh statement snapshot.
-- An absent generation means zero, so readers never insert or lock a row.
CREATE FUNCTION ortak_lock_office_authority(target UUID) RETURNS BIGINT
LANGUAGE plpgsql VOLATILE STRICT AS $$
DECLARE
    office_community UUID;
    current_generation BIGINT;
    lifecycle TEXT;
BEGIN
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Office authority requires READ COMMITTED isolation'
            USING ERRCODE = 'invalid_transaction_state';
    END IF;
    -- Same key as buzz_db::deletion::SCHEMA_DESTRUCTION_LOCK_KEY.
    -- A nonblocking shared lock avoids migration/table-lock inversion.
    IF NOT pg_try_advisory_xact_lock_shared(7094711454081051697::BIGINT) THEN
        RAISE EXCEPTION 'Office authority schema fence is busy'
            USING ERRCODE = 'serialization_failure';
    END IF;
    PERFORM pg_advisory_xact_lock_shared(ortak_office_company_lock_key(target));
    SELECT community_id INTO office_community FROM office_company_bindings
     WHERE company_id = target;
    IF office_community IS NOT NULL THEN
        -- Never wait with the company fence held on the reverse-order
        -- community mutation/deletion fence. The caller retries the entire tx.
        IF NOT pg_try_advisory_xact_lock_shared(community_deletion_lock_key(office_community))
           OR NOT pg_try_advisory_xact_lock_shared(ortak_office_community_lock_key(office_community)) THEN
            RAISE EXCEPTION 'Office authority community fence is busy'
                USING ERRCODE = 'serialization_failure';
        END IF;
        SELECT deletion_state INTO lifecycle FROM communities WHERE id = office_community;
        IF lifecycle IS DISTINCT FROM 'active' THEN
            RAISE EXCEPTION 'Office authority community is not active'
                USING ERRCODE = 'object_not_in_prerequisite_state';
        END IF;
    END IF;
    SELECT generation INTO current_generation FROM office_authority_generations
     WHERE company_id = target;
    RETURN COALESCE(current_generation, 0);
END
$$;

-- UPDATE/DELETE row triggers can execute after PostgreSQL has locked their
-- target tuple. Waiting here could deadlock a fenced reader which next needs
-- that tuple. A try-lock aborts the entire writer with a retryable SQLSTATE.
CREATE FUNCTION ortak_advance_office_authority(target UUID, source_table TEXT) RETURNS VOID
LANGUAGE plpgsql VOLATILE STRICT AS $$
BEGIN
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Office authority requires READ COMMITTED isolation'
            USING ERRCODE = 'invalid_transaction_state';
    END IF;
    IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
        RAISE EXCEPTION 'Office authority company fence is busy'
            USING ERRCODE = 'serialization_failure';
    END IF;
    INSERT INTO office_authority_generations (company_id, generation, changed_table)
    VALUES (target, 1, source_table)
    ON CONFLICT (company_id) DO UPDATE
       SET generation = office_authority_generations.generation + 1,
           changed_at = clock_timestamp(), changed_table = EXCLUDED.changed_table;
END
$$;

-- Arguments: scope (community/company/binding), followed by authoritative
-- fields. Cosmetic fields, lease churn, counters and run lifecycle do not bump.
CREATE FUNCTION ortak_fence_office_mutation() RETURNS TRIGGER
LANGUAGE plpgsql VOLATILE AS $$
DECLARE
    previous JSONB := CASE WHEN TG_OP <> 'INSERT' THEN to_jsonb(OLD) END;
    proposed JSONB := CASE WHEN TG_OP <> 'DELETE' THEN to_jsonb(NEW) END;
    target UUID;
    target_company UUID;
    field TEXT;
    changed BOOLEAN := TG_OP <> 'UPDATE';
BEGIN
    IF TG_OP = 'UPDATE' THEN
        FOREACH field IN ARRAY TG_ARGV[1:TG_NARGS - 1] LOOP
            IF previous -> field IS DISTINCT FROM proposed -> field THEN
                changed := true;
                EXIT;
            END IF;
        END LOOP;
        IF NOT changed THEN RETURN NEW; END IF;
    END IF;

    -- A new canonical event cannot invalidate an authorized existing event or
    -- parent. Missing canonical/parent events cannot have yielded a wake.
    IF TG_TABLE_NAME LIKE 'events%' AND TG_OP = 'INSERT' THEN RETURN NEW; END IF;
    -- A parentless metadata row has the same meaning as its absence.
    IF TG_TABLE_NAME = 'thread_metadata' AND TG_OP = 'INSERT'
       AND proposed ->> 'parent_event_id' IS NULL
       AND proposed ->> 'parent_event_created_at' IS NULL THEN RETURN NEW; END IF;
    -- Runs acquire publish provenance only through a signed office outbox row.
    IF TG_TABLE_NAME = 'runs' AND TG_OP = 'INSERT' THEN RETURN NEW; END IF;
    IF TG_TABLE_NAME = 'outbox'
       AND NOT (COALESCE(previous ->> 'kind' = 'office_publish'
                         AND previous ->> 'signed_event_id' IS NOT NULL, false)
                OR COALESCE(proposed ->> 'kind' = 'office_publish'
                            AND proposed ->> 'signed_event_id' IS NOT NULL, false)) THEN
        RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
    END IF;

    IF TG_ARGV[0] IN ('community', 'binding', 'community_root') THEN
        -- Cover both old/new scopes; sorted order plus nonblocking acquisition
        -- handles cross-company writes and community mapping insert/delete.
        FOR target IN
            SELECT DISTINCT value::UUID FROM (VALUES
                (previous ->> CASE WHEN TG_ARGV[0] = 'community_root' THEN 'id' ELSE 'community_id' END),
                (proposed ->> CASE WHEN TG_ARGV[0] = 'community_root' THEN 'id' ELSE 'community_id' END)
            ) AS scopes(value) WHERE value IS NOT NULL ORDER BY value::UUID
        LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_community_lock_key(target)) THEN
                RAISE EXCEPTION 'Office authority community mutation fence is busy'
                    USING ERRCODE = 'serialization_failure';
            END IF;
            -- A binding inserted in this transaction is not yet visible to a
            -- BEFORE trigger's lookup; its explicit company is fenced below.
            SELECT company_id INTO target_company FROM office_company_bindings
             WHERE community_id = target;
            IF target_company IS NOT NULL THEN
                PERFORM ortak_advance_office_authority(target_company, TG_TABLE_NAME);
            END IF;
        END LOOP;
    END IF;
    IF TG_ARGV[0] IN ('company', 'binding', 'company_root') THEN
        FOR target IN
            SELECT DISTINCT value::UUID FROM (VALUES
                (previous ->> CASE WHEN TG_ARGV[0] = 'company_root' THEN 'id' ELSE 'company_id' END),
                (proposed ->> CASE WHEN TG_ARGV[0] = 'company_root' THEN 'id' ELSE 'company_id' END)
            ) AS scopes(value) WHERE value IS NOT NULL ORDER BY value::UUID
        LOOP
            PERFORM ortak_advance_office_authority(target, TG_TABLE_NAME);
        END LOOP;
    END IF;
    RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END
$$;

-- Alphabetically after community_write_fence_*: preserve its deletion checks.
CREATE TRIGGER ortak_office_authority_channels BEFORE INSERT OR UPDATE OR DELETE ON channels
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community', 'community_id', 'id', 'channel_type', 'visibility', 'archived_at', 'deleted_at');
CREATE TRIGGER ortak_office_authority_channel_members BEFORE INSERT OR UPDATE OR DELETE ON channel_members
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community', 'community_id', 'channel_id', 'pubkey', 'role', 'removed_at');
CREATE TRIGGER ortak_office_authority_relay_members BEFORE INSERT OR UPDATE OR DELETE ON relay_members
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community', 'community_id', 'pubkey');
CREATE TRIGGER ortak_office_authority_users BEFORE INSERT OR UPDATE OR DELETE ON users
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community', 'community_id', 'pubkey', 'agent_type', 'agent_owner_pubkey', 'deactivated_at');
CREATE TRIGGER ortak_office_authority_events BEFORE UPDATE OR DELETE ON events
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community', 'community_id', 'id', 'created_at', 'pubkey', 'kind', 'tags', 'content', 'sig', 'channel_id', 'deleted_at');
CREATE TRIGGER ortak_office_authority_thread_metadata BEFORE INSERT OR UPDATE OR DELETE ON thread_metadata
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community', 'community_id', 'event_id', 'event_created_at', 'parent_event_id', 'parent_event_created_at');
CREATE TRIGGER ortak_office_authority_communities BEFORE UPDATE OR DELETE ON communities
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community_root', 'id', 'deletion_state', 'deletion_fence_generation', 'deleted_at');
CREATE TRIGGER ortak_office_authority_company_bindings BEFORE INSERT OR DELETE ON office_company_bindings
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('binding', 'community_id', 'company_id');
CREATE TRIGGER ortak_office_authority_employee_bindings BEFORE INSERT OR UPDATE OR DELETE ON employee_office_bindings
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company', 'company_id', 'id', 'employee_id', 'public_key', 'signer_ref', 'valid_from', 'valid_until', 'verified_at');
CREATE TRIGGER ortak_office_authority_employees BEFORE INSERT OR UPDATE OR DELETE ON employees
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company', 'company_id', 'id', 'status', 'active_revision_id');
CREATE TRIGGER ortak_office_authority_employee_revisions BEFORE INSERT OR UPDATE OR DELETE ON employee_revisions
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company', 'company_id', 'id', 'employee_id', 'manifest');
CREATE TRIGGER ortak_office_authority_employee_aliases BEFORE INSERT OR UPDATE OR DELETE ON employee_aliases
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company', 'company_id', 'alias', 'employee_id', 'revision_id');
CREATE TRIGGER ortak_office_authority_runs BEFORE UPDATE OR DELETE ON runs
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company', 'company_id', 'id', 'employee_id', 'employee_revision_id', 'message_id', 'root_message_id', 'routing_decision_id', 'runtime_adapter');
CREATE TRIGGER ortak_office_authority_outbox BEFORE INSERT OR UPDATE OR DELETE ON outbox
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company', 'company_id', 'kind', 'run_id', 'signed_event_id');

CREATE TRIGGER ortak_office_authority_runtime_bindings BEFORE INSERT OR UPDATE OR DELETE ON employee_runtime_bindings
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company', 'company_id', 'revision_id', 'employee_id', 'adapter', 'profile_ref', 'model', 'workspace_ref', 'credential_refs', 'options', 'validated_at');
CREATE TRIGGER ortak_office_authority_inbox BEFORE UPDATE OR DELETE ON office_inbox
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company', 'company_id', 'event_id', 'event_created_at', 'event_kind', 'author_pubkey', 'channel_id');
CREATE TRIGGER ortak_office_authority_companies BEFORE UPDATE ON companies
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company_root', 'id', 'status', 'routing_policy');

-- TRUNCATE bypasses row triggers and cannot express a bounded company scope.
-- Retention/deletion workers must use their fenced DELETE paths instead.
CREATE FUNCTION ortak_reject_office_truncate() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'Office authority tables require scoped DELETE, not TRUNCATE'
        USING ERRCODE = 'object_not_in_prerequisite_state';
END
$$;
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON channels
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON channel_members
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON relay_members
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON users
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON events
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON thread_metadata
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON communities
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON office_company_bindings
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON employee_office_bindings
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON employees
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON employee_revisions
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON employee_aliases
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON runs
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON outbox
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON office_authority_generations
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON employee_runtime_bindings
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON office_inbox
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON companies
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

-- Time advances without a row mutation. Check clock_timestamp at deferred
-- constraint execution, after any blocked root/row lock and just before commit.
-- Historical NULL witnesses remain NULL; runtime admission rejects them.
CREATE FUNCTION ortak_check_routing_office_authority() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    current_generation BIGINT;
BEGIN
    IF NEW.office_authority_generation IS NULL THEN RETURN NEW; END IF;
    current_generation := ortak_lock_office_authority(NEW.company_id);
    IF current_generation <> NEW.office_authority_generation
       OR (NEW.office_authority_valid_before IS NOT NULL
           AND clock_timestamp() >= NEW.office_authority_valid_before) THEN
        RAISE EXCEPTION 'Office routing authority changed or expired before commit'
            USING ERRCODE = 'serialization_failure';
    END IF;
    RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER ortak_routing_office_authority_at_commit
AFTER INSERT ON routing_decisions DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION ortak_check_routing_office_authority();


-- Every prepare/re-prepare writes a fresh token, even when the generation and
-- deadline are unchanged. That forces a deferred check after a blocked row
-- lock; lifecycle-only updates retain the token so cancellation stays possible.
CREATE FUNCTION ortak_check_run_office_authority() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    current_generation BIGINT;
BEGIN
    IF NEW.office_admission_generation IS NULL THEN RETURN NEW; END IF;
    IF TG_OP = 'UPDATE'
       AND OLD.office_admission_generation IS NOT DISTINCT FROM NEW.office_admission_generation
       AND OLD.office_admission_valid_before IS NOT DISTINCT FROM NEW.office_admission_valid_before
       AND OLD.office_admission_token IS NOT DISTINCT FROM NEW.office_admission_token THEN
        RETURN NEW;
    END IF;
    current_generation := ortak_lock_office_authority(NEW.company_id);
    IF current_generation <> NEW.office_admission_generation
       OR (NEW.office_admission_valid_before IS NOT NULL
           AND clock_timestamp() >= NEW.office_admission_valid_before) THEN
        RAISE EXCEPTION 'Office run admission authority changed or expired before commit'
            USING ERRCODE = 'serialization_failure';
    END IF;
    RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER ortak_run_office_authority_at_commit
AFTER INSERT OR UPDATE ON runs DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION ortak_check_run_office_authority();

-- Do not allow resetting the coalesced reconciliation signal. The update
-- guard also catches accidental direct writes rather than only helper calls.
CREATE FUNCTION ortak_guard_office_generation() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'Office authority generations cannot be deleted'
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    IF TG_OP = 'UPDATE' AND (NEW.company_id IS DISTINCT FROM OLD.company_id
                            OR NEW.generation <= OLD.generation) THEN
        RAISE EXCEPTION 'Office authority generations must advance'
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(NEW.company_id)) THEN
        RAISE EXCEPTION 'Office authority generation fence is busy'
            USING ERRCODE = 'serialization_failure';
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER ortak_office_generation_guard BEFORE INSERT OR UPDATE OR DELETE ON office_authority_generations
FOR EACH ROW EXECUTE FUNCTION ortak_guard_office_generation();


-- Current event partitions also reject direct TRUNCATE. New partitions must
-- attach this statement trigger before serving (row guards clone themselves).
DO $$
DECLARE
    partition_table REGCLASS;
BEGIN
    FOR partition_table IN
        SELECT relid FROM pg_partition_tree('events'::REGCLASS) WHERE isleaf
    LOOP
        EXECUTE format('CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON %s FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()', partition_table);
    END LOOP;
END
$$;
