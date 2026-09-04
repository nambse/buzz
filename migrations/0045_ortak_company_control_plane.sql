-- Ortak Milestone 1: durable company control plane.
--
-- Architecture v0 (docs/ortak/ARCHITECTURE_V0.md §5) makes PostgreSQL the
-- source of truth for the company boundary, employee identity and immutable
-- revisions, adapter bindings, the Office inbox handoff, routing decisions,
-- delivery-chain authority, runs and ordered run events, provisioning
-- operations, and the transactional dispatch outbox. This migration is purely
-- additive: it creates new relations, functions, and triggers, registers the
-- company registry in the operator-global allowlist, and attaches the
-- community write fence to the one bridge table that carries community_id.
-- No inherited Buzz relation is altered, so a later migration can revert it
-- with `DROP TABLE ... CASCADE` plus the function drops, the shape 0044 used.
--
-- Tenant contract. Ortak records are scoped by `company_id`, never by the
-- Buzz `community_id`. Every company-scoped table carries
-- `company_id NOT NULL REFERENCES companies(id)`, and every primary key,
-- unique constraint, unique index, and foreign key leads with `company_id`,
-- so no key is observable across companies and every join carries the
-- company tuple. The only bridge between the two tenant keys is
-- `office_company_bindings`, which the server resolves from the
-- authenticated host's community. A client-supplied company identifier is
-- never an authority and is never copied into these rows.
--
-- Closed vocabularies are stored as snake_case TEXT matching the serde
-- representation of the `ortak-domain` enums, so audit rows read the same in
-- SQL and in Rust. Office message identifiers are the 32 raw bytes of the
-- signed event id, matching `events.id`.

-- ── Shared guards ────────────────────────────────────────────────────────────

-- Append-only audit relations reject every UPDATE and DELETE.
CREATE FUNCTION ortak_reject_row_mutation() RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'ortak: % rows are immutable (% rejected)', TG_TABLE_NAME, TG_OP
        USING ERRCODE = 'object_not_in_prerequisite_state';
END;
$$ LANGUAGE plpgsql;

-- Opaque credential-manager references are the only credential-shaped values
-- allowed in ordinary rows (Architecture v0 invariant 10). The grammar mirrors
-- `ortak_domain::CredentialRef::parse`: a `credential://` or `secret://`
-- locator of at most 512 characters, no empty or dot path segments.
CREATE FUNCTION ortak_is_credential_ref(value TEXT) RETURNS BOOLEAN
LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT value ~ '^(credential|secret)://[A-Za-z0-9._:@#-]+(/[A-Za-z0-9._:@#-]+)*$'
       AND value !~ '(://|/)\.\.?(/|$)'
       AND length(split_part(value, '://', 2)) <= 512
$$;

-- True when `refs` is a JSON array whose every element is a credential
-- reference string.
CREATE FUNCTION ortak_all_credential_refs(refs JSONB) RETURNS BOOLEAN
LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT jsonb_typeof(refs) = 'array'
       AND NOT EXISTS (
           SELECT 1
             FROM jsonb_array_elements(refs) AS element
            WHERE jsonb_typeof(element) <> 'string'
               OR NOT ortak_is_credential_ref(element #>> '{}')
       )
$$;

-- ── Companies ────────────────────────────────────────────────────────────────
-- The company boundary. v0 deploys one row, but every Ortak record carries
-- company_id from the start. Not community-scoped: the Buzz community is
-- joined only through office_company_bindings.

CREATE TABLE companies (
    id              UUID NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Stable machine name used by fixtures, manifests, and operator tooling.
    slug            TEXT NOT NULL CHECK (slug ~ '^[a-z0-9][a-z0-9_-]{0,63}$'),
    display_name    TEXT NOT NULL CHECK (btrim(display_name) <> ''),
    status          TEXT NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'suspended')),
    -- Company-wide routing policy (`ortak_domain::RoutingPolicy` JSON). The
    -- domain validator owns the value ranges; decisions and delivery chains
    -- pin the version/fingerprint they were evaluated against.
    routing_policy  JSONB NOT NULL DEFAULT '{}'::jsonb
                    CHECK (jsonb_typeof(routing_policy) = 'object'),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_companies_id_not_nil CHECK (id <> '00000000-0000-0000-0000-000000000000'::uuid)
);

CREATE UNIQUE INDEX idx_companies_slug ON companies (slug);

INSERT INTO _operator_global_tables (table_name, reason) VALUES
    ('companies', 'Ortak company registry; the company boundary itself, joined to a Buzz community only through office_company_bindings');

-- ── Office ↔ company binding ─────────────────────────────────────────────────
-- Unique, server-owned mapping from an authenticated Buzz community (resolved
-- from the connection host) to exactly one Ortak company. Unknown mappings
-- fail closed; the relay resolves company_id here and never from a client.
-- This is the only Ortak relation carrying community_id, so it joins the
-- community write fence and deletion catalog like any other scoped table.

CREATE TABLE office_company_bindings (
    community_id    UUID NOT NULL REFERENCES communities(id),
    company_id      UUID NOT NULL REFERENCES companies(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id),
    UNIQUE (company_id)
);

-- A binding is never re-pointed in place; remove and recreate under review.
CREATE TRIGGER trg_office_company_bindings_immutable
    BEFORE UPDATE ON office_company_bindings
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

