-- Reviewed project context is retained evidence, not a Honcho erasure receipt.
CREATE FUNCTION ortak_reviewed_fact_source_visible(
    company UUID, project UUID, employee TEXT, message BYTEA, artifact UUID,
    community UUID, channel UUID
) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT (message IS NOT NULL AND EXISTS(
        SELECT 1 FROM office_inbox i
        JOIN events e ON e.community_id=community AND e.id=i.event_id AND e.created_at=i.event_created_at
            AND e.channel_id=i.channel_id AND e.kind=i.event_kind AND e.pubkey=i.author_pubkey
        WHERE i.company_id=company AND i.event_id=message AND i.channel_id=channel
            AND i.state='decided' AND e.kind IN(9,40002) AND e.deleted_at IS NULL))
    OR (artifact IS NOT NULL AND EXISTS(
        SELECT 1 FROM artifacts a
        JOIN work_items w ON w.company_id=a.company_id AND w.id=a.work_item_id AND w.project_id=a.project_id
        WHERE a.company_id=company AND a.id=artifact AND a.project_id=project AND a.employee_id=employee
            AND (w.source_message_id IS NULL OR EXISTS(
                SELECT 1 FROM office_inbox i
                JOIN events e ON e.community_id=community AND e.id=i.event_id AND e.created_at=i.event_created_at
                    AND e.channel_id=i.channel_id AND e.kind=i.event_kind AND e.pubkey=i.author_pubkey
                WHERE i.company_id=company AND i.event_id=w.source_message_id AND i.channel_id=channel
                    AND i.state='decided' AND e.kind IN(9,40002) AND e.deleted_at IS NULL))))
$$;

CREATE TABLE reviewed_memory_facts (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    id UUID NOT NULL CHECK(id<>'00000000-0000-0000-0000-000000000000'),
    project_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    source_message_id BYTEA,
    source_artifact_id UUID,
    content TEXT NOT NULL CHECK(octet_length(content) BETWEEN 1 AND 4096 AND btrim(content)<>''
        AND regexp_replace(content,E'[\n\t]','','g') !~ '[[:cntrl:]]'),
    version BIGINT NOT NULL DEFAULT 1 CHECK(version IN(1,2)),
    approved_by TEXT NOT NULL CHECK(approved_by ~ '^[0-9a-f]{64}$'),
    approved_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL CHECK(expires_at>approved_at AND expires_at<=approved_at+INTERVAL '90 days'),
    promotion_operation_id UUID NOT NULL,
    revoked_by TEXT CHECK(revoked_by ~ '^[0-9a-f]{64}$'),
    revoked_at TIMESTAMPTZ,
    revoke_reason TEXT CHECK(octet_length(revoke_reason) BETWEEN 1 AND 512 AND btrim(revoke_reason)<>'' AND revoke_reason !~ '[[:cntrl:]]'),
    revocation_operation_id UUID,
    PRIMARY KEY(company_id,id),
    FOREIGN KEY(company_id,project_id) REFERENCES projects(company_id,id),
    FOREIGN KEY(company_id,employee_id) REFERENCES employees(company_id,id),
    FOREIGN KEY(company_id,source_message_id) REFERENCES office_inbox(company_id,event_id),
    FOREIGN KEY(company_id,source_artifact_id) REFERENCES artifacts(company_id,id),
    CHECK((source_message_id IS NULL)<>(source_artifact_id IS NULL)),
    CHECK((version=1 AND revoked_by IS NULL AND revoked_at IS NULL AND revoke_reason IS NULL AND revocation_operation_id IS NULL)
        OR (version=2 AND revoked_by IS NOT NULL AND revoked_at IS NOT NULL AND revoked_at>=approved_at AND revoke_reason IS NOT NULL AND revocation_operation_id IS NOT NULL))
);
CREATE INDEX idx_reviewed_memory_scope ON reviewed_memory_facts(company_id,project_id,employee_id,id);
CREATE INDEX idx_reviewed_memory_recall ON reviewed_memory_facts(company_id,project_id,employee_id,approved_at DESC,id DESC)
    WHERE revoked_at IS NULL;

