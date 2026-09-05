-- Buzz initial Postgres schema — multi-tenant.
--
-- Source of truth for fresh database setup. This is a clean, from-scratch
-- schema in which `community_id` is a first-class, server-resolved key on
-- every tenant-scoped row. It is NOT additive over the single-community
-- schema; the rewrite replaces it. Existing single-community deployments
-- migrate via the documented backfill migration (0002), which assigns all
-- pre-existing rows to one default community.
--
-- The governing contract is docs/multi-tenant-conformance.md. Every table
-- below cites the conformance surface it implements. The invariant behind the
-- whole schema (conformance "row zero"): a request's community is resolved
-- from the connection host by the server, never supplied by the client, and
-- every scoped row carries that immutable `community_id`.
--
-- Migration-lint obligations enforced by the Lane 0 lint harness:
--   1. Every tenant-scoped table has `community_id NOT NULL`.
--   2. No UNIQUE / PRIMARY KEY / FK on a scoped table is observable across
--      communities: each leads with `community_id` (or, for child rows whose
--      parent already pins the community, joins carry the community tuple).
--   3. `channels.community_id` is immutable (trigger below; no UPDATE path).
--   4. Operator-global tables are named in the explicit allowlist, not implied.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ── Custom types ──────────────────────────────────────────────────────────────

CREATE TYPE channel_type AS ENUM ('stream', 'forum', 'dm', 'workflow');
CREATE TYPE channel_visibility AS ENUM ('open', 'private');
CREATE TYPE member_role AS ENUM ('owner', 'admin', 'member', 'guest', 'bot');
CREATE TYPE workflow_status AS ENUM ('active', 'disabled', 'archived');
CREATE TYPE run_status AS ENUM ('pending', 'running', 'waiting_approval', 'completed', 'failed', 'cancelled');
CREATE TYPE approval_status AS ENUM ('pending', 'granted', 'denied', 'expired');
CREATE TYPE delivery_method AS ENUM ('webhook', 'websocket');
CREATE TYPE subscription_status AS ENUM ('active', 'paused', 'deleted');
CREATE TYPE pause_reason AS ENUM ('user', 'system', 'rate_limit');
CREATE TYPE channel_add_policy AS ENUM ('anyone', 'owner_only', 'nobody');

-- ── Communities ───────────────────────────────────────────────────────────────
-- Conformance: row zero (host binding). The host map. `resolve_host(host)`
-- reads exactly one row here to mint the request's TenantContext. This table
-- is OPERATOR-GLOBAL: it is the registry of tenants, not itself tenant-scoped,
-- so it carries no `community_id` of its own (its `id` IS the community key).
-- Listed in the lint allowlist as operator-global.
--
-- Host normalization (Lane 0 contract): `host` is stored already-normalized —
-- ASCII-lowercased, trailing dot stripped, default port omitted. The UNIQUE is
-- on `lower(host)` belt-and-suspenders so `Relay.Example` and `relay.example`
-- can never become two tenants even if a writer forgets to normalize.
-- `resolve_host()` (buzz-core) applies the identical normalization before
-- lookup, so resolution and storage agree by construction.

CREATE TABLE communities (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    host            VARCHAR(255) NOT NULL,
    signing_key     BYTEA,
    -- Per-community workspace icon (NIP-11 `icon`), set via kind:9033.
    -- Added by migration 0003; kept here so desired-state applies match.
    icon            TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    archived_at     TIMESTAMPTZ,
    deletion_state  TEXT NOT NULL DEFAULT 'active' CHECK (deletion_state IN ('active', 'quiescing', 'fenced', 'tombstone')),
    deletion_fence_generation BIGINT NOT NULL DEFAULT 0 CHECK (deletion_fence_generation >= 0),
    deleted_at      TIMESTAMPTZ,
    CONSTRAINT chk_communities_id_not_nil CHECK (id <> '00000000-0000-0000-0000-000000000000'::uuid)
);

CREATE UNIQUE INDEX idx_communities_host ON communities (lower(host));