SELECT attach_community_write_fence('office_company_bindings');

-- ── Employees ────────────────────────────────────────────────────────────────
-- Stable identity and lifecycle. The id is the human-readable
-- `ortak_domain::EmployeeId`; it survives runtime, model, profile, and key
-- rotation, and runs, visits, and outbox rows key on it.

CREATE TABLE employees (
    company_id          UUID NOT NULL REFERENCES companies(id),
    id                  TEXT NOT NULL CHECK (id ~ '^[a-z0-9][a-z0-9_-]{0,63}$'),
    status              TEXT NOT NULL DEFAULT 'draft'
                        CHECK (status IN ('draft', 'active', 'paused', 'disabled')),
    -- Revision currently in effect; NULL until provisioning activates one.
    -- The foreign key is added after employee_revisions exists and also pins
    -- the revision to this employee.
    active_revision_id  UUID,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, id),
    -- Written as NOT (... IS NULL) because pgschema drops OR ... IS NOT NULL.
    CHECK (NOT (status = 'active' AND active_revision_id IS NULL))
);

-- Immutable effective configuration. A revision is never edited; changing
-- an employee writes a new revision and moves employees.active_revision_id.
CREATE TABLE employee_revisions (
    company_id              UUID NOT NULL REFERENCES companies(id),
    id                      UUID NOT NULL DEFAULT gen_random_uuid(),
    employee_id             TEXT NOT NULL,
    -- Monotonic per employee: (company_id, employee_id, revision_number) is
    -- the human-facing identity of a revision, `id` the join key.
    revision_number         BIGINT NOT NULL CHECK (revision_number > 0),
    -- Secret-free definition: role/persona, responsibilities, domains,
    -- aliases, permission policy, and routing policy. Adapter bindings live
    -- in the binding tables and reference this revision.
    manifest                JSONB NOT NULL CHECK (jsonb_typeof(manifest) = 'object'),
    -- Canonical SHA-256 of the manifest, so a run can prove which
    -- configuration it used and identical content is detectable.
    manifest_fingerprint    BYTEA NOT NULL CHECK (octet_length(manifest_fingerprint) = 32),
    provisioning_mode       TEXT NOT NULL CHECK (provisioning_mode IN ('create', 'adopt')),
    created_by              TEXT,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, employee_id, revision_number),
    -- Lets dependants pin (employee, revision) in one foreign key.
    UNIQUE (company_id, employee_id, id),
    FOREIGN KEY (company_id, employee_id) REFERENCES employees (company_id, id)
);

CREATE TRIGGER trg_employee_revisions_immutable
    BEFORE UPDATE OR DELETE ON employee_revisions
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

ALTER TABLE employees
    ADD CONSTRAINT employees_active_revision_fk
    FOREIGN KEY (company_id, id, active_revision_id)
    REFERENCES employee_revisions (company_id, employee_id, id);

-- Runtime adapter binding for one revision (Hermes first). Adapter and
-- external profile reference only; no secrets.
CREATE TABLE employee_runtime_bindings (
    company_id          UUID NOT NULL REFERENCES companies(id),
    revision_id         UUID NOT NULL,
    employee_id         TEXT NOT NULL,
    adapter             TEXT NOT NULL CHECK (adapter ~ '^[a-z][a-z0-9_-]{0,63}$'),
    provisioning_mode   TEXT NOT NULL CHECK (provisioning_mode IN ('create', 'adopt')),
    -- External profile reference; required for adopted profiles.
    profile_ref         TEXT,
    model               TEXT NOT NULL CHECK (btrim(model) <> ''),
    workspace_ref       TEXT NOT NULL CHECK (btrim(workspace_ref) <> ''),
    -- JSON array of opaque credential references, never credential values.
    credential_refs     JSONB NOT NULL DEFAULT '[]'::jsonb
                        CHECK (ortak_all_credential_refs(credential_refs)),
    options             JSONB NOT NULL DEFAULT '{}'::jsonb
                        CHECK (jsonb_typeof(options) = 'object'),
    -- Set when the runtime adapter validated the binding (capabilities and
    -- health); activation requires it.
    validated_at        TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, revision_id),
    FOREIGN KEY (company_id, employee_id, revision_id)
        REFERENCES employee_revisions (company_id, employee_id, id),
    CHECK (NOT (provisioning_mode = 'adopt' AND profile_ref IS NULL))
);

CREATE INDEX idx_employee_runtime_bindings_employee
    ON employee_runtime_bindings (company_id, employee_id);

-- Memory adapter binding for one revision (Honcho first).
CREATE TABLE employee_memory_bindings (
    company_id          UUID NOT NULL REFERENCES companies(id),
    revision_id         UUID NOT NULL,
    employee_id         TEXT NOT NULL,
    adapter             TEXT NOT NULL CHECK (adapter ~ '^[a-z][a-z0-9_-]{0,63}$'),
    provisioning_mode   TEXT NOT NULL CHECK (provisioning_mode IN ('create', 'adopt')),
    -- Service-discovery or configuration reference, never a token.
    endpoint_ref        TEXT NOT NULL CHECK (btrim(endpoint_ref) <> ''),
    workspace           TEXT NOT NULL CHECK (btrim(workspace) <> ''),
    user_peer           TEXT NOT NULL CHECK (btrim(user_peer) <> ''),
    employee_peer       TEXT NOT NULL CHECK (btrim(employee_peer) <> ''),
    options             JSONB NOT NULL DEFAULT '{}'::jsonb
                        CHECK (jsonb_typeof(options) = 'object'),
    validated_at        TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, revision_id),
    FOREIGN KEY (company_id, employee_id, revision_id)
        REFERENCES employee_revisions (company_id, employee_id, id)
);

