-- Ortak Milestone 6: Work and Projects foundation.
--
-- Architecture v0 §7 gives the company durable, assignable work: a Project is
-- the context/policy boundary for related work, a WorkItem is one unit of
-- work with acceptance criteria, approval gates, employee assignments,
-- same-project dependencies, attachments to canonical records, and an
-- append-only history. This migration is purely additive: it creates the
-- work relations, their guard functions and triggers, and the foreign key
-- that `runs.work_item_id` (reserved by 0045) was waiting for. No inherited
-- Buzz relation is altered, so a later migration can revert it with
-- `DROP TABLE ... CASCADE` plus the function drops.
--
-- Tenant contract (same as 0045). Every relation carries
-- `company_id NOT NULL REFERENCES companies(id)`, and every primary key,
-- unique constraint, unique index, and foreign key leads with `company_id`.
-- Dependencies additionally carry `project_id` in both foreign keys so a
-- dependency can only join two items of the same company *and* project.
--
-- Concurrency contract. `work_items.version` is the optimistic-concurrency
-- token: the application compares the caller's expected version under the
-- row lock, and the guard trigger refuses any update that does not advance
-- it by exactly one. `work_item_history.sequence` is dense from 0 and the
-- application writes it as `version - 1`, so one committed version equals
-- one committed history event and a reader can detect gaps.
--
-- Closed vocabularies are stored as snake_case TEXT matching the
-- `ortak_domain` work enums. Office message identifiers are the 32 raw
-- bytes of the signed event id, matching `office_inbox.event_id`.

-- ── Projects ─────────────────────────────────────────────────────────────────
-- Company work/context boundary. The slug is the stable, company-unique
-- machine name and the idempotency key of project creation; it never changes.

CREATE TABLE projects (
    company_id      UUID NOT NULL REFERENCES companies(id),
    id              UUID NOT NULL DEFAULT gen_random_uuid(),
    slug            TEXT NOT NULL CHECK (slug ~ '^[a-z0-9][a-z0-9_-]{0,63}$'),
    name            TEXT NOT NULL CHECK (btrim(name) <> '' AND octet_length(name) <= 200),
    description     TEXT NOT NULL DEFAULT '' CHECK (octet_length(description) <= 8192),
    status          TEXT NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'archived')),
    -- Optimistic-concurrency token; one per committed history event.
    version         BIGINT NOT NULL DEFAULT 1 CHECK (version >= 1),
    created_by_type TEXT NOT NULL CHECK (created_by_type IN ('human', 'employee', 'system')),
    created_by_id   TEXT CHECK (created_by_id IS NULL OR octet_length(created_by_id) <= 256),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    archived_at     TIMESTAMPTZ,
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, slug),
    CHECK ((created_by_type = 'system') = (created_by_id IS NULL)),
    CHECK ((status = 'archived') = (archived_at IS NOT NULL))
);

-- Identity, slug, and creation facts are pinned; the version advances by
-- exactly one per update; an archived project stays archived.
CREATE FUNCTION projects_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id <> OLD.company_id
       OR NEW.id <> OLD.id
       OR NEW.slug <> OLD.slug
       OR NEW.created_by_type <> OLD.created_by_type
       OR NEW.created_by_id IS DISTINCT FROM OLD.created_by_id
       OR NEW.created_at <> OLD.created_at
       OR NEW.version <> OLD.version + 1
       OR (OLD.status = 'archived' AND NEW.status <> 'archived')
    THEN
        RAISE EXCEPTION 'ortak: project % pins its identity and only advances', OLD.id
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_projects_guard
    BEFORE UPDATE ON projects
    FOR EACH ROW EXECUTE FUNCTION projects_guard();

CREATE TRIGGER trg_projects_no_delete
    BEFORE DELETE ON projects
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- Append-only project history; sequence is dense from 0 and equals
-- `version - 1` of the event that produced it.
CREATE TABLE project_history (
    company_id      UUID NOT NULL REFERENCES companies(id),
    project_id      UUID NOT NULL,
    sequence        BIGINT NOT NULL CHECK (sequence >= 0),
    event_type      TEXT NOT NULL CHECK (event_type ~ '^[a-z][a-z0-9_.]{0,63}$'),
    actor_type      TEXT NOT NULL CHECK (actor_type IN ('human', 'employee', 'system')),
    actor_id        TEXT CHECK (actor_id IS NULL OR octet_length(actor_id) <= 256),
    -- Bounded typed event payload (`ortak_domain::ProjectEvent` JSON).
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb
                    CHECK (jsonb_typeof(payload) = 'object' AND octet_length(payload::text) <= 8192),
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, project_id, sequence),
    FOREIGN KEY (company_id, project_id) REFERENCES projects (company_id, id),
    CHECK ((actor_type = 'system') = (actor_id IS NULL))
);