-- ── Channels ──────────────────────────────────────────────────────────────────
-- Conformance: "Channels and channel membership". `community_id` immutable.
-- Channel UUIDs stay valid wire identifiers, but they are NOT globally unique:
-- the PK is `(community_id, id)`, so the same UUID may legitimately exist in two
-- communities (conformance lists "same channel UUID collision in two
-- communities" as a required isolation test). Handlers always carry `ctx`, so
-- `(ctx.community, h)` names exactly one channel; a client-supplied `h` can
-- never reach another community's channel.

CREATE TABLE channels (
    id              UUID NOT NULL DEFAULT gen_random_uuid(),
    community_id    UUID NOT NULL REFERENCES communities(id),
    name            VARCHAR(255) NOT NULL,
    channel_type    channel_type NOT NULL DEFAULT 'stream',
    visibility      channel_visibility NOT NULL DEFAULT 'open',
    description     TEXT,
    canvas          TEXT,
    created_by      BYTEA NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    archived_at     TIMESTAMPTZ,
    deleted_at      TIMESTAMPTZ,
    nip29_group_id  VARCHAR(255),
    topic_required  BOOLEAN NOT NULL DEFAULT FALSE,
    max_members     INT,
    topic           TEXT,
    topic_set_by    BYTEA,
    topic_set_at    TIMESTAMPTZ,
    purpose         TEXT,
    purpose_set_by  BYTEA,
    purpose_set_at  TIMESTAMPTZ,
    participant_hash BYTEA,
    ttl_seconds     INT,
    ttl_deadline    TIMESTAMPTZ,
    PRIMARY KEY (community_id, id),
    CONSTRAINT chk_channels_id_not_nil CHECK (id <> '00000000-0000-0000-0000-000000000000'::uuid)
);

-- nip29 group id and DM participant hash are unique WITHIN a community, not globally.
CREATE UNIQUE INDEX idx_channels_nip29_group ON channels (community_id, nip29_group_id)
    WHERE nip29_group_id IS NOT NULL;
CREATE UNIQUE INDEX idx_channels_dm_hash ON channels (community_id, participant_hash)
    WHERE participant_hash IS NOT NULL;
CREATE INDEX idx_channels_community_type ON channels (community_id, channel_type);
CREATE INDEX idx_channels_community_visibility ON channels (community_id, visibility);
CREATE INDEX idx_channels_created_by ON channels (community_id, created_by);
CREATE INDEX idx_channels_ttl_expiry ON channels (ttl_deadline)
    WHERE ttl_seconds IS NOT NULL AND archived_at IS NULL AND deleted_at IS NULL;
-- Tenant-independent channel-id → community lookups (Db::communities_of_channels,
-- Db::community_of_channel) carry no community_id predicate, so no
-- community_id-leading index can serve them. Covering + partial: index-only scan.
-- Not UNIQUE — the same channel id may exist under more than one community.
CREATE INDEX idx_channels_id_live ON channels (id) INCLUDE (community_id)
    WHERE deleted_at IS NULL;

-- channels.community_id is immutable: a channel can never be re-tenanted.
-- (Conformance: "Migration lint forbids channel re-tenanting except through an
-- explicitly modeled admission path." We have no such path, so: hard block.)
CREATE FUNCTION channels_community_id_immutable() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.community_id IS DISTINCT FROM OLD.community_id THEN
        RAISE EXCEPTION 'channels.community_id is immutable (channel % cannot be re-tenanted)', OLD.id
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_channels_community_id_immutable
    BEFORE UPDATE ON channels
    FOR EACH ROW EXECUTE FUNCTION channels_community_id_immutable();

-- ── Channel members ───────────────────────────────────────────────────────────
-- Conformance: "Channels and channel membership". PK leads with community_id.

CREATE TABLE channel_members (
    community_id UUID NOT NULL REFERENCES communities(id),
    channel_id  UUID NOT NULL,
    pubkey      BYTEA NOT NULL,
    role        member_role NOT NULL DEFAULT 'member',
    joined_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    invited_by  BYTEA,
    removed_at  TIMESTAMPTZ,
    removed_by  BYTEA,
    hidden_at   TIMESTAMPTZ,
    PRIMARY KEY (community_id, channel_id, pubkey),
    FOREIGN KEY (community_id, channel_id)
        REFERENCES channels (community_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_channel_members_pubkey ON channel_members (community_id, pubkey)
    WHERE removed_at IS NULL;

-- ── Users ─────────────────────────────────────────────────────────────────────
-- Conformance: "Users, profiles, NIP-05, and user search". One profile per
-- (community, pubkey): the same key reposts kind:0 in each community it joins.

CREATE TABLE users (
    community_id        UUID NOT NULL REFERENCES communities(id),
    pubkey              BYTEA NOT NULL,
    nip05_handle        VARCHAR(255),
    display_name        VARCHAR(255),
    avatar_url          TEXT,
    about               TEXT,
    agent_type          VARCHAR(255),
    capabilities        JSONB,
    okta_user_id        VARCHAR(255),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deactivated_at      TIMESTAMPTZ,
    metadata_event_id   BYTEA,
    agent_owner_pubkey  BYTEA,
    channel_add_policy  channel_add_policy NOT NULL DEFAULT 'anyone',
    PRIMARY KEY (community_id, pubkey),
    CONSTRAINT chk_users_pubkey_len CHECK (LENGTH(pubkey) = 32),
    -- agent owner is a user in the SAME community.
    FOREIGN KEY (community_id, agent_owner_pubkey)
        REFERENCES users (community_id, pubkey) ON DELETE SET NULL
);

-- NIP-05 handle and Okta id unique within a community, not globally.
CREATE UNIQUE INDEX idx_users_nip05 ON users (community_id, lower(nip05_handle))
    WHERE nip05_handle IS NOT NULL;
CREATE UNIQUE INDEX idx_users_okta ON users (community_id, okta_user_id)
    WHERE okta_user_id IS NOT NULL;

-- ── Events (partitioned by month on created_at) ──────────────────────────────
-- Conformance: "Channel-less global events and DMs". `community_id` leads the
-- PK and every hot-path index. Partition stays BY RANGE (created_at) — the
-- monthly partition manager is unchanged (Max's call, plan §5/Lane0 contract).
-- Cross-community dedup: same signed event may exist in two communities;
-- (community_id, created_at, id) dedupes within one, allows across.

CREATE TABLE events (
    community_id UUID NOT NULL REFERENCES communities(id),
    id          BYTEA NOT NULL,
    pubkey      BYTEA NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL,
    kind        INT NOT NULL,
    tags        JSONB NOT NULL,
    content     TEXT NOT NULL,
    -- Full-text search vector (Typesense → Postgres FTS). Generated/STORED so
    -- it is a single source of truth — no sidecar indexer to keep coherent
    -- (Quinn option A, Lane-0 call). 'simple' config = no stemming/stopwords,
    -- matching the existing substring-ish search semantics; the search lane can
    -- revisit the config behind evidence. Tenant scoping is by the
    -- community-leading btree filters BitmapAnd-ed with the GIN probe, so the
    -- GIN index itself stays the minimal `GIN (search_tsv)` (Max's caveat:
    -- avoid btree_gin unless EXPLAIN proves it buys something).
    -- Privacy: encrypted/private routing wrappers and p-gated membership notices
    -- must never be discoverable through NIP-50 full-text search. NULL tsvector
    -- never matches `@@`.
    -- Keep in sync with migrations (final state: 0001 + 0005 + 0014 + 0033).
    search_tsv  TSVECTOR GENERATED ALWAYS AS (
        CASE WHEN kind IN (1059, 30179, 30300, 30350, 30622, 44100, 44101, 44200) THEN NULL::tsvector
             ELSE to_tsvector('simple', content)
        END
    ) STORED,
    sig         BYTEA NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    channel_id  UUID,
    deleted_at  TIMESTAMPTZ,
    d_tag       TEXT,
    not_before  BIGINT,
    delivered_at BIGINT,
    PRIMARY KEY (community_id, created_at, id)
) PARTITION BY RANGE (created_at);

CREATE TABLE events_p_past PARTITION OF events
    FOR VALUES FROM (MINVALUE) TO ('2026-01-01');
CREATE TABLE events_p2026_01 PARTITION OF events
    FOR VALUES FROM ('2026-01-01') TO ('2026-02-01');
CREATE TABLE events_p2026_02 PARTITION OF events
    FOR VALUES FROM ('2026-02-01') TO ('2026-03-01');
CREATE TABLE events_p2026_03 PARTITION OF events
    FOR VALUES FROM ('2026-03-01') TO ('2026-04-01');
CREATE TABLE events_p2026_04 PARTITION OF events
    FOR VALUES FROM ('2026-04-01') TO ('2026-05-01');
CREATE TABLE events_p2026_05 PARTITION OF events
    FOR VALUES FROM ('2026-05-01') TO ('2026-06-01');
CREATE TABLE events_p2026_06 PARTITION OF events
    FOR VALUES FROM ('2026-06-01') TO ('2026-07-01');
CREATE TABLE events_p_future PARTITION OF events
    FOR VALUES FROM ('2026-07-01') TO (MAXVALUE);

-- Direct id lookup: the PK can't serve `WHERE id=$1` because created_at sits
-- between community_id and id. This index makes the scoped form
-- `WHERE community_id=$ AND id=$` index-served, not a partition scan.
CREATE INDEX idx_events_community_id ON events (community_id, id, created_at DESC);
-- Hot-path indexes, all community-leading.
CREATE INDEX idx_events_community_channel_created
    ON events (community_id, channel_id, created_at DESC, id);
CREATE INDEX idx_events_community_pubkey_kind_created
    ON events (community_id, pubkey, kind, created_at DESC, id);
CREATE INDEX idx_events_community_kind_created
    ON events (community_id, kind, created_at DESC, id);
CREATE INDEX idx_events_community_deleted ON events (community_id, deleted_at);
-- Addressable (replaceable) and NIP-33 parameterized lookups.
CREATE INDEX idx_events_addressable
    ON events (community_id, kind, pubkey, channel_id, deleted_at);
CREATE INDEX idx_events_parameterized
    ON events (community_id, kind, pubkey, d_tag, created_at DESC, id)
    WHERE d_tag IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX idx_events_not_before ON events (community_id, not_before)
    WHERE not_before IS NOT NULL AND deleted_at IS NULL AND delivered_at IS NULL;
-- Full-text search. Minimal GIN over the generated tsvector; community scoping
-- is supplied by the community-leading btree filters above (BitmapAnd), so this
-- stays a single-column GIN. The search lane confirms the final spelling with
-- EXPLAIN before its work lands (Quinn option A; Max's index-spelling caveat).
CREATE INDEX idx_events_search_tsv ON events USING GIN (search_tsv);

-- ── Event mentions ────────────────────────────────────────────────────────────
-- Conformance: "Channel-less global events and DMs" (#p fan-out). The join to
-- events MUST carry the community tuple (e.community_id = m.community_id AND
-- e.id = m.event_id) — bare e.id = m.event_id would leak cross-community
-- mentions (Max, verified at event.rs:222).

CREATE TABLE event_mentions (
    community_id        UUID NOT NULL REFERENCES communities(id),
    pubkey_hex          VARCHAR(64) NOT NULL,
    event_id            BYTEA NOT NULL,
    event_created_at    TIMESTAMPTZ NOT NULL,
    channel_id          UUID,
    event_kind          INT,
    PRIMARY KEY (community_id, pubkey_hex, event_id)
);

CREATE INDEX idx_event_mentions_pubkey_created
    ON event_mentions (community_id, pubkey_hex, event_created_at DESC);
CREATE INDEX idx_event_mentions_pubkey_kind_created
    ON event_mentions (community_id, pubkey_hex, event_kind, event_created_at DESC);

-- ── Subscriptions ─────────────────────────────────────────────────────────────
-- Conformance: "Mesh, agents, ACP/MCP, and CLI" (persisted subscriptions).

CREATE TABLE subscriptions (
    community_id        UUID NOT NULL REFERENCES communities(id),
    id                  VARCHAR(255) NOT NULL,
    owner_pubkey        BYTEA NOT NULL,
    filter_kinds        JSONB,
    filter_authors      JSONB,
    filter_channel_ids  JSONB,
    filter_since        TIMESTAMPTZ,
    filter_until        TIMESTAMPTZ,
    delivery_method     delivery_method NOT NULL DEFAULT 'webhook',
    delivery_url        TEXT,
    status              subscription_status NOT NULL DEFAULT 'active',
    pause_reason        pause_reason,
    delivered_count     BIGINT NOT NULL DEFAULT 0,
    error_count         BIGINT NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (community_id, id),
    FOREIGN KEY (community_id, owner_pubkey) REFERENCES users (community_id, pubkey)
);

-- ── Delivery log (partitioned by month on delivered_at) ──────────────────────
-- Conformance: subscription delivery audit. community_id carried for tenant
-- attribution; child of subscriptions.

CREATE TABLE delivery_log (
    community_id    UUID NOT NULL REFERENCES communities(id),
    id              BIGINT GENERATED ALWAYS AS IDENTITY,
    subscription_id VARCHAR(255),
    event_id        BYTEA,
    method          delivery_method,
    delivered_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    success         BOOLEAN,
    http_status     INT,
    error_message   TEXT,
    attempt_number  INT DEFAULT 1,
    PRIMARY KEY (delivered_at, id)
) PARTITION BY RANGE (delivered_at);

CREATE TABLE delivery_log_p_past PARTITION OF delivery_log
    FOR VALUES FROM (MINVALUE) TO ('2026-03-01');
CREATE TABLE delivery_log_p2026_03 PARTITION OF delivery_log
    FOR VALUES FROM ('2026-03-01') TO ('2026-04-01');
CREATE TABLE delivery_log_p2026_04 PARTITION OF delivery_log
    FOR VALUES FROM ('2026-04-01') TO ('2026-05-01');
CREATE TABLE delivery_log_p2026_05 PARTITION OF delivery_log
    FOR VALUES FROM ('2026-05-01') TO ('2026-06-01');
CREATE TABLE delivery_log_p2026_06 PARTITION OF delivery_log
    FOR VALUES FROM ('2026-06-01') TO ('2026-07-01');
CREATE TABLE delivery_log_p_future PARTITION OF delivery_log
    FOR VALUES FROM ('2026-07-01') TO (MAXVALUE);

CREATE INDEX idx_delivery_log_community_sub ON delivery_log (community_id, subscription_id);

-- ── Workflows ─────────────────────────────────────────────────────────────────
-- Conformance: "Workflows, runs, approvals, webhooks, schedules". Definition's
-- community fixed at create from req.community; runs/approvals inherit it.

CREATE TABLE workflows (
    community_id    UUID NOT NULL REFERENCES communities(id),
    id              UUID NOT NULL DEFAULT gen_random_uuid(),
    name            VARCHAR(255) NOT NULL,
    owner_pubkey    BYTEA NOT NULL,
    channel_id      UUID,
    definition      JSONB NOT NULL,
    definition_hash BYTEA NOT NULL,
    status          workflow_status NOT NULL DEFAULT 'active',
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (community_id, id),
    FOREIGN KEY (community_id, owner_pubkey) REFERENCES users (community_id, pubkey),
    FOREIGN KEY (community_id, channel_id) REFERENCES channels (community_id, id)
);

CREATE INDEX idx_workflows_channel_active ON workflows (community_id, channel_id, status, enabled);
-- Scheduler scans enabled schedule workflows; community_id returned per row so
-- side effects run under the owning tenant's context (Lane0 contract §4a.5).
CREATE INDEX idx_workflows_enabled ON workflows (enabled, status) WHERE enabled;

-- ── Workflow runs ─────────────────────────────────────────────────────────────

CREATE TABLE workflow_runs (
    community_id        UUID NOT NULL REFERENCES communities(id),
    id                  UUID NOT NULL DEFAULT gen_random_uuid(),
    workflow_id         UUID NOT NULL,
    status              run_status NOT NULL DEFAULT 'pending',
    trigger_event_id    BYTEA,
    current_step        INT NOT NULL DEFAULT 0,
    execution_trace     JSONB NOT NULL DEFAULT '[]',
    trigger_context     JSONB,
    started_at          TIMESTAMPTZ,
    completed_at        TIMESTAMPTZ,
    error_message       TEXT,
    error_code          TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (community_id, id),
    FOREIGN KEY (community_id, workflow_id)
        REFERENCES workflows (community_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_workflow_runs_workflow ON workflow_runs (community_id, workflow_id);
CREATE INDEX idx_workflow_runs_status ON workflow_runs (community_id, status);

-- ── Workflow approvals ────────────────────────────────────────────────────────
-- token-hash lookup scoped: approval token grants cannot act on another
-- community's same hash (conformance).

CREATE TABLE workflow_approvals (
    community_id    UUID NOT NULL REFERENCES communities(id),
    token           BYTEA NOT NULL,
    workflow_id     UUID NOT NULL,
    run_id          UUID NOT NULL,
    step_id         VARCHAR(64) NOT NULL,
    step_index      INT NOT NULL,
    approver_spec   TEXT NOT NULL,
    status          approval_status NOT NULL DEFAULT 'pending',
    approver_pubkey BYTEA,
    note            TEXT,
    granted_at      TIMESTAMPTZ,
    denied_at       TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (community_id, token),
    FOREIGN KEY (community_id, workflow_id)
        REFERENCES workflows (community_id, id) ON DELETE CASCADE,
    FOREIGN KEY (community_id, run_id)
        REFERENCES workflow_runs (community_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_workflow_approvals_workflow ON workflow_approvals (community_id, workflow_id);
CREATE INDEX idx_workflow_approvals_run ON workflow_approvals (community_id, run_id);
CREATE INDEX idx_workflow_approvals_status ON workflow_approvals (community_id, status);

-- ── Scheduled workflow fires (cron claim) ─────────────────────────────────────
-- Plan §5: the at-most-once cron fire claim. UNIQUE (community_id, workflow_id,
-- scheduled_for) — only the pod that wins the claim insert creates the run.
-- Restart-safe (DB-durable). community is server provenance: the scheduler passes
-- workflow.community_id from list_all_enabled_workflows(), never a client input.
-- workflow_id is NOT globally unique under the (community_id, id) workflow key, so
-- the claim binds both community and id explicitly rather than resolving from id.
-- workflow_run_id links the won claim to the run it created (audit; NULL until the
-- post-insert attach, and stays NULL if run creation failed after a won claim).
-- The FK to workflow_runs uses NO ACTION (not SET NULL): community_id is shared
-- with the claim PK and is NOT NULL, so SET NULL is unimplementable here; a future
-- delete of a still-linked run is blocked rather than orphaning the at-most-once
-- claim row. workflow_runs are not pruned today, so this is a guardrail, not a path.

CREATE TABLE scheduled_workflow_fires (
    community_id    UUID NOT NULL REFERENCES communities(id),
    workflow_id     UUID NOT NULL,
    scheduled_for   TIMESTAMPTZ NOT NULL,
    claimed_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    workflow_run_id UUID,
    PRIMARY KEY (community_id, workflow_id, scheduled_for),
    FOREIGN KEY (community_id, workflow_id)
        REFERENCES workflows (community_id, id) ON DELETE CASCADE,
    FOREIGN KEY (community_id, workflow_run_id)
        REFERENCES workflow_runs (community_id, id) ON DELETE NO ACTION
);

-- The interval anchor reads MAX(scheduled_for) per workflow; the janitor prunes
-- by claimed_at globally (operator concern). See plan §5 retention coupling.
CREATE INDEX idx_scheduled_fires_claimed_at ON scheduled_workflow_fires (claimed_at);

-- ── API tokens ────────────────────────────────────────────────────────────────
-- Conformance: "API tokens and NIP-98 replay". token_hash uniqueness scoped to
-- (community_id, token_hash); channel claims reference channels in same community.

CREATE TABLE api_tokens (
    community_id        UUID NOT NULL REFERENCES communities(id),
    id                  UUID NOT NULL DEFAULT gen_random_uuid(),
    token_hash          BYTEA NOT NULL,
    owner_pubkey        BYTEA NOT NULL,
    name                VARCHAR(255) NOT NULL,
    scopes              JSONB NOT NULL,
    channel_ids         JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at          TIMESTAMPTZ,
    last_used_at        TIMESTAMPTZ,
    revoked_at          TIMESTAMPTZ,
    revoked_by          BYTEA,
    created_by_self_mint BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (community_id, id),
    FOREIGN KEY (community_id, owner_pubkey) REFERENCES users (community_id, pubkey),
    CONSTRAINT chk_api_tokens_hash_len CHECK (LENGTH(token_hash) = 32)
);

CREATE UNIQUE INDEX idx_api_tokens_hash ON api_tokens (community_id, token_hash);

-- ── Rate limit violations ─────────────────────────────────────────────────────
-- OPERATOR-GLOBAL: a deployment-health / abuse table, never tenant-observable.
-- Listed in the lint allowlist. Carries community_id as an attribution label
-- only (nullable, no uniqueness over it).

CREATE TABLE rate_limit_violations (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    community_id    UUID,
    pubkey          BYTEA,
    violation_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    limit_type      VARCHAR(64),
    limit_value     INT,
    actual_value    INT,
    action_taken    VARCHAR(64)
);

-- ── Thread metadata ───────────────────────────────────────────────────────────
-- Conformance: thread lookups filter by community before event matching.

CREATE TABLE thread_metadata (
    community_id            UUID NOT NULL REFERENCES communities(id),
    event_created_at        TIMESTAMPTZ NOT NULL,
    event_id                BYTEA NOT NULL,
    channel_id              UUID NOT NULL,
    parent_event_id         BYTEA,
    parent_event_created_at TIMESTAMPTZ,
    root_event_id           BYTEA,
    root_event_created_at   TIMESTAMPTZ,
    depth                   INT NOT NULL DEFAULT 0,
    reply_count             INT NOT NULL DEFAULT 0,
    descendant_count        INT NOT NULL DEFAULT 0,
    last_reply_at           TIMESTAMPTZ,
    broadcast               BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (community_id, event_created_at, event_id),
    FOREIGN KEY (community_id, channel_id) REFERENCES channels (community_id, id)
);

CREATE INDEX idx_thread_metadata_parent ON thread_metadata (community_id, parent_event_id);
CREATE INDEX idx_thread_metadata_root ON thread_metadata (community_id, root_event_id);
CREATE INDEX idx_thread_metadata_channel_depth
    ON thread_metadata (community_id, channel_id, depth, event_created_at);
CREATE INDEX idx_thread_metadata_event_id ON thread_metadata (community_id, event_id);

-- ── Reactions ─────────────────────────────────────────────────────────────────
-- Conformance: reactions filter by community before event/pubkey matching.

CREATE TABLE reactions (
    community_id        UUID NOT NULL REFERENCES communities(id),
    event_created_at    TIMESTAMPTZ NOT NULL,
    event_id            BYTEA NOT NULL,
    pubkey              BYTEA NOT NULL,
    emoji               VARCHAR(66) NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    removed_at          TIMESTAMPTZ,
    reaction_event_id   BYTEA,
    PRIMARY KEY (community_id, event_created_at, event_id, pubkey, emoji)
);

CREATE INDEX idx_reactions_event ON reactions (community_id, event_id, event_created_at);
CREATE INDEX idx_reactions_pubkey ON reactions (community_id, pubkey);
-- A reaction's source event id is unique within a community.
CREATE UNIQUE INDEX idx_reactions_source_event ON reactions (community_id, reaction_event_id)
    WHERE reaction_event_id IS NOT NULL;

-- ── Pubkey allowlist ──────────────────────────────────────────────────────────
-- Conformance: "Relay membership, pubkey allowlist, archived identities".
-- PK becomes (community_id, pubkey).

CREATE TABLE pubkey_allowlist (
    community_id UUID NOT NULL REFERENCES communities(id),
    pubkey      BYTEA NOT NULL,
    added_by    BYTEA,
    added_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    note        TEXT,
    PRIMARY KEY (community_id, pubkey)
);

-- ── Relay members (NIP-43) ────────────────────────────────────────────────────
-- Conformance: membership gate, community-scoped. pubkey stored as hex TEXT
-- (unchanged wire form). PK (community_id, pubkey).

CREATE TABLE relay_members (
    community_id UUID NOT NULL REFERENCES communities(id),
    pubkey      TEXT NOT NULL,
    role        TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    added_by    TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, pubkey)
);

CREATE INDEX idx_relay_members_role ON relay_members (community_id, role);

-- ── Join policy acceptances ──────────────────────────────────────────────────
-- Durable evidence of the policy version accepted when an invite claim grants
-- relay membership. The composite foreign key keeps evidence bound to a live
-- member in the same community and removes it with that membership.

CREATE TABLE join_policy_acceptances (
    community_id UUID NOT NULL,
    pubkey TEXT NOT NULL,
    policy_version TEXT NOT NULL CHECK (length(policy_version) = 64),
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, pubkey, policy_version),
    FOREIGN KEY (community_id, pubkey)
        REFERENCES relay_members (community_id, pubkey) ON DELETE CASCADE
);

-- ── Relay invites (use-limited invite links) ──────────────────────────────────
-- Conformance: durable invite records for atomic redemption, community-scoped.
-- Stores only SHA-256(code) as 32-byte BYTEA; never the reusable bearer code.
-- PK and UNIQUE both lead with community_id. max_uses NULL = unlimited.

CREATE TABLE relay_invites (
    community_id  UUID        NOT NULL REFERENCES communities(id),
    id           UUID        NOT NULL DEFAULT gen_random_uuid(),
    token_hash   BYTEA       NOT NULL CHECK (length(token_hash) = 32),
    role         TEXT        NOT NULL DEFAULT 'member' CHECK (role = 'member'),
    max_uses     INTEGER     CHECK (max_uses BETWEEN 1 AND 10000),
    use_count    INTEGER     NOT NULL DEFAULT 0 CHECK (use_count >= 0),
    expires_at   TIMESTAMPTZ NOT NULL,
    created_by   TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, id),
    UNIQUE (community_id, token_hash),
    CHECK (max_uses IS NULL OR use_count <= max_uses)
);

CREATE INDEX relay_invites_expires_at_idx ON relay_invites (expires_at);

-- ── Archived identities (NIP-IA) ──────────────────────────────────────────────
-- Conformance: archive cannot hide a key in another community. PK scoped.

CREATE TABLE archived_identities (
    community_id      UUID NOT NULL REFERENCES communities(id),
    pubkey            TEXT NOT NULL,
    consent_path      TEXT NOT NULL CHECK (consent_path IN ('self', 'owner', 'admin')),
    actor             TEXT NOT NULL,
    reason            TEXT,
    replaced_by       TEXT,
    request_event_id  TEXT NOT NULL,
    archived_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, pubkey)
);

-- ── Audit log ─────────────────────────────────────────────────────────────────
-- Conformance: "Audit log and observability". Per-community hash chain:
-- uniqueness (community_id, seq) and (community_id, hash). One chain per tenant.
-- (Lane Audit/Dawn builds the chain logic; Lane 0 fixes the scoped schema.)

CREATE TABLE audit_log (
    community_id    UUID NOT NULL REFERENCES communities(id),
    seq             BIGINT NOT NULL,
    hash            BYTEA NOT NULL,
    prev_hash       BYTEA,
    action          VARCHAR(64) NOT NULL,
    actor_pubkey    BYTEA,
    object_id       TEXT,
    detail          JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (community_id, seq)
);

CREATE UNIQUE INDEX idx_audit_log_hash ON audit_log (community_id, hash);

-- ── NIP-56 reports (kind:1984 ingest) ─────────────────────────────────────────
-- One row per accepted report event. Reports are signals, never triggers:
-- nothing auto-actions on them (NIP-56). Reporter identity is visible to
-- moderators in the queue but never revealed to the reported author.

CREATE TABLE moderation_reports (
    community_id        UUID NOT NULL REFERENCES communities(id),
    id                  UUID NOT NULL DEFAULT gen_random_uuid(),
    -- The signed kind:1984 event id (stored for audit/idempotency).
    report_event_id     BYTEA NOT NULL CHECK (length(report_event_id) = 32),
    reporter_pubkey     BYTEA NOT NULL CHECK (length(reporter_pubkey) = 32),
    -- What was reported. Exactly one target class per row (CHECK-enforced below).
    target_kind         TEXT NOT NULL CHECK (target_kind IN ('event', 'pubkey', 'blob')),
    target_event_id     BYTEA CHECK (target_event_id IS NULL OR length(target_event_id) = 32),
    target_pubkey       BYTEA CHECK (target_pubkey IS NULL OR length(target_pubkey) = 32),
    target_blob_sha256  BYTEA CHECK (target_blob_sha256 IS NULL OR length(target_blob_sha256) = 32),
    -- Channel inferred from an in-tenant target event row, when resolvable.
    channel_id          UUID,
    -- NIP-56 report type: illegal|nudity|malware|spam|impersonation|profanity|other.
    report_type         TEXT NOT NULL,
    -- Reporter's optional free-text context (mod-queue-only; never public).
    note                TEXT,
    status              TEXT NOT NULL DEFAULT 'open'
                        CHECK (status IN ('open', 'processing', 'resolved', 'dismissed', 'escalated')),
    -- Non-null when status='processing': the relay_admin_actions row that claimed this report.
    active_action_id    UUID,
    resolved_by         BYTEA,
    resolved_at         TIMESTAMPTZ,
    -- moderation_actions row that resolved this report, if any.
    action_id           UUID,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, id),
    -- Exactly one target class per row: target_kind is authoritative and the
    -- matching column (only) is populated. Queue/action code never guesses.
    CHECK (
        (target_kind = 'event'  AND target_event_id IS NOT NULL AND target_pubkey IS NULL     AND target_blob_sha256 IS NULL) OR
        (target_kind = 'pubkey' AND target_event_id IS NULL     AND target_pubkey IS NOT NULL AND target_blob_sha256 IS NULL) OR
        (target_kind = 'blob'   AND target_event_id IS NULL     AND target_pubkey IS NULL     AND target_blob_sha256 IS NOT NULL)
    ),
    -- Same-community channel provenance (channels are soft-deleted, never
    -- hard-deleted, so this FK cannot dangle).
    FOREIGN KEY (community_id, channel_id) REFERENCES channels (community_id, id)
);

-- Queue reads: open reports, newest first, per community.
CREATE INDEX idx_moderation_reports_status
    ON moderation_reports (community_id, status, created_at DESC);
-- Group-by-target for triage aggregation.
CREATE INDEX idx_moderation_reports_target_event
    ON moderation_reports (community_id, target_event_id)
    WHERE target_event_id IS NOT NULL;
CREATE INDEX idx_moderation_reports_target_pubkey
    ON moderation_reports (community_id, target_pubkey)
    WHERE target_pubkey IS NOT NULL;
-- Idempotency: one row per report event per community.
CREATE UNIQUE INDEX idx_moderation_reports_event
    ON moderation_reports (community_id, report_event_id);

-- ── Bans + timeouts (one restriction row per member) ──────────────────────────
-- Ban = connection block, enforced at the NIP-42 auth seam
-- ("blocked: you are banned from this community") + join/ingest surfaces.
-- Timeout = write-block only ("restricted: you are timed out until <ts>").
-- A row may be ban-only, timeout-only, or both over its lifetime.

CREATE TABLE community_bans (
    community_id    UUID NOT NULL REFERENCES communities(id),
    pubkey          BYTEA NOT NULL CHECK (length(pubkey) = 32),
    banned          BOOLEAN NOT NULL DEFAULT false,
    -- NULL + banned=true ⇒ permanent.
    ban_expires_at  TIMESTAMPTZ,
    ban_reason      TEXT,
    -- Write-block until this timestamp; NULL or past ⇒ not timed out.
    muted_until     TIMESTAMPTZ,
    mute_reason     TEXT,
    -- Moderator who last modified this row.
    actor_pubkey    BYTEA NOT NULL CHECK (length(actor_pubkey) = 32),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, pubkey)
);

-- ── Moderation audit ──────────────────────────────────────────────────────────
-- One row per accepted moderation action. Full detail (reporter identities,
-- private reasons, matched NIP-OA principal) stays mod/audit-only; the public
-- tombstone carries only action_id + reason_code + sanitized public_reason.

CREATE TABLE moderation_actions (
    community_id    UUID NOT NULL REFERENCES communities(id),
    id              UUID NOT NULL DEFAULT gen_random_uuid(),
    actor_pubkey    BYTEA NOT NULL CHECK (length(actor_pubkey) = 32),
    action          TEXT NOT NULL CHECK (action IN (
                        'delete_message', 'kick', 'ban', 'unban',
                        'timeout', 'untimeout', 'dismiss_report', 'escalate',
                        'resolve:delete', 'resolve:kick', 'resolve:ban',
                        'resolve:timeout')),
    target_pubkey   BYTEA CHECK (target_pubkey IS NULL OR length(target_pubkey) = 32),
    target_event_id BYTEA CHECK (target_event_id IS NULL OR length(target_event_id) = 32),
    channel_id      UUID,
    -- Machine-readable rule/reason code (e.g. "spam", "community_rule_3").
    reason_code     TEXT,
    -- Sanitized, safe for the public tombstone.
    public_reason   TEXT,
    -- Mod-only context; never leaves the audit surface.
    private_reason  TEXT,
    -- NIP-OA: which principal matched a ban ('self' | 'owner'); audit-only,
    -- the client never learns which.
    matched_principal TEXT CHECK (matched_principal IS NULL OR matched_principal IN ('self', 'owner')),
    -- Deployment authority type for HTTP-initiated actions.
    actor_authority   TEXT NOT NULL DEFAULT 'community'
                      CHECK (actor_authority IN ('community', 'relay_operator', 'relay_moderator')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, id),
    FOREIGN KEY (community_id, channel_id) REFERENCES channels (community_id, id)
);

CREATE INDEX idx_moderation_actions_created
    ON moderation_actions (community_id, created_at DESC);
CREATE INDEX idx_moderation_actions_target_pubkey
    ON moderation_actions (community_id, target_pubkey)
    WHERE target_pubkey IS NOT NULL;

-- Same-community resolution provenance: a report can only be resolved by an
-- action row in its own community. Added after moderation_actions exists.
ALTER TABLE moderation_reports
    ADD FOREIGN KEY (community_id, action_id)
    REFERENCES moderation_actions (community_id, id);

-- ── Lint allowlist registry ───────────────────────────────────────────────────
-- The explicit registry of tables that are deliberately operator-global (NOT
-- tenant-scoped). The migration-lint harness reads this: any table NOT listed
-- here MUST carry a NOT NULL community_id and lead its uniques with it. Making
-- the allowlist a DB table (not a hard-coded list in the linter) keeps the
-- registry next to the schema it governs and reviewable in one migration diff.

CREATE TABLE _operator_global_tables (
    table_name  TEXT PRIMARY KEY,
    reason      TEXT NOT NULL
);

INSERT INTO _operator_global_tables (table_name, reason) VALUES
    ('communities',           'the tenant registry itself; id IS the community key'),
    ('rate_limit_violations', 'deployment abuse/health; never tenant-observable; community_id is an attribution label only'),
    ('_operator_global_tables', 'the registry table itself');

-- ── Additive tenant tables represented in migrations 0002/0007/0017 ──────────
-- Keep desired-state schema parity with the embedded SQLx migration path.
CREATE TABLE git_repo_names (
    community_id  UUID NOT NULL REFERENCES communities(id),
    repo_id       TEXT NOT NULL,
    owner_pubkey  TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, repo_id)
);
CREATE INDEX idx_git_repo_names_owner ON git_repo_names (community_id, owner_pubkey);

CREATE TABLE parameterized_event_watermarks (
    community_id  UUID NOT NULL REFERENCES communities(id),
    kind          INT NOT NULL,
    pubkey        BYTEA NOT NULL,
    d_tag         TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL,
    event_id      BYTEA NOT NULL,
    PRIMARY KEY (community_id, kind, pubkey, d_tag)
);
CREATE INDEX idx_event_mentions_community_event
    ON event_mentions (community_id, event_id);

CREATE TABLE product_feedback (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    community_id UUID REFERENCES communities(id) ON DELETE SET NULL,
    event_id BYTEA NOT NULL CHECK (length(event_id) = 32),
    submitter_pubkey BYTEA NOT NULL CHECK (length(submitter_pubkey) = 32),
    category TEXT CHECK (category IN ('bug', 'praise', 'needs-work')),
    body TEXT NOT NULL CHECK (length(btrim(body)) > 0),
    tags JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(tags) = 'array'),
    event_created_at TIMESTAMPTZ NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Operator-managed lifecycle status.
    status TEXT NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'reviewed', 'archived')),
    UNIQUE (event_id)
);
CREATE INDEX idx_product_feedback_received
    ON product_feedback (received_at DESC, id);
CREATE INDEX idx_product_feedback_community_received
    ON product_feedback (community_id, received_at DESC, id);
INSERT INTO _operator_global_tables (table_name, reason) VALUES
    ('product_feedback', 'deployment product inbox; community_id is provenance only');
-- NIP-PL effective lease state and durable wake outbox. Every key is led by
-- community_id: client-provided origin is confirmation only, never routing.
CREATE TABLE push_leases (
    community_id UUID NOT NULL REFERENCES communities(id),
    author BYTEA NOT NULL CHECK (length(author) = 32),
    installation_id TEXT NOT NULL CHECK (octet_length(installation_id) BETWEEN 1 AND 64),
    source_event_id BYTEA NOT NULL CHECK (length(source_event_id) = 32),
    source_created_at BIGINT NOT NULL,
    generation BIGINT NOT NULL CHECK (generation > 0),
    active BOOLEAN NOT NULL,
    endpoint_enabled BOOLEAN NOT NULL DEFAULT true,
    app_profile TEXT,
    endpoint_hash BYTEA CHECK (endpoint_hash IS NULL OR length(endpoint_hash) = 32),
    endpoint_grant TEXT,
    max_class TEXT CHECK (max_class IS NULL OR max_class IN ('silent','default','time_sensitive','urgent')),
    subscriptions JSONB,
    expires_at BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, author, installation_id),
    UNIQUE (community_id, source_event_id),
    CHECK ((active AND app_profile IS NOT NULL AND endpoint_hash IS NOT NULL AND endpoint_grant IS NOT NULL AND max_class IS NOT NULL AND subscriptions IS NOT NULL)
        OR (NOT active AND app_profile IS NULL AND endpoint_hash IS NULL AND endpoint_grant IS NULL AND max_class IS NULL AND subscriptions IS NULL))
);
CREATE UNIQUE INDEX push_leases_endpoint_unique
    ON push_leases (community_id, author, app_profile, endpoint_hash)
    WHERE active;