CREATE INDEX idx_employee_memory_bindings_employee
    ON employee_memory_bindings (company_id, employee_id);

-- Office signing identity. Public key plus an opaque signer reference; the
-- private key never exists in this database. Rotation inserts a new row,
-- closes the old validity window after an overlap, and keeps the old row so
-- historical signatures stay attributable.
CREATE TABLE employee_office_bindings (
    company_id              UUID NOT NULL REFERENCES companies(id),
    id                      UUID NOT NULL DEFAULT gen_random_uuid(),
    employee_id             TEXT NOT NULL,
    -- Revision that introduced this signing identity.
    revision_id             UUID NOT NULL,
    provisioning_mode       TEXT NOT NULL CHECK (provisioning_mode IN ('create', 'adopt')),
    public_key              BYTEA NOT NULL CHECK (octet_length(public_key) = 32),
    -- Credential-manager / KMS / remote-signer reference.
    signer_ref              TEXT NOT NULL CHECK (ortak_is_credential_ref(signer_ref)),
    home_channel_ref        TEXT,
    -- Validity window. NULL valid_until marks the employee's open-ended key.
    valid_from              TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_until             TIMESTAMPTZ,
    rotated_from_binding_id UUID,
    -- Set when the signer proved it produces `public_key`; activation
    -- requires it (Architecture v0 §4.7).
    verified_at             TIMESTAMPTZ,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, id),
    -- One employee per Office key, including retired keys, so a historical
    -- event resolves to exactly one employee.
    UNIQUE (company_id, public_key),
    FOREIGN KEY (company_id, employee_id, revision_id)
        REFERENCES employee_revisions (company_id, employee_id, id),
    FOREIGN KEY (company_id, rotated_from_binding_id)
        REFERENCES employee_office_bindings (company_id, id),
    CHECK (valid_until IS NULL OR valid_until > valid_from),
    CHECK (rotated_from_binding_id IS NULL OR rotated_from_binding_id <> id)
);

-- At most one open-ended signing identity per employee.
CREATE UNIQUE INDEX idx_employee_office_bindings_current
    ON employee_office_bindings (company_id, employee_id)
    WHERE valid_until IS NULL;

-- Company-unique normalized aliases used by deterministic routing. The
-- application writes the `ortak_domain::normalize_alias` form (trimmed, no
-- leading '@', NFKC lower-cased, single spaces); the checks reject shapes
-- normalization can never produce, and the primary key is the durable
-- backstop for the uniqueness validated when a revision is saved.
CREATE TABLE employee_aliases (
    company_id      UUID NOT NULL REFERENCES companies(id),
    alias           TEXT NOT NULL CHECK (
                        alias <> ''
                        AND octet_length(alias) <= 256
                        AND alias !~ '^@'
                        AND alias !~ '^\s'
                        AND alias !~ '\s$'
                        AND alias !~ '\s\s'
                    ),
    employee_id     TEXT NOT NULL,
    -- Revision whose saved definition validated this alias set.
    revision_id     UUID NOT NULL,
    -- Which employee field produced the alias.
    source          TEXT NOT NULL CHECK (source IN ('id', 'name', 'alias')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, alias),
    FOREIGN KEY (company_id, employee_id, revision_id)
        REFERENCES employee_revisions (company_id, employee_id, id)
);

CREATE INDEX idx_employee_aliases_employee
    ON employee_aliases (company_id, employee_id);

-- ── Office inbox ─────────────────────────────────────────────────────────────
-- One durable accepted-message handoff per company/event. The relay inserts
-- this row in the same transaction as the signed `events` row, resolving
-- company_id through office_company_bindings, before acknowledging the
-- sender (Architecture v0 invariant 8). Routing workers claim rows here;
-- semantic scoring runs outside any claim transaction.

CREATE TABLE office_inbox (
    company_id          UUID NOT NULL REFERENCES companies(id),
    -- Signed Office event id (32 raw bytes).
    event_id            BYTEA NOT NULL CHECK (octet_length(event_id) = 32),
    -- Partition key of the `events` row, kept so consumers join back to the
    -- signed event without a cross-partition scan. There is no foreign key:
    -- `events` is partitioned by created_at and keyed by community.
    event_created_at    TIMESTAMPTZ NOT NULL,
    event_kind          INT NOT NULL,
    author_pubkey       BYTEA NOT NULL CHECK (octet_length(author_pubkey) = 32),
    channel_id          UUID,
    -- pending → claimed → decided | dropped; failed is terminal after the
    -- bounded retries and stays visible for operator inspection.
    state               TEXT NOT NULL DEFAULT 'pending'
                        CHECK (state IN ('pending', 'claimed', 'decided', 'dropped', 'failed')),
    -- Monotonic claim fence. A worker records the generation it claimed and
    -- the authoritative routing commit re-checks it, so a stale or late
    -- worker cannot finalize (Architecture v0 §4.2).
    claim_generation    BIGINT NOT NULL DEFAULT 0 CHECK (claim_generation >= 0),
    claimed_by          TEXT,
    claim_expires_at    TIMESTAMPTZ,
    attempt_count       INT NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    retry_after         TIMESTAMPTZ,
    last_error          TEXT,
    received_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    finalized_at        TIMESTAMPTZ,
    PRIMARY KEY (company_id, event_id),
    CHECK (NOT (state = 'claimed' AND (claimed_by IS NULL OR claim_expires_at IS NULL))),
    CHECK (NOT (state IN ('decided', 'dropped', 'failed') AND finalized_at IS NULL))
);