CREATE TRIGGER trg_project_history_immutable
    BEFORE UPDATE OR DELETE ON project_history
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- ── Work items ───────────────────────────────────────────────────────────────
-- One unit of company work. `source_message_id` is the decided Office
-- message the item was promoted from and the idempotency key of promotion;
-- it must be an inbox row of the same company.

CREATE TABLE work_items (
    company_id                  UUID NOT NULL REFERENCES companies(id),
    id                          UUID NOT NULL DEFAULT gen_random_uuid(),
    project_id                  UUID NOT NULL,
    title                       TEXT NOT NULL CHECK (btrim(title) <> '' AND octet_length(title) <= 200),
    description                 TEXT NOT NULL DEFAULT '' CHECK (octet_length(description) <= 8192),
    priority                    TEXT NOT NULL DEFAULT 'normal'
                                CHECK (priority IN ('low', 'normal', 'high', 'urgent')),
    -- `ortak_domain::WorkState`; completed and cancelled are terminal.
    state                       TEXT NOT NULL DEFAULT 'proposed'
                                CHECK (state IN ('proposed', 'ready', 'in_progress', 'blocked', 'review', 'completed', 'cancelled')),
    -- Optimistic-concurrency token; one per committed history event.
    version                     BIGINT NOT NULL DEFAULT 1 CHECK (version >= 1),
    source_message_id           BYTEA CHECK (source_message_id IS NULL OR octet_length(source_message_id) = 32),
    -- Dispatching decision of the source message, when one existed.
    source_routing_decision_id  UUID,
    created_by_type             TEXT NOT NULL CHECK (created_by_type IN ('human', 'employee', 'system')),
    created_by_id               TEXT CHECK (created_by_id IS NULL OR octet_length(created_by_id) <= 256),
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at                TIMESTAMPTZ,
    cancelled_at                TIMESTAMPTZ,
    PRIMARY KEY (company_id, id),
    -- Lets dependencies pin (project, item) in one foreign key.
    UNIQUE (company_id, project_id, id),
    FOREIGN KEY (company_id, project_id) REFERENCES projects (company_id, id),
    FOREIGN KEY (company_id, source_message_id) REFERENCES office_inbox (company_id, event_id),
    FOREIGN KEY (company_id, source_routing_decision_id) REFERENCES routing_decisions (company_id, id),
    CHECK (NOT (source_message_id IS NULL AND source_routing_decision_id IS NOT NULL)),
    CHECK ((created_by_type = 'system') = (created_by_id IS NULL)),
    CHECK ((state = 'completed') = (completed_at IS NOT NULL)),
    CHECK ((state = 'cancelled') = (cancelled_at IS NOT NULL))
);

-- One work item per promoted message: promotion is idempotent.
CREATE UNIQUE INDEX idx_work_items_source_message
    ON work_items (company_id, source_message_id)
    WHERE source_message_id IS NOT NULL;

-- Project work list, newest first, keyset on (created_at, id).
CREATE INDEX idx_work_items_project_created
    ON work_items (company_id, project_id, created_at DESC, id DESC);

-- Identity, project, source, and creation facts are pinned; the version
-- advances by exactly one per update; a terminal item never changes state.
CREATE FUNCTION work_items_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id <> OLD.company_id
       OR NEW.id <> OLD.id
       OR NEW.project_id <> OLD.project_id
       OR NEW.source_message_id IS DISTINCT FROM OLD.source_message_id
       OR NEW.source_routing_decision_id IS DISTINCT FROM OLD.source_routing_decision_id
       OR NEW.created_by_type <> OLD.created_by_type
       OR NEW.created_by_id IS DISTINCT FROM OLD.created_by_id
       OR NEW.created_at <> OLD.created_at
       OR NEW.version <> OLD.version + 1
       OR (OLD.state IN ('completed', 'cancelled') AND NEW.state <> OLD.state)
    THEN
        RAISE EXCEPTION 'ortak: work item % pins its identity and only advances', OLD.id
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_work_items_guard
    BEFORE UPDATE ON work_items
    FOR EACH ROW EXECUTE FUNCTION work_items_guard();

