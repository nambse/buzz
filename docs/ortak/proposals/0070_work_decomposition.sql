-- PROPOSAL ONLY: E5 fresh structural decomposition. Root owns final numbering,
-- immutable migrations, desired schema, reconciliation and parity validation.
-- Company evidence survives Office/community binding purge.
CREATE TABLE work_decomposition (
    company_id UUID NOT NULL,
    project_id UUID NOT NULL,
    parent_id UUID NOT NULL,
    child_id UUID NOT NULL CHECK(child_id<>'00000000-0000-0000-0000-000000000000'),
    parent_version BIGINT NOT NULL CHECK(parent_version>1),
    depth SMALLINT NOT NULL CHECK(depth BETWEEN 1 AND 8),
    actor_pubkey TEXT NOT NULL CHECK(actor_pubkey ~ '^[0-9a-f]{64}$'),
    operation_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(company_id,child_id),
    UNIQUE(company_id,actor_pubkey,operation_id),
    CHECK(parent_id<>child_id),
    FOREIGN KEY(company_id,project_id,parent_id) REFERENCES work_items(company_id,project_id,id),
    FOREIGN KEY(company_id,project_id,child_id) REFERENCES work_items(company_id,project_id,id)
        DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(company_id,actor_pubkey,operation_id) REFERENCES work_api_operations(company_id,actor_pubkey,operation_id)
        DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX work_decomposition_parent ON work_decomposition(company_id,parent_id,child_id);

CREATE FUNCTION ortak_work_decomposition_reserve() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE parent work_items%ROWTYPE; parent_depth SMALLINT; children INTEGER;
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=NEW.project_id
        AND status='active' FOR UPDATE NOWAIT;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: decomposition project is unavailable' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    SELECT * INTO parent FROM work_items WHERE company_id=NEW.company_id
        AND project_id=NEW.project_id AND id=NEW.parent_id FOR UPDATE NOWAIT;
    IF NOT FOUND OR parent.state IN('completed','cancelled') OR parent.version+1<>NEW.parent_version
        OR EXISTS(SELECT 1 FROM work_items WHERE company_id=NEW.company_id AND id=NEW.child_id) THEN
        RAISE EXCEPTION 'ortak: decomposition requires a mutable parent and a fresh child'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    SELECT coalesce((SELECT depth FROM work_decomposition
        WHERE company_id=NEW.company_id AND child_id=NEW.parent_id),0) INTO parent_depth;
    SELECT count(*) INTO children FROM work_decomposition
        WHERE company_id=NEW.company_id AND parent_id=NEW.parent_id;
    IF NEW.depth<>parent_depth+1 OR children>=32 OR NEW.created_at<>now() THEN
        RAISE EXCEPTION 'ortak: decomposition bound or provenance differs' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_decomposition_reserve BEFORE INSERT ON work_decomposition
    FOR EACH ROW EXECUTE FUNCTION ortak_work_decomposition_reserve();
CREATE TRIGGER work_decomposition_immutable BEFORE UPDATE OR DELETE ON work_decomposition
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER work_decomposition_no_truncate BEFORE TRUNCATE ON work_decomposition
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_work_decomposition_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM work_items parent JOIN work_items child
          ON child.company_id=parent.company_id AND child.project_id=parent.project_id
        JOIN work_item_history ph ON ph.company_id=parent.company_id AND ph.work_item_id=parent.id
          AND ph.version=NEW.parent_version AND ph.sequence=NEW.parent_version-1
        JOIN work_item_history ch ON ch.company_id=child.company_id AND ch.work_item_id=child.id
          AND ch.version=1 AND ch.sequence=0
        JOIN work_api_operations receipt ON receipt.company_id=NEW.company_id
          AND receipt.actor_pubkey=NEW.actor_pubkey AND receipt.operation_id=NEW.operation_id
        WHERE parent.company_id=NEW.company_id AND parent.project_id=NEW.project_id
          AND parent.id=NEW.parent_id AND parent.version=NEW.parent_version
          AND parent.state NOT IN('completed','cancelled')
          AND child.id=NEW.child_id AND child.version=1 AND child.state='proposed'
          AND child.source_message_id IS NULL AND child.source_routing_decision_id IS NULL
          AND child.created_by_type='human' AND child.created_by_id=NEW.actor_pubkey
          AND child.created_at=NEW.created_at
          AND ph.event_type='work.child_created' AND ph.actor_type='human' AND ph.actor_id=NEW.actor_pubkey
          AND ph.payload=jsonb_build_object('event','child_created','child_id',NEW.child_id)
          AND ch.event_type='work.created' AND ch.actor_type='human' AND ch.actor_id=NEW.actor_pubkey
          AND receipt.action='create_work_item' AND receipt.project_id=NEW.project_id
          AND receipt.work_item_id=NEW.child_id AND receipt.result_version=1
          AND (receipt.valid_before IS NULL OR receipt.valid_before>clock_timestamp())
    ) OR EXISTS(SELECT 1 FROM work_assignments WHERE company_id=NEW.company_id AND work_item_id=NEW.child_id)
      OR EXISTS(SELECT 1 FROM work_dependencies WHERE company_id=NEW.company_id AND work_item_id=NEW.child_id)
      OR EXISTS(SELECT 1 FROM work_attachments WHERE company_id=NEW.company_id AND work_item_id=NEW.child_id)
      OR EXISTS(SELECT 1 FROM work_acceptance_criteria WHERE company_id=NEW.company_id AND work_item_id=NEW.child_id AND status<>'pending')
      OR EXISTS(SELECT 1 FROM work_approvals WHERE company_id=NEW.company_id AND work_item_id=NEW.child_id AND status<>'pending') THEN
        RAISE EXCEPTION 'ortak: decomposition must commit independent creation and parent history atomically'
            USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER work_decomposition_at_commit AFTER INSERT ON work_decomposition
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_work_decomposition_commit();