CREATE INDEX idx_office_inbox_due
    ON office_inbox (company_id, retry_after, received_at)
    WHERE state IN ('pending', 'claimed');

-- ── Delivery chains ──────────────────────────────────────────────────────────
-- Authoritative loop-prevention state (Architecture v0 invariant 7). One row
-- per company/root message pins the limits and serializes the counters; a
-- routing commit that may wake an employee locks this row first. Pure
-- `DeliveryChain` snapshots are derived from these rows and never authorize
-- a dispatch.

CREATE TABLE delivery_chains (
    company_id          UUID NOT NULL REFERENCES companies(id),
    root_message_id     BYTEA NOT NULL CHECK (octet_length(root_message_id) = 32),
    -- Policy pinned when the root was first locked. Ceilings mirror
    -- ortak_domain::HARD_MAX_CHAIN_HOPS and HARD_MAX_CHAIN_WAKES.
    policy_version      TEXT NOT NULL CHECK (policy_version ~ '^[A-Za-z0-9._-]{1,64}$'),
    policy_fingerprint  TEXT NOT NULL CHECK (policy_fingerprint ~ '^sha256:[0-9a-f]{64}$'),
    max_hops            SMALLINT NOT NULL CHECK (max_hops BETWEEN 1 AND 8),
    max_wakes           INT NOT NULL CHECK (max_wakes BETWEEN 1 AND 64),
    -- Authoritative counters, advanced only inside the routing commit that
    -- holds this row's lock. hop_count counts committed dispatch batches
    -- that reserved at least one new wake (the initial human batch is hop
    -- 1); wake_count counts reserved employee visits.
    hop_count           SMALLINT NOT NULL DEFAULT 0
                        CHECK (hop_count >= 0 AND hop_count <= max_hops),
    wake_count          INT NOT NULL DEFAULT 0
                        CHECK (wake_count >= 0 AND wake_count <= max_wakes),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, root_message_id)
);

-- Pinned limits never change and counters only advance; chain state is never
-- reset by a client, a model, or a retry.
CREATE FUNCTION delivery_chains_advance_only() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id <> OLD.company_id
       OR NEW.root_message_id <> OLD.root_message_id
       OR NEW.policy_version <> OLD.policy_version
       OR NEW.policy_fingerprint <> OLD.policy_fingerprint
       OR NEW.max_hops <> OLD.max_hops
       OR NEW.max_wakes <> OLD.max_wakes
       OR NEW.hop_count < OLD.hop_count
       OR NEW.wake_count < OLD.wake_count
    THEN
        RAISE EXCEPTION 'ortak: delivery chain % pins its limits and its counters only advance',
            encode(OLD.root_message_id, 'hex')
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_delivery_chains_advance_only
    BEFORE UPDATE ON delivery_chains
    FOR EACH ROW EXECUTE FUNCTION delivery_chains_advance_only();

CREATE TRIGGER trg_delivery_chains_no_delete
    BEFORE DELETE ON delivery_chains
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- ── Routing decisions ────────────────────────────────────────────────────────
-- Exactly one dispatching decision per company/input message (invariant 2).
-- The row pins the policy, candidate revisions, scorer versions, and input
-- hash that produced it, so every wake is explainable (invariant 6).