CREATE INDEX push_leases_expiry ON push_leases (community_id, expires_at) WHERE active;

CREATE TABLE push_wake_outbox (
    community_id UUID NOT NULL REFERENCES communities(id),
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    author BYTEA NOT NULL CHECK (length(author) = 32),
    installation_id TEXT NOT NULL,
    lease_generation BIGINT NOT NULL CHECK (lease_generation > 0),
    endpoint_hash BYTEA NOT NULL CHECK (length(endpoint_hash) = 32),
    event_id BYTEA NOT NULL CHECK (length(event_id) = 32),
    class TEXT NOT NULL CHECK (class IN ('silent','default','time_sensitive','urgent')),
    expires_at BIGINT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','sending','delivered','failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_until TIMESTAMPTZ,
    claim_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, id),
    FOREIGN KEY (community_id, author, installation_id)
        REFERENCES push_leases (community_id, author, installation_id),
    UNIQUE (community_id, endpoint_hash, event_id)
);
CREATE INDEX push_wake_outbox_due
    ON push_wake_outbox (community_id, next_attempt_at) WHERE state = 'pending';
CREATE INDEX push_wake_outbox_recovery
    ON push_wake_outbox (community_id, lease_until) WHERE state = 'sending';
-- Durable event-to-push matching follower. The trigger runs in the event insert
-- transaction, so every accepted persistent event has a crash-safe match job and
-- rejected/rolled-back events never do. Processing is idempotent through the
-- push_wake_outbox endpoint/event unique key.
CREATE TABLE push_match_queue (
    community_id UUID NOT NULL REFERENCES communities(id),
    event_id BYTEA NOT NULL CHECK (length(event_id) = 32),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','matching')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_until TIMESTAMPTZ,
    claim_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, event_id)
);
CREATE INDEX push_match_queue_due
    ON push_match_queue (next_attempt_at, created_at) WHERE state = 'pending';