CREATE TABLE reviewed_memory_operations (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    actor_pubkey TEXT NOT NULL CHECK(actor_pubkey ~ '^[0-9a-f]{64}$'),
    operation_id UUID NOT NULL CHECK(operation_id<>'00000000-0000-0000-0000-000000000000'),
    action TEXT NOT NULL CHECK(action IN('promote','revoke')),
    request_hash BYTEA NOT NULL CHECK(octet_length(request_hash)=32),
    fact_id UUID NOT NULL,
    project_id UUID NOT NULL,
    result_version BIGINT NOT NULL CHECK((action='promote' AND result_version=1) OR (action='revoke' AND result_version=2)),
    auth_event_id BYTEA NOT NULL CHECK(octet_length(auth_event_id)=32),
    valid_before TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,actor_pubkey,operation_id),
    UNIQUE(company_id,fact_id,action),
    FOREIGN KEY(company_id,fact_id) REFERENCES reviewed_memory_facts(company_id,id),
    FOREIGN KEY(company_id,project_id) REFERENCES projects(company_id,id)
);
-- Retained evidence references durable projects and the permanent community
-- tombstone, as in migration 0061. An approved purge severs transient Office/API
-- bindings without erasing facts or receipts. Current bindings remain mandatory
-- at promotion and on every authorized read/replay; community write fences also
-- reject new retained evidence after quiescing, including executor-GUC writes.
ALTER TABLE reviewed_memory_facts
    ADD CONSTRAINT reviewed_memory_promotion_receipt FOREIGN KEY(company_id,approved_by,promotion_operation_id)
        REFERENCES reviewed_memory_operations(company_id,actor_pubkey,operation_id) DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT reviewed_memory_revocation_receipt FOREIGN KEY(company_id,revoked_by,revocation_operation_id)
        REFERENCES reviewed_memory_operations(company_id,actor_pubkey,operation_id) DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION ortak_reviewed_fact_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE channel UUID;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF OLD.version<>1 OR NEW.version<>2
            OR (to_jsonb(NEW)-'version'-'revoked_by'-'revoked_at'-'revoke_reason'-'revocation_operation_id') IS DISTINCT FROM
               (to_jsonb(OLD)-'version'-'revoked_by'-'revoked_at'-'revoke_reason'-'revocation_operation_id') THEN
            RAISE EXCEPTION 'ortak: reviewed fact only permits one retained revocation' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        RETURN NEW;
    END IF;
    PERFORM ortak_lock_office_authority(NEW.company_id);
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=NEW.project_id FOR SHARE;
    SELECT b.channel_id INTO channel FROM project_api_bindings b
        JOIN projects p ON p.company_id=b.company_id AND p.id=b.project_id
        JOIN employees e ON e.company_id=b.company_id AND e.id=NEW.employee_id
        WHERE b.company_id=NEW.company_id AND b.project_id=NEW.project_id AND b.community_id=NEW.community_id
            AND p.status='active' AND e.status='active';
    IF channel IS NULL OR NEW.version<>1 OR NEW.approved_at>clock_timestamp() OR NEW.expires_at<=clock_timestamp()
        OR NOT ortak_reviewed_fact_source_visible(NEW.company_id,NEW.project_id,NEW.employee_id,
            NEW.source_message_id,NEW.source_artifact_id,NEW.community_id,channel) THEN
        RAISE EXCEPTION 'ortak: reviewed fact requires current scoped evidence and approval' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    -- At most 1024 retained approvals per exact audience; revoked evidence counts.
    PERFORM pg_advisory_xact_lock(hashtextextended(format('ortak-reviewed-memory-scope:%s:%s:%s',
        NEW.company_id,NEW.project_id,NEW.employee_id),0));
    IF (SELECT count(*) FROM reviewed_memory_facts WHERE company_id=NEW.company_id AND project_id=NEW.project_id
        AND employee_id=NEW.employee_id)>=1024 THEN
        RAISE EXCEPTION 'ortak: reviewed memory scope is full' USING ERRCODE='program_limit_exceeded';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER reviewed_fact_guard BEFORE INSERT OR UPDATE ON reviewed_memory_facts
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_fact_guard();
CREATE TRIGGER reviewed_fact_no_delete BEFORE DELETE ON reviewed_memory_facts
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER reviewed_fact_no_truncate BEFORE TRUNCATE ON reviewed_memory_facts
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER reviewed_memory_operation_immutable BEFORE UPDATE OR DELETE ON reviewed_memory_operations
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER reviewed_memory_operation_no_truncate BEFORE TRUNCATE ON reviewed_memory_operations
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
SELECT attach_community_write_fence('reviewed_memory_facts');
SELECT attach_community_write_fence('reviewed_memory_operations');

CREATE FUNCTION ortak_reviewed_fact_receipt_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE actor TEXT; operation UUID; expected_action TEXT;
BEGIN
    actor:=CASE WHEN TG_OP='INSERT' THEN NEW.approved_by ELSE NEW.revoked_by END;
    operation:=CASE WHEN TG_OP='INSERT' THEN NEW.promotion_operation_id ELSE NEW.revocation_operation_id END;
    expected_action:=CASE WHEN TG_OP='INSERT' THEN 'promote' ELSE 'revoke' END;
    IF NOT EXISTS(SELECT 1 FROM reviewed_memory_operations o
        WHERE o.company_id=NEW.company_id AND o.community_id=NEW.community_id AND o.actor_pubkey=actor
            AND o.operation_id=operation AND o.action=expected_action AND o.fact_id=NEW.id
            AND o.project_id=NEW.project_id AND o.result_version=NEW.version
            AND o.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed fact transition requires an atomic receipt' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER reviewed_fact_receipt_at_commit AFTER INSERT OR UPDATE ON reviewed_memory_facts
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_fact_receipt_at_commit();

CREATE FUNCTION ortak_reviewed_memory_operation_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.valid_before IS NOT NULL AND clock_timestamp()>=NEW.valid_before THEN
        RAISE EXCEPTION 'ortak: reviewed memory authority expired before commit' USING ERRCODE='serialization_failure';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM reviewed_memory_facts f WHERE f.company_id=NEW.company_id
        AND f.community_id=NEW.community_id AND f.id=NEW.fact_id AND f.project_id=NEW.project_id
        AND f.xmin::text::bigint=txid_current()%4294967296
        AND ((NEW.action='promote' AND f.approved_by=NEW.actor_pubkey AND f.promotion_operation_id=NEW.operation_id)
            OR (NEW.action='revoke' AND f.revoked_by=NEW.actor_pubkey AND f.revocation_operation_id=NEW.operation_id))) THEN
        RAISE EXCEPTION 'ortak: reviewed memory receipt requires its atomic fact transition' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER reviewed_memory_operation_at_commit AFTER INSERT ON reviewed_memory_operations
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_memory_operation_at_commit();