CREATE TABLE routing_decisions (
    company_id              UUID NOT NULL REFERENCES companies(id),
    id                      UUID NOT NULL DEFAULT gen_random_uuid(),
    message_id              BYTEA NOT NULL CHECK (octet_length(message_id) = 32),
    root_message_id         BYTEA NOT NULL CHECK (octet_length(root_message_id) = 32),
    -- office_inbox.claim_generation fenced by the commit.
    inbox_claim_generation  BIGINT NOT NULL CHECK (inbox_claim_generation >= 0),
    origin_type             TEXT NOT NULL
                            CHECK (origin_type IN ('human', 'employee', 'integration', 'system')),
    origin_id               TEXT,
    mode                    TEXT NOT NULL CHECK (mode IN ('deterministic', 'semantic', 'silent')),
    -- `ortak_domain::RoutingReason` in snake_case.
    summary_reason          TEXT NOT NULL CHECK (summary_reason ~ '^[a-z][a-z0-9_]{0,63}$'),
    policy_version          TEXT NOT NULL CHECK (policy_version ~ '^[A-Za-z0-9._-]{1,64}$'),
    policy_fingerprint      TEXT NOT NULL CHECK (policy_fingerprint ~ '^sha256:[0-9a-f]{64}$'),
    -- SHA-256 of the bounded router/scorer input (message, candidate
    -- revision set, policy). A changed hash after scoring forces a re-score.
    input_hash              BYTEA NOT NULL CHECK (octet_length(input_hash) = 32),
    -- Pinned candidate revision ids (JSON array of UUID strings, stable order).
    candidate_revision_ids  JSONB NOT NULL DEFAULT '[]'::jsonb
                            CHECK (jsonb_typeof(candidate_revision_ids) = 'array'),
    -- Requested targets that resolved to no employee row (JSON array of
    -- {target, reason}); recipients below always reference real employees.
    excluded_targets        JSONB NOT NULL DEFAULT '[]'::jsonb
                            CHECK (jsonb_typeof(excluded_targets) = 'array'),
    scorer_adapter          TEXT,
    scorer_model            TEXT,
    scorer_prompt_version   TEXT,
    scorer_version          TEXT,
    scorer_latency_ms       INT CHECK (scorer_latency_ms IS NULL OR scorer_latency_ms >= 0),
    scorer_usage            JSONB,
    -- Employees newly reserved and woken by this commit. The batch consumed
    -- one hop exactly when it woke at least one employee.
    wake_count              INT NOT NULL DEFAULT 0
                            CHECK (wake_count >= 0 AND wake_count <= 16),
    hop_consumed            BOOLEAN NOT NULL DEFAULT false,
    -- Chain counters as they stood after this commit, for explanation.
    chain_hop_count         SMALLINT CHECK (chain_hop_count IS NULL OR chain_hop_count >= 0),
    chain_wake_count        INT CHECK (chain_wake_count IS NULL OR chain_wake_count >= 0),
    decided_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, message_id),
    CHECK (hop_consumed = (wake_count > 0)),
    CHECK (mode <> 'silent' OR wake_count = 0),
    CHECK (NOT (mode = 'semantic' AND scorer_adapter IS NULL)),
    CHECK ((scorer_adapter IS NULL) = (scorer_version IS NULL))
);

CREATE INDEX idx_routing_decisions_root
    ON routing_decisions (company_id, root_message_id, decided_at);

CREATE TRIGGER trg_routing_decisions_immutable
    BEFORE UPDATE OR DELETE ON routing_decisions
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- Per-candidate action, reason, score, and bounded evidence.
CREATE TABLE routing_recipients (
    company_id              UUID NOT NULL REFERENCES companies(id),
    routing_decision_id     UUID NOT NULL,
    employee_id             TEXT NOT NULL,
    -- Stable order within the decision.
    position                SMALLINT NOT NULL CHECK (position >= 0),
    action                  TEXT NOT NULL CHECK (action IN ('wake', 'drop')),
    reason                  TEXT NOT NULL CHECK (reason ~ '^[a-z][a-z0-9_]{0,63}$'),
    score                   REAL CHECK (score IS NULL OR (score >= 0 AND score <= 1)),
    -- Stable evidence labels; at most 8, matching SemanticScore::validate.
    evidence                JSONB NOT NULL DEFAULT '[]'::jsonb
                            CHECK (jsonb_typeof(evidence) = 'array' AND jsonb_array_length(evidence) <= 8),
    -- Candidate revision evaluated; NULL only when the employee had none.
    employee_revision_id    UUID,
    PRIMARY KEY (company_id, routing_decision_id, employee_id),
    UNIQUE (company_id, routing_decision_id, position),
    -- Lets visits pin (decision, employee, action) in one foreign key.
    UNIQUE (company_id, routing_decision_id, employee_id, action),
    FOREIGN KEY (company_id, routing_decision_id)
        REFERENCES routing_decisions (company_id, id),
    FOREIGN KEY (company_id, employee_id)
        REFERENCES employees (company_id, id),
    FOREIGN KEY (company_id, employee_id, employee_revision_id)
        REFERENCES employee_revisions (company_id, employee_id, id)
);

CREATE INDEX idx_routing_recipients_employee
    ON routing_recipients (company_id, employee_id);

CREATE TRIGGER trg_routing_recipients_immutable
    BEFORE UPDATE OR DELETE ON routing_recipients
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- Later what-if / audit evaluations of an already-decided message. They are
-- structurally non-dispatching: they reference the canonical decision, keep
-- their recipients as audit JSON, and cannot own visits, runs, or outbox rows.
CREATE TABLE routing_re_evaluations (
    company_id              UUID NOT NULL REFERENCES companies(id),
    id                      UUID NOT NULL DEFAULT gen_random_uuid(),
    routing_decision_id     UUID NOT NULL,
    message_id              BYTEA NOT NULL CHECK (octet_length(message_id) = 32),
    requested_by            TEXT NOT NULL CHECK (btrim(requested_by) <> ''),
    purpose                 TEXT NOT NULL CHECK (purpose ~ '^[a-z][a-z0-9_]{0,63}$'),
    mode                    TEXT NOT NULL CHECK (mode IN ('deterministic', 'semantic', 'silent')),
    summary_reason          TEXT NOT NULL CHECK (summary_reason ~ '^[a-z][a-z0-9_]{0,63}$'),
    policy_version          TEXT NOT NULL CHECK (policy_version ~ '^[A-Za-z0-9._-]{1,64}$'),
    policy_fingerprint      TEXT NOT NULL CHECK (policy_fingerprint ~ '^sha256:[0-9a-f]{64}$'),
    input_hash              BYTEA NOT NULL CHECK (octet_length(input_hash) = 32),
    candidate_revision_ids  JSONB NOT NULL DEFAULT '[]'::jsonb
                            CHECK (jsonb_typeof(candidate_revision_ids) = 'array'),
    scorer_adapter          TEXT,
    scorer_model            TEXT,
    scorer_prompt_version   TEXT,
    scorer_version          TEXT,
    scorer_latency_ms       INT CHECK (scorer_latency_ms IS NULL OR scorer_latency_ms >= 0),
    -- Audit-only recipient rows (JSON array of RecipientDecision).
    recipients              JSONB NOT NULL DEFAULT '[]'::jsonb
                            CHECK (jsonb_typeof(recipients) = 'array'),
    dispatching             BOOLEAN NOT NULL DEFAULT false CHECK (dispatching = false),
    evaluated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, id),
    FOREIGN KEY (company_id, routing_decision_id)
        REFERENCES routing_decisions (company_id, id),
    CHECK ((scorer_adapter IS NULL) = (scorer_version IS NULL))
);

