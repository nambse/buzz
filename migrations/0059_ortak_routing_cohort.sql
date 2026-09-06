-- Server-owned routing selection and bounded durable stored-event reconciliation.
-- No row means central routing is off. Capture records accepted input while
-- dispatch waits for one bounded reconciliation receipt per selected channel.
CREATE TABLE office_routing_cohorts (
    company_id UUID PRIMARY KEY REFERENCES companies(id),
    community_id UUID NOT NULL,
    state TEXT NOT NULL DEFAULT 'off' CHECK (state IN ('off','capture','enabled')),
    capture_id UUID NOT NULL DEFAULT gen_random_uuid(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (company_id, community_id),
    FOREIGN KEY (company_id, community_id)
        REFERENCES office_company_bindings(company_id, community_id)
);

CREATE TABLE office_routing_channels (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    PRIMARY KEY (company_id, channel_id),
    FOREIGN KEY (company_id, community_id)
        REFERENCES office_routing_cohorts(company_id, community_id) ON DELETE CASCADE,
    FOREIGN KEY (community_id, channel_id) REFERENCES channels(community_id, id)
);

CREATE TABLE office_routing_employees (
    company_id UUID NOT NULL REFERENCES office_routing_cohorts(company_id) ON DELETE CASCADE,
    employee_id TEXT NOT NULL,
    PRIMARY KEY (company_id, employee_id),
    FOREIGN KEY (company_id, employee_id) REFERENCES employees(company_id, id)
);

CREATE TABLE office_inbox_reconciliations (
    company_id UUID NOT NULL REFERENCES companies(id),
    capture_id UUID NOT NULL,
    community_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    upper_created_at TIMESTAMPTZ,
    upper_event_id BYTEA CHECK (octet_length(upper_event_id)=32),
    cursor_created_at TIMESTAMPTZ,
    cursor_event_id BYTEA CHECK (octet_length(cursor_event_id)=32),
    scanned BIGINT NOT NULL DEFAULT 0 CHECK (scanned>=0),
    inserted BIGINT NOT NULL DEFAULT 0 CHECK (inserted>=0 AND inserted<=scanned),
    started_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    completed_at TIMESTAMPTZ,
    PRIMARY KEY (company_id, capture_id, channel_id),
    FOREIGN KEY (company_id, community_id) REFERENCES office_company_bindings(company_id, community_id),
    FOREIGN KEY (community_id, channel_id) REFERENCES channels(community_id, id),
    CHECK ((upper_created_at IS NULL)=(upper_event_id IS NULL)),
    CHECK ((cursor_created_at IS NULL)=(cursor_event_id IS NULL)),
    CHECK (cursor_created_at IS NULL OR (upper_created_at IS NOT NULL AND
           (cursor_created_at,cursor_event_id)<=(upper_created_at,upper_event_id))),
    CHECK (upper_created_at IS NOT NULL OR completed_at IS NOT NULL)
);

-- Matches both bounded forward pages and the reverse maximum-key lookup.
CREATE INDEX idx_events_ortak_reconciliation
ON events(community_id,channel_id,created_at,id)
WHERE kind IN (9,40002) AND deleted_at IS NULL;

CREATE FUNCTION ortak_guard_routing_cohort_state() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' AND
       ROW(NEW.company_id,NEW.community_id) IS DISTINCT FROM ROW(OLD.company_id,OLD.community_id) THEN
        RAISE EXCEPTION 'central routing cohort identity is immutable' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='UPDATE' AND NEW.state IN ('off','capture') AND NEW.state<>OLD.state THEN
        NEW.capture_id:=gen_random_uuid();
    END IF;
    IF NEW.state='enabled' AND (TG_OP='INSERT' OR OLD.state<>'enabled') THEN
        IF TG_OP='INSERT' OR OLD.state<>'capture' OR NEW.capture_id<>OLD.capture_id
           OR NOT EXISTS (SELECT 1 FROM office_routing_channels s WHERE s.company_id=NEW.company_id)
           OR NOT EXISTS (SELECT 1 FROM office_routing_employees s WHERE s.company_id=NEW.company_id)
           OR EXISTS (
               SELECT 1 FROM office_routing_channels s
               WHERE s.company_id=NEW.company_id AND NOT EXISTS (
                   SELECT 1 FROM office_inbox_reconciliations r
                   WHERE r.company_id=s.company_id AND r.community_id=s.community_id
                     AND r.channel_id=s.channel_id AND r.capture_id=NEW.capture_id
                     AND r.completed_at IS NOT NULL)) THEN
            RAISE EXCEPTION 'central routing capture requires completed current reconciliation'
                USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
    ELSIF TG_OP='UPDATE' AND NEW.state='enabled' AND NEW.capture_id<>OLD.capture_id THEN
        RAISE EXCEPTION 'central routing capture cannot change while enabled' USING ERRCODE='check_violation';
    END IF;
    NEW.updated_at:=clock_timestamp();
    RETURN NEW;
END
$$;

CREATE FUNCTION ortak_invalidate_routing_capture() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE target UUID;
BEGIN
    IF TG_OP='UPDATE' AND to_jsonb(OLD)=to_jsonb(NEW) THEN RETURN NEW; END IF;
    FOR target IN SELECT DISTINCT value FROM (VALUES
        (CASE WHEN TG_OP<>'INSERT' THEN OLD.company_id END),
        (CASE WHEN TG_OP<>'DELETE' THEN NEW.company_id END)) AS scopes(value)
        WHERE value IS NOT NULL ORDER BY value
    LOOP
        UPDATE office_routing_cohorts SET capture_id=gen_random_uuid(),
            state=CASE WHEN state='off' THEN 'off' ELSE 'capture' END
        WHERE company_id=target;
    END LOOP;
    RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
END
$$;

CREATE TRIGGER ortak_office_authority_routing_cohorts BEFORE INSERT OR UPDATE OR DELETE ON office_routing_cohorts
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company','company_id','community_id','state','capture_id');
CREATE TRIGGER ortak_office_authority_routing_channels BEFORE INSERT OR UPDATE OR DELETE ON office_routing_channels
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company','company_id','community_id','channel_id');
CREATE TRIGGER ortak_office_authority_routing_employees BEFORE INSERT OR UPDATE OR DELETE ON office_routing_employees
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company','company_id','employee_id');
CREATE TRIGGER ortak_routing_cohort_state BEFORE INSERT OR UPDATE ON office_routing_cohorts
FOR EACH ROW EXECUTE FUNCTION ortak_guard_routing_cohort_state();
CREATE TRIGGER ortak_routing_channels_capture AFTER INSERT OR UPDATE OR DELETE ON office_routing_channels
FOR EACH ROW EXECUTE FUNCTION ortak_invalidate_routing_capture();
CREATE TRIGGER ortak_routing_employees_capture AFTER INSERT OR UPDATE OR DELETE ON office_routing_employees
FOR EACH ROW EXECUTE FUNCTION ortak_invalidate_routing_capture();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON office_routing_cohorts
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON office_routing_channels
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON office_routing_employees
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON office_inbox_reconciliations
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

-- These tables participate in the inherited community deletion write fence.
SELECT attach_community_write_fence('office_routing_cohorts');
SELECT attach_community_write_fence('office_routing_channels');
SELECT attach_community_write_fence('office_inbox_reconciliations');

CREATE FUNCTION ortak_guard_inbox_reconciliation() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE canonical_upper RECORD; page_count BIGINT; missing_count BIGINT;
BEGIN
    IF TG_OP='DELETE' THEN
        RAISE EXCEPTION 'central routing reconciliation evidence is retained'
            USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='UPDATE' AND OLD.completed_at IS NOT NULL THEN
        IF to_jsonb(NEW) IS DISTINCT FROM to_jsonb(OLD) THEN
            RAISE EXCEPTION 'completed reconciliation evidence is immutable'
                USING ERRCODE='check_violation';
        END IF;
        RETURN NEW;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM office_routing_cohorts c JOIN office_routing_channels s
          ON s.company_id=c.company_id AND s.community_id=c.community_id
        WHERE c.company_id=NEW.company_id AND c.community_id=NEW.community_id
          AND c.capture_id=NEW.capture_id AND c.state IN ('capture','enabled')
          AND s.channel_id=NEW.channel_id
    ) THEN
        RAISE EXCEPTION 'reconciliation requires the current selected capture'
            USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='INSERT' THEN
        SELECT e.created_at,e.id INTO canonical_upper FROM events e
        WHERE e.community_id=NEW.community_id AND e.channel_id=NEW.channel_id
          AND e.kind IN (9,40002) AND e.deleted_at IS NULL
        ORDER BY e.created_at DESC,e.id DESC LIMIT 1;
        IF NEW.scanned<>0 OR NEW.inserted<>0 OR NEW.cursor_created_at IS NOT NULL
           OR NEW.cursor_event_id IS NOT NULL
           OR ROW(NEW.upper_created_at,NEW.upper_event_id)
              IS DISTINCT FROM ROW(canonical_upper.created_at,canonical_upper.id)
           OR ((NEW.completed_at IS NOT NULL)<>(canonical_upper.id IS NULL)) THEN
            RAISE EXCEPTION 'reconciliation must pin the canonical unscanned window'
                USING ERRCODE='check_violation';
        END IF;
        RETURN NEW;
    END IF;
    IF ROW(NEW.company_id,NEW.capture_id,NEW.community_id,NEW.channel_id,
           NEW.upper_created_at,NEW.upper_event_id,NEW.started_at)
       IS DISTINCT FROM ROW(OLD.company_id,OLD.capture_id,OLD.community_id,OLD.channel_id,
           OLD.upper_created_at,OLD.upper_event_id,OLD.started_at)
       OR NEW.scanned<OLD.scanned OR NEW.scanned-OLD.scanned>256
       OR NEW.inserted<OLD.inserted OR NEW.inserted-OLD.inserted>NEW.scanned-OLD.scanned
       OR (OLD.cursor_created_at IS NOT NULL AND (NEW.cursor_created_at IS NULL OR
           (NEW.cursor_created_at,NEW.cursor_event_id)<(OLD.cursor_created_at,OLD.cursor_event_id))) THEN
        RAISE EXCEPTION 'reconciliation window is immutable and progress must be bounded and monotonic'
            USING ERRCODE='check_violation';
    END IF;
    -- A caller cannot stamp a cursor/completion without the corresponding
    -- canonical inbox facts. The LIMIT bounds even a forged oversized cursor.
    SELECT count(*),count(*) FILTER (WHERE NOT EXISTS (
        SELECT 1 FROM office_inbox i WHERE i.company_id=NEW.company_id AND i.event_id=p.id
          AND i.event_created_at=p.created_at AND i.event_kind=p.kind
          AND i.author_pubkey=p.pubkey AND i.channel_id=NEW.channel_id
    )) INTO page_count,missing_count FROM (
        SELECT e.id,e.created_at,e.kind,e.pubkey FROM events e
        WHERE e.community_id=NEW.community_id AND e.channel_id=NEW.channel_id
          AND e.kind IN (9,40002) AND e.deleted_at IS NULL
          AND (OLD.cursor_created_at IS NULL OR
               (e.created_at,e.id)>(OLD.cursor_created_at,OLD.cursor_event_id))
          AND (e.created_at,e.id)<=(NEW.cursor_created_at,NEW.cursor_event_id)
        ORDER BY e.created_at,e.id LIMIT 257
    ) p;
    IF page_count<>NEW.scanned-OLD.scanned OR missing_count<>0 THEN
        RAISE EXCEPTION 'reconciliation progress requires exact canonical inbox facts'
            USING ERRCODE='check_violation';
    END IF;
    IF NEW.completed_at IS NOT NULL AND EXISTS (
        SELECT 1 FROM events e WHERE e.community_id=NEW.community_id
          AND e.channel_id=NEW.channel_id AND e.kind IN (9,40002) AND e.deleted_at IS NULL
          AND (e.created_at,e.id)<=(NEW.upper_created_at,NEW.upper_event_id)
          AND (NEW.cursor_created_at IS NULL OR
               (e.created_at,e.id)>(NEW.cursor_created_at,NEW.cursor_event_id))
    ) THEN
        RAISE EXCEPTION 'reconciliation cannot complete before its pinned window'
            USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER ortak_inbox_reconciliation_evidence BEFORE INSERT OR UPDATE OR DELETE
ON office_inbox_reconciliations FOR EACH ROW EXECUTE FUNCTION ortak_guard_inbox_reconciliation();