CREATE INDEX push_match_queue_recovery
    ON push_match_queue (lease_until) WHERE state = 'matching';

-- T1b push gate (keep in sync with migrations/0023). Enqueue only when the
-- community has an active, endpoint-enabled, unexpired lease; the shared
-- advisory lock pairs with the exclusive lock taken by lease activations
-- (crates/buzz-db/src/push.rs) to close the lost-wake race.
CREATE FUNCTION enqueue_push_match_job() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    -- Keep this allowlist identical to the relay's validated NIP-PL descriptor.
    -- Centralizing it on the events table covers every durable producer,
    -- including internal paths that bypass live dispatch.
    IF NEW.kind IN (9, 40002, 45001, 45003) THEN
        PERFORM pg_advisory_xact_lock_shared(
            hashtextextended('buzz_push_gate:' || NEW.community_id::text, 0));
        IF EXISTS (
            SELECT 1 FROM push_leases
            WHERE community_id = NEW.community_id
              AND active
              AND endpoint_enabled
              AND expires_at > EXTRACT(EPOCH FROM now())::bigint
        ) THEN
            INSERT INTO push_match_queue (community_id, event_id)
            VALUES (NEW.community_id, NEW.id)
            ON CONFLICT DO NOTHING;
        END IF;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER events_enqueue_push_match
AFTER INSERT ON events
FOR EACH ROW EXECUTE FUNCTION enqueue_push_match_job();

-- Channel TTL refresh (keep in sync with migrations/0024). Runs deferred, in
-- the transaction that makes a channel-scoped event durable, so a TTL
-- transition committed while ingest was in flight is never missed. The
-- per-channel advisory lock is SHARED here — permanent-channel commits admit
-- each other — and taken EXCLUSIVE by TTL transitions (update_channel in
-- crates/buzz-db/src/channel.rs), which forces the same total order the
-- 0022 row lock provided without serializing the hot path.
CREATE FUNCTION refresh_channel_ttl_after_event_insert() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    channel_ttl INTEGER;
BEGIN
    -- Kind 9007 creates the channel and initializes its deadline itself.
    IF NEW.channel_id IS NOT NULL AND NEW.kind <> 9007 THEN
        BEGIN
            PERFORM pg_advisory_xact_lock_shared(hashtextextended(
                'buzz_channel_ttl:' || NEW.community_id::text || ':' || NEW.channel_id::text, 0));

            SELECT ttl_seconds INTO channel_ttl
            FROM channels
            WHERE community_id = NEW.community_id AND id = NEW.channel_id;

            IF channel_ttl IS NOT NULL THEN
                UPDATE channels
                SET ttl_deadline = clock_timestamp() + make_interval(secs => ttl_seconds)
                WHERE community_id = NEW.community_id
                  AND id = NEW.channel_id
                  AND ttl_seconds IS NOT NULL
                  AND archived_at IS NULL
                  AND deleted_at IS NULL;
            END IF;
        EXCEPTION WHEN OTHERS THEN
            -- Preserve the existing best-effort contract: a TTL refresh failure
            -- must not reject an otherwise valid durable event.
            RAISE WARNING 'channel TTL refresh failed for community %, channel %: %',
                NEW.community_id, NEW.channel_id, SQLERRM;
        END;
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER events_refresh_channel_ttl
AFTER INSERT ON events
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION refresh_channel_ttl_after_event_insert();

-- Channel roster snapshot fence (keep in sync with migrations/0032).
-- Prevent mixed-version relay pods from publishing a stale NIP-29 member
-- snapshot after a newer canonical roster has been committed.
--
-- Old binaries already serialize kind 39002 replacement on the replacement
-- advisory key. This trigger adds the channel-membership key at INSERT time,
-- after that canonical key, and validates every p tag against the current
-- active membership set and roles. New binaries take both keys in the same
-- order before capture and replacement. Thus old and new writers remain
-- compatible during a rolling deploy.
CREATE OR REPLACE FUNCTION guard_channel_roster_snapshot()
RETURNS TRIGGER AS $$
DECLARE
    canonical_members TEXT[];
    snapshot_members TEXT[];
BEGIN
    IF NEW.kind <> 39002 OR NEW.channel_id IS NULL THEN
        RETURN NEW;
    END IF;

    PERFORM pg_advisory_xact_lock(hashtextextended(
        'buzz_channel_membership:' || NEW.community_id::text || ':' || NEW.channel_id::text,
        0
    ));

    SELECT COALESCE(
               array_agg(encode(cm.pubkey, 'hex') || ':' || cm.role::text ORDER BY cm.pubkey),
               ARRAY[]::TEXT[]
           )
      INTO canonical_members
      FROM channel_members cm
     WHERE cm.community_id = NEW.community_id
       AND cm.channel_id = NEW.channel_id
       AND cm.removed_at IS NULL;

    -- A roster is canonical only when every p tag uses the emitted four-field
    -- shape, contains a 32-byte hex pubkey and valid authoritative role, has no
    -- duplicate members, and exactly matches the active membership rows.
    IF EXISTS (
        SELECT 1
          FROM jsonb_array_elements(NEW.tags) AS roster_tag(tag_json)
         WHERE roster_tag.tag_json->>0 = 'p'
           AND (
               jsonb_array_length(roster_tag.tag_json) <> 4
               OR COALESCE(roster_tag.tag_json->>1, '') !~ '^[0-9a-fA-F]{64}$'
               OR roster_tag.tag_json->>2 <> ''
               OR COALESCE(roster_tag.tag_json->>3, '') NOT IN ('owner', 'admin', 'bot', 'member', 'guest')
           )
    ) THEN
        RAISE EXCEPTION 'kind 39002 roster contains an invalid p tag'
            USING ERRCODE = '23514';
    END IF;

    SELECT COALESCE(
               array_agg(
                   lower((roster_tag.tag_json->>1)) || ':' || (roster_tag.tag_json->>3)
                   ORDER BY decode((roster_tag.tag_json->>1), 'hex')
               ),
               ARRAY[]::TEXT[]
           )
      INTO snapshot_members
      FROM jsonb_array_elements(NEW.tags) AS roster_tag(tag_json)
     WHERE roster_tag.tag_json->>0 = 'p';

    IF snapshot_members IS DISTINCT FROM canonical_members THEN
        RAISE EXCEPTION 'kind 39002 roster does not match canonical channel membership'
            USING ERRCODE = '23514';
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_events_guard_channel_roster_snapshot ON events;
CREATE TRIGGER trg_events_guard_channel_roster_snapshot
    BEFORE INSERT ON events
    FOR EACH ROW EXECUTE FUNCTION guard_channel_roster_snapshot();