CREATE INDEX idx_routing_re_evaluations_decision
    ON routing_re_evaluations (company_id, routing_decision_id, evaluated_at);

CREATE TRIGGER trg_routing_re_evaluations_immutable
    BEFORE UPDATE OR DELETE ON routing_re_evaluations
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- One reservation per employee per root (invariant 7). Only a WAKE recipient
-- of a committed decision can reserve, the reservation is consumed by that
-- commit, and it is never released because runtime delivery later retries
-- or fails.
CREATE TABLE delivery_chain_visits (
    company_id              UUID NOT NULL REFERENCES companies(id),
    root_message_id         BYTEA NOT NULL CHECK (octet_length(root_message_id) = 32),
    employee_id             TEXT NOT NULL,
    routing_decision_id     UUID NOT NULL,
    recipient_action        TEXT NOT NULL CHECK (recipient_action = 'wake'),
    -- Hop number of the committed batch that reserved this visit (1-based).
    batch_hop               SMALLINT NOT NULL CHECK (batch_hop BETWEEN 1 AND 8),
    reserved_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, root_message_id, employee_id),
    FOREIGN KEY (company_id, root_message_id)
        REFERENCES delivery_chains (company_id, root_message_id),
    FOREIGN KEY (company_id, routing_decision_id)
        REFERENCES routing_decisions (company_id, id),
    FOREIGN KEY (company_id, routing_decision_id, employee_id, recipient_action)
        REFERENCES routing_recipients (company_id, routing_decision_id, employee_id, action)
);

CREATE INDEX idx_delivery_chain_visits_decision
    ON delivery_chain_visits (company_id, routing_decision_id);

CREATE TRIGGER trg_delivery_chain_visits_immutable
    BEFORE UPDATE OR DELETE ON delivery_chain_visits
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- ── Runs ─────────────────────────────────────────────────────────────────────
-- One employee execution, pinned to the revision it started with. A run
-- never disappears into an opaque process: queued, running, waiting,
-- completed, failed, and cancelled are durable states (invariant 11).

CREATE TABLE runs (
    company_id              UUID NOT NULL REFERENCES companies(id),
    id                      UUID NOT NULL DEFAULT gen_random_uuid(),
    employee_id             TEXT NOT NULL,
    employee_revision_id    UUID NOT NULL,
    -- Dispatch provenance. Conversational runs carry the decision that woke
    -- them and its message/root; Work-originated runs leave them NULL.
    routing_decision_id     UUID,
    message_id              BYTEA CHECK (message_id IS NULL OR octet_length(message_id) = 32),
    root_message_id         BYTEA CHECK (root_message_id IS NULL OR octet_length(root_message_id) = 32),
    -- Work attachment; the work_items relation arrives with Milestone 6.
    work_item_id            UUID,
    runtime_adapter         TEXT NOT NULL CHECK (runtime_adapter ~ '^[a-z][a-z0-9_-]{0,63}$'),
    -- Adapter-side run correlation reference.
    runtime_run_ref         TEXT,
    status                  TEXT NOT NULL DEFAULT 'queued'
                            CHECK (status IN ('queued', 'running', 'waiting', 'completed', 'failed', 'cancelled')),
    -- Typed completion: reply, channel, or silent. Required once completed.
    delivery_intent         TEXT CHECK (delivery_intent IS NULL OR delivery_intent IN ('reply', 'channel', 'silent')),
    cancel_reason           TEXT,
    error_code              TEXT,
    error_message           TEXT,
    queued_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at              TIMESTAMPTZ,
    finished_at             TIMESTAMPTZ,
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, id),
    -- Dispatch idempotency key (routing_decision_id, employee_id): at most
    -- one run per decision recipient.
    UNIQUE (company_id, routing_decision_id, employee_id),
    FOREIGN KEY (company_id, employee_id, employee_revision_id)
        REFERENCES employee_revisions (company_id, employee_id, id),
    FOREIGN KEY (company_id, routing_decision_id, employee_id)
        REFERENCES routing_recipients (company_id, routing_decision_id, employee_id),
    -- A decision-originated run must hold this employee's chain reservation.
    FOREIGN KEY (company_id, root_message_id, employee_id)
        REFERENCES delivery_chain_visits (company_id, root_message_id, employee_id),
    CHECK (routing_decision_id IS NULL OR NOT (message_id IS NULL OR root_message_id IS NULL)),
    CHECK (NOT (status = 'completed' AND delivery_intent IS NULL)),
    CHECK (NOT (status IN ('completed', 'failed', 'cancelled') AND finished_at IS NULL))
);