CREATE TRIGGER trg_work_items_no_delete
    BEFORE DELETE ON work_items
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- Runs attach to work (Architecture v0 §7); 0045 reserved the column.
ALTER TABLE runs
    ADD CONSTRAINT runs_work_item_fk
    FOREIGN KEY (company_id, work_item_id)
    REFERENCES work_items (company_id, id);

-- Append-only, dense work history. One row per committed version:
-- sequence = version - 1 of the event that produced it.
CREATE TABLE work_item_history (
    company_id      UUID NOT NULL REFERENCES companies(id),
    work_item_id    UUID NOT NULL,
    sequence        BIGINT NOT NULL CHECK (sequence >= 0),
    -- Version of the item after this event.
    version         BIGINT NOT NULL CHECK (version = sequence + 1),
    event_type      TEXT NOT NULL CHECK (event_type ~ '^[a-z][a-z0-9_.]{0,63}$'),
    actor_type      TEXT NOT NULL CHECK (actor_type IN ('human', 'employee', 'system')),
    actor_id        TEXT CHECK (actor_id IS NULL OR octet_length(actor_id) <= 256),
    from_state      TEXT CHECK (from_state IS NULL OR from_state IN ('proposed', 'ready', 'in_progress', 'blocked', 'review', 'completed', 'cancelled')),
    to_state        TEXT CHECK (to_state IS NULL OR to_state IN ('proposed', 'ready', 'in_progress', 'blocked', 'review', 'completed', 'cancelled')),
    -- Bounded typed event payload (`ortak_domain::WorkEvent` JSON); never a
    -- raw message, runtime output, or credential value.
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb
                    CHECK (jsonb_typeof(payload) = 'object' AND octet_length(payload::text) <= 8192),
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, work_item_id, sequence),
    FOREIGN KEY (company_id, work_item_id) REFERENCES work_items (company_id, id),
    CHECK ((actor_type = 'system') = (actor_id IS NULL)),
    CHECK ((from_state IS NULL) = (to_state IS NULL))
);

-- Sequences are appended without gaps, the shape run_events uses.
CREATE FUNCTION work_item_history_require_predecessor() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.sequence > 0 AND NOT EXISTS (
        SELECT 1
          FROM work_item_history previous
         WHERE previous.company_id = NEW.company_id
           AND previous.work_item_id = NEW.work_item_id
           AND previous.sequence = NEW.sequence - 1
    ) THEN
        RAISE EXCEPTION 'ortak: work item % history sequence % has no predecessor', NEW.work_item_id, NEW.sequence
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_work_item_history_require_predecessor
    BEFORE INSERT ON work_item_history
    FOR EACH ROW EXECUTE FUNCTION work_item_history_require_predecessor();

CREATE TRIGGER trg_work_item_history_immutable
    BEFORE UPDATE OR DELETE ON work_item_history
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- ── Assignments ──────────────────────────────────────────────────────────────
-- At most one assignment row per (item, employee); release keeps the row
-- and the history event records the change. Only existing employees of the
-- same company can be assigned; the application additionally requires them
-- to be `active`.

CREATE TABLE work_assignments (
    company_id          UUID NOT NULL REFERENCES companies(id),
    work_item_id        UUID NOT NULL,
    employee_id         TEXT NOT NULL,
    role                TEXT NOT NULL CHECK (role IN ('owner', 'contributor', 'reviewer')),
    status              TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'released')),
    assigned_by_type    TEXT NOT NULL CHECK (assigned_by_type IN ('human', 'employee', 'system')),
    assigned_by_id      TEXT CHECK (assigned_by_id IS NULL OR octet_length(assigned_by_id) <= 256),
    assigned_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    released_at         TIMESTAMPTZ,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, work_item_id, employee_id),
    FOREIGN KEY (company_id, work_item_id) REFERENCES work_items (company_id, id),
    FOREIGN KEY (company_id, employee_id) REFERENCES employees (company_id, id),
    CHECK ((assigned_by_type = 'system') = (assigned_by_id IS NULL)),
    CHECK ((status = 'released') = (released_at IS NOT NULL))
);

-- Employee work queue.
CREATE INDEX idx_work_assignments_employee_active
    ON work_assignments (company_id, employee_id)
    WHERE status = 'active';