-- Replica-fence floor guard (keep in sync with migrations/0021). A deferred
-- constraint trigger re-checks, inside COMMIT processing, that channel-bearing
-- event rows are no older than `buzz.created_at_floor` seconds before commit
-- time (clock_timestamp(), NOT the transaction-frozen now()). This turns the
-- relay's ingest-time created_at envelope into a commit-time storage
-- invariant, which is what lets keyset-cursor pages below the replica fence
-- be served by a read replica without holes. Enforcement is armed per session
-- via the GUC (set by the relay's writer pool on connect); sessions without
-- the GUC (pg_restore, manual backfills) bypass it and must hold the replica
-- fence closed for their duration. The only structural exemption is
-- channel_id IS NULL: those rows never appear in keyset-paged windows.
CREATE FUNCTION events_created_at_floor_guard() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    floor_secs numeric := nullif(current_setting('buzz.created_at_floor', true), '')::numeric;
BEGIN
    IF floor_secs IS NOT NULL
       AND floor_secs > 0
       AND NEW.channel_id IS NOT NULL
       AND NEW.created_at < clock_timestamp() - make_interval(secs => floor_secs)
    THEN
        RAISE EXCEPTION
            'events.created_at % is more than % s before commit time %; below the replica-fence floor',
            NEW.created_at, floor_secs, clock_timestamp()
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END
$$;

-- INSERT OR UPDATE OF: an UPDATE can move a previously exempt row into the
-- guarded set (channel_id NULL -> NOT NULL) or move a channel row's
-- created_at below the fence, so both mutation paths re-run the guard on the
-- NEW row. A created_at rewrite that crosses partition bounds runs as
-- DELETE + INSERT and hits the cloned AFTER INSERT guard on the destination
-- partition; an in-partition rewrite fires the UPDATE OF arm.
CREATE CONSTRAINT TRIGGER events_created_at_floor
    AFTER INSERT OR UPDATE OF created_at, channel_id ON events
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    EXECUTE FUNCTION events_created_at_floor_guard();

-- Durable, deployment-global authority for the public NIP-PL push gateway.
-- This state is intentionally outside relay community tenancy: installations
-- delegate to relay signing keys and may authorize multiple relay deployments.
CREATE TABLE push_gateway_challenges (
    id UUID PRIMARY KEY,
    challenge_hash BYTEA NOT NULL CHECK (length(challenge_hash) = 32),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX push_gateway_challenges_expiry ON push_gateway_challenges (expires_at);

CREATE TABLE push_gateway_installations (
    id UUID PRIMARY KEY,
    app_attest_key_id BYTEA NOT NULL UNIQUE CHECK (octet_length(app_attest_key_id) BETWEEN 1 AND 128),
    app_attest_public_key BYTEA NOT NULL CHECK (octet_length(app_attest_public_key) BETWEEN 33 AND 256),
    assertion_counter BIGINT NOT NULL CHECK (assertion_counter BETWEEN 0 AND 4294967295),
    app_profile TEXT NOT NULL CHECK (app_profile = 'buzz-ios-dogfood'),
    token_ciphertext BYTEA NOT NULL CHECK (octet_length(token_ciphertext) BETWEEN 1 AND 2048),
    token_fingerprint BYTEA NOT NULL CHECK (length(token_fingerprint) = 32),
    endpoint_epoch BIGINT NOT NULL CHECK (endpoint_epoch > 0),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (app_profile, token_fingerprint)
);
CREATE INDEX push_gateway_installations_expiry ON push_gateway_installations (expires_at) WHERE revoked_at IS NULL;

CREATE TABLE push_gateway_delegations (
    id UUID PRIMARY KEY,
    installation_id UUID NOT NULL REFERENCES push_gateway_installations(id),
    relay_pubkey BYTEA NOT NULL CHECK (length(relay_pubkey) = 32),
    endpoint_epoch BIGINT NOT NULL CHECK (endpoint_epoch > 0),
    generation BIGINT NOT NULL CHECK (generation > 0),
    not_before TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (installation_id, relay_pubkey),
    CHECK (not_before < expires_at)
);
CREATE INDEX push_gateway_delegations_expiry ON push_gateway_delegations (expires_at) WHERE revoked_at IS NULL;

CREATE TABLE push_gateway_endpoint_quotas (
    token_fingerprint BYTEA PRIMARY KEY CHECK (length(token_fingerprint) = 32),
    window_started_at TIMESTAMPTZ NOT NULL,
    admitted BIGINT NOT NULL CHECK (admitted >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX push_gateway_endpoint_quotas_updated ON push_gateway_endpoint_quotas (updated_at);

CREATE TABLE push_gateway_delivery_auth_replays (
    relay_pubkey BYTEA NOT NULL CHECK (length(relay_pubkey) = 32),
    auth_event_id BYTEA NOT NULL CHECK (length(auth_event_id) = 32),
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (relay_pubkey, auth_event_id)
);
CREATE INDEX push_gateway_delivery_auth_replays_expiry ON push_gateway_delivery_auth_replays (expires_at);

CREATE TABLE push_gateway_delivery_request_replays (
    relay_pubkey BYTEA NOT NULL CHECK (length(relay_pubkey) = 32),
    request_id UUID NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (relay_pubkey, request_id)
);
CREATE INDEX push_gateway_delivery_request_replays_expiry ON push_gateway_delivery_request_replays (expires_at);

INSERT INTO _operator_global_tables (table_name, reason) VALUES
    ('push_gateway_challenges', 'public gateway one-time challenges span relay communities'),
    ('push_gateway_installations', 'public gateway installation authority spans relay communities'),
    ('push_gateway_delegations', 'public gateway relay delegations span relay communities'),
    ('push_gateway_endpoint_quotas', 'public gateway endpoint abuse ceilings span relay communities'),
    ('push_gateway_delivery_auth_replays', 'public gateway signed-event replay admission spans relay communities'),
    ('push_gateway_delivery_request_replays', 'public gateway stable request-id admission spans relay communities');

-- ── Replica heartbeat (read-replica freshness fence) ─────────────────────────
-- Portable read-side freshness observation for the replica fence (see
-- crates/buzz-db/src/replica_fence.rs and migrations/0026). Exactly one row;
-- the single-row token UPDATE is the serialization point that makes tokens
-- globally commit-ordered across relay pods. `epoch` detects token resets
-- (restore/re-seed) so a stale retained token can never masquerade as fresh
-- coverage. Deployment-global by design: describes replication topology,
-- never tenant data.

CREATE TABLE replica_heartbeat (
    id    smallint PRIMARY KEY CHECK (id = 1),
    epoch uuid     NOT NULL DEFAULT gen_random_uuid(),
    token bigint   NOT NULL DEFAULT 0
) WITH (
    vacuum_truncate = false
);

INSERT INTO replica_heartbeat (id) VALUES (1);

INSERT INTO _operator_global_tables (table_name, reason) VALUES
    ('replica_heartbeat', 'single-row replication freshness token; describes deployment topology, never tenant data');

-- ── Ortak company control plane (migration 0045) ─────────────────────────────
-- Durable company boundary, employee identity/revisions/bindings, Office inbox,
-- routing decisions, delivery-chain authority, runs, provisioning, and outbox.
-- Ortak relations are scoped by company_id; office_company_bindings is the only
-- one carrying community_id and is fenced by the bootstrap loop below like any
-- other community-scoped table. Kept in sync with migration 0045.

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
-- Company-scoped Activity run list, newest first with a keyset on
-- (queued_at, id) (migration 0046).
CREATE INDEX idx_runs_company_queued
    ON runs (company_id, queued_at DESC, id DESC);

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

-- ── Ortak Work and Projects (migration 0047) ─────────────────────────────────
-- Company-scoped projects and work items with optimistic versions, dense
-- append-only histories, same-project dependencies, criteria and approval
-- gates that resolve once, and attachments to canonical records. Kept in sync
-- with migration 0047. `work_items.version` advances by exactly one per
-- committed history event and `work_item_history.sequence = version - 1`.

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

-- ── Whole-community deletion control plane (migration 0029) ─────────────────
CREATE TABLE community_deletion_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    community_id UUID NOT NULL REFERENCES communities(id),
    community_host TEXT NOT NULL,
    stage TEXT NOT NULL DEFAULT 'submitted' CHECK (stage IN (
        'submitted', 'inventoried', 'approved', 'fenced', 'drained',
        'bindings_removed', 'postgres_purged', 'cache_purged',
        'logically_verified', 'retention_pending', 'aborted'
    )),
    requested_by TEXT NOT NULL,
    reason TEXT,
    schema_manifest JSONB,
    storage_manifest JSONB,
    destructive_storage_manifest JSONB,
    destructive_storage_frozen_at TIMESTAMPTZ,
    inventory_manifest JSONB,
    inventory_digest BYTEA CHECK (inventory_digest IS NULL OR length(inventory_digest) = 32),
    inventory_frozen_at TIMESTAMPTZ,
    fence_generation BIGINT CHECK (fence_generation IS NULL OR fence_generation > 0),
    lease_owner TEXT,
    lease_generation BIGINT NOT NULL DEFAULT 0 CHECK (lease_generation >= 0),
    lease_until TIMESTAMPTZ,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    retry_stage TEXT CHECK (retry_stage IS NULL OR retry_stage IN (
        'approved', 'fenced', 'drained', 'bindings_removed',
        'postgres_purged', 'cache_purged', 'logically_verified'
    )),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error TEXT,
    last_error_at TIMESTAMPTZ,
    blocked_at TIMESTAMPTZ,
    blocked_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    pre_quiesce_archived_at TIMESTAMPTZ,
    quiescing_started_at TIMESTAMPTZ,
    aborted_by TEXT,
    abort_reason TEXT,
    aborted_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    CHECK ((blocked_at IS NULL) = (blocked_reason IS NULL)),
    CHECK ((stage = 'aborted') = (aborted_at IS NOT NULL)),
    CHECK ((aborted_at IS NULL) = (aborted_by IS NULL)),
    CHECK ((aborted_at IS NULL) = (abort_reason IS NULL)),
    CHECK ((inventory_frozen_at IS NULL) = (inventory_digest IS NULL)),
    UNIQUE (id, community_id, inventory_digest)
);
CREATE UNIQUE INDEX community_deletion_requests_active_community
    ON community_deletion_requests (community_id)
    WHERE stage <> 'aborted';
CREATE INDEX community_deletion_requests_runnable
    ON community_deletion_requests (next_attempt_at, created_at)
    WHERE blocked_at IS NULL
      AND stage IN ('approved', 'fenced', 'drained', 'bindings_removed',
                    'postgres_purged', 'cache_purged', 'logically_verified');
CREATE INDEX community_deletion_requests_lease
    ON community_deletion_requests (lease_until) WHERE lease_owner IS NOT NULL;

CREATE TABLE community_deletion_approvals (
    request_id UUID PRIMARY KEY,
    community_id UUID NOT NULL,
    inventory_digest BYTEA NOT NULL CHECK (length(inventory_digest) = 32),
    approved_by TEXT NOT NULL,
    note TEXT,
    approved_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (request_id, community_id, inventory_digest)
        REFERENCES community_deletion_requests(id, community_id, inventory_digest)
        ON DELETE RESTRICT
);

CREATE FUNCTION prevent_community_deletion_request_retargeting()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.community_id IS DISTINCT FROM OLD.community_id
        OR NEW.community_host IS DISTINCT FROM OLD.community_host
    THEN
        RAISE EXCEPTION 'community deletion target identity is immutable'
            USING ERRCODE = 'integrity_constraint_violation';
    END IF;
    IF OLD.inventory_frozen_at IS NOT NULL AND (
        NEW.schema_manifest IS DISTINCT FROM OLD.schema_manifest
        OR NEW.storage_manifest IS DISTINCT FROM OLD.storage_manifest
        OR NEW.inventory_manifest IS DISTINCT FROM OLD.inventory_manifest
        OR NEW.inventory_digest IS DISTINCT FROM OLD.inventory_digest
        OR NEW.inventory_frozen_at IS DISTINCT FROM OLD.inventory_frozen_at
    ) THEN
        RAISE EXCEPTION 'frozen community deletion inventory is immutable'
            USING ERRCODE = 'integrity_constraint_violation';
    END IF;
    IF OLD.destructive_storage_frozen_at IS NOT NULL AND (
        NEW.destructive_storage_manifest IS DISTINCT FROM OLD.destructive_storage_manifest
        OR NEW.destructive_storage_frozen_at IS DISTINCT FROM OLD.destructive_storage_frozen_at
    ) THEN
        RAISE EXCEPTION 'frozen destructive storage manifest is immutable'
            USING ERRCODE = 'integrity_constraint_violation';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER community_deletion_request_retargeting_guard
BEFORE UPDATE ON community_deletion_requests
FOR EACH ROW
EXECUTE FUNCTION prevent_community_deletion_request_retargeting();

CREATE FUNCTION prevent_community_deletion_approval_removal()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'community deletion approval evidence is immutable'
        USING ERRCODE = 'integrity_constraint_violation';
END;
$$;

CREATE TRIGGER community_deletion_approval_removal_guard
BEFORE UPDATE OR DELETE ON community_deletion_approvals
FOR EACH ROW
EXECUTE FUNCTION prevent_community_deletion_approval_removal();

CREATE TABLE community_deletion_checkpoints (
    request_id UUID NOT NULL REFERENCES community_deletion_requests(id) ON DELETE RESTRICT,
    sequence BIGINT GENERATED ALWAYS AS IDENTITY,
    stage TEXT NOT NULL,
    unit_key TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('started', 'completed', 'failed')),
    lease_generation BIGINT NOT NULL CHECK (lease_generation > 0),
    attempts INTEGER NOT NULL DEFAULT 1 CHECK (attempts > 0),
    detail JSONB NOT NULL DEFAULT '{}'::jsonb,
    error TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    PRIMARY KEY (request_id, sequence),
    UNIQUE (request_id, stage, unit_key),
    CHECK ((status = 'completed') = (completed_at IS NOT NULL)),
    CHECK ((status = 'failed') = (error IS NOT NULL))
);

-- Frozen destructive key list, chunked out of the request row so a large
-- tenant (100k-1M objects) never materializes as one multi-hundred-MB JSONB
-- value. Rows are written once in the fenced stage, stamped `deleted_at` as
-- the executor confirms each chunk removed, and dropped at logical
-- verification. The request row keeps only per-prefix count/bytes/digest
-- summaries; the chunk stream must hash to those frozen digests.
CREATE TABLE community_deletion_manifest_keys (
    request_id UUID NOT NULL REFERENCES community_deletion_requests(id) ON DELETE CASCADE,
    chunk_no BIGINT NOT NULL CHECK (chunk_no >= 0),
    prefix TEXT NOT NULL,
    keys JSONB NOT NULL,
    deleted_at TIMESTAMPTZ,
    PRIMARY KEY (request_id, chunk_no)
);

-- Chunk content is immutable once written; the only permitted update is the
-- one-way deleted_at stamp. New chunks are permitted only while the request is
-- fenced and its destructive manifest remains unfrozen. Removal is permitted
-- only while the destructive manifest has not yet frozen (a retried partial
-- freeze rewrites its chunks) or once the request has passed logical
-- verification (terminal cleanup).
CREATE FUNCTION protect_community_deletion_manifest_keys()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    frozen_at TIMESTAMPTZ;
    request_stage TEXT;
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF NEW.request_id IS DISTINCT FROM OLD.request_id
            OR NEW.chunk_no IS DISTINCT FROM OLD.chunk_no
            OR NEW.prefix IS DISTINCT FROM OLD.prefix
            OR NEW.keys IS DISTINCT FROM OLD.keys
            OR OLD.deleted_at IS NOT NULL
        THEN
            RAISE EXCEPTION 'community deletion manifest key chunks are immutable'
                USING ERRCODE = 'integrity_constraint_violation';
        END IF;
        RETURN NEW;
    END IF;
    SELECT destructive_storage_frozen_at, stage
      INTO frozen_at, request_stage
      FROM community_deletion_requests
     WHERE id = CASE WHEN TG_OP = 'INSERT' THEN NEW.request_id ELSE OLD.request_id END
     FOR UPDATE;
    IF TG_OP = 'INSERT' THEN
        IF FOUND AND frozen_at IS NULL AND request_stage = 'fenced' THEN
            RETURN NEW;
        END IF;
        RAISE EXCEPTION 'community deletion manifest key chunks require an unfrozen fenced request'
            USING ERRCODE = 'integrity_constraint_violation';
    END IF;
    IF NOT FOUND
        OR frozen_at IS NULL
        OR request_stage IN ('logically_verified', 'retention_pending')
    THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'community deletion manifest key chunks cannot be removed mid-execution'
        USING ERRCODE = 'integrity_constraint_violation';
END;
$$;

CREATE TRIGGER community_deletion_manifest_keys_guard
BEFORE INSERT OR UPDATE OR DELETE ON community_deletion_manifest_keys
FOR EACH ROW
EXECUTE FUNCTION protect_community_deletion_manifest_keys();

-- Fleet-wide object-store taxonomy sweep evidence. This is an independent
-- observability record: community deletion inventories only the target's owned
-- prefixes and does not gate submission or execution on sweep state.
CREATE TABLE storage_taxonomy_sweeps (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    started_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    listed_objects BIGINT NOT NULL CHECK (listed_objects >= 0),
    unknown_object_count BIGINT NOT NULL CHECK (unknown_object_count >= 0),
    unknown_key_sample JSONB NOT NULL DEFAULT '[]'::jsonb,
    object_cap BIGINT NOT NULL CHECK (object_cap > 0),
    CHECK (completed_at >= started_at)
);
CREATE INDEX storage_taxonomy_sweeps_latest
    ON storage_taxonomy_sweeps (completed_at DESC);

CREATE TABLE community_serving_write_leases (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    community_id UUID NOT NULL REFERENCES communities(id),
    operation TEXT NOT NULL,
    owner TEXT NOT NULL,
    generation BIGINT NOT NULL DEFAULT 1 CHECK (generation > 0),
    -- Community fence generation observed when this lease was acquired.
    fence_generation BIGINT NOT NULL CHECK (fence_generation >= 0),
    lease_until TIMESTAMPTZ NOT NULL,
    heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX community_serving_write_leases_active
    ON community_serving_write_leases (community_id, lease_until);

CREATE TABLE community_deletion_executor_heartbeats (
    executor_id TEXT PRIMARY KEY,
    mode TEXT NOT NULL CHECK (mode IN ('run', 'drain', 'worker')),
    request_id UUID REFERENCES community_deletion_requests(id) ON DELETE SET NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    draining BOOLEAN NOT NULL DEFAULT false,
    stopped_at TIMESTAMPTZ
);
INSERT INTO _operator_global_tables (table_name, reason) VALUES
    ('community_deletion_requests', 'deployment deletion lifecycle and frozen inventory'),
    ('community_deletion_approvals', 'deployment operator destructive approvals'),
    ('community_deletion_checkpoints', 'deployment deletion executor checkpoints and failures'),
    ('community_deletion_manifest_keys', 'deployment deletion frozen destructive key chunks'),
    ('storage_taxonomy_sweeps', 'deployment object-store taxonomy sweep evidence'),
    ('community_serving_write_leases', 'deployment serving side-effect leases drained by deletion'),
    ('community_deletion_executor_heartbeats', 'deployment deletion worker liveness');

CREATE FUNCTION community_deletion_lock_key(target UUID) RETURNS BIGINT
LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT hashtextextended('buzz-community-deletion:' || target::text, 0)
$$;
-- Keep the deletion control plane writable while its target tenant is fenced.
-- This predicate is the single SQL source of truth used by attachment and live
-- catalog validation.
CREATE FUNCTION community_write_fence_excluded_table(target NAME) RETURNS BOOLEAN
LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT target::TEXT = ANY (ARRAY[
        'community_deletion_requests', 'community_deletion_approvals',
        'community_deletion_checkpoints', 'community_serving_write_leases',
        'community_deletion_executor_heartbeats', 'product_feedback',
        'rate_limit_violations'
    ]::TEXT[])
$$;

-- Fleet-wide writers filter candidates through this VOLATILE predicate in
-- the mutating statement so fenced tenants are skipped before row triggers run.
CREATE FUNCTION community_write_allowed(target UUID) RETURNS BOOLEAN
LANGUAGE plpgsql VOLATILE AS $$
DECLARE
    lifecycle TEXT;
BEGIN
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'community writes require READ COMMITTED isolation'
            USING ERRCODE = 'invalid_transaction_state';
    END IF;

    IF target IS NULL THEN
        RETURN true;
    END IF;

    PERFORM pg_advisory_xact_lock_shared(community_deletion_lock_key(target));
    SELECT deletion_state
      INTO lifecycle
      FROM communities
     WHERE id = target;
    RETURN FOUND AND lifecycle = 'active';
END
$$;

CREATE FUNCTION assert_community_write_allowed(target UUID) RETURNS VOID
LANGUAGE plpgsql AS $$
DECLARE
    lifecycle TEXT;
    generation BIGINT;
    executor_community TEXT;
    executor_generation TEXT;
    serving_community TEXT;
    serving_lease_id TEXT;
    serving_owner TEXT;
    serving_generation TEXT;
    serving_fence_generation TEXT;
    serving_lease_valid BOOLEAN := false;
BEGIN
    -- The fence proof requires a fresh statement snapshot after lock grant;
    -- pinned RR/Serializable snapshots can retain pre-fence authorization.
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'community writes require READ COMMITTED isolation'
            USING ERRCODE = 'invalid_transaction_state';
    END IF;

    -- Nullable operator-attribution rows without a tenant are unrelated.
    IF target IS NULL THEN
        RETURN;
    END IF;

    PERFORM pg_advisory_xact_lock_shared(community_deletion_lock_key(target));
    SELECT deletion_state, deletion_fence_generation
      INTO lifecycle, generation
      FROM communities
     WHERE id = target;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'community write rejected: community % is missing', target
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;

    -- Authorization is evaluated independently for every community checked.
    executor_community := current_setting('buzz.deletion_executor_community', true);
    executor_generation := current_setting('buzz.deletion_fence_generation', true);
    IF executor_community = target::TEXT
       AND executor_generation ~ '^[0-9]+$'
       AND executor_generation::BIGINT = generation THEN
        RETURN;
    END IF;

    -- A serving mutation admitted before quiescing may finish only while its
    -- exact durable lease remains current and bound to this fence generation.
    serving_community := current_setting('buzz.serving_write_community', true);
    serving_lease_id := current_setting('buzz.serving_write_lease_id', true);
    serving_owner := current_setting('buzz.serving_write_owner', true);
    serving_generation := current_setting('buzz.serving_write_generation', true);
    serving_fence_generation := current_setting('buzz.serving_write_fence_generation', true);
    IF lifecycle IN ('active', 'quiescing')
       AND serving_community = target::TEXT
       AND serving_lease_id ~ '^[0-9a-fA-F-]{36}$'
       AND serving_generation ~ '^[0-9]+$'
       AND serving_fence_generation ~ '^[0-9]+$'
       AND serving_fence_generation::BIGINT = generation THEN
        SELECT EXISTS(
            SELECT 1 FROM community_serving_write_leases lease
             WHERE lease.id = serving_lease_id::UUID
               AND lease.community_id = target
               AND lease.owner = serving_owner
               AND lease.generation = serving_generation::BIGINT
               AND lease.fence_generation = serving_fence_generation::BIGINT
               AND lease.lease_until >= now()
        ) INTO serving_lease_valid;
        IF serving_lease_valid THEN
            RETURN;
        END IF;
    END IF;

    IF lifecycle <> 'active' THEN
        RAISE EXCEPTION 'community write fenced: community % generation %', target, generation
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
END
$$;

CREATE FUNCTION enforce_community_write_fence() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        PERFORM assert_community_write_allowed(NEW.community_id);
    ELSIF TG_OP = 'DELETE' THEN
        PERFORM assert_community_write_allowed(OLD.community_id);
    ELSIF OLD.community_id IS NOT DISTINCT FROM NEW.community_id THEN
        PERFORM assert_community_write_allowed(OLD.community_id);
    ELSIF OLD.community_id IS NULL THEN
        PERFORM assert_community_write_allowed(NEW.community_id);
    ELSIF NEW.community_id IS NULL THEN
        PERFORM assert_community_write_allowed(OLD.community_id);
    ELSIF OLD.community_id < NEW.community_id THEN
        PERFORM assert_community_write_allowed(OLD.community_id);
        PERFORM assert_community_write_allowed(NEW.community_id);
    ELSE
        PERFORM assert_community_write_allowed(NEW.community_id);
        PERFORM assert_community_write_allowed(OLD.community_id);
    END IF;

    RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END
$$;

CREATE FUNCTION enforce_community_tombstone() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    executor_community TEXT := current_setting('buzz.deletion_executor_community', true);
    executor_generation TEXT := current_setting('buzz.deletion_fence_generation', true);
    expected_generation BIGINT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.deletion_state <> 'active' OR OLD.deleted_at IS NOT NULL THEN
            RAISE EXCEPTION 'community tombstones are permanent'
                USING ERRCODE = 'object_not_in_prerequisite_state';
        END IF;
        RETURN OLD;
    END IF;
    expected_generation := CASE WHEN NEW.deletion_fence_generation > OLD.deletion_fence_generation
        THEN NEW.deletion_fence_generation ELSE OLD.deletion_fence_generation END;
    IF executor_community = OLD.id::text AND executor_generation ~ '^[0-9]+$'
       AND executor_generation::BIGINT = expected_generation THEN RETURN NEW; END IF;
    IF OLD.deletion_state <> 'active' OR NEW.deletion_state <> OLD.deletion_state
       OR NEW.deletion_fence_generation <> OLD.deletion_fence_generation
       OR NEW.deleted_at IS DISTINCT FROM OLD.deleted_at THEN
        RAISE EXCEPTION 'community tombstone mutation rejected: community % generation %',
            OLD.id, OLD.deletion_fence_generation
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER communities_deletion_tombstone BEFORE UPDATE OR DELETE ON communities
FOR EACH ROW EXECUTE FUNCTION enforce_community_tombstone();
-- Attach the universal fence to one community-scoped relation. Future
-- migrations must invoke this helper explicitly after CREATE/ALTER introduces
-- community_id; the migration lint enforces that contract.
CREATE FUNCTION attach_community_write_fence(target REGCLASS) RETURNS VOID
LANGUAGE plpgsql AS $$
DECLARE
    relation_name NAME;
BEGIN
    SELECT c.relname
      INTO relation_name
      FROM pg_class c
      JOIN pg_namespace n ON n.oid = c.relnamespace
     WHERE c.oid = target
       AND n.nspname = current_schema()
       AND c.relkind IN ('r', 'p')
       AND NOT c.relispartition;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'community write fence target % is not a table in the current schema', target
            USING ERRCODE = 'wrong_object_type';
    END IF;
    IF community_write_fence_excluded_table(relation_name) THEN
        RETURN;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_attribute
         WHERE attrelid = target AND attname = 'community_id' AND NOT attisdropped
    ) THEN
        RAISE EXCEPTION 'community write fence target % has no community_id', target
            USING ERRCODE = 'undefined_column';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
         WHERE tgrelid = target
           AND tgname = 'community_write_fence_' || relation_name
           AND NOT tgisinternal
    ) THEN
        EXECUTE format(
            'CREATE TRIGGER %I BEFORE INSERT OR UPDATE OR DELETE ON %s '
            'FOR EACH ROW EXECUTE FUNCTION enforce_community_write_fence()',
            'community_write_fence_' || relation_name,
            target
        );
    END IF;
END
$$;

-- Attach the universal fence to every existing table carrying community_id,
-- including deployment-private sidecars whose community_id is provenance.
DO $$
DECLARE
    target REGCLASS;
BEGIN
    FOR target IN
        SELECT c.oid::REGCLASS
          FROM pg_class c
          JOIN pg_namespace n ON n.oid = c.relnamespace
          JOIN pg_attribute a ON a.attrelid = c.oid
         WHERE n.nspname = current_schema()
           AND c.relkind IN ('r', 'p')
           AND NOT c.relispartition
           AND a.attname = 'community_id'
           AND NOT a.attisdropped
           AND NOT community_write_fence_excluded_table(c.relname)
         ORDER BY c.oid::REGCLASS::TEXT
    LOOP
        PERFORM attach_community_write_fence(target);
    END LOOP;
END
$$;

-- Desired-state schema application does not replay migration history, so keep
-- these explicit calls as first-class catalog declarations. They also make the
-- fence contract visible to migration linting instead of hiding it only in the
-- dynamic bootstrap loop above.
SELECT attach_community_write_fence('api_tokens');
SELECT attach_community_write_fence('archived_identities');
SELECT attach_community_write_fence('audit_log');
SELECT attach_community_write_fence('channel_members');
SELECT attach_community_write_fence('channels');
SELECT attach_community_write_fence('community_bans');
SELECT attach_community_write_fence('delivery_log');
SELECT attach_community_write_fence('event_mentions');
SELECT attach_community_write_fence('events');
SELECT attach_community_write_fence('git_repo_names');
SELECT attach_community_write_fence('join_policy_acceptances');
SELECT attach_community_write_fence('moderation_actions');
SELECT attach_community_write_fence('moderation_reports');
SELECT attach_community_write_fence('parameterized_event_watermarks');
SELECT attach_community_write_fence('pubkey_allowlist');
SELECT attach_community_write_fence('push_leases');
SELECT attach_community_write_fence('push_match_queue');
SELECT attach_community_write_fence('push_wake_outbox');
SELECT attach_community_write_fence('reactions');
SELECT attach_community_write_fence('relay_invites');
SELECT attach_community_write_fence('relay_members');
SELECT attach_community_write_fence('scheduled_workflow_fires');
SELECT attach_community_write_fence('subscriptions');
SELECT attach_community_write_fence('thread_metadata');
SELECT attach_community_write_fence('users');
SELECT attach_community_write_fence('workflow_approvals');
SELECT attach_community_write_fence('workflow_runs');
SELECT attach_community_write_fence('workflows');

-- ── Relay operator/moderator roster ──────────────────────────────────────────
-- Deployment-level principals staffed via the admin API. Config-backed operators
-- (RELAY_OPERATOR_PUBKEYS, RELAY_OWNER_PUBKEY owner-fallback) are NOT seeded here;
-- they are authoritative in config and outrank any DB row.

CREATE TABLE relay_operators (
    pubkey      BYTEA NOT NULL PRIMARY KEY CHECK (length(pubkey) = 32),
    role        TEXT NOT NULL CHECK (role IN ('operator', 'moderator')),
    added_by    BYTEA NOT NULL CHECK (length(added_by) = 32),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO _operator_global_tables (table_name, reason) VALUES
    ('relay_operators', 'deployment-global operator/moderator roster; no community_id intentionally');

-- ── Relay admin actions (HTTP enforcement state machine) ──────────────────────
-- One row per HTTP report-resolution enforcement action. Tracks the durable
-- state machine from claim → enforcing → succeeded|failed|cancelled.

CREATE TABLE relay_admin_actions (
    id              UUID NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
    report_id       UUID NOT NULL,
    report_community_id UUID NOT NULL,
    -- Client-generated idempotency key (signed in NIP-98 request body).
    request_id      UUID NOT NULL,
    -- Principal who claimed the report.
    actor_pubkey    BYTEA NOT NULL CHECK (length(actor_pubkey) = 32),
    actor_role      TEXT NOT NULL CHECK (actor_role IN ('operator', 'moderator')),
    -- The enforcement action requested.
    action          TEXT NOT NULL,
    reason          TEXT,
    -- Timeout expiration for timeout actions; NULL otherwise.
    timeout_until   TIMESTAMPTZ,
    -- Durable state machine: pending → enforcing → succeeded|failed|cancelled.
    state           TEXT NOT NULL DEFAULT 'pending'
                    CHECK (state IN ('pending', 'enforcing', 'succeeded', 'failed', 'cancelled')),
    -- Step marker: the last durably committed mutation step (NULL = none yet).
    -- Values: 'mutation_committed' (core DB mutation done), 'artifacts_done' (tombstone/notice done).
    step_marker     TEXT CHECK (step_marker IN ('mutation_committed', 'artifacts_done')),
    -- Principal who cancelled a pre-mutation failed action; NULL until cancelled.
    -- Attributes the cancel transition on the action row itself, mirroring
    -- moderation_reports.resolved_by for report resolution.
    cancelled_by    BYTEA CHECK (cancelled_by IS NULL OR length(cancelled_by) = 32),
    -- Error from the last failure, if any.
    error_message   TEXT,
    -- Per-action exclusive lease (migration 0037): fences concurrent same-request
    -- retries and lets the recovery worker claim/re-drive stranded actions.
    action_lease_token      UUID,
    action_lease_expires_at TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Report-scoped idempotency: one action per (report, request_id).
    UNIQUE (report_community_id, report_id, request_id),
    FOREIGN KEY (report_community_id, report_id)
        REFERENCES moderation_reports (community_id, id)
);

CREATE INDEX idx_relay_admin_actions_report
    ON relay_admin_actions (report_community_id, report_id);
CREATE INDEX idx_relay_admin_actions_state
    ON relay_admin_actions (state)
    WHERE state IN ('pending', 'enforcing');
-- Recovery worker (migration 0037): find stranded actions by lease expiry.
CREATE INDEX idx_relay_admin_actions_lease
    ON relay_admin_actions (action_lease_expires_at)
    WHERE state IN ('pending', 'enforcing');

INSERT INTO _operator_global_tables (table_name, reason) VALUES
    ('relay_admin_actions', 'deployment-global enforcement state machine; community_id is embedded in report FK');

-- ── Relay admin outbox (durable enforcement delivery) ────────────────────────
-- Transactional outbox for durable artifact/notice delivery.

CREATE TABLE relay_admin_outbox (
    id          UUID NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
    action_id   UUID NOT NULL REFERENCES relay_admin_actions(id),
    -- Delivery task type: 'tombstone' | 'system_message' | 'reporter_notice'.
    task_type   TEXT NOT NULL,
    -- Task payload (JSON).
    payload     JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Lease-based delivery: held_by identifies the worker pod.
    held_by     TEXT,
    lease_expires_at TIMESTAMPTZ,
    -- Delivery state: pending → delivered | failed.
    state       TEXT NOT NULL DEFAULT 'pending'
                CHECK (state IN ('pending', 'delivered', 'failed')),
    -- Deduplication key: prevents re-creating an artifact after delivery.
    dedup_key   TEXT UNIQUE,
    error_message TEXT,
    -- Retryable delivery with backoff (migration 0037): failures reschedule via
    -- retry_after rather than terminating immediately.
    attempt_count INT NOT NULL DEFAULT 0,
    retry_after   TIMESTAMPTZ,
    -- Per-claim ownership fence (migration 0038): completion/failure updates
    -- require the token written at claim time, so a stale worker cannot overwrite
    -- a newer worker's terminal update.
    outbox_claim_token UUID,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_relay_admin_outbox_action
    ON relay_admin_outbox (action_id);
CREATE INDEX idx_relay_admin_outbox_pending
    ON relay_admin_outbox (retry_after, created_at)
    WHERE state = 'pending';

INSERT INTO _operator_global_tables (table_name, reason) VALUES
    ('relay_admin_outbox', 'deployment-global enforcement artifact delivery queue');

-- ── Relay operator audit (append-only roster mutation trail) ─────────────────
-- One row per PUT/DELETE /operators/{pubkey} mutation. The roster is the
-- deployment-wide root of trust and its mutations overwrite/remove in place;
-- this append-only trail records who granted, elevated, or revoked whom, and
-- when, so privilege changes are as auditable as the enforcement actions those
-- principals perform. Written only inside the upsert/delete transactions; no
-- UPDATE/DELETE path.

CREATE TABLE relay_operator_audit (
    id            UUID NOT NULL PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_pubkey  BYTEA NOT NULL CHECK (length(actor_pubkey) = 32),
    target_pubkey BYTEA NOT NULL CHECK (length(target_pubkey) = 32),
    op            TEXT NOT NULL CHECK (op IN ('grant', 'revoke')),
    prev_role     TEXT CHECK (prev_role IN ('operator', 'moderator')),
    new_role      TEXT CHECK (new_role IN ('operator', 'moderator')),
    -- created_at is wall-clock occurrence time (clock_timestamp()), informational
    -- only — not monotonic, so it never establishes order. `seq` is the sole
    -- chronology key: mutations write their audit row under the serializing lock,
    -- so identity order equals the true privilege chain. Reads use ORDER BY seq.
    created_at    TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    seq           BIGINT GENERATED ALWAYS AS IDENTITY
);

CREATE INDEX idx_relay_operator_audit_target
    ON relay_operator_audit (target_pubkey, seq);

INSERT INTO _operator_global_tables (table_name, reason) VALUES
    ('relay_operator_audit', 'deployment-global append-only roster mutation audit trail; no community_id intentionally');


-- Ortak Office authority fence (migration 0048).
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

-- Durable adapter stop acknowledgements. Local terminal run state alone is
-- insufficient: a lost start acknowledgement must still be stopped by run key.
CREATE TABLE runtime_cancellations (
    company_id UUID NOT NULL REFERENCES companies(id),
    run_id UUID NOT NULL,
    reason TEXT NOT NULL CHECK (reason IN ('office_revoked', 'human_requested')),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'acknowledged', 'failed')),
    attempt_count INT NOT NULL DEFAULT 0 CHECK (attempt_count BETWEEN 0 AND 20),
    max_attempts INT NOT NULL DEFAULT 20 CHECK (max_attempts BETWEEN 1 AND 20),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK (last_error_code ~ '^[a-z][a-z0-9_.]{0,63}$'),
    acknowledged_at TIMESTAMPTZ,
    PRIMARY KEY (company_id, run_id),
    FOREIGN KEY (company_id, run_id) REFERENCES runs(company_id, id),
    CHECK (attempt_count <= max_attempts),
    CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL)),
    CHECK ((state = 'acknowledged') = (acknowledged_at IS NOT NULL)),
    CHECK (state = 'pending' OR lease_token IS NULL)
);
CREATE INDEX idx_runtime_cancellations_due
    ON runtime_cancellations (company_id, next_attempt_at, requested_at, run_id)
    WHERE state = 'pending';