CREATE INDEX idx_runs_employee_status
    ON runs (company_id, employee_id, status, queued_at);
CREATE INDEX idx_runs_active
    ON runs (company_id, updated_at)
    WHERE status IN ('queued', 'running', 'waiting');

-- Ordered, normalized, append-only activity. (company_id, run_id, sequence)
-- is the replay cursor; sequences are dense so a reader can detect gaps.
CREATE TABLE run_events (
    company_id      UUID NOT NULL REFERENCES companies(id),
    run_id          UUID NOT NULL,
    sequence        BIGINT NOT NULL CHECK (sequence >= 0),
    -- Normalized RunEvent kind (Architecture v0 §4.6), e.g. `tool_call.started`.
    event_type      TEXT NOT NULL CHECK (event_type ~ '^[a-z][a-z0-9_.]{0,63}$'),
    occurred_at     TIMESTAMPTZ NOT NULL,
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Runtime adapter cursor for resume-from-cursor ingestion; unique per
    -- run so a replayed external event is rejected, not duplicated.
    runtime_cursor  TEXT,
    -- Bounded, redacted payload. Large output lives in object storage and is
    -- referenced by artifact_ref with size/hash metadata in the payload.
    payload         JSONB NOT NULL DEFAULT '{}'::jsonb
                    CHECK (jsonb_typeof(payload) = 'object' AND octet_length(payload::text) <= 65536),
    artifact_ref    TEXT,
    PRIMARY KEY (company_id, run_id, sequence),
    FOREIGN KEY (company_id, run_id) REFERENCES runs (company_id, id)
);

CREATE UNIQUE INDEX idx_run_events_runtime_cursor
    ON run_events (company_id, run_id, runtime_cursor)
    WHERE runtime_cursor IS NOT NULL;

-- Sequences are appended without gaps: sequence 0 opens the run, every later
-- event requires its predecessor. Concurrent appenders that both pass this
-- check collide on the primary key, so exactly one wins.
CREATE FUNCTION run_events_require_predecessor() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.sequence > 0 AND NOT EXISTS (
        SELECT 1
          FROM run_events previous
         WHERE previous.company_id = NEW.company_id
           AND previous.run_id = NEW.run_id
           AND previous.sequence = NEW.sequence - 1
    ) THEN
        RAISE EXCEPTION 'ortak: run % event sequence % has no predecessor', NEW.run_id, NEW.sequence
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_run_events_require_predecessor
    BEFORE INSERT ON run_events
    FOR EACH ROW EXECUTE FUNCTION run_events_require_predecessor();

CREATE TRIGGER trg_run_events_immutable
    BEFORE UPDATE OR DELETE ON run_events
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- ── Provisioning operations ──────────────────────────────────────────────────
-- Durable, resumable create/adopt/update saga (Architecture v0 §6). Retry
-- resumes at the failed step; adopted resources are never deleted by
-- compensation. Cem and Zeynep enter v0 as dry-run adopt operations.

CREATE TABLE provisioning_operations (
    company_id              UUID NOT NULL REFERENCES companies(id),
    id                      UUID NOT NULL DEFAULT gen_random_uuid(),
    employee_id             TEXT NOT NULL,
    mode                    TEXT NOT NULL CHECK (mode IN ('create', 'adopt', 'update')),
    -- Dry runs plan and validate without touching external resources.
    dry_run                 BOOLEAN NOT NULL DEFAULT true,
    -- Operator/client idempotency key; a retry resumes this row instead of
    -- starting a second saga.
    idempotency_key         TEXT NOT NULL CHECK (btrim(idempotency_key) <> ''),
    -- Secret-free manifest snapshot the saga executes.
    manifest                JSONB NOT NULL CHECK (jsonb_typeof(manifest) = 'object'),
    manifest_fingerprint    BYTEA NOT NULL CHECK (octet_length(manifest_fingerprint) = 32),
    status                  TEXT NOT NULL DEFAULT 'pending'
                            CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'compensating', 'compensated')),
    current_step            TEXT,
    -- Revision activated by a successful non-dry-run operation.
    result_revision_id      UUID,
    error_message           TEXT,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at             TIMESTAMPTZ,
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, idempotency_key),
    FOREIGN KEY (company_id, employee_id) REFERENCES employees (company_id, id),
    FOREIGN KEY (company_id, employee_id, result_revision_id)
        REFERENCES employee_revisions (company_id, employee_id, id),
    CHECK (NOT (status = 'succeeded' AND NOT dry_run AND result_revision_id IS NULL)),
    CHECK (NOT (status IN ('succeeded', 'failed', 'compensated') AND finished_at IS NULL))
);

CREATE INDEX idx_provisioning_operations_employee
    ON provisioning_operations (company_id, employee_id, created_at);
CREATE INDEX idx_provisioning_operations_active
    ON provisioning_operations (company_id, updated_at)
    WHERE status IN ('pending', 'running', 'compensating');