CREATE TRIGGER trg_work_assignments_no_delete
    BEFORE DELETE ON work_assignments
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- ── Dependencies ─────────────────────────────────────────────────────────────
-- `blocked_by` edges between two items of the same company and project.
-- Self-dependency is refused by the check; cycles are refused by the
-- application under the project row lock. Edges are never edited.

CREATE TABLE work_dependencies (
    company_id                  UUID NOT NULL REFERENCES companies(id),
    project_id                  UUID NOT NULL,
    work_item_id                UUID NOT NULL,
    depends_on_work_item_id     UUID NOT NULL,
    kind                        TEXT NOT NULL DEFAULT 'blocked_by' CHECK (kind = 'blocked_by'),
    created_by_type             TEXT NOT NULL CHECK (created_by_type IN ('human', 'employee', 'system')),
    created_by_id               TEXT CHECK (created_by_id IS NULL OR octet_length(created_by_id) <= 256),
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, work_item_id, depends_on_work_item_id),
    FOREIGN KEY (company_id, project_id, work_item_id)
        REFERENCES work_items (company_id, project_id, id),
    FOREIGN KEY (company_id, project_id, depends_on_work_item_id)
        REFERENCES work_items (company_id, project_id, id),
    CHECK (work_item_id <> depends_on_work_item_id),
    CHECK ((created_by_type = 'system') = (created_by_id IS NULL))
);

-- Project graph load for the transactional cycle check.
CREATE INDEX idx_work_dependencies_project
    ON work_dependencies (company_id, project_id);

CREATE TRIGGER trg_work_dependencies_immutable
    BEFORE UPDATE OR DELETE ON work_dependencies
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- ── Acceptance criteria ──────────────────────────────────────────────────────
-- Text is pinned at creation; status only moves pending → satisfied.

CREATE TABLE work_acceptance_criteria (
    company_id          UUID NOT NULL REFERENCES companies(id),
    work_item_id        UUID NOT NULL,
    id                  UUID NOT NULL DEFAULT gen_random_uuid(),
    position            SMALLINT NOT NULL CHECK (position >= 0),
    text                TEXT NOT NULL CHECK (btrim(text) <> '' AND octet_length(text) <= 1024),
    status              TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'satisfied')),
    satisfied_by_type   TEXT CHECK (satisfied_by_type IS NULL OR satisfied_by_type IN ('human', 'employee', 'system')),
    satisfied_by_id     TEXT CHECK (satisfied_by_id IS NULL OR octet_length(satisfied_by_id) <= 256),
    satisfied_at        TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, work_item_id, id),
    UNIQUE (company_id, work_item_id, position),
    FOREIGN KEY (company_id, work_item_id) REFERENCES work_items (company_id, id),
    CHECK ((status = 'satisfied') = (satisfied_at IS NOT NULL)),
    CHECK ((status = 'satisfied') = (satisfied_by_type IS NOT NULL)),
    CHECK (NOT (satisfied_by_type = 'system' AND satisfied_by_id IS NOT NULL)),
    CHECK (NOT (satisfied_by_type IN ('human', 'employee') AND satisfied_by_id IS NULL))
);

CREATE FUNCTION work_acceptance_criteria_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id <> OLD.company_id
       OR NEW.work_item_id <> OLD.work_item_id
       OR NEW.id <> OLD.id
       OR NEW.position <> OLD.position
       OR NEW.text <> OLD.text
       OR NEW.created_at <> OLD.created_at
       OR (OLD.status = 'satisfied')
       OR NEW.status <> 'satisfied'
    THEN
        RAISE EXCEPTION 'ortak: acceptance criterion % pins its text and only moves pending -> satisfied', OLD.id
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_work_acceptance_criteria_guard
    BEFORE UPDATE ON work_acceptance_criteria
    FOR EACH ROW EXECUTE FUNCTION work_acceptance_criteria_guard();

CREATE TRIGGER trg_work_acceptance_criteria_no_delete
    BEFORE DELETE ON work_acceptance_criteria
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- ── Approval gates ───────────────────────────────────────────────────────────
-- Gate code and requirement are pinned; status only moves pending →
-- approved | rejected, once.