CREATE FUNCTION ortak_runtime_cancellation_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id <> OLD.company_id OR NEW.run_id <> OLD.run_id
       OR NEW.reason <> OLD.reason OR NEW.requested_at <> OLD.requested_at
       OR NEW.max_attempts <> OLD.max_attempts OR NEW.attempt_count < OLD.attempt_count
       OR OLD.state <> 'pending'
    THEN
        RAISE EXCEPTION 'ortak: cancellation attribution is immutable and terminal state is final'
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_runtime_cancellations_guard BEFORE UPDATE ON runtime_cancellations
    FOR EACH ROW EXECUTE FUNCTION ortak_runtime_cancellation_guard();
CREATE TRIGGER trg_runtime_cancellations_no_delete BEFORE DELETE ON runtime_cancellations
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();


-- Private MVP product request audit and supervised cancellation queue.
-- Purely additive company-scoped schema; never stores signed auth JSON or keys.

CREATE TABLE ortak_api_audit (
    company_id UUID NOT NULL REFERENCES companies(id),
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    actor_pubkey TEXT NOT NULL CHECK (actor_pubkey ~ '^[0-9a-f]{64}$'),
    auth_event_id BYTEA NOT NULL CHECK (octet_length(auth_event_id) = 32),
    action TEXT NOT NULL CHECK (action IN ('access', 'read_runs', 'read_run', 'read_events', 'read_employees', 'read_employee', 'cancel_run')),
    outcome TEXT NOT NULL CHECK (outcome IN ('denied', 'not_found', 'requested', 'already_requested', 'already_terminal')),
    -- Requested identifier only. No FK: denied and nonexistent targets must
    -- be auditable without looking up another company's row.
    requested_run_id UUID,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, id)
);
CREATE INDEX idx_ortak_api_audit_time ON ortak_api_audit (company_id, recorded_at DESC, id DESC);
CREATE TRIGGER trg_ortak_api_audit_immutable BEFORE UPDATE OR DELETE ON ortak_api_audit
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TABLE run_cancel_requests (
    company_id UUID NOT NULL REFERENCES companies(id),
    run_id UUID NOT NULL,
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    requested_by TEXT NOT NULL CHECK (requested_by ~ '^[0-9a-f]{64}$'),
    auth_event_id BYTEA NOT NULL CHECK (octet_length(auth_event_id) = 32),
    reason_code TEXT NOT NULL DEFAULT 'human_requested' CHECK (reason_code = 'human_requested'),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- pending is a request, never a claim that Hermes has stopped.
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'acknowledged', 'failed')),
    attempts INT NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 20),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK (last_error_code ~ '^[a-z][a-z0-9_.]{0,63}$'),
    acknowledged_at TIMESTAMPTZ,
    PRIMARY KEY (company_id, run_id),
    UNIQUE (company_id, id),
    FOREIGN KEY (company_id, run_id) REFERENCES runs(company_id, id),
    CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL)),
    CHECK ((status = 'acknowledged') = (acknowledged_at IS NOT NULL)),
    CHECK (status = 'pending' OR lease_token IS NULL)
);
CREATE INDEX idx_run_cancel_requests_due ON run_cancel_requests (company_id, next_attempt_at, requested_at)
    WHERE status = 'pending';

