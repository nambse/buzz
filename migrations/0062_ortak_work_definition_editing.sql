-- Pending Work definition edits retain identity, review evidence and atomic history.
-- Permit pending text amendments without erasing criterion identity or review facts.
CREATE OR REPLACE FUNCTION work_acceptance_criteria_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id <> OLD.company_id OR NEW.work_item_id <> OLD.work_item_id
       OR NEW.id <> OLD.id OR NEW.position <> OLD.position
       OR NEW.created_at <> OLD.created_at OR OLD.status = 'satisfied'
    THEN
        RAISE EXCEPTION 'ortak: acceptance criterion identity and satisfied evidence are immutable'
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    IF NEW.text = OLD.text AND NEW.status = 'satisfied' THEN
        RETURN NEW;
    END IF;
    IF NEW.text = OLD.text OR NEW.status <> 'pending'
       OR NEW.satisfied_at IS NOT NULL OR NEW.satisfied_by_type IS NOT NULL
       OR NEW.satisfied_by_id IS NOT NULL
       OR NOT EXISTS (
         SELECT 1 FROM work_items w JOIN projects p
           ON p.company_id=w.company_id AND p.id=w.project_id
         WHERE w.company_id=NEW.company_id AND w.id=NEW.work_item_id
           AND p.status='active' AND w.state IN ('proposed','ready','in_progress','blocked')
           AND NOT EXISTS (SELECT 1 FROM work_acceptance_criteria c
             WHERE c.company_id=w.company_id AND c.work_item_id=w.id AND c.status<>'pending')
           AND NOT EXISTS (SELECT 1 FROM work_approvals a
             WHERE a.company_id=w.company_id AND a.work_item_id=w.id AND a.status<>'pending'))
    THEN
        RAISE EXCEPTION 'ortak: definition editing requires pending pre-review work'
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- A changed criterion needs the same transaction's item version and definition history.
-- Deferred because the atomic command writes children before parent/history/receipt.
CREATE FUNCTION work_definition_criterion_history_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.text IS DISTINCT FROM OLD.text AND NOT EXISTS (
      SELECT 1 FROM work_items w JOIN work_item_history h
        ON h.company_id=w.company_id AND h.work_item_id=w.id
        AND h.version=w.version AND h.sequence=w.version-1
      WHERE w.company_id=NEW.company_id AND w.id=NEW.work_item_id
        AND w.xmin::text::bigint=txid_current()%4294967296
        AND h.xmin::text::bigint=txid_current()%4294967296
        AND h.event_type='work.definition_edited'
        AND h.payload->>'event'='definition_edited'
        AND h.payload->'edited_criterion_ids' ? NEW.id::text)
    THEN
        RAISE EXCEPTION 'ortak: criterion edit requires atomic definition history'
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
CREATE CONSTRAINT TRIGGER trg_work_definition_criterion_history
    AFTER UPDATE ON work_acceptance_criteria DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION work_definition_criterion_history_guard();

-- No tables or enum changes. Existing no-delete, identity, status, project/archive,
-- version, dense immutable history, operation receipt and community fences stay intact.

-- Preserve the original promotion definition efficiently after many later edits.
CREATE INDEX idx_work_history_first_definition_edit
    ON work_item_history(company_id,work_item_id,sequence)
    WHERE event_type='work.definition_edited';