-- Step-level state so a retry resumes exactly where the saga stopped.
CREATE TABLE provisioning_operation_steps (
    company_id          UUID NOT NULL REFERENCES companies(id),
    operation_id        UUID NOT NULL,
    step_index          SMALLINT NOT NULL CHECK (step_index >= 0),
    step_name           TEXT NOT NULL CHECK (step_name ~ '^[a-z][a-z0-9_]{0,63}$'),
    state               TEXT NOT NULL DEFAULT 'pending'
                        CHECK (state IN ('pending', 'running', 'succeeded', 'failed', 'compensating', 'compensated', 'skipped')),
    -- Per-step key handed to the adapter so a retried step resumes rather
    -- than duplicating the external mutation.
    idempotency_key     TEXT NOT NULL CHECK (btrim(idempotency_key) <> ''),
    attempt_count       INT NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    -- The step attached to a pre-existing external resource; compensation
    -- must never delete it.
    adopted_existing    BOOLEAN NOT NULL DEFAULT false,
    -- Secret-free adapter receipt.
    result              JSONB NOT NULL DEFAULT '{}'::jsonb
                        CHECK (jsonb_typeof(result) = 'object'),
    error_message       TEXT,
    started_at          TIMESTAMPTZ,
    finished_at         TIMESTAMPTZ,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, operation_id, step_index),
    UNIQUE (company_id, operation_id, step_name),
    UNIQUE (company_id, idempotency_key),
    FOREIGN KEY (company_id, operation_id)
        REFERENCES provisioning_operations (company_id, id),
    CHECK (NOT (state IN ('succeeded', 'failed', 'compensated', 'skipped') AND finished_at IS NULL))
);

-- ── Dispatch / delivery outbox ───────────────────────────────────────────────
-- Transactional outbox committed together with the routing decision, visit
-- reservations, and chain counters (invariant 8), and with run completion
-- for Office delivery. Leased, retried with backoff, terminal on exhaustion,
-- and re-openable by an operator.

CREATE TABLE outbox (
    company_id              UUID NOT NULL REFERENCES companies(id),
    id                      UUID NOT NULL DEFAULT gen_random_uuid(),
    -- run_dispatch: start a run for one decision recipient.
    -- office_publish: publish a frozen signed Office event for a run.
    kind                    TEXT NOT NULL CHECK (kind IN ('run_dispatch', 'office_publish')),
    -- Company-unique idempotency key. For run_dispatch it is derived from
    -- (routing_decision_id, employee_id); the partial unique index below
    -- makes that key structural.
    dedup_key               TEXT NOT NULL CHECK (btrim(dedup_key) <> ''),
    routing_decision_id     UUID,
    employee_id             TEXT,
    run_id                  UUID,
    payload                 JSONB NOT NULL DEFAULT '{}'::jsonb
                            CHECK (jsonb_typeof(payload) = 'object'),
    -- Frozen signed Office event (id and exact serialized bytes) persisted
    -- before the first publish attempt; retries resend these bytes and never
    -- re-sign with a new timestamp or id.
    signed_event_id         BYTEA CHECK (signed_event_id IS NULL OR octet_length(signed_event_id) = 32),
    signed_event_bytes      BYTEA,
    state                   TEXT NOT NULL DEFAULT 'pending'
                            CHECK (state IN ('pending', 'delivered', 'failed')),
    attempt_count           INT NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    max_attempts            SMALLINT NOT NULL DEFAULT 8 CHECK (max_attempts >= 1),
    retry_after             TIMESTAMPTZ,
    -- Per-claim lease. Completion and failure updates must present the token
    -- written at claim time, so a stale worker cannot overwrite a newer
    -- worker's terminal update (same pattern as relay_admin_outbox).
    lease_owner             TEXT,
    lease_token             UUID,
    lease_expires_at        TIMESTAMPTZ,
    last_error              TEXT,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at            TIMESTAMPTZ,
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, dedup_key),
    FOREIGN KEY (company_id, routing_decision_id, employee_id)
        REFERENCES routing_recipients (company_id, routing_decision_id, employee_id),
    FOREIGN KEY (company_id, run_id) REFERENCES runs (company_id, id),
    CHECK (NOT (kind = 'run_dispatch' AND (routing_decision_id IS NULL OR employee_id IS NULL))),
    CHECK (NOT (kind = 'office_publish' AND run_id IS NULL)),
    CHECK (NOT (kind = 'office_publish' AND state = 'delivered' AND signed_event_id IS NULL)),
    CHECK ((signed_event_id IS NULL) = (signed_event_bytes IS NULL)),
    CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL)),
    CHECK (NOT (state = 'delivered' AND delivered_at IS NULL)),
    CHECK (attempt_count <= max_attempts)
);

-- Idempotent dispatch: one run_dispatch row per (decision, employee).
CREATE UNIQUE INDEX idx_outbox_run_dispatch
    ON outbox (company_id, routing_decision_id, employee_id)
    WHERE kind = 'run_dispatch';
-- One publish row per frozen signed event.
CREATE UNIQUE INDEX idx_outbox_signed_event
    ON outbox (company_id, signed_event_id)
    WHERE signed_event_id IS NOT NULL;
CREATE INDEX idx_outbox_due
    ON outbox (company_id, retry_after, created_at)
    WHERE state = 'pending';