CREATE FUNCTION ortak_cancel_request_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id <> OLD.company_id OR NEW.run_id <> OLD.run_id OR NEW.id <> OLD.id
       OR NEW.requested_by <> OLD.requested_by OR NEW.auth_event_id <> OLD.auth_event_id
       OR NEW.reason_code <> OLD.reason_code OR NEW.requested_at <> OLD.requested_at
       OR NEW.attempts < OLD.attempts OR OLD.status <> 'pending'
    THEN
        RAISE EXCEPTION 'ortak: cancellation request attribution is immutable and terminal state is final'
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_run_cancel_requests_guard BEFORE UPDATE ON run_cancel_requests
    FOR EACH ROW EXECUTE FUNCTION ortak_cancel_request_guard();
CREATE TRIGGER trg_run_cancel_requests_no_delete BEFORE DELETE ON run_cancel_requests
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- Completion commits its publication job, including when the process crashes
-- before a worker can construct the canonical draft.
CREATE TABLE runtime_office_outputs (
    company_id UUID NOT NULL REFERENCES companies(id),
    run_id UUID NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','enqueued','failed')),
    attempt_count INT NOT NULL DEFAULT 0 CHECK (attempt_count BETWEEN 0 AND 20),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK (last_error_code ~ '^[a-z][a-z0-9_.]{0,63}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    draft_kind INT CHECK (draft_kind IN (9,40002)),
    draft_tags JSONB CHECK (jsonb_typeof(draft_tags)='array' AND octet_length(draft_tags::text)<=32768),
    draft_content TEXT CHECK (octet_length(draft_content) BETWEEN 1 AND 32768 AND btrim(draft_content)<>''),
    draft_created_at TIMESTAMPTZ,
    source_facts JSONB CHECK (jsonb_typeof(source_facts)='object' AND octet_length(source_facts::text)<=4096),
    office_authority_generation BIGINT CHECK (office_authority_generation>=0),
    office_authority_valid_before TIMESTAMPTZ,
    office_authority_token UUID,
    outbox_id UUID,
    enqueued_at TIMESTAMPTZ,
    PRIMARY KEY (company_id,run_id),
    FOREIGN KEY (company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY (company_id,outbox_id) REFERENCES outbox(company_id,id),
    CHECK ((lease_token IS NULL)=(lease_expires_at IS NULL)),
    CHECK (state='pending' OR lease_token IS NULL),
    CHECK ((state='enqueued')=(outbox_id IS NOT NULL)),
    CHECK ((state='enqueued')=(enqueued_at IS NOT NULL)),
    CHECK ((draft_kind IS NULL AND draft_tags IS NULL AND draft_content IS NULL
            AND draft_created_at IS NULL AND source_facts IS NULL AND office_authority_generation IS NULL
            AND office_authority_valid_before IS NULL AND office_authority_token IS NULL)
        OR (draft_kind IS NOT NULL AND draft_tags IS NOT NULL AND draft_content IS NOT NULL
            AND draft_created_at IS NOT NULL AND source_facts IS NOT NULL AND office_authority_generation IS NOT NULL
            AND office_authority_token IS NOT NULL)),
    CHECK (state<>'enqueued' OR draft_kind IS NOT NULL)
);
CREATE INDEX idx_runtime_office_outputs_due ON runtime_office_outputs
    (company_id,next_attempt_at,created_at,run_id) WHERE state='pending';

CREATE FUNCTION ortak_runtime_office_output_guard() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.company_id<>OLD.company_id OR NEW.run_id<>OLD.run_id OR NEW.created_at<>OLD.created_at
       OR NEW.attempt_count<OLD.attempt_count OR OLD.state<>'pending'
       OR (NEW.state='enqueued' AND NEW.office_authority_token IS NOT DISTINCT FROM OLD.office_authority_token)
       OR (OLD.draft_kind IS NOT NULL AND
           ROW(NEW.draft_kind,NEW.draft_tags,NEW.draft_content,NEW.draft_created_at,NEW.source_facts)
           IS DISTINCT FROM ROW(OLD.draft_kind,OLD.draft_tags,OLD.draft_content,OLD.draft_created_at,OLD.source_facts))
    THEN
        RAISE EXCEPTION 'ortak: Office output draft is immutable and terminal job state is final'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_runtime_office_output_guard BEFORE UPDATE ON runtime_office_outputs
    FOR EACH ROW EXECUTE FUNCTION ortak_runtime_office_output_guard();
CREATE TRIGGER trg_runtime_office_output_no_delete BEFORE DELETE ON runtime_office_outputs
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE FUNCTION ortak_schedule_completed_office_output() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.status='completed' AND NEW.delivery_intent IN ('reply','channel') THEN
        INSERT INTO runtime_office_outputs(company_id,run_id) VALUES (NEW.company_id,NEW.id)
        ON CONFLICT (company_id,run_id) DO NOTHING;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_runs_schedule_office_output AFTER INSERT OR UPDATE OF status,delivery_intent ON runs
    FOR EACH ROW EXECUTE FUNCTION ortak_schedule_completed_office_output();

CREATE FUNCTION ortak_check_office_output_authority() RETURNS TRIGGER AS $$
DECLARE current_generation BIGINT;
BEGIN
    IF NEW.draft_kind IS NULL OR (TG_OP='UPDATE' AND
       ROW(NEW.office_authority_token,NEW.office_authority_generation,NEW.office_authority_valid_before)
       IS NOT DISTINCT FROM ROW(OLD.office_authority_token,OLD.office_authority_generation,OLD.office_authority_valid_before)) THEN
        RETURN NEW;
    END IF;
    current_generation:=ortak_lock_office_authority(NEW.company_id);
    IF NEW.office_authority_generation IS DISTINCT FROM current_generation
       OR (NEW.office_authority_valid_before IS NOT NULL
           AND clock_timestamp()>=NEW.office_authority_valid_before) THEN
        RAISE EXCEPTION 'ortak: Office output authority changed or expired' USING ERRCODE='serialization_failure';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE CONSTRAINT TRIGGER trg_runtime_office_output_authority AFTER INSERT OR UPDATE ON runtime_office_outputs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_office_output_authority();

INSERT INTO runtime_office_outputs(company_id,run_id)
SELECT company_id,id FROM runs WHERE status='completed' AND delivery_intent IN ('reply','channel')
ON CONFLICT (company_id,run_id) DO NOTHING;

-- Ortak durable memory jobs and immutable run context (0052).
-- Memory identity participates in the same generation as Office admission.
CREATE TRIGGER ortak_office_authority_memory_bindings BEFORE INSERT OR UPDATE OR DELETE ON employee_memory_bindings
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company','company_id','revision_id','employee_id','adapter','provisioning_mode','endpoint_ref','workspace','user_peer','employee_peer','options','validated_at');
CREATE TRIGGER ortak_office_no_truncate BEFORE TRUNCATE ON employee_memory_bindings
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

-- Exact serialized pre-start input. The runtime owns validation and first-writer
-- admission; the database prevents retries from replacing an existing request.
CREATE TABLE run_context_snapshots (
    company_id UUID NOT NULL,
    run_id UUID NOT NULL,
    spec_bytes BYTEA NOT NULL CHECK (octet_length(spec_bytes) BETWEEN 1 AND 262144),
    spec_hash BYTEA NOT NULL CHECK (octet_length(spec_hash)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id,run_id),
    FOREIGN KEY (company_id,run_id) REFERENCES runs(company_id,id)
);
CREATE TRIGGER trg_run_context_snapshot_immutable BEFORE UPDATE OR DELETE ON run_context_snapshots
FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- Acknowledged Office replies are the only automatic memory input. RunScratch
-- preserves the original run boundary; no project/global promotion is implied.
CREATE TABLE runtime_memory_writes (
    company_id UUID NOT NULL,
    run_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    employee_revision_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    outbox_id UUID NOT NULL,
    signed_event_id BYTEA NOT NULL CHECK (octet_length(signed_event_id)=32),
    binding JSONB NOT NULL CHECK (jsonb_typeof(binding)='object' AND octet_length(binding::text)<=32768),
    source_facts JSONB NOT NULL CHECK (jsonb_typeof(source_facts)='object' AND octet_length(source_facts::text)<=4096),
    content TEXT NOT NULL CHECK (octet_length(content) BETWEEN 1 AND 32768 AND btrim(content)<>''),
    recorded_at TIMESTAMPTZ NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (idempotency_key='office-output:'||run_id::text),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','acknowledged','failed')),
    attempt_count INT NOT NULL DEFAULT 0 CHECK (attempt_count BETWEEN 0 AND 20),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK (last_error_code ~ '^[a-z][a-z0-9_.]{0,63}$'),
    admission_generation BIGINT CHECK (admission_generation>=0),
    admission_valid_before TIMESTAMPTZ,
    admission_token UUID,
    receipt JSONB CHECK (jsonb_typeof(receipt)='object' AND octet_length(receipt::text)<=4096),
    acknowledged_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id,run_id),
    UNIQUE (company_id,outbox_id),
    FOREIGN KEY (company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY (company_id,outbox_id) REFERENCES outbox(company_id,id),
    FOREIGN KEY (company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id),
    CHECK ((lease_token IS NULL)=(lease_expires_at IS NULL)),
    CHECK (state='pending' OR lease_token IS NULL),
    CHECK ((admission_generation IS NULL)=(admission_token IS NULL)),
    CHECK ((state='acknowledged')=(receipt IS NOT NULL)),
    CHECK ((state='acknowledged')=(acknowledged_at IS NOT NULL))
);
CREATE INDEX idx_runtime_memory_writes_due ON runtime_memory_writes
    (company_id,next_attempt_at,created_at,run_id) WHERE state='pending';

CREATE FUNCTION ortak_memory_write_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.state<>'pending' OR NEW.attempt_count<OLD.attempt_count OR
       ROW(NEW.company_id,NEW.run_id,NEW.employee_id,NEW.employee_revision_id,NEW.channel_id,
           NEW.outbox_id,NEW.signed_event_id,NEW.binding,NEW.source_facts,NEW.content,
           NEW.recorded_at,NEW.idempotency_key,NEW.created_at)
       IS DISTINCT FROM
       ROW(OLD.company_id,OLD.run_id,OLD.employee_id,OLD.employee_revision_id,OLD.channel_id,
           OLD.outbox_id,OLD.signed_event_id,OLD.binding,OLD.source_facts,OLD.content,
           OLD.recorded_at,OLD.idempotency_key,OLD.created_at) THEN
        RAISE EXCEPTION 'ortak: memory request and terminal receipt are immutable'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_memory_write_guard BEFORE UPDATE ON runtime_memory_writes
FOR EACH ROW EXECUTE FUNCTION ortak_memory_write_guard();
CREATE TRIGGER trg_memory_write_no_delete BEFORE DELETE ON runtime_memory_writes
FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE FUNCTION ortak_insert_acknowledged_memory_write(target_company UUID,target_outbox UUID)
RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    -- FK insertion would otherwise wait for run FOR UPDATE while the caller
    -- holds outbox. Cancellation takes run before outbox. Refuse immediately
    -- on that inversion; the pending delivery lease remains safely retryable.
    PERFORM r.id FROM outbox o
    JOIN runtime_office_outputs j ON j.company_id=o.company_id AND j.outbox_id=o.id
    JOIN runs r ON r.company_id=j.company_id AND r.id=j.run_id
    WHERE o.company_id=target_company AND o.id=target_outbox AND o.kind='office_publish'
      AND o.state='delivered' AND j.state='enqueued' AND r.status='completed'
      AND r.delivery_intent IN ('reply','channel')
    FOR KEY SHARE OF r NOWAIT;
    INSERT INTO runtime_memory_writes(company_id,run_id,employee_id,employee_revision_id,channel_id,
        outbox_id,signed_event_id,binding,source_facts,content,recorded_at,idempotency_key)
    SELECT r.company_id,r.id,r.employee_id,r.employee_revision_id,i.channel_id,
        o.id,o.signed_event_id,rev.manifest->'memory',j.source_facts,j.draft_content,
        o.delivered_at,'office-output:'||r.id::text
    FROM outbox o JOIN runtime_office_outputs j ON j.company_id=o.company_id AND j.outbox_id=o.id
    JOIN runs r ON r.company_id=j.company_id AND r.id=j.run_id
    JOIN employee_revisions rev ON rev.company_id=r.company_id AND rev.employee_id=r.employee_id AND rev.id=r.employee_revision_id
    JOIN office_inbox i ON i.company_id=r.company_id AND i.event_id=r.message_id
    WHERE o.company_id=target_company AND o.id=target_outbox AND o.kind='office_publish'
      AND o.state='delivered' AND o.signed_event_id IS NOT NULL AND o.signed_event_bytes IS NOT NULL
      AND o.run_id=r.id AND j.state='enqueued' AND r.status='completed'
      AND r.delivery_intent IN ('reply','channel') AND i.channel_id IS NOT NULL
      AND jsonb_typeof(rev.manifest->'memory')='object'
      AND NOT EXISTS (SELECT 1 FROM runtime_cancellations x WHERE x.company_id=r.company_id AND x.run_id=r.id)
      AND NOT EXISTS (SELECT 1 FROM run_cancel_requests x WHERE x.company_id=r.company_id AND x.run_id=r.id)
    ON CONFLICT (company_id,run_id) DO NOTHING;
END;
$$;
CREATE FUNCTION ortak_schedule_acknowledged_memory_write() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.kind='office_publish' AND NEW.state='delivered' AND OLD.state<>'delivered' THEN
        -- A NOWAIT FK-parent check prevents waiting in outbox→run order.
        -- Claim/prepare later revalidate current authority.
        PERFORM ortak_insert_acknowledged_memory_write(NEW.company_id,NEW.id);
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_outbox_schedule_memory_write AFTER UPDATE OF state ON outbox
FOR EACH ROW EXECUTE FUNCTION ortak_schedule_acknowledged_memory_write();

CREATE FUNCTION ortak_check_memory_write_authority() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.admission_token IS NOT NULL AND (TG_OP='INSERT' OR
       ROW(NEW.admission_token,NEW.admission_generation,NEW.admission_valid_before)
       IS DISTINCT FROM ROW(OLD.admission_token,OLD.admission_generation,OLD.admission_valid_before)) THEN
        IF NEW.admission_generation IS DISTINCT FROM ortak_lock_office_authority(NEW.company_id)
           OR (NEW.admission_valid_before IS NOT NULL AND clock_timestamp()>=NEW.admission_valid_before) THEN
            RAISE EXCEPTION 'ortak: memory write admission changed or expired' USING ERRCODE='serialization_failure';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
CREATE CONSTRAINT TRIGGER trg_memory_write_authority AFTER INSERT OR UPDATE ON runtime_memory_writes
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_memory_write_authority();

-- Existing acknowledged completions receive the same deterministic job. This
-- function also supports desired-state reconciliation without duplicate work.
SELECT ortak_insert_acknowledged_memory_write(company_id,id)
FROM outbox WHERE kind='office_publish' AND state='delivered';

-- Ortak waking routing claim expiry (0053).
-- A successful score is not permission to dispatch after its inbox lease ends.
-- Production routing already holds inbox before root/recipient/outbox locks.
-- Keep same-generation zero-wake finalization available after scorer timeout.
CREATE FUNCTION ortak_check_routing_claim_expiry() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    current_claim RECORD;
BEGIN
    -- Legacy rows without Office authority remain inert at runtime admission,
    -- matching0048. Every production routing commit supplies its Office witness.
    IF NEW.wake_count = 0 OR NEW.office_authority_generation IS NULL THEN
        RETURN NEW;
    END IF;
    SELECT state, claim_generation, claim_expires_at INTO current_claim
    FROM office_inbox
    WHERE company_id = NEW.company_id AND event_id = NEW.message_id
    FOR UPDATE;
    -- Evaluate the clock after lock acquisition, including any finalization wait.
    IF NOT FOUND OR current_claim.state NOT IN ('claimed', 'decided')
       OR current_claim.claim_generation IS DISTINCT FROM NEW.inbox_claim_generation
       OR current_claim.claim_expires_at IS NULL
       OR clock_timestamp() >= current_claim.claim_expires_at THEN
        RAISE EXCEPTION 'ortak: waking routing claim changed or expired before commit'
            USING ERRCODE = 'serialization_failure';
    END IF;
    RETURN NEW;
END
$$;
CREATE CONSTRAINT TRIGGER ortak_routing_claim_expiry_at_commit
AFTER INSERT ON routing_decisions DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION ortak_check_routing_claim_expiry();


-- Ortak Work API access (0054).
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