CREATE TABLE work_approvals (
    company_id          UUID NOT NULL REFERENCES companies(id),
    work_item_id        UUID NOT NULL,
    id                  UUID NOT NULL DEFAULT gen_random_uuid(),
    gate                TEXT NOT NULL CHECK (gate ~ '^[a-z][a-z0-9_]{0,63}$'),
    required            BOOLEAN NOT NULL DEFAULT true,
    status              TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected')),
    resolved_by_type    TEXT CHECK (resolved_by_type IS NULL OR resolved_by_type IN ('human', 'employee', 'system')),
    resolved_by_id      TEXT CHECK (resolved_by_id IS NULL OR octet_length(resolved_by_id) <= 256),
    reason              TEXT CHECK (reason IS NULL OR octet_length(reason) <= 1024),
    resolved_at         TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, work_item_id, id),
    UNIQUE (company_id, work_item_id, gate),
    FOREIGN KEY (company_id, work_item_id) REFERENCES work_items (company_id, id),
    CHECK ((status <> 'pending') = (resolved_at IS NOT NULL)),
    CHECK ((status <> 'pending') = (resolved_by_type IS NOT NULL)),
    CHECK (NOT (resolved_by_type = 'system' AND resolved_by_id IS NOT NULL)),
    CHECK (NOT (resolved_by_type IN ('human', 'employee') AND resolved_by_id IS NULL))
);

CREATE FUNCTION work_approvals_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id <> OLD.company_id
       OR NEW.work_item_id <> OLD.work_item_id
       OR NEW.id <> OLD.id
       OR NEW.gate <> OLD.gate
       OR NEW.required <> OLD.required
       OR NEW.created_at <> OLD.created_at
       OR (OLD.status <> 'pending')
       OR NEW.status = 'pending'
    THEN
        RAISE EXCEPTION 'ortak: approval gate % pins its definition and resolves once', OLD.id
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_work_approvals_guard
    BEFORE UPDATE ON work_approvals
    FOR EACH ROW EXECUTE FUNCTION work_approvals_guard();

CREATE TRIGGER trg_work_approvals_no_delete
    BEFORE DELETE ON work_approvals
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- ── Attachments ──────────────────────────────────────────────────────────────
-- References to canonical company records that exist today: a decided
-- Office message (office_inbox), a routing decision, or a run. Exactly one
-- reference column is set and it matches `kind`; artifacts join here once
-- the artifacts relation exists. Attachments are never edited or removed.

CREATE TABLE work_attachments (
    company_id          UUID NOT NULL REFERENCES companies(id),
    work_item_id        UUID NOT NULL,
    id                  UUID NOT NULL DEFAULT gen_random_uuid(),
    kind                TEXT NOT NULL CHECK (kind IN ('office_message', 'routing_decision', 'run')),
    message_id          BYTEA CHECK (message_id IS NULL OR octet_length(message_id) = 32),
    routing_decision_id UUID,
    run_id              UUID,
    label               TEXT CHECK (label IS NULL OR (btrim(label) <> '' AND octet_length(label) <= 256)),
    attached_by_type    TEXT NOT NULL CHECK (attached_by_type IN ('human', 'employee', 'system')),
    attached_by_id      TEXT CHECK (attached_by_id IS NULL OR octet_length(attached_by_id) <= 256),
    attached_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, work_item_id, id),
    FOREIGN KEY (company_id, work_item_id) REFERENCES work_items (company_id, id),
    FOREIGN KEY (company_id, message_id) REFERENCES office_inbox (company_id, event_id),
    FOREIGN KEY (company_id, routing_decision_id) REFERENCES routing_decisions (company_id, id),
    FOREIGN KEY (company_id, run_id) REFERENCES runs (company_id, id),
    CHECK ((kind = 'office_message') = (message_id IS NOT NULL)),
    CHECK ((kind = 'routing_decision') = (routing_decision_id IS NOT NULL)),
    CHECK ((kind = 'run') = (run_id IS NOT NULL)),
    CHECK ((attached_by_type = 'system') = (attached_by_id IS NULL))
);

-- One attachment per referenced record per item.
CREATE UNIQUE INDEX idx_work_attachments_message
    ON work_attachments (company_id, work_item_id, message_id)
    WHERE message_id IS NOT NULL;
CREATE UNIQUE INDEX idx_work_attachments_decision
    ON work_attachments (company_id, work_item_id, routing_decision_id)
    WHERE routing_decision_id IS NOT NULL;
CREATE UNIQUE INDEX idx_work_attachments_run
    ON work_attachments (company_id, work_item_id, run_id)
    WHERE run_id IS NOT NULL;

CREATE TRIGGER trg_work_attachments_immutable
    BEFORE UPDATE OR DELETE ON work_attachments
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
