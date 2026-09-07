-- E1: explicit human project grants, one immutable Office channel, and retry receipts.
-- Company grants alone never expose these projects. No existing project is adopted.
ALTER TABLE office_company_bindings ADD CONSTRAINT uq_work_api_company_community
    UNIQUE (company_id, community_id);

CREATE TABLE project_api_bindings (
    company_id UUID NOT NULL REFERENCES companies(id),
    project_id UUID NOT NULL,
    community_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    created_by TEXT NOT NULL CHECK (created_by ~ '^[0-9a-f]{64}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, project_id),
    FOREIGN KEY (company_id, project_id) REFERENCES projects(company_id, id),
    FOREIGN KEY (company_id, community_id) REFERENCES office_company_bindings(company_id, community_id),
    FOREIGN KEY (community_id, channel_id) REFERENCES channels(community_id, id)
);
CREATE TRIGGER project_api_binding_immutable BEFORE UPDATE OR DELETE ON project_api_bindings
FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER project_api_binding_no_truncate BEFORE TRUNCATE ON project_api_bindings
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
SELECT attach_community_write_fence('project_api_bindings');

CREATE TABLE project_access_grants (
    company_id UUID NOT NULL REFERENCES companies(id),
    project_id UUID NOT NULL,
    actor_pubkey TEXT NOT NULL CHECK (actor_pubkey ~ '^[0-9a-f]{64}$'),
    role TEXT NOT NULL CHECK (role IN ('viewer','contributor','reviewer','owner')),
    granted_by TEXT NOT NULL CHECK (granted_by ~ '^[0-9a-f]{64}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ,
    PRIMARY KEY (company_id, project_id, actor_pubkey),
    FOREIGN KEY (company_id, project_id) REFERENCES project_api_bindings(company_id, project_id)
);
CREATE INDEX idx_project_access_principal ON project_access_grants(company_id, actor_pubkey, project_id)
WHERE revoked_at IS NULL;

-- Readers/ordinary writers hold the project SHARE lock before looking up grants.
-- Fence every ACL write, including an absent-row insertion, with that same parent.
-- NOWAIT refuses reverse-order SQL contention instead of forming a lock cycle.
-- Do not use the Office exclusive mutation fence: API authentication holds its
-- shared counterpart on another connection throughout the request.
CREATE FUNCTION ortak_project_access_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        RAISE EXCEPTION 'ortak: project grants are revoked, never deleted'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF TG_OP='UPDATE' AND (NEW.company_id,NEW.project_id,NEW.actor_pubkey,NEW.granted_by,NEW.created_at)
        IS DISTINCT FROM (OLD.company_id,OLD.project_id,OLD.actor_pubkey,OLD.granted_by,OLD.created_at) THEN
        RAISE EXCEPTION 'ortak: project grant identity is immutable'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=NEW.project_id FOR UPDATE NOWAIT;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: project grant parent is missing' USING ERRCODE='foreign_key_violation';
    END IF;
    NEW.updated_at=clock_timestamp();
    RETURN NEW;
END
$$;
CREATE TRIGGER project_access_guard BEFORE INSERT OR UPDATE OR DELETE ON project_access_grants
FOR EACH ROW EXECUTE FUNCTION ortak_project_access_guard();
CREATE TRIGGER project_access_no_truncate BEFORE TRUNCATE ON project_access_grants
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

-- The receipt is also the immutable success audit: authenticated human, NIP-98
-- event, exact operation identity/hash, project/item target and resulting version.
-- It stores no request text, private key, provider configuration or cached response.
CREATE TABLE work_api_operations (
    company_id UUID NOT NULL REFERENCES companies(id),
    actor_pubkey TEXT NOT NULL CHECK (actor_pubkey ~ '^[0-9a-f]{64}$'),
    operation_id UUID NOT NULL CHECK (operation_id <> '00000000-0000-0000-0000-000000000000'),
    action TEXT NOT NULL CHECK (action IN ('create_project','create_work_item','mutate_work_item')),
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    project_id UUID NOT NULL,
    work_item_id UUID,
    result_version BIGINT NOT NULL CHECK (result_version>=1),
    auth_event_id BYTEA NOT NULL CHECK (octet_length(auth_event_id)=32),
    valid_before TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, actor_pubkey, operation_id),
    FOREIGN KEY (company_id, project_id) REFERENCES project_api_bindings(company_id, project_id),
    FOREIGN KEY (company_id, work_item_id) REFERENCES work_items(company_id, id),
    CHECK ((action='create_project') = (work_item_id IS NULL))
);
CREATE INDEX idx_work_api_operations_project ON work_api_operations(company_id, project_id, created_at DESC);
CREATE TRIGGER work_api_operation_immutable BEFORE UPDATE OR DELETE ON work_api_operations
FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER work_api_operation_no_truncate BEFORE TRUNCATE ON work_api_operations
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_check_work_api_receipt() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.valid_before IS NOT NULL AND clock_timestamp()>=NEW.valid_before THEN
        RAISE EXCEPTION 'ortak: Work authority expired before commit' USING ERRCODE='serialization_failure';
    END IF;
    IF NEW.work_item_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM work_items WHERE company_id=NEW.company_id AND id=NEW.work_item_id AND project_id=NEW.project_id
    ) THEN
        RAISE EXCEPTION 'ortak: Work receipt target differs from project' USING ERRCODE='foreign_key_violation';
    END IF;
    RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER work_api_receipt_at_commit AFTER INSERT ON work_api_operations
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_work_api_receipt();
