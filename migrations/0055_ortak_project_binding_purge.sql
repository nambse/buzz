-- Community deletion severs API access, not the independent company's Work history.
-- Retained grants/receipts cannot grant access without a live API/Office binding.
ALTER TABLE project_access_grants
    DROP CONSTRAINT project_access_grants_company_id_project_id_fkey,
    ADD CONSTRAINT project_access_grants_company_id_project_id_fkey
        FOREIGN KEY (company_id, project_id) REFERENCES projects(company_id, id);
ALTER TABLE work_api_operations
    DROP CONSTRAINT work_api_operations_company_id_project_id_fkey,
    ADD CONSTRAINT work_api_operations_company_id_project_id_fkey
        FOREIGN KEY (company_id, project_id) REFERENCES projects(company_id, id);

CREATE FUNCTION ortak_assert_project_binding_purge(target UUID, committing BOOLEAN)
RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE
    request_text TEXT := current_setting('buzz.deletion_request_id', true);
    purge_request_id UUID;
    allowed BOOLEAN;
BEGIN
    IF request_text IS NULL OR request_text !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN
        RAISE EXCEPTION 'ortak: project binding requires an approved deletion executor'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    purge_request_id := request_text::UUID;
    -- Lock first, then use a fresh statement snapshot after any lock wait.
    -- A lease takeover cannot change authority while the purge commits.
    PERFORM 1 FROM community_deletion_requests r WHERE r.id=purge_request_id FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: project binding deletion request is missing'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    PERFORM 1 FROM community_deletion_approvals a WHERE a.request_id=purge_request_id FOR SHARE;
    SELECT EXISTS (
        SELECT 1 FROM community_deletion_requests r
        JOIN community_deletion_approvals a ON a.request_id=r.id
            AND a.community_id=r.community_id AND a.inventory_digest=r.inventory_digest
        JOIN communities c ON c.id=r.community_id
        WHERE r.id=purge_request_id AND r.community_id=target AND r.blocked_at IS NULL
          AND r.lease_owner=current_setting('buzz.deletion_lease_owner', true)
          AND r.lease_generation::TEXT=current_setting('buzz.deletion_lease_generation', true)
          AND r.lease_until>clock_timestamp()
          AND r.fence_generation=c.deletion_fence_generation
          AND target::TEXT=current_setting('buzz.deletion_executor_community', true)
          AND r.fence_generation::TEXT=current_setting('buzz.deletion_fence_generation', true)
          AND ((NOT committing AND r.stage='bindings_removed' AND c.deletion_state='fenced')
            OR (committing AND r.stage='postgres_purged' AND c.deletion_state='tombstone'))
    ) INTO allowed;
    IF NOT allowed THEN
        RAISE EXCEPTION 'ortak: project binding deletion authority is not current'
            USING ERRCODE=CASE WHEN committing THEN '40001' ELSE '55000' END;
    END IF;
END
$$;

CREATE FUNCTION ortak_guard_project_api_binding() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP<>'DELETE' THEN
        RAISE EXCEPTION 'ortak: project API binding identity is immutable'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    PERFORM ortak_assert_project_binding_purge(OLD.community_id, false);
    RETURN OLD;
END
$$;
CREATE FUNCTION ortak_project_binding_purge_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    -- SET CONSTRAINTS IMMEDIATE before the complete purge fails closed too.
    PERFORM ortak_assert_project_binding_purge(OLD.community_id, true);
    RETURN OLD;
END
$$;
DROP TRIGGER project_api_binding_immutable ON project_api_bindings;
CREATE TRIGGER project_api_binding_immutable BEFORE UPDATE OR DELETE ON project_api_bindings
FOR EACH ROW EXECUTE FUNCTION ortak_guard_project_api_binding();
CREATE CONSTRAINT TRIGGER project_api_binding_purge_at_commit AFTER DELETE ON project_api_bindings
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_project_binding_purge_at_commit();
