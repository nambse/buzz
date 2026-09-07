-- PROPOSAL ONLY: E4 dependency editing. Root owns immutable integration/parity.
-- Parent/child decomposition is a separate slice.
ALTER TABLE work_dependencies
    ADD COLUMN id UUID NOT NULL DEFAULT gen_random_uuid()
        CHECK(id<>'00000000-0000-0000-0000-000000000000'),
    ADD COLUMN released_at TIMESTAMPTZ,
    ADD CONSTRAINT work_dependencies_company_id_id_key UNIQUE(company_id,id);

-- Original edge identity and creation provenance survive remove/re-add. Each
-- command records its actor/reason in dense Work history and its atomic receipt.
-- A hidden target can be removed via the edge UUID without revealing target IDs.
CREATE FUNCTION ortak_work_dependency_edit_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE source_state TEXT;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (to_jsonb(NEW)-'released_at') IS DISTINCT FROM (to_jsonb(OLD)-'released_at')
            OR (OLD.released_at IS NULL)=(NEW.released_at IS NULL)
            OR NEW.released_at>clock_timestamp() THEN
            RAISE EXCEPTION 'ortak: dependency only permits retained release or reactivation'
                USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
    ELSIF NEW.released_at IS NOT NULL THEN
        RAISE EXCEPTION 'ortak: dependency must be created active'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    -- Direct writers must not invert the graph's project-before-item lock order.
    -- Ordinary commands already hold EXCLUSIVE before reading current authority.
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=NEW.project_id
        AND status='active' FOR UPDATE NOWAIT;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: dependency project is unavailable'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    SELECT state INTO source_state FROM work_items
        WHERE company_id=NEW.company_id AND project_id=NEW.project_id AND id=NEW.work_item_id
        FOR UPDATE NOWAIT;
    IF source_state IS NULL OR source_state IN('completed','cancelled') THEN
        RAISE EXCEPTION 'ortak: dependency source is immutable or missing'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;

DROP TRIGGER trg_work_dependencies_immutable ON work_dependencies;
CREATE TRIGGER trg_work_dependencies_no_delete BEFORE DELETE ON work_dependencies
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER trg_work_dependencies_no_truncate BEFORE TRUNCATE ON work_dependencies
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
DROP TRIGGER work_dependency_authority_guard ON work_dependencies;
CREATE TRIGGER work_dependency_authority_guard BEFORE INSERT OR UPDATE ON work_dependencies
    FOR EACH ROW EXECUTE FUNCTION ortak_work_dependency_edit_guard();
DROP TRIGGER work_authority_dependencies ON work_dependencies;
CREATE TRIGGER work_authority_dependencies AFTER INSERT OR UPDATE ON work_dependencies
    FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();

CREATE INDEX idx_work_dependencies_active_project
    ON work_dependencies(company_id,project_id,work_item_id,depends_on_work_item_id)
    WHERE released_at IS NULL;

-- No transient community binding FK: these company-owned Work relations remain
-- after an approved Office purge, while current project/Office reads fail closed.
