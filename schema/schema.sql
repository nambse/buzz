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
    payload_mode TEXT NOT NULL DEFAULT 'ordinary' CHECK(payload_mode IN('ordinary','confidential_dm_v1')),
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
-- application under the project row lock. Release/reactivation retain edge identity.

CREATE TABLE work_dependencies (
    id UUID NOT NULL DEFAULT gen_random_uuid() CHECK(id<>'00000000-0000-0000-0000-000000000000'),
    released_at TIMESTAMPTZ,
    company_id                  UUID NOT NULL REFERENCES companies(id),
    project_id                  UUID NOT NULL,
    work_item_id                UUID NOT NULL,
    depends_on_work_item_id     UUID NOT NULL,
    kind                        TEXT NOT NULL DEFAULT 'blocked_by' CHECK (kind = 'blocked_by'),
    created_by_type             TEXT NOT NULL CHECK (created_by_type IN ('human', 'employee', 'system')),
    created_by_id               TEXT CHECK (created_by_id IS NULL OR octet_length(created_by_id) <= 256),
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (company_id, work_item_id, depends_on_work_item_id),
    CONSTRAINT work_dependencies_company_id_id_key UNIQUE(company_id,id),
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

CREATE TRIGGER trg_work_dependencies_no_delete BEFORE DELETE ON work_dependencies
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE INDEX idx_work_dependencies_active_project
    ON work_dependencies(company_id,project_id,work_item_id,depends_on_work_item_id)
    WHERE released_at IS NULL;

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
    IF TG_TABLE_NAME LIKE 'events%' AND TG_OP = 'INSERT' THEN RETURN NEW; END IF;
    IF TG_TABLE_NAME = 'thread_metadata' AND TG_OP = 'INSERT'
       AND ortak_conversation_thread_insert_neutral75(proposed) THEN RETURN NEW; END IF;
    IF TG_TABLE_NAME = 'runs' AND TG_OP = 'INSERT' THEN RETURN NEW; END IF;
    IF TG_TABLE_NAME = 'outbox'
       AND NOT (COALESCE(previous ->> 'kind' = 'office_publish'
                         AND previous ->> 'signed_event_id' IS NOT NULL, false)
                OR COALESCE(proposed ->> 'kind' = 'office_publish'
                            AND proposed ->> 'signed_event_id' IS NOT NULL, false)) THEN
        RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF TG_ARGV[0] IN ('community', 'binding', 'community_root') THEN
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

-- A retained private DM keeps its original participant identity.
CREATE FUNCTION ortak_private_dm_identity() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.channel_type = 'dm' AND OLD.participant_hash IS NOT NULL
       AND (NEW.channel_type IS DISTINCT FROM OLD.channel_type
            OR NEW.visibility IS DISTINCT FROM OLD.visibility
            OR NEW.participant_hash IS DISTINCT FROM OLD.participant_hash) THEN
        RAISE EXCEPTION 'A retained DM participant identity cannot be replaced'
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER ortak_private_dm_identity BEFORE UPDATE ON channels
FOR EACH ROW EXECUTE FUNCTION ortak_private_dm_identity();

-- Alphabetically after community_write_fence_*: preserve its deletion checks.
CREATE TRIGGER ortak_office_authority_channels BEFORE INSERT OR UPDATE OR DELETE ON channels
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation(
    'community', 'community_id', 'id', 'channel_type', 'visibility',
    'archived_at', 'deleted_at', 'participant_hash', 'ttl_seconds', 'ttl_deadline');
CREATE TRIGGER ortak_office_authority_channel_members BEFORE INSERT OR UPDATE OR DELETE ON channel_members
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community', 'community_id', 'channel_id', 'pubkey', 'role', 'removed_at');
CREATE TRIGGER ortak_office_authority_relay_members BEFORE INSERT OR UPDATE OR DELETE ON relay_members
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community', 'community_id', 'pubkey');
CREATE TRIGGER ortak_office_authority_users BEFORE INSERT OR UPDATE OR DELETE ON users
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community', 'community_id', 'pubkey', 'agent_type', 'agent_owner_pubkey', 'deactivated_at');
CREATE TRIGGER ortak_office_authority_events BEFORE UPDATE OR DELETE ON events
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community', 'community_id', 'id', 'created_at', 'pubkey', 'kind', 'tags', 'content', 'sig', 'channel_id', 'deleted_at');
CREATE TRIGGER ortak_office_authority_thread_metadata BEFORE INSERT OR UPDATE OR DELETE ON thread_metadata
FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('community','community_id','event_id',
    'event_created_at','channel_id','parent_event_id','parent_event_created_at',
    'root_event_id','root_event_created_at','depth');
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
    IF NEW.payload_mode='confidential_dm_v1' THEN RETURN NEW; END IF;
    IF NEW.work_item_id IS NULL AND NEW.routing_decision_id IS NOT NULL
       AND NEW.status='completed' AND NEW.delivery_intent IN('reply','channel') THEN
        INSERT INTO runtime_office_outputs(company_id,run_id) VALUES(NEW.company_id,NEW.id)
        ON CONFLICT(company_id,run_id) DO NOTHING;
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
    IF NEW.wake_count = 0 OR NEW.office_authority_generation IS NULL THEN
        RETURN NEW;
    END IF;
    IF EXISTS(SELECT 1 FROM public.runs r WHERE r.company_id=NEW.company_id
        AND r.id=NEW.id AND r.routing_decision_id=NEW.id AND r.payload_mode='confidential_dm_v1') THEN
        PERFORM public.ortak_lock_office_authority(NEW.company_id);
        IF NOT EXISTS(SELECT 1 FROM public.confidential_runs c
          JOIN public.runs r ON r.company_id=c.company_id AND r.id=c.run_id
          JOIN public.encrypted_dm_decrypt_jobs j ON j.company_id=c.company_id AND j.source_id=c.source_id
          JOIN public.confidential_dm_receipts receipt ON receipt.company_id=c.company_id AND receipt.source_id=c.source_id AND receipt.run_id=c.run_id
          JOIN public.office_inbox i ON i.company_id=c.company_id AND i.event_id=c.source_id
          WHERE c.company_id=NEW.company_id AND c.run_id=NEW.id AND c.source_id=NEW.message_id
            AND NEW.root_message_id=c.source_id AND NEW.inbox_claim_generation=0
            AND NEW.policy_version='confidential_dm_v1' AND NEW.mode='deterministic'
            AND NEW.origin_type='human' AND NEW.origin_id=encode(c.human_public_key,'hex')
            AND NEW.wake_count=1 AND NEW.hop_consumed
            AND NEW.office_authority_generation=j.office_generation
            AND NEW.office_authority_valid_before=j.claim_expires_at AND NEW.office_input_hash=j.source_hash
            AND NEW.input_hash=public.digest(c.source_bytes,'sha256')
            AND j.state='verified' AND j.claim_expires_at>clock_timestamp() AND j.valid_before>clock_timestamp()
            AND c.admission_deadline=j.claim_expires_at AND c.admission_deadline>clock_timestamp()
            AND (c.claim_generation,c.claim_token,c.claim_worker)=(j.claim_generation,j.claim_token,j.worker_id)
            AND (receipt.claim_generation,receipt.claim_token,receipt.claim_worker)=(j.claim_generation,j.claim_token,j.worker_id)
            AND NOT receipt.duplicate_rumor
            AND (r.employee_id,r.employee_revision_id,r.employee_lifecycle_epoch,r.office_admission_token)=
                (j.employee_id,j.employee_revision_id,j.employee_lifecycle_epoch,j.claim_token)
            AND i.state='decided' AND i.event_kind=1059 AND i.channel_id IS NULL
            AND i.event_created_at=j.source_created_at AND i.author_pubkey=j.source_author
            AND public.ortak_confidential_dm_current(c.company_id,c.run_id)) THEN
            RAISE EXCEPTION 'ortak: confidential decrypt claim changed or expired before commit'
                USING ERRCODE='serialization_failure';
        END IF;
        RETURN NEW;
    END IF;
    -- Unchanged migration53 ordinary inbox-claim branch.
    SELECT state, claim_generation, claim_expires_at INTO current_claim
    FROM office_inbox
    WHERE company_id = NEW.company_id AND event_id = NEW.message_id
    FOR UPDATE;
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
FOR EACH ROW EXECUTE FUNCTION ortak_guard_project_api_binding();
CREATE CONSTRAINT TRIGGER project_api_binding_purge_at_commit AFTER DELETE ON project_api_bindings
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_project_binding_purge_at_commit();
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
    FOREIGN KEY (company_id, project_id) REFERENCES projects(company_id, id)
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
    FOREIGN KEY (company_id, project_id) REFERENCES projects(company_id, id),
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

-- Fresh external probes precede a bounded, sealed activation admission.
-- The repository explicitly defers this guard immediately before its final
-- success write and commits next. No network call runs in that transaction.
CREATE FUNCTION ortak_check_activation_admission_at_commit() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    activation_receipt JSONB;
    admission JSONB;
    activation_attempt INTEGER;
    issued_at TIMESTAMPTZ;
    expires_at TIMESTAMPTZ;
    checked_at TIMESTAMPTZ;
BEGIN
    IF TG_OP='UPDATE' AND OLD.status='succeeded'
        AND OLD.result_revision_id IS NOT DISTINCT FROM NEW.result_revision_id THEN
        RETURN NEW;
    END IF;
    SELECT s.result, s.attempt_count INTO activation_receipt, activation_attempt
    FROM provisioning_operation_steps s
    WHERE s.company_id=NEW.company_id AND s.operation_id=NEW.id
      AND s.step_index=9 AND s.step_name='activate_revision' AND s.state='succeeded'
      AND s.idempotency_key='provisioning:'||NEW.id::TEXT||':activate_revision'
    FOR SHARE;
    IF NOT FOUND OR activation_attempt<=0 THEN
        RAISE EXCEPTION 'ortak: successful activation requires its exact receipt'
            USING ERRCODE='40001';
    END IF;
    admission := activation_receipt->'admission';
    IF jsonb_typeof(admission) IS DISTINCT FROM 'object'
        OR admission->>'format' IS DISTINCT FROM 'ortak.activation/v1'
        OR admission->>'operation_id' IS DISTINCT FROM NEW.id::TEXT
        OR admission->>'employee_id' IS DISTINCT FROM NEW.employee_id
        OR activation_receipt->>'result_revision_id' IS DISTINCT FROM NEW.result_revision_id::TEXT
        OR jsonb_typeof(activation_receipt->'evidence') IS DISTINCT FROM 'object'
        OR jsonb_typeof(admission->'attempt_count') IS DISTINCT FROM 'number'
        OR (admission->>'attempt_count' ~ '^[1-9][0-9]{0,9}$') IS DISTINCT FROM true
        OR jsonb_typeof(admission->'manifest_fingerprint') IS DISTINCT FROM 'string'
        OR (admission->>'manifest_fingerprint' ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true
        OR jsonb_typeof(admission->'observed_at') IS DISTINCT FROM 'string'
        OR jsonb_typeof(admission->'valid_before') IS DISTINCT FROM 'string'
        OR length(admission->>'observed_at')>64 OR length(admission->>'valid_before')>64 THEN
        RAISE EXCEPTION 'ortak: activation admission correlation is invalid'
            USING ERRCODE='40001';
    END IF;
    IF (admission->>'attempt_count')::BIGINT<>activation_attempt THEN
        RAISE EXCEPTION 'ortak: activation admission attempt is stale'
            USING ERRCODE='40001';
    END IF;
    BEGIN
        issued_at := (admission->>'observed_at')::TIMESTAMPTZ;
        expires_at := (admission->>'valid_before')::TIMESTAMPTZ;
    EXCEPTION WHEN invalid_datetime_format OR datetime_field_overflow THEN
        RAISE EXCEPTION 'ortak: activation admission clock is invalid'
            USING ERRCODE='40001';
    END;
    checked_at := clock_timestamp();
    IF NOT isfinite(issued_at) OR NOT isfinite(expires_at)
        OR issued_at>checked_at OR expires_at<=checked_at
        OR expires_at<=issued_at OR expires_at-issued_at>interval '15 seconds' THEN
        RAISE EXCEPTION 'ortak: activation admission expired before commit'
            USING ERRCODE='40001';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM provisioning_operations o
        JOIN companies c ON c.id=o.company_id AND c.status='active'
        JOIN employees e ON e.company_id=o.company_id AND e.id=o.employee_id
            AND e.status='active' AND e.active_revision_id=o.result_revision_id
        JOIN employee_revisions r ON r.company_id=o.company_id AND r.employee_id=o.employee_id
            AND r.id=o.result_revision_id
        WHERE o.company_id=NEW.company_id AND o.id=NEW.id AND NOT o.dry_run
          AND o.status='succeeded' AND o.result_revision_id=NEW.result_revision_id
          AND o.employee_id=NEW.employee_id AND o.manifest_fingerprint=NEW.manifest_fingerprint
          AND r.manifest_fingerprint=decode(admission->>'manifest_fingerprint','hex')
          AND r.manifest->>'id'=o.employee_id AND r.manifest->>'status'='active'
          AND r.created_by='provisioning:'||o.id::TEXT
          AND EXISTS (SELECT 1 FROM employee_runtime_bindings b
              WHERE b.company_id=o.company_id AND b.revision_id=r.id
                AND b.employee_id=o.employee_id AND b.validated_at=issued_at)
          -- A refreshed same-key binding retains its original revision provenance.
          AND EXISTS (SELECT 1 FROM employee_office_bindings b
              WHERE b.company_id=o.company_id AND b.employee_id=o.employee_id
                AND b.public_key=decode(r.manifest->'office'->>'public_key','hex')
                AND b.signer_ref=r.manifest->'office'->>'signer_ref'
                AND b.verified_at=issued_at AND b.valid_from<=checked_at
                AND (b.valid_until IS NULL OR b.valid_until>checked_at))
          AND ((r.manifest->'memory') IS NULL OR r.manifest->'memory'='null'::JSONB
            OR EXISTS (SELECT 1 FROM employee_memory_bindings b
                WHERE b.company_id=o.company_id AND b.revision_id=r.id
                  AND b.employee_id=o.employee_id AND b.validated_at=issued_at))
    ) THEN
        RAISE EXCEPTION 'ortak: activation admission does not match committed authority'
            USING ERRCODE='40001';
    END IF;
    RETURN NEW;
END
$$;

-- Successful activation is durable audit history. These guards do not acquire
-- parent locks from a step tuple, so they add no step->operation lock edge.
CREATE FUNCTION ortak_guard_activation_operation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.result_revision_id IS NOT NULL THEN
        IF TG_OP='DELETE' THEN
            RAISE EXCEPTION 'ortak: activated operation is immutable' USING ERRCODE='55000';
        END IF;
        IF NEW IS DISTINCT FROM OLD THEN
            RAISE EXCEPTION 'ortak: activated operation is immutable' USING ERRCODE='55000';
        END IF;
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END
$$;
CREATE FUNCTION ortak_guard_activation_receipt() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.step_name='activate_revision' AND OLD.state='succeeded' THEN
        IF TG_OP='DELETE' THEN
            RAISE EXCEPTION 'ortak: activation receipt is immutable' USING ERRCODE='55000';
        END IF;
        IF NEW IS DISTINCT FROM OLD THEN
            RAISE EXCEPTION 'ortak: activation receipt is immutable' USING ERRCODE='55000';
        END IF;
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER ortak_activation_operation_immutable BEFORE UPDATE OR DELETE ON provisioning_operations
FOR EACH ROW EXECUTE FUNCTION ortak_guard_activation_operation();
CREATE TRIGGER ortak_activation_receipt_immutable BEFORE UPDATE OR DELETE ON provisioning_operation_steps
FOR EACH ROW EXECUTE FUNCTION ortak_guard_activation_receipt();
CREATE TRIGGER ortak_activation_operation_no_truncate BEFORE TRUNCATE ON provisioning_operations
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER ortak_activation_receipt_no_truncate BEFORE TRUNCATE ON provisioning_operation_steps
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE CONSTRAINT TRIGGER ortak_activation_admission_at_commit AFTER INSERT OR UPDATE ON provisioning_operations
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
WHEN (NEW.status='succeeded' AND NOT NEW.dry_run AND NEW.result_revision_id IS NOT NULL)
EXECUTE FUNCTION ortak_check_activation_admission_at_commit();

-- Real prepared-resource provisioning and durable Office profile publication.
CREATE TABLE office_identity_profiles (
    company_id UUID NOT NULL REFERENCES companies(id),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 256 AND idempotency_key ~ '^[A-Za-z0-9:_.-]+$'),
    community_id UUID NOT NULL REFERENCES communities(id),
    employee_id TEXT NOT NULL,
    request_hash BYTEA NOT NULL CHECK (octet_length(request_hash)=32),
    event_id BYTEA NOT NULL CHECK (octet_length(event_id)=32),
    signed_event_bytes BYTEA NOT NULL CHECK (octet_length(signed_event_bytes) BETWEEN 1 AND 16384),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    acknowledged_at TIMESTAMPTZ,
    PRIMARY KEY (company_id,idempotency_key),
    FOREIGN KEY (company_id,employee_id) REFERENCES employees(company_id,id),
    FOREIGN KEY (company_id,idempotency_key)
        REFERENCES provisioning_operation_steps(company_id,idempotency_key),
    CHECK (acknowledged_at IS NULL OR acknowledged_at>=created_at)
);

CREATE FUNCTION ortak_office_profile_receipt_immutable() RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP IN ('DELETE','TRUNCATE') THEN
        RAISE EXCEPTION 'Office profile receipts are immutable' USING ERRCODE='check_violation';
    END IF;
    IF (to_jsonb(NEW)-'acknowledged_at') IS DISTINCT FROM (to_jsonb(OLD)-'acknowledged_at')
       OR (OLD.acknowledged_at IS NOT NULL AND NEW.acknowledged_at IS DISTINCT FROM OLD.acknowledged_at) THEN
        RAISE EXCEPTION 'Office profile receipt bytes are immutable' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_office_identity_profiles_immutable
    BEFORE UPDATE OR DELETE ON office_identity_profiles
    FOR EACH ROW EXECUTE FUNCTION ortak_office_profile_receipt_immutable();

-- Freeze all public adapter/receipt selections separately from the manifest.
-- A retry cannot replace original native ownership or diagnostic provenance.
CREATE TABLE provisioning_runner_selections (
    company_id UUID NOT NULL,
    operation_id UUID NOT NULL,
    configuration_fingerprint BYTEA NOT NULL CHECK (octet_length(configuration_fingerprint)=32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, operation_id),
    FOREIGN KEY (company_id, operation_id) REFERENCES provisioning_operations(company_id, id)
);

CREATE FUNCTION ortak_provisioning_selection_immutable() RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'Provisioning runner selections are immutable' USING ERRCODE='check_violation';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_provisioning_runner_selections_immutable
    BEFORE UPDATE OR DELETE ON provisioning_runner_selections
    FOR EACH ROW EXECUTE FUNCTION ortak_provisioning_selection_immutable();

CREATE TRIGGER trg_office_identity_profiles_no_truncate
    BEFORE TRUNCATE ON office_identity_profiles
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_office_profile_receipt_immutable();

CREATE TRIGGER trg_provisioning_runner_selections_no_truncate
    BEFORE TRUNCATE ON provisioning_runner_selections
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_provisioning_selection_immutable();

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
    community_id UUID NOT NULL REFERENCES communities(id),
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

-- Transactional hints for authorized durable Activity streams.
-- Transactional NOTIFY carries public scope hints only. Durable data and the
-- company authority fence stay authoritative; a lost hint is repaired on
-- reconnect after LISTEN, never treated as event delivery or a cursor.
CREATE FUNCTION ortak_activity_notify() RETURNS TRIGGER AS $$
DECLARE
    facts JSONB := to_jsonb(NEW);
    run UUID;
BEGIN
    IF TG_ARGV[0] <> '' THEN
        run := (facts ->> TG_ARGV[0])::UUID;
        IF run IS NULL THEN RETURN NEW; END IF;
    END IF;
    PERFORM pg_notify('ortak_activity_v1', json_build_object(
        'company_id', (facts ->> 'company_id')::UUID, 'run_id', run)::TEXT);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_activity_events AFTER INSERT ON run_events
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_runs AFTER INSERT OR UPDATE OF status, delivery_intent, cancel_reason, error_code, error_message, started_at, finished_at ON runs
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('id');
CREATE TRIGGER trg_activity_cancel_requests AFTER INSERT OR UPDATE OF status, last_error_code, acknowledged_at ON run_cancel_requests
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_cancellations AFTER INSERT OR UPDATE OF state, last_error_code, acknowledged_at ON runtime_cancellations
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_office_outputs AFTER INSERT OR UPDATE OF state, outbox_id, last_error_code ON runtime_office_outputs
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_outbox AFTER INSERT OR UPDATE OF state, last_error, delivered_at ON outbox
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_memory_writes AFTER INSERT OR UPDATE OF state, attempt_count, next_attempt_at, last_error_code, receipt, acknowledged_at ON runtime_memory_writes
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
CREATE TRIGGER trg_activity_context AFTER INSERT ON run_context_snapshots
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');
-- Existing Office triggers bump this generation in the SAME transaction as
-- channel/relay membership, audience, user, company or source authority changes.
CREATE TRIGGER trg_activity_authority AFTER INSERT OR UPDATE ON office_authority_generations
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('');

-- Migration0061 retained evidence authority.
SELECT attach_community_write_fence('office_identity_profiles');

CREATE FUNCTION ortak_guard_retained_office_authority() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    -- Lock the current binding, then use a fresh statement snapshot after any
    -- lock wait. The universal community fence has already locked lifecycle.
    PERFORM 1 FROM office_company_bindings b
    WHERE b.company_id=NEW.company_id AND b.community_id=NEW.community_id FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'retained Office evidence requires current binding'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM office_company_bindings b JOIN communities c ON c.id=b.community_id
        WHERE b.company_id=NEW.company_id AND b.community_id=NEW.community_id
          AND c.deletion_state='active' AND c.deleted_at IS NULL
    ) THEN
        RAISE EXCEPTION 'retained Office evidence requires active community authority'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END
$$;

-- Alphabetically after community_write_fence_*; no bypass via executor GUCs.
CREATE TRIGGER ortak_retained_office_authority BEFORE INSERT OR UPDATE
ON office_identity_profiles FOR EACH ROW EXECUTE FUNCTION ortak_guard_retained_office_authority();
CREATE TRIGGER ortak_retained_office_authority BEFORE INSERT OR UPDATE
ON office_inbox_reconciliations FOR EACH ROW EXECUTE FUNCTION ortak_guard_retained_office_authority();

-- Existing reconciliation current-capture/channel validation remains mandatory.
-- Mutable office_routing_channels/cohorts are purged before Office bindings;
-- cohort FK cascades remove only routing employee selections, never employees.

-- Work definition editing (migration0062).
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

-- Work execution and human review (migration0063).
-- E2: human-authorized Work execution, immutable text deliverable, human review.
CREATE TABLE work_authority_generations (
    company_id UUID NOT NULL REFERENCES companies(id),
    project_id UUID NOT NULL,
    generation BIGINT NOT NULL DEFAULT 0 CHECK (generation >= 0),
    PRIMARY KEY(company_id,project_id),
    FOREIGN KEY(company_id,project_id) REFERENCES projects(company_id,id)
);
INSERT INTO work_authority_generations(company_id,project_id) SELECT company_id,id FROM projects;
CREATE FUNCTION ortak_work_generation_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' OR (NEW.company_id,NEW.project_id) IS DISTINCT FROM (OLD.company_id,OLD.project_id)
       OR NEW.generation<>OLD.generation+1 THEN
        RAISE EXCEPTION 'ortak: Work generation only advances' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_generation_guard BEFORE UPDATE OR DELETE ON work_authority_generations
FOR EACH ROW EXECUTE FUNCTION ortak_work_generation_guard();
CREATE TRIGGER work_generation_no_truncate BEFORE TRUNCATE ON work_authority_generations
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_advance_work_authority() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; project UUID;
BEGIN
    company:=NEW.company_id;
    IF TG_TABLE_NAME='projects' THEN project:=NEW.id;
    ELSIF TG_TABLE_NAME IN ('work_items','project_access_grants') THEN project:=NEW.project_id;
    ELSE SELECT project_id INTO project FROM work_items WHERE company_id=company AND id=NEW.work_item_id;
    END IF;
    INSERT INTO work_authority_generations(company_id,project_id) VALUES(company,project)
    ON CONFLICT(company_id,project_id) DO UPDATE SET generation=work_authority_generations.generation+1;
    RETURN NEW;
END $$;
CREATE TRIGGER work_authority_projects AFTER INSERT OR UPDATE ON projects
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_grants AFTER INSERT OR UPDATE ON project_access_grants
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_items AFTER INSERT OR UPDATE ON work_items
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_assignments AFTER INSERT OR UPDATE ON work_assignments
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_dependencies AFTER INSERT OR UPDATE ON work_dependencies
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_criteria AFTER INSERT OR UPDATE ON work_acceptance_criteria
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();
CREATE TRIGGER work_authority_approvals AFTER INSERT OR UPDATE ON work_approvals
FOR EACH ROW EXECUTE FUNCTION ortak_advance_work_authority();

-- Domain commands lock project then item before child mutations. Direct writers
-- must obey the same parent authority; NOWAIT refuses reversed child lock order.
CREATE FUNCTION ortak_work_child_authority_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE parent_project UUID;
BEGIN
    IF TG_TABLE_NAME='work_assignments' THEN
        IF TG_OP='UPDATE' AND (NEW.company_id,NEW.work_item_id,NEW.employee_id) IS DISTINCT FROM (OLD.company_id,OLD.work_item_id,OLD.employee_id) THEN
            RAISE EXCEPTION 'ortak: Work assignment identity is immutable' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
    END IF;
    SELECT project_id INTO parent_project FROM work_items WHERE company_id=NEW.company_id AND id=NEW.work_item_id;
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=parent_project FOR SHARE NOWAIT;
    PERFORM 1 FROM work_items WHERE company_id=NEW.company_id AND id=NEW.work_item_id FOR UPDATE NOWAIT;
    IF NOT FOUND THEN RAISE EXCEPTION 'ortak: Work authority parent is missing' USING ERRCODE='foreign_key_violation'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_assignment_authority_guard BEFORE INSERT OR UPDATE ON work_assignments
    FOR EACH ROW EXECUTE FUNCTION ortak_work_child_authority_guard();
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

CREATE TRIGGER work_dependency_authority_guard BEFORE INSERT OR UPDATE ON work_dependencies
    FOR EACH ROW EXECUTE FUNCTION ortak_work_dependency_edit_guard();
CREATE TRIGGER work_criterion_authority_guard BEFORE INSERT OR UPDATE ON work_acceptance_criteria
    FOR EACH ROW EXECUTE FUNCTION ortak_work_child_authority_guard();
CREATE TRIGGER work_approval_authority_guard BEFORE INSERT OR UPDATE ON work_approvals
    FOR EACH ROW EXECUTE FUNCTION ortak_work_child_authority_guard();

ALTER TABLE runs ADD COLUMN work_admission_generation BIGINT CHECK(work_admission_generation>=0),
    ADD COLUMN work_admission_token UUID,
    ADD CONSTRAINT runs_work_admission_pair CHECK((work_admission_generation IS NULL)=(work_admission_token IS NULL)),
    ADD CONSTRAINT runs_work_origin_exclusive CHECK(work_item_id IS NULL OR
        (routing_decision_id IS NULL AND message_id IS NULL AND root_message_id IS NULL));

CREATE TABLE work_executions (
    company_id UUID NOT NULL REFERENCES companies(id),
    run_id UUID NOT NULL,
    project_id UUID NOT NULL,
    work_item_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    employee_revision_id UUID NOT NULL,
    requested_by TEXT NOT NULL CHECK(requested_by ~ '^[0-9a-f]{64}$'),
    operation_id UUID NOT NULL,
    requested_version BIGINT NOT NULL CHECK(requested_version>=1),
    execution_version BIGINT NOT NULL CHECK(execution_version=requested_version+1),
    definition_bytes BYTEA NOT NULL CHECK(octet_length(definition_bytes) BETWEEN 1 AND 32768),
    definition_hash BYTEA NOT NULL CHECK(octet_length(definition_hash)=32 AND definition_hash=sha256(definition_bytes)),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    reconciled_at TIMESTAMPTZ,
    result_code TEXT CHECK(result_code ~ '^[a-z][a-z0-9_]{0,63}$'),
    PRIMARY KEY(company_id,run_id),
    UNIQUE(company_id,requested_by,operation_id),
    UNIQUE(company_id,work_item_id,requested_version),
    FOREIGN KEY(company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY(company_id,project_id,work_item_id) REFERENCES work_items(company_id,project_id,id),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id),
    FOREIGN KEY(company_id,requested_by,operation_id) REFERENCES work_api_operations(company_id,actor_pubkey,operation_id)
        DEFERRABLE INITIALLY DEFERRED,
    CHECK((reconciled_at IS NULL)=(result_code IS NULL))
);
CREATE UNIQUE INDEX idx_work_execution_active ON work_executions(company_id,work_item_id) WHERE reconciled_at IS NULL;
CREATE INDEX idx_work_execution_item ON work_executions(company_id,work_item_id,requested_at DESC,run_id);
CREATE FUNCTION ortak_work_execution_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (to_jsonb(NEW)-'reconciled_at'-'result_code') IS DISTINCT FROM (to_jsonb(OLD)-'reconciled_at'-'result_code')
       OR OLD.reconciled_at IS NOT NULL OR NEW.reconciled_at IS NULL
       OR NOT EXISTS(SELECT 1 FROM runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND status IN('completed','failed','cancelled')) THEN
        RAISE EXCEPTION 'ortak: Work execution pins its request and only closes once' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_execution_guard BEFORE UPDATE ON work_executions FOR EACH ROW EXECUTE FUNCTION ortak_work_execution_guard();
CREATE TRIGGER work_execution_no_delete BEFORE DELETE ON work_executions FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER work_execution_no_truncate BEFORE TRUNCATE ON work_executions FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

ALTER TABLE outbox DROP CONSTRAINT outbox_kind_check;
ALTER TABLE outbox ADD CONSTRAINT outbox_kind_check CHECK(kind IN('run_dispatch','work_run_dispatch','office_publish')),
    ADD CONSTRAINT outbox_work_dispatch_shape CHECK(kind<>'work_run_dispatch' OR
        (run_id IS NOT NULL AND employee_id IS NOT NULL AND routing_decision_id IS NULL));
CREATE UNIQUE INDEX idx_outbox_work_dispatch ON outbox(company_id,run_id) WHERE kind='work_run_dispatch';
ALTER TABLE runtime_cancellations DROP CONSTRAINT runtime_cancellations_reason_check;
ALTER TABLE runtime_cancellations ADD CONSTRAINT runtime_cancellations_reason_check
    CHECK(reason IN('office_revoked','human_requested','work_revoked'));

CREATE TABLE artifacts (
    company_id UUID NOT NULL REFERENCES companies(id),
    id UUID NOT NULL,
    project_id UUID NOT NULL,
    work_item_id UUID NOT NULL,
    run_id UUID NOT NULL,
    terminal_sequence BIGINT NOT NULL,
    employee_id TEXT NOT NULL,
    employee_revision_id UUID NOT NULL,
    media_type TEXT NOT NULL DEFAULT 'text/plain; charset=utf-8' CHECK(media_type='text/plain; charset=utf-8'),
    content_bytes BYTEA NOT NULL CHECK(octet_length(content_bytes) BETWEEN 1 AND 32768),
    content_hash BYTEA NOT NULL CHECK(octet_length(content_hash)=32 AND content_hash=sha256(content_bytes)),
    size_bytes INT NOT NULL CHECK(size_bytes=octet_length(content_bytes)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,run_id),
    UNIQUE(company_id,work_item_id,id),
    FOREIGN KEY(company_id,project_id,work_item_id) REFERENCES work_items(company_id,project_id,id),
    FOREIGN KEY(company_id,run_id) REFERENCES work_executions(company_id,run_id),
    FOREIGN KEY(company_id,run_id,terminal_sequence) REFERENCES run_events(company_id,run_id,sequence),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id)
);
CREATE TRIGGER artifacts_immutable BEFORE UPDATE OR DELETE ON artifacts FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER artifacts_no_truncate BEFORE TRUNCATE ON artifacts FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
ALTER TABLE work_attachments ADD COLUMN artifact_id UUID,
    ADD CONSTRAINT work_attachment_artifact_fk FOREIGN KEY(company_id,work_item_id,artifact_id) REFERENCES artifacts(company_id,work_item_id,id),
    ADD CONSTRAINT work_attachment_artifact_shape CHECK((kind='artifact')=(artifact_id IS NOT NULL));
ALTER TABLE work_attachments DROP CONSTRAINT work_attachments_kind_check;
ALTER TABLE work_attachments ADD CONSTRAINT work_attachments_kind_check CHECK(kind IN('office_message','routing_decision','run','artifact'));
CREATE UNIQUE INDEX idx_work_attachments_artifact ON work_attachments(company_id,work_item_id,artifact_id) WHERE artifact_id IS NOT NULL;

CREATE TABLE runtime_work_outputs (
    company_id UUID NOT NULL REFERENCES companies(id),
    run_id UUID NOT NULL,
    terminal_sequence BIGINT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','materialized','failed')),
    artifact_id UUID,
    attempt_count INT NOT NULL DEFAULT 0 CHECK(attempt_count BETWEEN 0 AND 20),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK(last_error_code ~ '^[a-z][a-z0-9_]{0,63}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    completed_at TIMESTAMPTZ,
    PRIMARY KEY(company_id,run_id),
    FOREIGN KEY(company_id,run_id) REFERENCES work_executions(company_id,run_id),
    FOREIGN KEY(company_id,run_id,terminal_sequence) REFERENCES run_events(company_id,run_id,sequence),
    FOREIGN KEY(company_id,artifact_id) REFERENCES artifacts(company_id,id),
    CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),
    CHECK((state='pending')=(completed_at IS NULL)),
    CHECK(state='pending' OR lease_token IS NULL),
    CHECK((state='materialized')=(artifact_id IS NOT NULL)),
    CHECK(state<>'failed' OR last_error_code IS NOT NULL)
);
CREATE INDEX idx_runtime_work_outputs_due ON runtime_work_outputs(company_id,next_attempt_at,created_at,run_id) WHERE state='pending';
CREATE FUNCTION ortak_work_output_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.company_id,NEW.run_id,NEW.terminal_sequence,NEW.created_at) IS DISTINCT FROM
       (OLD.company_id,OLD.run_id,OLD.terminal_sequence,OLD.created_at)
       OR NEW.attempt_count<OLD.attempt_count OR OLD.state<>'pending' THEN
        RAISE EXCEPTION 'ortak: Work output attribution is immutable and terminal state is final' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_output_guard BEFORE UPDATE ON runtime_work_outputs FOR EACH ROW EXECUTE FUNCTION ortak_work_output_guard();
CREATE TRIGGER work_output_no_delete BEFORE DELETE ON runtime_work_outputs FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER work_output_no_truncate BEFORE TRUNCATE ON runtime_work_outputs FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_schedule_work_output() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.event_type IN('run.completed','run.failed','run.cancelled') AND EXISTS(
        SELECT 1 FROM work_executions WHERE company_id=NEW.company_id AND run_id=NEW.run_id) THEN
        INSERT INTO runtime_work_outputs(company_id,run_id,terminal_sequence) VALUES(NEW.company_id,NEW.run_id,NEW.sequence)
        ON CONFLICT(company_id,run_id) DO NOTHING;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_output_schedule AFTER INSERT ON run_events FOR EACH ROW EXECUTE FUNCTION ortak_schedule_work_output();

CREATE FUNCTION ortak_check_work_execution_request() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE definition JSONB;
BEGIN
    definition:=convert_from(NEW.definition_bytes,'UTF8')::jsonb;
    IF NOT EXISTS(
        SELECT 1 FROM runs r JOIN work_items w ON w.company_id=r.company_id AND w.id=r.work_item_id
        JOIN work_item_history h ON h.company_id=w.company_id AND h.work_item_id=w.id AND h.version=NEW.execution_version
        JOIN work_api_operations o ON o.company_id=NEW.company_id AND o.actor_pubkey=NEW.requested_by AND o.operation_id=NEW.operation_id
        JOIN outbox ticket ON ticket.company_id=r.company_id AND ticket.run_id=r.id AND ticket.kind='work_run_dispatch'
        JOIN work_attachments attachment ON attachment.company_id=r.company_id AND attachment.work_item_id=w.id AND attachment.run_id=r.id
        WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.work_item_id=NEW.work_item_id
        AND r.employee_id=NEW.employee_id AND r.employee_revision_id=NEW.employee_revision_id
        AND r.routing_decision_id IS NULL AND r.message_id IS NULL AND r.root_message_id IS NULL
        AND ticket.employee_id=NEW.employee_id AND ticket.routing_decision_id IS NULL
        AND w.project_id=NEW.project_id AND w.version=NEW.execution_version AND w.state='in_progress'
        AND h.event_type='work.execution_requested' AND h.actor_type='human' AND h.actor_id=NEW.requested_by
        AND h.payload->>'run_id'=NEW.run_id::text AND h.payload->>'employee_id'=NEW.employee_id
        AND o.action='mutate_work_item' AND o.project_id=NEW.project_id AND o.work_item_id=NEW.work_item_id
        AND o.result_version=NEW.execution_version
        AND o.request_hash=sha256(convert_to(format('["start_execution","%s",%s,"%s"]',NEW.work_item_id,NEW.requested_version,NEW.employee_id),'UTF8'))
        AND h.xmin::text::bigint=txid_current()%4294967296 AND o.xmin::text::bigint=txid_current()%4294967296
        AND definition->>'type'='work_item' AND definition->>'work_item_id'=w.id::text
        AND definition->>'project_id'=w.project_id::text AND definition->>'title'=w.title AND definition->>'description'=w.description
        AND definition->'acceptance_criteria'=coalesce((SELECT jsonb_agg(jsonb_build_object('id',cr.id,'text',cr.text) ORDER BY cr.position)
            FROM work_acceptance_criteria cr WHERE cr.company_id=w.company_id AND cr.work_item_id=w.id),'[]'::jsonb)
    ) THEN
        RAISE EXCEPTION 'ortak: Work execution requires its atomic human request, definition and run provenance'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER work_execution_request_at_commit AFTER INSERT ON work_executions
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_work_execution_request();

CREATE FUNCTION ortak_work_run_identity_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (OLD.work_item_id IS NOT NULL OR NEW.work_item_id IS NOT NULL) AND
        (NEW.company_id,NEW.id,NEW.work_item_id,NEW.employee_id,NEW.employee_revision_id,NEW.runtime_adapter,
         NEW.routing_decision_id,NEW.message_id,NEW.root_message_id,NEW.queued_at)
        IS DISTINCT FROM
        (OLD.company_id,OLD.id,OLD.work_item_id,OLD.employee_id,OLD.employee_revision_id,OLD.runtime_adapter,
         OLD.routing_decision_id,OLD.message_id,OLD.root_message_id,OLD.queued_at) THEN
        RAISE EXCEPTION 'ortak: Work run origin and configuration pins are immutable' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER work_run_identity BEFORE UPDATE ON runs FOR EACH ROW EXECUTE FUNCTION ortak_work_run_identity_guard();

CREATE FUNCTION ortak_check_run_work_authority() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE current_run runs%ROWTYPE;
BEGIN
    -- INSERT can precede the one final admission UPDATE in the same transaction.
    SELECT * INTO current_run FROM runs WHERE company_id=NEW.company_id AND id=NEW.id;
    IF current_run.work_item_id IS NULL THEN
        IF current_run.work_admission_generation IS NOT NULL THEN
            RAISE EXCEPTION 'ortak: Work admission requires Work origin' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP='UPDATE' AND NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token
        AND NEW.work_admission_generation IS NOT DISTINCT FROM OLD.work_admission_generation THEN RETURN NEW; END IF;
    IF NOT EXISTS(SELECT 1 FROM work_executions x
        JOIN work_items w ON w.company_id=x.company_id AND w.id=x.work_item_id
        JOIN projects p ON p.company_id=x.company_id AND p.id=x.project_id
        JOIN work_authority_generations g ON g.company_id=x.company_id AND g.project_id=x.project_id
        JOIN project_access_grants a ON a.company_id=x.company_id AND a.project_id=x.project_id AND a.actor_pubkey=x.requested_by
        JOIN work_assignments assignment ON assignment.company_id=x.company_id AND assignment.work_item_id=x.work_item_id AND assignment.employee_id=x.employee_id
        WHERE x.company_id=current_run.company_id AND x.run_id=current_run.id AND x.work_item_id=current_run.work_item_id
        AND x.employee_id=current_run.employee_id AND x.employee_revision_id=current_run.employee_revision_id
        AND g.generation=current_run.work_admission_generation AND current_run.work_admission_token IS NOT NULL
        AND p.status='active' AND w.state='in_progress' AND w.version=x.execution_version
        AND a.role IN('owner','contributor') AND a.revoked_at IS NULL
        AND assignment.status='active' AND assignment.role IN('owner','contributor')) THEN
        RAISE EXCEPTION 'ortak: Work admission changed before commit' USING ERRCODE='serialization_failure';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER work_run_admission_at_commit AFTER INSERT OR UPDATE ON runs
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_run_work_authority();

CREATE FUNCTION ortak_check_work_output_provenance() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; run UUID; job runtime_work_outputs%ROWTYPE; final_turn JSONB; final_text TEXT; fragments BIGINT; payload_bytes BIGINT; truncated BOOLEAN;
BEGIN
    company:=NEW.company_id;
    IF TG_TABLE_NAME='artifacts' THEN run:=NEW.run_id;
    ELSE run:=NEW.run_id;
    END IF;
    SELECT * INTO job FROM runtime_work_outputs WHERE company_id=company AND run_id=run;
    IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM runs r JOIN run_events ev ON ev.company_id=r.company_id AND ev.run_id=r.id
        WHERE r.company_id=company AND r.id=run AND ev.sequence=job.terminal_sequence
        AND ((r.status='completed' AND ev.event_type='run.completed') OR (r.status='failed' AND ev.event_type='run.failed')
            OR (r.status='cancelled' AND ev.event_type='run.cancelled')))
    THEN RAISE EXCEPTION 'ortak: Work output requires canonical terminal provenance' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF job.state='materialized' THEN
        SELECT payload->'turn' INTO final_turn FROM run_events WHERE company_id=company AND run_id=run
            AND sequence<job.terminal_sequence AND event_type='assistant.delta' ORDER BY sequence DESC LIMIT 1;
        SELECT count(*),coalesce(sum(octet_length(payload::text)),0),
            bool_or(NOT coalesce(
                payload->>'event_type'='assistant.delta'
                AND jsonb_typeof(payload->'turn')='number'
                AND (payload->>'turn') ~ '^(0|[1-9][0-9]{0,9})$'
                AND (payload->>'turn')::numeric<=4294967295
                AND jsonb_typeof(payload->'delta')='object'
                AND jsonb_typeof(payload->'delta'->'text')='string'
                AND (NOT (payload->'delta' ? 'truncated') OR payload->'delta'->'truncated'='false'::jsonb)
                AND (payload->'delta'->'original_bytes' IS NULL OR payload->'delta'->'original_bytes'='null'::jsonb)
                AND (payload->'delta'->'original_sha256' IS NULL OR payload->'delta'->'original_sha256'='null'::jsonb),false))
            INTO fragments,payload_bytes,truncated FROM run_events
            WHERE company_id=company AND run_id=run AND sequence<job.terminal_sequence
            AND event_type='assistant.delta' AND payload->'turn'=final_turn;
        IF fragments=0 OR fragments>4096 OR payload_bytes>1048576 OR truncated THEN
            RAISE EXCEPTION 'ortak: Work artifact requires a complete bounded final turn' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        SELECT string_agg(payload->'delta'->>'text','' ORDER BY sequence) INTO final_text FROM run_events
            WHERE company_id=company AND run_id=run AND sequence<job.terminal_sequence
            AND event_type='assistant.delta' AND payload->'turn'=final_turn;
        IF final_text IS NULL OR btrim(final_text,U&'\0009\000A\000B\000C\000D\0020\0085\00A0\1680\2000\2001\2002\2003\2004\2005\2006\2007\2008\2009\200A\2028\2029\202F\205F\3000')='' OR octet_length(final_text)>32768 THEN
            RAISE EXCEPTION 'ortak: Work artifact final text is empty or oversized' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        IF NOT EXISTS(SELECT 1 FROM artifacts art
            JOIN work_executions x ON x.company_id=art.company_id AND x.run_id=art.run_id
            JOIN runs r ON r.company_id=x.company_id AND r.id=x.run_id
            JOIN work_items w ON w.company_id=art.company_id AND w.id=art.work_item_id
            JOIN work_item_history h ON h.company_id=w.company_id AND h.work_item_id=w.id AND h.version=x.execution_version+1
            JOIN work_attachments attachment ON attachment.company_id=w.company_id AND attachment.work_item_id=w.id AND attachment.artifact_id=art.id
            WHERE art.company_id=company AND art.id=job.artifact_id AND art.run_id=run AND art.terminal_sequence=job.terminal_sequence
            AND art.project_id=x.project_id AND art.work_item_id=x.work_item_id
            AND art.content_bytes=convert_to(final_text,'UTF8')
            AND art.employee_id=x.employee_id AND art.employee_revision_id=x.employee_revision_id
            AND r.status='completed' AND r.delivery_intent='silent' AND w.state='review' AND w.version=x.execution_version+1
            AND h.event_type='work.execution_result_ready' AND h.actor_type='system' AND h.actor_id IS NULL
            AND h.payload->>'artifact_id'=art.id::text AND h.payload->>'run_id'=run::text
            AND h.xmin::text::bigint=txid_current()%4294967296 AND art.xmin::text::bigint=txid_current()%4294967296
            AND w.xmin::text::bigint=txid_current()%4294967296 AND attachment.xmin::text::bigint=txid_current()%4294967296
            AND x.result_code='result_ready' AND x.reconciled_at IS NOT NULL
            AND NOT EXISTS(SELECT 1 FROM work_acceptance_criteria cr WHERE cr.company_id=w.company_id AND cr.work_item_id=w.id AND cr.status<>'pending')
            AND NOT EXISTS(SELECT 1 FROM work_approvals ap WHERE ap.company_id=w.company_id AND ap.work_item_id=w.id AND ap.status<>'pending'))
        THEN RAISE EXCEPTION 'ortak: Work deliverable and review must commit atomically without human decisions' USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
    ELSIF TG_TABLE_NAME='artifacts' THEN
        RAISE EXCEPTION 'ortak: artifacts require their materialized Work output receipt' USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER work_output_provenance_at_commit AFTER INSERT OR UPDATE ON runtime_work_outputs
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_work_output_provenance();
CREATE CONSTRAINT TRIGGER artifact_provenance_at_commit AFTER INSERT ON artifacts
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_work_output_provenance();

-- Work ACL changes and late output receipts wake existing authenticated streams.
CREATE TRIGGER trg_activity_work_authority AFTER INSERT OR UPDATE ON work_authority_generations
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('');
CREATE TRIGGER trg_activity_work_outputs AFTER INSERT OR UPDATE OF state,artifact_id,last_error_code ON runtime_work_outputs
    FOR EACH ROW EXECUTE FUNCTION ortak_activity_notify('run_id');

-- Audited employee provisioning management (migration0064).
-- No credential values. Complete prepared selections are private server data.
CREATE TABLE employee_management_policies (
 company_id UUID NOT NULL REFERENCES companies(id), public_key TEXT NOT NULL CHECK(public_key ~ '^[0-9a-f]{64}$'),
 fingerprint BYTEA NOT NULL CHECK(octet_length(fingerprint)=32), enabled BOOLEAN NOT NULL,
 employee_ids TEXT[] NOT NULL CHECK(cardinality(employee_ids) BETWEEN 1 AND 64),
 channel_ids UUID[] NOT NULL CHECK(cardinality(channel_ids) BETWEEN 1 AND 64),
 PRIMARY KEY(company_id,public_key)
);
CREATE TABLE prepared_employee_catalog (
 company_id UUID NOT NULL REFERENCES companies(id), id UUID NOT NULL, employee_id TEXT NOT NULL,
 label TEXT NOT NULL CHECK(octet_length(label) BETWEEN 1 AND 128), enabled BOOLEAN NOT NULL DEFAULT true,
 configuration JSONB NOT NULL CHECK(jsonb_typeof(configuration)='object' AND octet_length(configuration::text)<=65536),
 fingerprint BYTEA NOT NULL CHECK(octet_length(fingerprint)=32),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(company_id,id)
);
CREATE TABLE employee_configuration_drafts (
 company_id UUID NOT NULL REFERENCES companies(id), id UUID NOT NULL, employee_id TEXT NOT NULL,
 catalog_id UUID NOT NULL, actor TEXT NOT NULL, expected_revision_id UUID,
 configuration JSONB NOT NULL CHECK(jsonb_typeof(configuration)='object' AND octet_length(configuration::text)<=65536),
 fingerprint BYTEA NOT NULL CHECK(octet_length(fingerprint)=32),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(company_id,id),
 FOREIGN KEY(company_id,catalog_id) REFERENCES prepared_employee_catalog(company_id,id)
);
CREATE TABLE employee_management_commands (
 company_id UUID NOT NULL REFERENCES companies(id), id UUID NOT NULL, employee_id TEXT NOT NULL,
 actor TEXT NOT NULL CHECK(actor ~ '^[0-9a-f]{64}$'), auth_event_id BYTEA NOT NULL CHECK(octet_length(auth_event_id)=32),
 policy_fingerprint BYTEA NOT NULL CHECK(octet_length(policy_fingerprint)=32),
 policy_snapshot JSONB NOT NULL CHECK(jsonb_typeof(policy_snapshot)='object' AND octet_length(policy_snapshot::text)<=16384),
 action TEXT NOT NULL CHECK(action IN ('adopt','update','retry','compensate')),
 idempotency_key TEXT NOT NULL CHECK(octet_length(idempotency_key) BETWEEN 1 AND 128),
 request_fingerprint BYTEA NOT NULL CHECK(octet_length(request_fingerprint)=32),
 draft_id UUID, operation_id UUID, expected_revision_id UUID,
 configuration JSONB CHECK(configuration IS NULL OR (jsonb_typeof(configuration)='object' AND octet_length(configuration::text)<=65536)),
 channel_ids UUID[] NOT NULL CHECK(cardinality(channel_ids) BETWEEN 0 AND 64),
 status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','running','succeeded','failed','blocked')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
 next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 lease_token UUID, lease_expires_at TIMESTAMPTZ, error_code TEXT CHECK(error_code ~ '^[a-z][a-z0-9_]{0,63}$'),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(company_id,id), UNIQUE(company_id,idempotency_key),
 FOREIGN KEY(company_id,draft_id) REFERENCES employee_configuration_drafts(company_id,id),
 FOREIGN KEY(company_id,operation_id) REFERENCES provisioning_operations(company_id,id),
 CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),
 CHECK(action='compensate' OR configuration IS NOT NULL)
);
CREATE UNIQUE INDEX employee_management_one_pending ON employee_management_commands(company_id,employee_id)
 WHERE status IN ('pending','running');
CREATE INDEX employee_management_due ON employee_management_commands(company_id,next_attempt_at,id)
 WHERE status IN ('pending','running');
CREATE TABLE employee_management_audit (
 company_id UUID NOT NULL REFERENCES companies(id), id UUID NOT NULL DEFAULT gen_random_uuid(),
 actor TEXT NOT NULL, auth_event_id BYTEA NOT NULL CHECK(octet_length(auth_event_id)=32),
 employee_id TEXT, command_id UUID, action TEXT NOT NULL CHECK(action IN ('catalog','draft','adopt','update','retry','compensate','command')),
 outcome TEXT NOT NULL CHECK(outcome IN ('accepted','read','denied','not_found','conflict','replayed')),
 created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), PRIMARY KEY(company_id,id)
);
CREATE FUNCTION ortak_management_immutable() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP IN ('DELETE','TRUNCATE') THEN RAISE EXCEPTION 'Management history is retained' USING ERRCODE='check_violation'; END IF;
 IF TG_TABLE_NAME='prepared_employee_catalog' AND (to_jsonb(NEW)-'enabled')=(to_jsonb(OLD)-'enabled') THEN RETURN NEW; END IF;
 IF TG_TABLE_NAME='employee_management_commands' AND
   (to_jsonb(NEW)-ARRAY['operation_id','status','attempts','next_attempt_at','lease_token','lease_expires_at','error_code','updated_at'])=
   (to_jsonb(OLD)-ARRAY['operation_id','status','attempts','next_attempt_at','lease_token','lease_expires_at','error_code','updated_at'])
   AND (OLD.operation_id IS NULL OR NEW.operation_id IS NOT DISTINCT FROM OLD.operation_id)
   AND NEW.attempts>=OLD.attempts
   AND (OLD.status IN ('pending','running') OR NEW=OLD) THEN RETURN NEW; END IF;
 RAISE EXCEPTION 'Management selection is immutable' USING ERRCODE='check_violation';
END $$;
CREATE TRIGGER prepared_employee_catalog_immutable BEFORE UPDATE OR DELETE ON prepared_employee_catalog FOR EACH ROW EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_configuration_drafts_immutable BEFORE UPDATE OR DELETE ON employee_configuration_drafts FOR EACH ROW EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_management_commands_immutable BEFORE UPDATE OR DELETE ON employee_management_commands FOR EACH ROW EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_management_audit_immutable BEFORE UPDATE OR DELETE ON employee_management_audit FOR EACH ROW EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER prepared_employee_catalog_no_truncate BEFORE TRUNCATE ON prepared_employee_catalog FOR EACH STATEMENT EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_configuration_drafts_no_truncate BEFORE TRUNCATE ON employee_configuration_drafts FOR EACH STATEMENT EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_management_commands_no_truncate BEFORE TRUNCATE ON employee_management_commands FOR EACH STATEMENT EXECUTE FUNCTION ortak_management_immutable();
CREATE TRIGGER employee_management_audit_no_truncate BEFORE TRUNCATE ON employee_management_audit FOR EACH STATEMENT EXECUTE FUNCTION ortak_management_immutable();

-- Caller holds Office authority before policy/command/employee/operation locks.
-- Shared policy locks prevent an old worker committing after policy replacement.
CREATE FUNCTION ortak_management_actor_allowed(target UUID, actor_key TEXT, policy_hash BYTEA, employee TEXT, channels UUID[]) RETURNS BOOLEAN
LANGUAGE plpgsql VOLATILE AS $$
DECLARE p employee_management_policies%ROWTYPE; community UUID; key_bytes BYTEA;
BEGIN
 SELECT * INTO p FROM employee_management_policies WHERE company_id=target AND public_key=actor_key FOR SHARE;
 IF NOT FOUND OR NOT p.enabled OR p.fingerprint<>policy_hash OR NOT(employee=ANY(p.employee_ids)) OR NOT(channels<@p.channel_ids) THEN RETURN false; END IF;
 SELECT b.community_id INTO community FROM office_company_bindings b JOIN companies c ON c.id=b.company_id
 JOIN communities cm ON cm.id=b.community_id WHERE b.company_id=target AND c.status='active' AND cm.deletion_state='active' AND cm.deleted_at IS NULL;
 IF community IS NULL THEN RETURN false; END IF;
 key_bytes:=decode(actor_key,'hex');
 IF NOT(EXISTS(SELECT 1 FROM relay_members WHERE community_id=community AND pubkey=actor_key)
     OR EXISTS(SELECT 1 FROM channel_members WHERE community_id=community AND pubkey=key_bytes AND removed_at IS NULL))
   OR EXISTS(SELECT 1 FROM users WHERE community_id=community AND pubkey=key_bytes AND (deactivated_at IS NOT NULL OR agent_type IS NOT NULL OR agent_owner_pubkey IS NOT NULL))
   OR EXISTS(SELECT 1 FROM employee_office_bindings WHERE company_id=target AND public_key=key_bytes)
   OR EXISTS(SELECT 1 FROM channel_members WHERE community_id=community AND pubkey=key_bytes AND role='bot') THEN RETURN false; END IF;
 RETURN NOT EXISTS(SELECT 1 FROM unnest(channels) selected(id) WHERE NOT EXISTS(
   SELECT 1 FROM channels c WHERE c.community_id=community AND c.id=selected.id AND c.deleted_at IS NULL
   AND c.channel_type::text='stream' AND (c.visibility::text='open' OR EXISTS(
     SELECT 1 FROM channel_members m WHERE m.community_id=community AND m.channel_id=c.id AND m.pubkey=key_bytes AND m.removed_at IS NULL))));
END $$;

CREATE FUNCTION ortak_management_guard(target UUID, command UUID, token UUID, operation UUID) RETURNS VOID
LANGUAGE plpgsql VOLATILE AS $$
DECLARE c employee_management_commands%ROWTYPE; op provisioning_operations%ROWTYPE; current_revision UUID; current_status TEXT;
BEGIN
 PERFORM set_config('lock_timeout','500ms',true);
 PERFORM set_config('statement_timeout','2s',true);
 PERFORM ortak_lock_office_authority(target);
 -- Read attribution before taking policy -> command locks. Immutable columns
 -- cannot change while the policy is checked.
 SELECT * INTO c FROM employee_management_commands WHERE company_id=target AND id=command;
 IF NOT FOUND OR NOT ortak_management_actor_allowed(target,c.actor,c.policy_fingerprint,c.employee_id,c.channel_ids) THEN
   RAISE EXCEPTION 'Management authority refused' USING ERRCODE='insufficient_privilege';
 END IF;
 SELECT * INTO c FROM employee_management_commands WHERE company_id=target AND id=command FOR UPDATE;
 IF c.status<>'running' OR c.lease_token IS DISTINCT FROM token OR c.lease_expires_at<=clock_timestamp() THEN
   RAISE EXCEPTION 'Management lease refused' USING ERRCODE='insufficient_privilege';
 END IF;
 IF c.operation_id IS NULL AND c.configuration IS NOT NULL THEN
   SELECT * INTO op FROM provisioning_operations WHERE company_id=target AND employee_id=c.employee_id AND idempotency_key=c.configuration->>'operation_key';
   IF FOUND THEN
     IF op.manifest IS DISTINCT FROM c.configuration->'manifest' OR op.mode IS DISTINCT FROM c.configuration->>'mode' OR op.dry_run THEN
       RAISE EXCEPTION 'Management operation mismatch' USING ERRCODE='check_violation';
     END IF;
     UPDATE employee_management_commands SET operation_id=op.id WHERE company_id=target AND id=command;
     c.operation_id:=op.id;
   END IF;
 END IF;
 IF operation IS NOT NULL AND c.operation_id IS DISTINCT FROM operation THEN
   RAISE EXCEPTION 'Management operation mismatch' USING ERRCODE='check_violation';
 END IF;
 IF c.operation_id IS NOT NULL THEN
   SELECT * INTO op FROM provisioning_operations WHERE company_id=target AND id=c.operation_id;
   IF NOT FOUND OR op.employee_id<>c.employee_id OR (c.action<>'compensate' AND
     (op.manifest IS DISTINCT FROM c.configuration->'manifest' OR op.mode IS DISTINCT FROM c.configuration->>'mode'
      OR op.idempotency_key IS DISTINCT FROM c.configuration->>'operation_key' OR op.dry_run)) THEN
     RAISE EXCEPTION 'Management operation scope mismatch' USING ERRCODE='check_violation';
   END IF;
 END IF;
 IF c.action<>'compensate' THEN
   SELECT active_revision_id,status INTO current_revision,current_status FROM employees WHERE company_id=target AND id=c.employee_id FOR SHARE;
   IF current_status='disabled' OR (current_revision IS DISTINCT FROM c.expected_revision_id AND NOT EXISTS(
     SELECT 1 FROM provisioning_operations o WHERE o.company_id=target AND o.id=c.operation_id AND o.result_revision_id=current_revision AND o.status='succeeded')) THEN
     RAISE EXCEPTION 'Management revision superseded' USING ERRCODE='check_violation';
   END IF;
 END IF;
 PERFORM set_config('ortak.management_command',command::text,true);
 PERFORM set_config('ortak.management_token',token::text,true);
END $$;

-- Direct CLI replays cannot bypass the delegated actor/lease for a managed
-- operation. Deferred validation also catches lease expiry before final commit.
CREATE FUNCTION ortak_management_operation_fence() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE target UUID; operation UUID; selected UUID; token UUID; managed BOOLEAN;
BEGIN
 target:=NEW.company_id;
 IF TG_TABLE_NAME='provisioning_operations' THEN operation:=NEW.id; ELSE operation:=NEW.operation_id; END IF;
 SELECT EXISTS(SELECT 1 FROM employee_management_commands c JOIN provisioning_operations o ON o.company_id=c.company_id
   AND (o.id=c.operation_id OR o.idempotency_key=c.configuration->>'operation_key')
   WHERE c.company_id=target AND o.id=operation) INTO managed;
 IF NOT managed THEN RETURN NEW; END IF;
 selected:=nullif(current_setting('ortak.management_command',true),'')::uuid;
 token:=nullif(current_setting('ortak.management_token',true),'')::uuid;
 IF selected IS NULL OR token IS NULL THEN RAISE EXCEPTION 'Managed operation requires its executor' USING ERRCODE='insufficient_privilege'; END IF;
 PERFORM ortak_management_guard(target,selected,token,operation);
 RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER employee_management_operation_at_commit AFTER INSERT OR UPDATE ON provisioning_operations
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_management_operation_fence();
CREATE CONSTRAINT TRIGGER employee_management_step_at_commit AFTER INSERT OR UPDATE ON provisioning_operation_steps
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_management_operation_fence();

-- Ortak employee lifecycle (migration0065).
-- Epoch pins permanently invalidate work predating a disable, including after re-enable.
ALTER TABLE employees ADD COLUMN lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(lifecycle_epoch>=0);
ALTER TABLE routing_recipients ADD COLUMN employee_lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(employee_lifecycle_epoch>=0);
ALTER TABLE runs ADD COLUMN employee_lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(employee_lifecycle_epoch>=0);
ALTER TABLE provisioning_operations ADD COLUMN employee_lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(employee_lifecycle_epoch>=0);
ALTER TABLE employee_configuration_drafts ADD COLUMN employee_lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(employee_lifecycle_epoch>=0),
 ADD COLUMN reenable BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE employee_management_commands ADD COLUMN employee_lifecycle_epoch BIGINT NOT NULL DEFAULT 0 CHECK(employee_lifecycle_epoch>=0);
ALTER TABLE employee_management_commands DROP CONSTRAINT employee_management_commands_action_check;
ALTER TABLE employee_management_commands ADD CONSTRAINT employee_management_commands_action_check CHECK(action IN('adopt','update','retry','compensate','disable','reenable'));
ALTER TABLE employee_management_commands DROP CONSTRAINT employee_management_commands_check1;
ALTER TABLE employee_management_commands ADD CONSTRAINT employee_management_commands_configuration_required CHECK(action IN('compensate','disable') OR configuration IS NOT NULL);
ALTER TABLE employee_management_audit DROP CONSTRAINT employee_management_audit_action_check;
ALTER TABLE employee_management_audit ADD CONSTRAINT employee_management_audit_action_check CHECK(action IN('catalog','draft','adopt','update','retry','compensate','command','disable','reenable'));

CREATE TABLE employee_lifecycle_events (
 company_id UUID NOT NULL REFERENCES companies(id), id UUID NOT NULL DEFAULT gen_random_uuid(), employee_id TEXT NOT NULL,
 action TEXT NOT NULL CHECK(action IN('disable','reenable')), lifecycle_epoch BIGINT NOT NULL CHECK(lifecycle_epoch>0),
 command_id UUID, command_lease_token UUID, command_lease_expires_at TIMESTAMPTZ,
 previous_revision_id UUID, result_revision_id UUID, created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(company_id,id), UNIQUE(company_id,employee_id,lifecycle_epoch,action),
 FOREIGN KEY(company_id,employee_id) REFERENCES employees(company_id,id),
 FOREIGN KEY(company_id,command_id) REFERENCES employee_management_commands(company_id,id),
 CHECK(action='disable' OR (command_id IS NOT NULL AND result_revision_id IS NOT NULL)),
 CHECK((command_id IS NULL)=(command_lease_token IS NULL)),
 CHECK((command_id IS NULL)=(command_lease_expires_at IS NULL))
);
-- Upgrade barrier for employees already disabled before epochs existed. Their
-- old recipients/runs/operations keep zero; a later re-enable must never revive
-- them. This is migration attribution (NULL command), not an invented human
-- disable timestamp. Apply before the transition-only event/epoch guards.
UPDATE employees SET lifecycle_epoch=1 WHERE status='disabled' AND lifecycle_epoch=0;
INSERT INTO employee_lifecycle_events(company_id,employee_id,action,lifecycle_epoch,previous_revision_id,result_revision_id)
SELECT company_id,id,'disable',lifecycle_epoch,active_revision_id,active_revision_id
FROM employees WHERE status='disabled' AND lifecycle_epoch=1;

-- Events can originate only from the actual employee transition trigger. A raw
-- INSERT cannot forge the retained attribution or lease witness.
CREATE FUNCTION ortak_guard_lifecycle_event_insert() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF pg_trigger_depth()<>2 THEN
   RAISE EXCEPTION 'Lifecycle event requires employee transition' USING ERRCODE='insufficient_privilege';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER employee_lifecycle_event_transition BEFORE INSERT ON employee_lifecycle_events FOR EACH ROW EXECUTE FUNCTION ortak_guard_lifecycle_event_insert();
CREATE TRIGGER employee_lifecycle_events_immutable BEFORE UPDATE OR DELETE ON employee_lifecycle_events FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER employee_lifecycle_events_no_truncate BEFORE TRUNCATE ON employee_lifecycle_events FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_pin_employee_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE epoch BIGINT;
BEGIN
 IF TG_OP='UPDATE' THEN
   IF NEW.employee_lifecycle_epoch IS DISTINCT FROM OLD.employee_lifecycle_epoch THEN
     RAISE EXCEPTION 'Employee lifecycle pin is immutable' USING ERRCODE='check_violation';
   END IF;
   RETURN NEW;
 END IF;
 PERFORM ortak_lock_office_authority(NEW.company_id);
 IF TG_TABLE_NAME='runs' THEN
 IF NEW.routing_decision_id IS NOT NULL THEN
   SELECT employee_lifecycle_epoch INTO epoch FROM routing_recipients WHERE company_id=NEW.company_id
     AND routing_decision_id=NEW.routing_decision_id AND employee_id=NEW.employee_id;
   IF epoch IS NULL THEN RAISE EXCEPTION 'Office lifecycle recipient missing' USING ERRCODE='check_violation'; END IF;
 END IF;
 ELSE
   SELECT lifecycle_epoch INTO epoch FROM employees WHERE company_id=NEW.company_id AND id=NEW.employee_id;
 END IF;
 IF TG_TABLE_NAME='runs' THEN
 IF epoch IS NULL THEN SELECT lifecycle_epoch INTO epoch FROM employees WHERE company_id=NEW.company_id AND id=NEW.employee_id; END IF;
 END IF;
 NEW.employee_lifecycle_epoch:=coalesce(epoch,0);
 RETURN NEW;
END $$;
CREATE TRIGGER lifecycle_pin_recipient BEFORE INSERT OR UPDATE ON routing_recipients FOR EACH ROW EXECUTE FUNCTION ortak_pin_employee_lifecycle();
CREATE TRIGGER lifecycle_pin_run BEFORE INSERT OR UPDATE ON runs FOR EACH ROW EXECUTE FUNCTION ortak_pin_employee_lifecycle();
CREATE TRIGGER lifecycle_pin_operation BEFORE INSERT OR UPDATE ON provisioning_operations FOR EACH ROW EXECUTE FUNCTION ortak_pin_employee_lifecycle();

-- No late snapshot/admission can refresh an old epoch into current authority.
CREATE FUNCTION ortak_check_run_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='UPDATE' AND NEW.office_admission_token IS NOT DISTINCT FROM OLD.office_admission_token
    AND NEW.office_admission_generation IS NOT DISTINCT FROM OLD.office_admission_generation
    AND NEW.office_admission_valid_before IS NOT DISTINCT FROM OLD.office_admission_valid_before
    AND NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token
    AND NEW.work_admission_generation IS NOT DISTINCT FROM OLD.work_admission_generation THEN RETURN NEW; END IF;
 IF NOT EXISTS(SELECT 1 FROM employees WHERE company_id=NEW.company_id AND id=NEW.employee_id
     AND status='active' AND lifecycle_epoch=NEW.employee_lifecycle_epoch) THEN
   RAISE EXCEPTION 'Employee lifecycle admission changed' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER lifecycle_run_admission AFTER INSERT OR UPDATE ON runs DEFERRABLE INITIALLY DEFERRED
 FOR EACH ROW EXECUTE FUNCTION ortak_check_run_lifecycle();

-- Old interrupted ordinary CLI operations may be retained/compensated, but may
-- not start another adapter step or activate after a disable/re-enable cycle.
CREATE FUNCTION ortak_check_provisioning_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE operation provisioning_operations%ROWTYPE; needs_admission BOOLEAN;
BEGIN
 IF TG_TABLE_NAME='provisioning_operations' THEN operation:=NEW;
   needs_admission:=NEW.status IN('running','succeeded') AND (TG_OP='INSERT' OR OLD.status<>'succeeded');
 ELSE
   SELECT * INTO operation FROM provisioning_operations WHERE company_id=NEW.company_id AND id=NEW.operation_id;
   needs_admission:=NEW.state IN('running','succeeded') AND operation.status<>'compensating';
 END IF;
 IF needs_admission AND EXISTS(SELECT 1 FROM employees WHERE company_id=operation.company_id AND id=operation.employee_id
      AND lifecycle_epoch<>operation.employee_lifecycle_epoch) THEN
   RAISE EXCEPTION 'Provisioning lifecycle epoch changed' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER lifecycle_provisioning_operation BEFORE INSERT OR UPDATE ON provisioning_operations FOR EACH ROW EXECUTE FUNCTION ortak_check_provisioning_lifecycle();
CREATE TRIGGER lifecycle_provisioning_step BEFORE INSERT OR UPDATE ON provisioning_operation_steps FOR EACH ROW EXECUTE FUNCTION ortak_check_provisioning_lifecycle();

CREATE OR REPLACE FUNCTION ortak_management_guard(target UUID, command UUID, token UUID, operation UUID) RETURNS VOID
LANGUAGE plpgsql VOLATILE AS $$
DECLARE c employee_management_commands%ROWTYPE; op provisioning_operations%ROWTYPE; current_revision UUID; current_status TEXT; current_epoch BIGINT;
BEGIN
 PERFORM set_config('lock_timeout','500ms',true);
 PERFORM set_config('statement_timeout','2s',true);
 PERFORM ortak_lock_office_authority(target);
 -- Read attribution before taking policy -> command locks. Immutable columns
 -- cannot change while the policy is checked.
 SELECT * INTO c FROM employee_management_commands WHERE company_id=target AND id=command;
 IF NOT FOUND OR NOT ortak_management_actor_allowed(target,c.actor,c.policy_fingerprint,c.employee_id,c.channel_ids) THEN
   RAISE EXCEPTION 'Management authority refused' USING ERRCODE='insufficient_privilege';
 END IF;
 SELECT * INTO c FROM employee_management_commands WHERE company_id=target AND id=command FOR UPDATE;
 IF c.status<>'running' OR c.lease_token IS DISTINCT FROM token OR c.lease_expires_at<=clock_timestamp() THEN
   RAISE EXCEPTION 'Management lease refused' USING ERRCODE='insufficient_privilege';
 END IF;
 IF c.operation_id IS NULL AND c.configuration IS NOT NULL THEN
   SELECT * INTO op FROM provisioning_operations WHERE company_id=target AND employee_id=c.employee_id AND idempotency_key=c.configuration->>'operation_key';
   IF FOUND THEN
     IF op.manifest IS DISTINCT FROM c.configuration->'manifest' OR op.mode IS DISTINCT FROM c.configuration->>'mode' OR op.dry_run THEN
       RAISE EXCEPTION 'Management operation mismatch' USING ERRCODE='check_violation';
     END IF;
     UPDATE employee_management_commands SET operation_id=op.id WHERE company_id=target AND id=command;
     c.operation_id:=op.id;
   END IF;
 END IF;
 IF operation IS NOT NULL AND c.operation_id IS DISTINCT FROM operation THEN
   RAISE EXCEPTION 'Management operation mismatch' USING ERRCODE='check_violation';
 END IF;
 IF c.operation_id IS NOT NULL THEN
   SELECT * INTO op FROM provisioning_operations WHERE company_id=target AND id=c.operation_id;
   IF NOT FOUND OR op.employee_id<>c.employee_id OR (c.action<>'compensate' AND
     (op.employee_lifecycle_epoch<>c.employee_lifecycle_epoch OR op.manifest IS DISTINCT FROM c.configuration->'manifest' OR op.mode IS DISTINCT FROM c.configuration->>'mode'
      OR op.idempotency_key IS DISTINCT FROM c.configuration->>'operation_key' OR op.dry_run)) THEN
     RAISE EXCEPTION 'Management operation scope mismatch' USING ERRCODE='check_violation';
   END IF;
 END IF;
 IF c.action<>'compensate' THEN
   SELECT active_revision_id,status,lifecycle_epoch INTO current_revision,current_status,current_epoch FROM employees WHERE company_id=target AND id=c.employee_id FOR SHARE;
   IF coalesce(current_epoch,0)<>c.employee_lifecycle_epoch OR (current_status='disabled' AND c.action NOT IN('reenable','disable')) OR (c.action='reenable' AND current_status<>'disabled' AND NOT EXISTS(SELECT 1 FROM provisioning_operations done WHERE done.company_id=target AND done.id=c.operation_id AND done.result_revision_id=current_revision AND done.status='succeeded')) OR (current_revision IS DISTINCT FROM c.expected_revision_id AND NOT EXISTS(
     SELECT 1 FROM provisioning_operations o WHERE o.company_id=target AND o.id=c.operation_id AND o.result_revision_id=current_revision AND o.status='succeeded')) THEN
     RAISE EXCEPTION 'Management revision superseded' USING ERRCODE='check_violation';
   END IF;
 END IF;
 PERFORM set_config('ortak.management_command',command::text,true);
 PERFORM set_config('ortak.management_token',token::text,true);
END $$;


-- Existing DB administration can always disable an employee; its automatic
-- retained event is marked by a NULL command rather than inventing a human.
-- Re-enable is exclusively a fresh, sealed management activation.
CREATE FUNCTION ortak_guard_employee_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE command UUID; token UUID; selected employee_management_commands%ROWTYPE;
BEGIN
 IF NEW.lifecycle_epoch<>OLD.lifecycle_epoch THEN
   RAISE EXCEPTION 'Lifecycle epoch advances only with disable' USING ERRCODE='check_violation';
 END IF;
 IF NEW.status='disabled' AND OLD.status<>'disabled' THEN
   NEW.lifecycle_epoch:=OLD.lifecycle_epoch+1;
   command:=nullif(current_setting('ortak.management_command',true),'')::uuid;
   IF command IS NOT NULL THEN
     token:=nullif(current_setting('ortak.management_token',true),'')::uuid;
     SELECT * INTO selected FROM employee_management_commands WHERE company_id=NEW.company_id AND id=command;
     IF selected.action IS DISTINCT FROM 'disable' OR selected.employee_id<>NEW.id OR selected.status<>'running'
        OR selected.lease_token IS DISTINCT FROM token OR selected.lease_expires_at<=clock_timestamp()
        OR selected.expected_revision_id IS DISTINCT FROM OLD.active_revision_id OR selected.employee_lifecycle_epoch<>OLD.lifecycle_epoch THEN
       RAISE EXCEPTION 'Disable intent changed' USING ERRCODE='insufficient_privilege';
     END IF;
   END IF;
   INSERT INTO employee_lifecycle_events(company_id,employee_id,action,lifecycle_epoch,command_id,command_lease_token,command_lease_expires_at,previous_revision_id,result_revision_id)
   VALUES(NEW.company_id,NEW.id,'disable',NEW.lifecycle_epoch,command,selected.lease_token,selected.lease_expires_at,OLD.active_revision_id,NEW.active_revision_id);
 ELSIF OLD.status='disabled' AND (NEW.status<>'disabled' OR NEW.active_revision_id IS DISTINCT FROM OLD.active_revision_id) THEN
   command:=nullif(current_setting('ortak.management_command',true),'')::uuid;
   token:=nullif(current_setting('ortak.management_token',true),'')::uuid;
   IF command IS NULL OR token IS NULL THEN RAISE EXCEPTION 'Re-enable requires sealed activation' USING ERRCODE='insufficient_privilege'; END IF;
   SELECT * INTO selected FROM employee_management_commands WHERE company_id=NEW.company_id AND id=command;
   IF selected.action IS DISTINCT FROM 'reenable' OR selected.employee_id<>NEW.id OR selected.status<>'running'
      OR selected.lease_token IS DISTINCT FROM token OR selected.lease_expires_at<=clock_timestamp()
      OR selected.expected_revision_id IS DISTINCT FROM OLD.active_revision_id OR selected.employee_lifecycle_epoch<>OLD.lifecycle_epoch
      OR NEW.status<>'active' OR NEW.active_revision_id IS NULL OR NEW.active_revision_id IS NOT DISTINCT FROM OLD.active_revision_id
      OR NOT EXISTS(SELECT 1 FROM employee_revisions r WHERE r.company_id=NEW.company_id AND r.employee_id=NEW.id AND r.id=NEW.active_revision_id
         AND r.created_by='provisioning:'||selected.operation_id::text AND r.xmin::text::bigint=txid_current()%4294967296) THEN
     RAISE EXCEPTION 'Re-enable intent changed' USING ERRCODE='insufficient_privilege';
   END IF;
   INSERT INTO employee_lifecycle_events(company_id,employee_id,action,lifecycle_epoch,command_id,command_lease_token,command_lease_expires_at,previous_revision_id,result_revision_id)
   VALUES(NEW.company_id,NEW.id,'reenable',NEW.lifecycle_epoch,command,selected.lease_token,selected.lease_expires_at,OLD.active_revision_id,NEW.active_revision_id);
 END IF;
 RETURN NEW;
END $$;
-- After the existing Office mutation trigger, which acquires exclusive authority.
CREATE TRIGGER ortak_z_employee_lifecycle BEFORE UPDATE ON employees FOR EACH ROW EXECUTE FUNCTION ortak_guard_employee_lifecycle();

CREATE FUNCTION ortak_check_lifecycle_activation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.command_id IS NOT NULL AND (NEW.command_lease_expires_at<=clock_timestamp() OR NOT EXISTS(
   SELECT 1 FROM employee_management_commands c WHERE c.company_id=NEW.company_id AND c.id=NEW.command_id
     AND c.employee_id=NEW.employee_id AND c.action=NEW.action
     AND c.expected_revision_id IS NOT DISTINCT FROM NEW.previous_revision_id
     AND c.employee_lifecycle_epoch=NEW.lifecycle_epoch-CASE WHEN NEW.action='disable' THEN 1 ELSE 0 END
     AND ortak_management_actor_allowed(c.company_id,c.actor,c.policy_fingerprint,c.employee_id,c.channel_ids)
     AND ((NEW.action='disable' AND c.status='succeeded' AND c.lease_token IS NULL AND c.lease_expires_at IS NULL)
       OR (NEW.action='reenable' AND c.status='running' AND c.lease_token=NEW.command_lease_token
         AND c.lease_expires_at=NEW.command_lease_expires_at)))) THEN
   RAISE EXCEPTION 'Lifecycle lease must remain valid at commit' USING ERRCODE='insufficient_privilege';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM employees e WHERE e.company_id=NEW.company_id AND e.id=NEW.employee_id
     AND e.lifecycle_epoch=NEW.lifecycle_epoch AND e.active_revision_id IS NOT DISTINCT FROM NEW.result_revision_id
     AND e.status=CASE WHEN NEW.action='disable' THEN 'disabled' ELSE 'active' END
     AND e.xmin::text::bigint=txid_current()%4294967296) THEN
   RAISE EXCEPTION 'Lifecycle transition must commit atomically' USING ERRCODE='serialization_failure';
 END IF;
 IF NEW.action='reenable' AND NOT EXISTS(SELECT 1 FROM employee_management_commands c
    JOIN provisioning_operations o ON o.company_id=c.company_id AND o.id=c.operation_id
    JOIN employees e ON e.company_id=c.company_id AND e.id=c.employee_id
    WHERE c.company_id=NEW.company_id AND c.id=NEW.command_id AND c.action='reenable'
    AND c.employee_id=NEW.employee_id AND c.employee_lifecycle_epoch=NEW.lifecycle_epoch
    AND o.status='succeeded' AND NOT o.dry_run AND o.mode='update'
    AND o.employee_lifecycle_epoch=NEW.lifecycle_epoch AND o.result_revision_id=NEW.result_revision_id
    AND e.status='active' AND e.active_revision_id=NEW.result_revision_id AND e.lifecycle_epoch=NEW.lifecycle_epoch) THEN
   RAISE EXCEPTION 'Re-enable activation must commit atomically' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER lifecycle_activation_at_commit AFTER INSERT ON employee_lifecycle_events DEFERRABLE INITIALLY DEFERRED
 FOR EACH ROW EXECUTE FUNCTION ortak_check_lifecycle_activation();

-- The shared Office fence covers these final effect commits. Terminal failure
-- receipts are still writable after revocation; they are retained accounting.
CREATE FUNCTION ortak_check_output_lifecycle() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE effect BOOLEAN;
BEGIN
 IF TG_TABLE_NAME='runtime_work_outputs' THEN effect:=NEW.state='materialized';
 ELSIF TG_TABLE_NAME='runtime_office_outputs' THEN effect:=NEW.state='enqueued';
 ELSIF TG_TABLE_NAME='runtime_memory_writes' THEN effect:=NEW.state='pending' AND NEW.admission_token IS NOT NULL;
   IF TG_OP='UPDATE' AND NEW.admission_token IS NOT DISTINCT FROM OLD.admission_token THEN effect:=false; END IF;
 ELSE effect:=true;
 END IF;
 IF effect AND NOT EXISTS(SELECT 1 FROM runs r JOIN employees e ON e.company_id=r.company_id AND e.id=r.employee_id
   WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND e.status='active' AND e.lifecycle_epoch=r.employee_lifecycle_epoch) THEN
   RAISE EXCEPTION 'Output lifecycle epoch changed' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER lifecycle_work_output_at_commit AFTER INSERT OR UPDATE ON runtime_work_outputs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_output_lifecycle();
CREATE CONSTRAINT TRIGGER lifecycle_artifact_at_commit AFTER INSERT ON artifacts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_output_lifecycle();
CREATE CONSTRAINT TRIGGER lifecycle_office_output_at_commit AFTER INSERT OR UPDATE ON runtime_office_outputs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_output_lifecycle();
CREATE CONSTRAINT TRIGGER lifecycle_memory_output_at_commit AFTER INSERT OR UPDATE ON runtime_memory_writes DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_check_output_lifecycle();

-- Reviewed project memory, migration0066.
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

CREATE TRIGGER trg_work_dependencies_no_truncate BEFORE TRUNCATE ON work_dependencies
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();


-- F2 durable runtime profile diagnostics, migration68.
-- The bridge owns OAuth values, real inference and its child containment proof.
-- This journal owns admission identity and recovery across CLI/API restarts.
CREATE TABLE provisioning_runtime_probes (
    company_id UUID NOT NULL,
    operation_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 20),
    probe_id UUID NOT NULL CHECK(probe_id<>'00000000-0000-0000-0000-000000000000'),
    bridge_origin TEXT NOT NULL CHECK(octet_length(bridge_origin)<=2048),
    bridge_token_env TEXT NOT NULL CHECK(bridge_token_env ~ '^[A-Za-z_][A-Za-z0-9_]{0,127}$'),
    state TEXT NOT NULL CHECK(state IN('running','succeeded','failed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    deadline TIMESTAMPTZ NOT NULL,
    contained_at TIMESTAMPTZ,
    error_code TEXT CHECK(error_code IN('probe_failed','probe_cancelled','probe_timeout',
        'probe_transport','probe_authority_changed','probe_unhealthy','probe_interrupted')),
    PRIMARY KEY(company_id,operation_id,generation),
    UNIQUE(company_id,probe_id),
    FOREIGN KEY(company_id,operation_id) REFERENCES provisioning_runner_selections(company_id,operation_id),
    CHECK(deadline>created_at AND deadline<=created_at+interval '90 seconds'),
    CHECK((state='running')=(contained_at IS NULL)),
    CHECK(contained_at IS NULL OR contained_at>=created_at),
    CHECK((state='failed')=(error_code IS NOT NULL))
);
-- New operations for the same employee must settle an older uncertain child.
CREATE UNIQUE INDEX provisioning_runtime_probe_one_running
    ON provisioning_runtime_probes(company_id,employee_id) WHERE state='running';

CREATE FUNCTION ortak_provisioning_runtime_probe_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE op provisioning_operations%ROWTYPE; prior INTEGER; epoch BIGINT; employee_status TEXT;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (to_jsonb(NEW)-ARRAY['state','contained_at','error_code']) IS DISTINCT FROM
           (to_jsonb(OLD)-ARRAY['state','contained_at','error_code'])
           OR OLD.state<>'running' OR NEW.state='running' OR NEW.contained_at>clock_timestamp() THEN
            RAISE EXCEPTION 'Runtime probe only permits one contained terminal receipt'
                USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        -- Revoked/expired operations can still retain successful cleanup. They
        -- cannot turn that cleanup into a current readiness witness.
        IF NEW.state='failed' THEN RETURN NEW; END IF;
    END IF;
    PERFORM ortak_lock_office_authority(NEW.company_id);
    IF NOT EXISTS(SELECT 1 FROM companies c JOIN office_company_bindings b ON b.company_id=c.id
        JOIN communities cm ON cm.id=b.community_id WHERE c.id=NEW.company_id AND c.status='active'
        AND cm.deletion_state='active' AND cm.deleted_at IS NULL) THEN
        RAISE EXCEPTION 'Runtime probe Office authority unavailable' USING ERRCODE='insufficient_privilege';
    END IF;
    SELECT * INTO op FROM provisioning_operations
        WHERE company_id=NEW.company_id AND id=NEW.operation_id FOR UPDATE NOWAIT;
    IF NOT FOUND OR op.employee_id<>NEW.employee_id OR op.dry_run
       OR op.status NOT IN('pending','running','failed')
       OR op.manifest->>'provisioning' IS DISTINCT FROM 'adopt'
       OR op.manifest#>>'{employee,runtime,adapter}' IS DISTINCT FROM 'hermes' THEN
        RAISE EXCEPTION 'Runtime probe operation unavailable' USING ERRCODE='check_violation';
    END IF;
    SELECT lifecycle_epoch,status INTO epoch,employee_status FROM employees
        WHERE company_id=NEW.company_id AND id=NEW.employee_id FOR SHARE;
    IF op.employee_lifecycle_epoch<>coalesce(epoch,0) OR (employee_status='disabled' AND NOT EXISTS(
        SELECT 1 FROM employee_management_commands c JOIN employees e
          ON e.company_id=c.company_id AND e.id=c.employee_id
        WHERE c.company_id=NEW.company_id AND c.id=nullif(current_setting('ortak.management_command',true),'')::uuid
          AND c.operation_id=op.id AND c.action='reenable' AND c.employee_lifecycle_epoch=e.lifecycle_epoch
          AND c.expected_revision_id IS NOT DISTINCT FROM e.active_revision_id)) THEN
        RAISE EXCEPTION 'Runtime probe lifecycle changed' USING ERRCODE='serialization_failure';
    END IF;
    IF TG_OP='INSERT' AND TG_WHEN='BEFORE' THEN
        SELECT coalesce(max(generation),0) INTO prior FROM provisioning_runtime_probes
            WHERE company_id=NEW.company_id AND operation_id=NEW.operation_id;
        IF NEW.generation<>prior+1 OR NEW.state<>'running' OR NEW.contained_at IS NOT NULL
           OR NEW.error_code IS NOT NULL OR NEW.created_at>clock_timestamp() OR NEW.deadline<=clock_timestamp() THEN
            RAISE EXCEPTION 'Runtime probe admission is not the next bounded attempt' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.deadline<=clock_timestamp() THEN
        RAISE EXCEPTION 'Runtime probe readiness expired before commit' USING ERRCODE='serialization_failure';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER provisioning_runtime_probe_guard BEFORE INSERT OR UPDATE ON provisioning_runtime_probes
    FOR EACH ROW EXECUTE FUNCTION ortak_provisioning_runtime_probe_guard();
CREATE TRIGGER provisioning_runtime_probe_no_delete BEFORE DELETE ON provisioning_runtime_probes
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER provisioning_runtime_probe_no_truncate BEFORE TRUNCATE ON provisioning_runtime_probes
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
-- Management admission/success cannot outlive the originating actor or lease.
-- Failed containment accounting remains possible after those rights disappear.
CREATE CONSTRAINT TRIGGER provisioning_runtime_probe_management_at_commit
    AFTER INSERT OR UPDATE ON provisioning_runtime_probes DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW WHEN(NEW.state<>'failed') EXECUTE FUNCTION ortak_management_operation_fence();
CREATE CONSTRAINT TRIGGER provisioning_runtime_probe_live_at_commit
    AFTER INSERT OR UPDATE ON provisioning_runtime_probes DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW WHEN(NEW.state<>'failed') EXECUTE FUNCTION ortak_provisioning_runtime_probe_guard();


CREATE TABLE reviewed_memory_targets (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    deployment_id UUID NOT NULL,
    binding JSONB NOT NULL CHECK(jsonb_typeof(binding)='object' AND octet_length(binding::text)<=8192),
    creation_receipt JSONB NOT NULL CHECK(jsonb_typeof(creation_receipt)='object' AND octet_length(creation_receipt::text)<=16384),
    binding_hash BYTEA NOT NULL CHECK(octet_length(binding_hash)=32),
    employee_revision_id UUID NOT NULL,
    employee_lifecycle_epoch BIGINT NOT NULL CHECK(employee_lifecycle_epoch>=0),
    runtime_consumption_enabled BOOLEAN NOT NULL DEFAULT false,
    consumption_epoch BIGINT NOT NULL DEFAULT 0 CHECK(consumption_epoch>=0),
    enabled BOOLEAN NOT NULL,
    valid_until TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,project_id,employee_id,deployment_id,binding_hash),
    FOREIGN KEY(company_id,project_id) REFERENCES projects(company_id,id),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id),
    CHECK(coalesce(creation_receipt->>'company_id'=company_id::text AND creation_receipt->>'employee_id'=employee_id
        AND creation_receipt->>'deployment_id'=deployment_id::text AND creation_receipt->'binding'=binding
        AND creation_receipt->>'request_hash' ~ '^[0-9a-f]{64}$' AND jsonb_typeof(creation_receipt->'native_ids')='object',false))
);

CREATE TABLE reviewed_memory_exports (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    project_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    target_id UUID NOT NULL,
    employee_revision_id UUID NOT NULL,
    employee_lifecycle_epoch BIGINT NOT NULL CHECK(employee_lifecycle_epoch>=0),
    content_hash BYTEA NOT NULL CHECK(octet_length(content_hash)=32),
    source_hash BYTEA NOT NULL CHECK(octet_length(source_hash)=32),
    requested_by TEXT NOT NULL CHECK(requested_by ~ '^[0-9a-f]{64}$'),
    operation_id UUID NOT NULL CHECK(operation_id<>'00000000-0000-0000-0000-000000000000'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,fact_id),
    FOREIGN KEY(company_id,fact_id) REFERENCES reviewed_memory_facts(company_id,id),
    FOREIGN KEY(company_id,project_id) REFERENCES projects(company_id,id),
    FOREIGN KEY(company_id,target_id) REFERENCES reviewed_memory_targets(company_id,id),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id)
);

-- Two stable operations suffice: scheduled withdrawal also handles expiry and
-- may precede an uncertain publication. Distinct expiry/withdraw keys would race
-- for the extension's one irreversible withdrawal identity.
CREATE TABLE reviewed_memory_export_jobs (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    action TEXT NOT NULL CHECK(action IN('publish','withdraw')),
    idempotency_key TEXT NOT NULL CHECK(idempotency_key ~ '^[a-z0-9:-]{1,200}$'),
    request_hash BYTEA NOT NULL CHECK(octet_length(request_hash)=32),
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','acknowledged','failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count BETWEEN 0 AND 20),
    total_attempts INTEGER NOT NULL DEFAULT 0 CHECK(total_attempts BETWEEN 0 AND 180),
    retry_version INTEGER NOT NULL DEFAULT 0 CHECK(retry_version BETWEEN 0 AND 8),
    next_attempt_at TIMESTAMPTZ NOT NULL,
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT CHECK(last_error_code IN('authority_refused','target_unavailable','service_retry','service_refused','database_retry','deadline','lease_exhausted')),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,fact_id,action),
    UNIQUE(company_id,idempotency_key),
    FOREIGN KEY(company_id,fact_id) REFERENCES reviewed_memory_exports(company_id,fact_id),
    CHECK((lease_token IS NULL)=(lease_expires_at IS NULL))
);
CREATE INDEX reviewed_memory_export_due ON reviewed_memory_export_jobs(company_id,next_attempt_at,fact_id,action)
    WHERE state='pending';

CREATE TABLE reviewed_memory_export_commands (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    actor_pubkey TEXT NOT NULL CHECK(actor_pubkey ~ '^[0-9a-f]{64}$'),
    operation_id UUID NOT NULL CHECK(operation_id<>'00000000-0000-0000-0000-000000000000'),
    fact_id UUID NOT NULL,
    action TEXT NOT NULL CHECK(action IN('publish','retry_publish','retry_withdraw')),
    request_hash BYTEA NOT NULL CHECK(octet_length(request_hash)=32),
    result_version INTEGER NOT NULL CHECK(result_version BETWEEN 0 AND 8),
    auth_event_id BYTEA NOT NULL CHECK(octet_length(auth_event_id)=32),
    valid_before TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,actor_pubkey,operation_id),
    UNIQUE(company_id,fact_id,action,result_version),
    FOREIGN KEY(company_id,fact_id) REFERENCES reviewed_memory_exports(company_id,fact_id) DEFERRABLE INITIALLY DEFERRED
);
ALTER TABLE reviewed_memory_exports ADD CONSTRAINT reviewed_export_instruction
    FOREIGN KEY(company_id,requested_by,operation_id)
    REFERENCES reviewed_memory_export_commands(company_id,actor_pubkey,operation_id) DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE reviewed_memory_export_receipts (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    action TEXT NOT NULL CHECK(action IN('publish','withdraw')),
    request_hash BYTEA NOT NULL CHECK(octet_length(request_hash)=32),
    binding_hash BYTEA NOT NULL CHECK(octet_length(binding_hash)=32),
    content_hash BYTEA CHECK(octet_length(content_hash)=32),
    remote_status TEXT NOT NULL CHECK(remote_status IN('active','expired','withdrawn')),
    erased_from_reviewed_store BOOLEAN NOT NULL,
    tombstone_at TIMESTAMPTZ,
    lease_token UUID NOT NULL,
    total_attempts INTEGER NOT NULL CHECK(total_attempts BETWEEN 1 AND 180),
    acknowledged_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,fact_id,action),
    FOREIGN KEY(company_id,fact_id,action) REFERENCES reviewed_memory_export_jobs(company_id,fact_id,action),
    CHECK(erased_from_reviewed_store=(tombstone_at IS NOT NULL)),
    CHECK(action<>'withdraw' OR (erased_from_reviewed_store AND remote_status<>'active'))
);

CREATE FUNCTION ortak_reviewed_export_source_hash(f reviewed_memory_facts)
RETURNS BYTEA LANGUAGE sql STABLE AS $$
    SELECT NULL::bytea
$$;

-- pgschema orders SQL function bodies before their cross-table dependencies.
-- This fail-closed bootstrap body is replaced by the exact migration75 body in
-- reconcile-schema-after-pgschema.sql; full catalog parity gates the result.
CREATE FUNCTION ortak_reviewed_export_eligible(company UUID, fact UUID, target UUID) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT false
$$;

CREATE FUNCTION ortak_reviewed_target_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE selecting BOOLEAN;
BEGIN
    IF TG_OP='UPDATE' AND
        (to_jsonb(NEW)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at'
            -'runtime_consumption_enabled'-'consumption_epoch'-'conversation_channel_id'
            -'conversation_consumption_enabled'-'conversation_consumption_epoch')
        IS DISTINCT FROM
        (to_jsonb(OLD)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'valid_until'-'updated_at'
            -'runtime_consumption_enabled'-'consumption_epoch'-'conversation_channel_id'
            -'conversation_consumption_enabled'-'conversation_consumption_epoch') THEN
        RAISE EXCEPTION 'ortak: reviewed target identity is immutable' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='INSERT' THEN
        IF NEW.consumption_epoch<>0 OR NEW.conversation_consumption_epoch<>0 THEN
            RAISE EXCEPTION 'ortak: invalid initial consumption epoch' USING ERRCODE='check_violation';
        END IF;
        selecting=NEW.conversation_channel_id IS NOT NULL;
    ELSE
        IF NEW.consumption_epoch<>OLD.consumption_epoch
            OR NEW.conversation_consumption_epoch<>OLD.conversation_consumption_epoch THEN
            RAISE EXCEPTION 'ortak: consumption epochs are server derived' USING ERRCODE='check_violation';
        END IF;
        IF OLD.conversation_channel_id IS NOT NULL
            AND NEW.conversation_channel_id IS DISTINCT FROM OLD.conversation_channel_id THEN
            RAISE EXCEPTION 'ortak: conversation target channel is immutable' USING ERRCODE='check_violation';
        END IF;
        IF OLD.runtime_consumption_enabled AND NOT NEW.runtime_consumption_enabled THEN
            NEW.consumption_epoch=OLD.consumption_epoch+1;
        END IF;
        IF OLD.conversation_consumption_enabled AND NOT NEW.conversation_consumption_enabled THEN
            NEW.conversation_consumption_epoch=OLD.conversation_consumption_epoch+1;
        END IF;
        selecting=OLD.conversation_channel_id IS NULL AND NEW.conversation_channel_id IS NOT NULL;
    END IF;
    IF NEW.enabled AND (NEW.valid_until<=clock_timestamp() OR NEW.valid_until>clock_timestamp()+INTERVAL '60 seconds') THEN
        RAISE EXCEPTION 'ortak: reviewed target witness must be short and live' USING ERRCODE='check_violation';
    END IF;
    -- A disable-only advertisement must still work after source/identity loss.
    -- In particular the existing advertise transaction briefly sets enabled=false
    -- before refreshing its selected rows. That is not conversation opt-out and
    -- must not advance its separate epoch or fail a stale-scope current check.
    IF selecting OR (NEW.conversation_consumption_enabled AND NEW.enabled) THEN
        PERFORM ortak_lock_office_authority(NEW.company_id);
        PERFORM 1 FROM projects p WHERE p.company_id=NEW.company_id AND p.id=NEW.project_id FOR SHARE NOWAIT;
        PERFORM 1 FROM conversation_memory_authorities authority
            WHERE authority.company_id=NEW.company_id AND authority.community_id=NEW.community_id
                AND authority.project_id=NEW.project_id AND authority.channel_id=NEW.conversation_channel_id FOR SHARE;
        IF NOT FOUND OR NOT ortak_conversation_scope_current(
                NEW.company_id,NEW.community_id,NEW.project_id,NEW.conversation_channel_id)
            OR NOT EXISTS (SELECT 1 FROM employees e
                JOIN employee_revisions rev ON rev.company_id=e.company_id AND rev.employee_id=e.id AND rev.id=e.active_revision_id
                JOIN employee_memory_bindings memory ON memory.company_id=e.company_id AND memory.employee_id=e.id AND memory.revision_id=e.active_revision_id
                WHERE e.company_id=NEW.company_id AND e.id=NEW.employee_id AND e.status='active'
                    AND NEW.employee_revision_id=e.active_revision_id AND NEW.employee_lifecycle_epoch=e.lifecycle_epoch
                    AND NEW.binding=rev.manifest->'memory' AND memory.validated_at IS NOT NULL
                    AND NEW.binding=jsonb_build_object('adapter',memory.adapter,'endpoint_ref',memory.endpoint_ref,
                        'workspace',memory.workspace,'user_peer',memory.user_peer,'employee_peer',memory.employee_peer,'options',memory.options))
            OR (NEW.conversation_consumption_enabled AND (NOT NEW.enabled OR NEW.valid_until<=clock_timestamp())) THEN
            RAISE EXCEPTION 'ortak: conversation target requires current selected scope and binding'
                USING ERRCODE='check_violation';
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER reviewed_target_guard BEFORE INSERT OR UPDATE ON reviewed_memory_targets FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_target_guard();

CREATE FUNCTION ortak_reviewed_export_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM reviewed_memory_facts f JOIN reviewed_memory_targets t ON t.company_id=f.company_id AND t.id=NEW.target_id
        WHERE f.company_id=NEW.company_id AND f.id=NEW.fact_id AND f.project_id=NEW.project_id AND f.employee_id=NEW.employee_id
        AND f.community_id=NEW.community_id AND NEW.content_hash=sha256(convert_to(f.content,'UTF8'))
        AND NEW.source_hash=ortak_reviewed_export_source_hash(f) AND t.employee_revision_id=NEW.employee_revision_id
        AND t.employee_lifecycle_epoch=NEW.employee_lifecycle_epoch AND (CASE WHEN f.audience_kind='conversation' THEN ortak_conversation_export_eligible(f.company_id,f.id,t.id) ELSE ortak_reviewed_export_eligible(f.company_id,f.id,t.id) END))
      OR NOT EXISTS(SELECT 1 FROM reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id AND o.fact_id=NEW.fact_id
        AND o.actor_pubkey=NEW.requested_by AND o.operation_id=NEW.operation_id AND o.action='publish' AND o.result_version=0
        AND o.xmin::text::bigint=txid_current()%4294967296)
      OR (SELECT count(*) FROM reviewed_memory_export_jobs j WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id
        AND j.state='pending' AND j.attempt_count=0 AND j.xmin::text::bigint=txid_current()%4294967296)<>2 THEN
        RAISE EXCEPTION 'ortak: reviewed export requires current fact, atomic instruction and two jobs' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER reviewed_export_at_commit AFTER INSERT ON reviewed_memory_exports DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_at_commit();

CREATE FUNCTION ortak_reviewed_export_stop() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    UPDATE reviewed_memory_export_jobs SET next_attempt_at=least(next_attempt_at,NEW.revoked_at),updated_at=clock_timestamp()
        WHERE company_id=NEW.company_id AND fact_id=NEW.id AND action='withdraw' AND state='pending';
    RETURN NEW;
END $$;
CREATE TRIGGER reviewed_export_stop AFTER UPDATE ON reviewed_memory_facts FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_stop();

CREATE FUNCTION ortak_reviewed_export_job_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE allowed BOOLEAN:=false;
BEGIN
    IF (NEW.company_id,NEW.community_id,NEW.fact_id,NEW.action,NEW.idempotency_key,NEW.request_hash)
        IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.fact_id,OLD.action,OLD.idempotency_key,OLD.request_hash)
        OR OLD.state='acknowledged' OR NEW.total_attempts<OLD.total_attempts OR NEW.total_attempts>OLD.total_attempts+1
        OR NEW.retry_version<OLD.retry_version OR NEW.retry_version>OLD.retry_version+1 THEN
        RAISE EXCEPTION 'ortak: reviewed job identity and progress are retained' USING ERRCODE='check_violation';
    END IF;
    IF NEW.retry_version=OLD.retry_version+1 THEN
        allowed:=OLD.state='failed' AND OLD.lease_token IS NULL AND NEW.state='pending' AND NEW.attempt_count=0
            AND NEW.total_attempts=OLD.total_attempts AND NEW.lease_token IS NULL AND NEW.last_error_code IS NULL
            AND NEW.next_attempt_at<=clock_timestamp();
    ELSIF NEW.attempt_count=OLD.attempt_count+1 AND NEW.total_attempts=OLD.total_attempts+1 THEN
        allowed:=OLD.state='pending' AND NEW.state='pending' AND OLD.next_attempt_at<=clock_timestamp()
            AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at<=clock_timestamp())
            AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token
            AND NEW.lease_expires_at>clock_timestamp() AND NEW.lease_expires_at<=clock_timestamp()+INTERVAL '60 seconds'
            AND NEW.next_attempt_at=OLD.next_attempt_at AND NEW.last_error_code IS NOT DISTINCT FROM OLD.last_error_code;
    ELSIF NEW.attempt_count=OLD.attempt_count AND NEW.total_attempts=OLD.total_attempts AND OLD.state='pending' THEN
        IF NEW.state='acknowledged' THEN
            allowed:=OLD.lease_token IS NOT NULL AND OLD.lease_expires_at>clock_timestamp()
                AND NEW.lease_token=OLD.lease_token AND NEW.lease_expires_at=OLD.lease_expires_at
                AND NEW.next_attempt_at=OLD.next_attempt_at AND NEW.last_error_code IS NULL;
        ELSIF NEW.state='failed' AND NEW.last_error_code='lease_exhausted' THEN
            allowed:=OLD.attempt_count=20 AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at<=clock_timestamp())
                AND NEW.lease_token IS NULL AND NEW.next_attempt_at=OLD.next_attempt_at;
        ELSIF NEW.state='pending' AND NEW.action='withdraw' AND NEW.next_attempt_at<=OLD.next_attempt_at THEN
            allowed:=(NEW.lease_token,NEW.lease_expires_at,NEW.last_error_code)
                IS NOT DISTINCT FROM (OLD.lease_token,OLD.lease_expires_at,OLD.last_error_code)
                AND EXISTS(SELECT 1 FROM reviewed_memory_facts f WHERE f.company_id=NEW.company_id AND f.id=NEW.fact_id
                    AND f.revoked_at IS NOT NULL AND NEW.next_attempt_at=least(OLD.next_attempt_at,f.revoked_at)
                    AND f.xmin::text::bigint=txid_current()%4294967296);
        ELSIF NEW.lease_token IS NULL AND NEW.last_error_code IS NOT NULL THEN
            allowed:=OLD.lease_token IS NOT NULL AND OLD.lease_expires_at>clock_timestamp()
                AND NEW.next_attempt_at>clock_timestamp() AND NEW.next_attempt_at<=clock_timestamp()+INTERVAL '301 seconds'
                AND (NEW.state='failed' OR NEW.state='pending' AND OLD.attempt_count<20);
        END IF;
    END IF;
    IF NOT coalesce(allowed,false) THEN
        RAISE EXCEPTION 'ortak: reviewed job transition lacks a due claim, live lease, stop or audited retry' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER reviewed_export_job_guard BEFORE UPDATE ON reviewed_memory_export_jobs FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_job_guard();

CREATE FUNCTION ortak_reviewed_export_job_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NOT EXISTS(SELECT 1 FROM reviewed_memory_exports x JOIN reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
            WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id AND x.community_id=NEW.community_id
            AND x.xmin::text::bigint=txid_current()%4294967296 AND NEW.state='pending' AND NEW.attempt_count=0
            AND NEW.total_attempts=0 AND NEW.retry_version=0 AND NEW.last_error_code IS NULL
            AND NEW.idempotency_key='reviewed:'||NEW.action||':'||NEW.fact_id::text
            AND NEW.lease_token IS NULL AND ((NEW.action='withdraw' AND NEW.next_attempt_at=f.expires_at)
                OR (NEW.action='publish' AND NEW.next_attempt_at<=clock_timestamp()))) THEN
            RAISE EXCEPTION 'ortak: reviewed job requires atomic publication' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.retry_version<>OLD.retry_version THEN
        IF NOT EXISTS(SELECT 1 FROM reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id AND o.fact_id=NEW.fact_id
            AND o.action='retry_'||NEW.action AND o.result_version=NEW.retry_version AND o.xmin::text::bigint=txid_current()%4294967296) THEN
            RAISE EXCEPTION 'ortak: reviewed retry requires atomic human command' USING ERRCODE='check_violation';
        END IF;
    END IF;
    IF NEW.state='acknowledged' AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts r
        WHERE r.company_id=NEW.company_id AND r.fact_id=NEW.fact_id AND r.action=NEW.action AND r.request_hash=NEW.request_hash
          AND r.lease_token=NEW.lease_token AND r.total_attempts=NEW.total_attempts AND NEW.lease_expires_at>clock_timestamp()
          AND r.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed acknowledgement requires atomic live-lease receipt' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER reviewed_export_job_at_commit AFTER INSERT OR UPDATE ON reviewed_memory_export_jobs DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_job_at_commit();

CREATE FUNCTION ortak_reviewed_export_command_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.valid_before IS NOT NULL AND NEW.valid_before<=clock_timestamp() THEN
        RAISE EXCEPTION 'ortak: reviewed command authority expired' USING ERRCODE='serialization_failure';
    END IF;
    IF (NEW.action='publish' AND NOT EXISTS(SELECT 1 FROM reviewed_memory_exports x WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id
        AND x.operation_id=NEW.operation_id AND x.requested_by=NEW.actor_pubkey AND NEW.result_version=0 AND x.xmin::text::bigint=txid_current()%4294967296))
        OR (NEW.action<>'publish' AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_jobs j WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id
            AND 'retry_'||j.action=NEW.action AND j.retry_version=NEW.result_version AND j.xmin::text::bigint=txid_current()%4294967296)) THEN
        RAISE EXCEPTION 'ortak: reviewed command requires its atomic effect' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER reviewed_export_command_at_commit AFTER INSERT ON reviewed_memory_export_commands DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_command_at_commit();

CREATE FUNCTION ortak_reviewed_export_receipt_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM reviewed_memory_export_jobs j
        JOIN reviewed_memory_exports x ON x.company_id=j.company_id AND x.fact_id=j.fact_id
        JOIN reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id AND j.action=NEW.action AND j.community_id=NEW.community_id
        AND j.state='acknowledged' AND j.request_hash=NEW.request_hash AND t.binding_hash=NEW.binding_hash
        AND (NEW.content_hash=x.content_hash OR NEW.content_hash IS NULL AND NEW.action='withdraw'
            AND NOT EXISTS(SELECT 1 FROM reviewed_memory_export_receipts p
                WHERE p.company_id=NEW.company_id AND p.fact_id=NEW.fact_id AND p.action='publish'))
        AND j.lease_token=NEW.lease_token AND j.total_attempts=NEW.total_attempts AND j.lease_expires_at>clock_timestamp()
        AND j.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed receipt requires its exact live job' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER reviewed_export_receipt_at_commit AFTER INSERT ON reviewed_memory_export_receipts DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_export_receipt_at_commit();

-- Fail-closed desired bootstrap; reconciler installs the exact migration71 view.
CREATE FUNCTION ortak_reviewed_export_view(company UUID,fact UUID) RETURNS JSONB LANGUAGE sql STABLE AS $$
    SELECT NULL::jsonb
$$;

DO $$ DECLARE relation TEXT; BEGIN
    FOREACH relation IN ARRAY ARRAY['reviewed_memory_targets','reviewed_memory_exports','reviewed_memory_export_jobs','reviewed_memory_export_commands','reviewed_memory_export_receipts'] LOOP
        EXECUTE format('CREATE TRIGGER reviewed_export_no_delete BEFORE DELETE ON %I FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()',relation);
        EXECUTE format('CREATE TRIGGER reviewed_export_no_truncate BEFORE TRUNCATE ON %I FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate()',relation);
        PERFORM attach_community_write_fence(relation);
    END LOOP;
    FOREACH relation IN ARRAY ARRAY['reviewed_memory_exports','reviewed_memory_export_commands','reviewed_memory_export_receipts'] LOOP
        EXECUTE format('CREATE TRIGGER reviewed_export_immutable BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation()',relation);
    END LOOP;
END $$;


-- Migration70: independent fresh Work decomposition and immutable history.
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

-- Migration71: reviewed uses. SQL eligibility/view bootstrap bodies fail closed;
-- mandatory reconciliation restores exact migration bodies before serving.
-- A prior publication revision is evidence, not employee identity. Current exact
-- binding/permissions and an explicit current runtime opt-in are authoritative.
CREATE FUNCTION ortak_reviewed_runtime_eligible(company UUID, fact UUID, target UUID, epoch BIGINT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT false
$$;

CREATE TABLE run_reviewed_memory_uses (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 7),
    fact_id UUID NOT NULL,
    target_id UUID NOT NULL,
    fact_version BIGINT NOT NULL CHECK(fact_version=1),
    consumption_epoch BIGINT NOT NULL CHECK(consumption_epoch>=0),
    content_hash BYTEA NOT NULL CHECK(octet_length(content_hash)=32),
    source_hash BYTEA NOT NULL CHECK(octet_length(source_hash)=32),
    binding_hash BYTEA NOT NULL CHECK(octet_length(binding_hash)=32),
    approval_id UUID NOT NULL,
    approved_by TEXT NOT NULL CHECK(approved_by ~ '^[0-9a-f]{64}$'),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id,ordinal),
    UNIQUE(company_id,run_id,fact_id),
    FOREIGN KEY(company_id,run_id) REFERENCES run_context_snapshots(company_id,run_id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(company_id,fact_id) REFERENCES reviewed_memory_exports(company_id,fact_id),
    FOREIGN KEY(company_id,target_id) REFERENCES reviewed_memory_targets(company_id,id)
);
CREATE INDEX idx_run_reviewed_memory_fact ON run_reviewed_memory_uses(company_id,fact_id,run_id);
CREATE INDEX idx_run_reviewed_memory_expiry ON run_reviewed_memory_uses(company_id,expires_at,run_id);
SELECT attach_community_write_fence('run_reviewed_memory_uses');

CREATE FUNCTION ortak_run_reviewed_memory_current(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT false
$$;

-- Caller holds Office -> project -> Work before acquiring sorted fact locks;
-- run/outbox locks follow. No provider work occurs under this fence.
CREATE FUNCTION ortak_lock_run_reviewed_memory(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE plpgsql AS $$
BEGIN
    PERFORM ortak_lock_office_authority(company);
    PERFORM p.id FROM projects p WHERE p.company_id=company AND p.id IN
        (SELECT f.project_id FROM reviewed_memory_facts f JOIN run_reviewed_memory_uses u
            ON u.company_id=f.company_id AND u.fact_id=f.id WHERE u.company_id=company AND u.run_id=run)
        ORDER BY p.id FOR SHARE OF p NOWAIT;
    PERFORM w.id FROM work_items w JOIN work_executions x ON x.company_id=w.company_id AND x.work_item_id=w.id
        WHERE x.company_id=company AND x.run_id=run ORDER BY w.id FOR SHARE OF w NOWAIT;
    PERFORM a.channel_id FROM conversation_memory_authorities a WHERE a.company_id=company
        AND EXISTS(SELECT 1 FROM run_reviewed_memory_uses u JOIN reviewed_memory_conversation_audiences f
            ON f.company_id=u.company_id AND f.fact_id=u.fact_id WHERE u.company_id=company AND u.run_id=run
                AND f.project_id=a.project_id AND f.channel_id=a.channel_id)
        ORDER BY a.company_id,a.project_id,a.channel_id FOR SHARE OF a NOWAIT;
    PERFORM a.channel_id FROM employee_memory_channel_authorities a WHERE a.company_id=company
        AND EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u
            JOIN employee_reviewed_memory_facts f ON f.company_id=u.company_id AND f.id=u.fact_id
            CROSS JOIN LATERAL ortak_employee_memory_run_origin(company,run,f.destination_channel_id) origin
            WHERE u.company_id=company AND u.run_id=run AND f.employee_id=a.employee_id
                AND f.community_id=a.community_id AND (a.channel_id IN(f.source_channel_id,f.destination_channel_id)
                    OR a.channel_id=(convert_from(origin.origin_bytes,'UTF8')::jsonb#>>'{source,channel_id}')::uuid))
        ORDER BY a.employee_id,a.channel_id FOR SHARE OF a NOWAIT;
    PERFORM f.id FROM reviewed_memory_facts f JOIN run_reviewed_memory_uses u ON u.company_id=f.company_id AND u.fact_id=f.id
        WHERE u.company_id=company AND u.run_id=run ORDER BY f.id FOR SHARE OF f NOWAIT;
    PERFORM t.id FROM reviewed_memory_targets t WHERE t.company_id=company AND EXISTS
        (SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=company AND u.run_id=run AND u.target_id=t.id)
        ORDER BY t.id FOR SHARE OF t NOWAIT;
    PERFORM f.id FROM employee_reviewed_memory_facts f JOIN run_employee_reviewed_memory_uses u
        ON u.company_id=f.company_id AND u.fact_id=f.id WHERE u.company_id=company AND u.run_id=run
        ORDER BY f.id FOR SHARE OF f NOWAIT;
    PERFORM t.id FROM employee_reviewed_memory_targets t WHERE t.company_id=company AND EXISTS
        (SELECT 1 FROM run_employee_reviewed_memory_uses u WHERE u.company_id=company AND u.run_id=run AND u.target_id=t.id)
        ORDER BY t.id FOR SHARE OF t NOWAIT;
    RETURN ortak_run_reviewed_memory_current(company,run);
END $$;

CREATE FUNCTION ortak_reviewed_use_immutable() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'ortak: reviewed run uses are retained and immutable' USING ERRCODE='check_violation'; END $$;
CREATE TRIGGER ortak_reviewed_use_immutable BEFORE UPDATE OR DELETE ON run_reviewed_memory_uses
    FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_use_immutable();
CREATE TRIGGER ortak_reviewed_use_no_truncate BEFORE TRUNCATE ON run_reviewed_memory_uses
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reviewed_use_immutable();

CREATE FUNCTION ortak_snapshot_scratch_jsonb(value JSON) RETURNS JSONB
    LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $function$
    SELECT regexp_replace(
        regexp_replace(value::text,
            $pattern$(?<!\\)((?:\\\\)*)\\u0001$pattern$,
            $replacement$\1\\u0001\\u0001$replacement$,'g'),
        $pattern$(?<!\\)((?:\\\\)*)\\u0000$pattern$,
        $replacement$\1\\u0001\\u0002$replacement$,'g')::jsonb
$function$;

CREATE FUNCTION ortak_reviewed_snapshot_consistent() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; run UUID; wire JSONB; used_count INTEGER; record JSONB; pin JSONB; i INTEGER=0; scratch_count INTEGER; total_bytes INTEGER=0; rendered JSONB; u run_reviewed_memory_uses; f reviewed_memory_facts;
BEGIN
    company=NEW.company_id; run=NEW.run_id;
    -- Even PostgreSQL json field access may unescape unrelated NUL values.
    -- Encode the whole comparison document before performing any field access.
    SELECT ortak_snapshot_scratch_jsonb(convert_from(s.spec_bytes,'UTF8')::json) INTO wire FROM run_context_snapshots s WHERE s.company_id=company AND s.run_id=run;
    SELECT count(*) INTO used_count FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run;
    IF wire IS NULL THEN RAISE EXCEPTION 'ortak: reviewed snapshot missing' USING ERRCODE='check_violation'; END IF;
    IF wire->'version'='5'::jsonb THEN
        PERFORM ortak_employee_snapshot_v5(company,run,wire);
        RETURN NEW;
    END IF;
    IF wire ? 'employee' OR EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses employee_use
        WHERE employee_use.company_id=company AND employee_use.run_id=run) THEN
        RAISE EXCEPTION 'legacy snapshot cannot carry employee context' USING ERRCODE='check_violation';
    END IF;
    IF wire->'version'='4'::jsonb THEN
        PERFORM ortak_conversation_snapshot76(company,run,wire);
        RETURN NEW;
    END IF;
    IF wire ? 'conversation' THEN
        RAISE EXCEPTION 'ortak: legacy snapshot cannot carry conversation context' USING ERRCODE='check_violation';
    END IF;
    IF wire->'version' IS DISTINCT FROM '3'::jsonb THEN
        IF used_count<>0 OR wire ? 'reviewed' THEN RAISE EXCEPTION 'ortak: legacy snapshot cannot contain reviewed context' USING ERRCODE='check_violation'; END IF;
        RETURN NEW;
    END IF;
    IF jsonb_typeof(wire#>'{reviewed,records}') IS DISTINCT FROM 'array'
        OR jsonb_array_length(wire#>'{reviewed,records}')<>used_count OR used_count>8
        OR NOT EXISTS(SELECT 1 FROM work_executions wx JOIN runs r ON r.company_id=wx.company_id AND r.id=wx.run_id
            WHERE wx.company_id=company AND wx.run_id=run AND wire#>>'{work_origin,project_id}'=wx.project_id::text
              AND wire#>>'{spec,employee_id}'=r.employee_id) THEN
        RAISE EXCEPTION 'ortak: reviewed snapshot scope or count differs' USING ERRCODE='check_violation';
    END IF;
    IF jsonb_typeof(wire#>'{recall,records}') IS DISTINCT FROM 'array'
        OR jsonb_typeof(wire#>'{spec,context,memory_context}') IS DISTINCT FROM 'array' THEN
        RAISE EXCEPTION 'ortak: reviewed snapshot context arrays missing' USING ERRCODE='check_violation';
    END IF;
    scratch_count=jsonb_array_length(wire#>'{recall,records}');
    IF scratch_count+used_count>8 OR jsonb_array_length(wire#>'{spec,context,memory_context}')<>scratch_count+used_count THEN
        RAISE EXCEPTION 'ortak: reviewed snapshot total record budget differs' USING ERRCODE='check_violation';
    END IF;
    -- Outer records are already encoded once. Serialized memory_context strings
    -- still contain original inner JSON escapes and need their own one encoding.
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{recall,records}') LOOP
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',i::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type','run_scratch_memory','trust','untrusted_data','record',record) THEN
            RAISE EXCEPTION 'ortak: scratch rendered context differs' USING ERRCODE='check_violation';
        END IF;
        -- Each encoded SOH pair represents exactly one original UTF-8 byte.
        -- Count bytes from the original content, not the comparison encoding.
        total_bytes=total_bytes+octet_length(record->>'content')
            -(octet_length(record->>'content')-octet_length(regexp_replace(record->>'content',E'\x01[\x01\x02]','','g')))/2;
        i=i+1;
    END LOOP;
    i=0;
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{reviewed,records}') LOOP
        pin=record->'pin';
        SELECT * INTO u FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run AND x.ordinal=i;
        SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=company AND x.id=u.fact_id;
        IF u.run_id IS NULL OR f.id IS NULL OR record->'content' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(to_json(f.content))
            OR NOT EXISTS(SELECT 1 FROM reviewed_memory_targets t WHERE t.company_id=company AND t.id=u.target_id AND ortak_snapshot_scratch_jsonb(t.binding::json)=wire->'memory_binding')
            OR pin IS DISTINCT FROM ortak_snapshot_scratch_jsonb(jsonb_build_object('fact_id',u.fact_id,'target_id',u.target_id,
                'fact_version',u.fact_version,'consumption_epoch',u.consumption_epoch,'content_hash',encode(u.content_hash,'hex'),
                'source_hash',encode(u.source_hash,'hex'),'binding_hash',encode(u.binding_hash,'hex'),
                'approval_id',u.approval_id,'approved_by',u.approved_by,'expires_at',pin->>'expires_at')::json)
            OR (pin->>'expires_at')::timestamptz IS DISTINCT FROM u.expires_at THEN
            RAISE EXCEPTION 'ortak: reviewed snapshot bytes differ from retained uses' USING ERRCODE='check_violation';
        END IF;
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',(scratch_count+i)::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type','reviewed_project_memory','trust','untrusted_data','record',record) THEN
            RAISE EXCEPTION 'ortak: reviewed rendered context differs' USING ERRCODE='check_violation';
        END IF;
        total_bytes=total_bytes+octet_length(f.content);
        i=i+1;
    END LOOP;
    IF total_bytes>16384 OR NOT ortak_run_reviewed_memory_current(company,run) THEN
        RAISE EXCEPTION 'ortak: reviewed context authority expired before commit' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER ortak_reviewed_snapshot_consistent AFTER INSERT ON run_context_snapshots
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_snapshot_consistent();
CREATE CONSTRAINT TRIGGER ortak_reviewed_use_consistent AFTER INSERT ON run_reviewed_memory_uses
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_snapshot_consistent();

CREATE FUNCTION ortak_reviewed_run_admission() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE selected_run UUID; conversation BOOLEAN;
BEGIN
    IF TG_TABLE_NAME='runs' THEN selected_run=NEW.id; ELSE selected_run=NEW.run_id; END IF;
    SELECT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
        AND u.run_id=selected_run AND u.conversation_audience_hash IS NOT NULL) OR EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u
        WHERE u.company_id=NEW.company_id AND u.run_id=selected_run) INTO conversation;
    IF TG_TABLE_NAME='runs' THEN
        IF NOT conversation THEN
            -- Preserve the reviewed-project admission trigger's legacy effect.
            IF NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token THEN RETURN NEW; END IF;
        ELSE
            IF (NEW.office_admission_token,NEW.office_admission_generation,NEW.office_admission_valid_before,
                NEW.work_admission_token,NEW.work_admission_generation,NEW.runtime_run_ref)
              IS NOT DISTINCT FROM
               (OLD.office_admission_token,OLD.office_admission_generation,OLD.office_admission_valid_before,
                OLD.work_admission_token,OLD.work_admission_generation,OLD.runtime_run_ref) THEN RETURN NEW; END IF;
            -- Exact74 lost-start ACK correlation is accounting after confirmed
            -- stop; no new token, output, bytes or active status can ride along.
            IF OLD.runtime_run_ref IS NULL AND NEW.runtime_run_ref IS NOT NULL
                AND (to_jsonb(NEW)-'runtime_run_ref'-'updated_at') IS NOT DISTINCT FROM (to_jsonb(OLD)-'runtime_run_ref'-'updated_at')
                AND EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.id
                    AND (c.state='acknowledged' OR c.state='pending' AND c.lease_token IS NOT NULL AND c.lease_expires_at>clock_timestamp()))
                AND NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader
                    WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.id AND reader.state<>'stopped') THEN RETURN NEW; END IF;
            IF NEW.status NOT IN('queued','running','waiting') THEN
                RAISE EXCEPTION 'ortak: terminal conversation run cannot gain fresh admission' USING ERRCODE='check_violation';
            END IF;
        END IF;
    END IF;
    IF NOT ortak_run_reviewed_memory_current(NEW.company_id,selected_run) THEN
        RAISE EXCEPTION 'ortak: reviewed memory use no longer permitted' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER ortak_reviewed_run_admission AFTER UPDATE ON runs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_run_admission();
CREATE CONSTRAINT TRIGGER ortak_reviewed_artifact_admission AFTER INSERT ON artifacts
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_run_admission();


-- One company notification wakes authorized streams; no per-run fan-out or
-- provider work occurs in withdrawal/target transactions.
CREATE TRIGGER trg_activity_reviewed_fact_use AFTER UPDATE OF version ON reviewed_memory_facts
    FOR EACH ROW WHEN(NEW.version IS DISTINCT FROM OLD.version) EXECUTE FUNCTION ortak_activity_notify('');
CREATE TRIGGER trg_activity_reviewed_target_use AFTER UPDATE ON reviewed_memory_targets
    FOR EACH ROW WHEN(NEW.consumption_epoch IS DISTINCT FROM OLD.consumption_epoch) EXECUTE FUNCTION ortak_activity_notify('');

-- Explicit immutable45 binding fence after its shared function definition.
SELECT attach_community_write_fence('office_company_bindings');

-- Migration74: selected workspace text tools and retained containment journal.

CREATE FUNCTION ortak_workspace_canonical(value JSONB) RETURNS TEXT LANGUAGE sql IMMUTABLE STRICT AS $$
    SELECT CASE jsonb_typeof(value)
        WHEN 'object' THEN '{'||coalesce((SELECT string_agg(to_json(key)::text||':'||ortak_workspace_canonical(val),',' ORDER BY key COLLATE "C") FROM jsonb_each(value) AS entries(key,val)),'')||'}'
        WHEN 'array' THEN '['||coalesce((SELECT string_agg(ortak_workspace_canonical(val),',' ORDER BY ordinal) FROM jsonb_array_elements(value) WITH ORDINALITY AS entries(val,ordinal)),'')||']'
        ELSE value::text END
$$;

CREATE TABLE workspace_bindings (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    project_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    id UUID NOT NULL,
    workspace_ref TEXT NOT NULL CHECK(workspace_ref ~ '^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$'),
    grant_bytes BYTEA NOT NULL CHECK(octet_length(grant_bytes) BETWEEN 1 AND 16384),
    manifest_hash BYTEA NOT NULL CHECK(octet_length(manifest_hash)=32),
    verification_id UUID NOT NULL,
    verified_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,verification_id),
    FOREIGN KEY(company_id,project_id) REFERENCES projects(company_id,id),
    FOREIGN KEY(company_id,employee_id) REFERENCES employees(company_id,id),
    CHECK(expires_at>verified_at AND verified_at<=created_at)
);
CREATE INDEX idx_workspace_bindings_selection ON workspace_bindings(company_id,project_id,employee_id,workspace_ref,id);

CREATE TABLE workspace_files (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    workspace_id UUID NOT NULL,
    id UUID NOT NULL,
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 7),
    logical_name TEXT NOT NULL CHECK(octet_length(logical_name) BETWEEN 1 AND 256
        AND logical_name ~ '^[A-Za-z0-9][A-Za-z0-9._/-]*$'
        AND logical_name !~ '(^|/)(\.|\.\.|)(/|$)'),
    media_type TEXT NOT NULL CHECK(media_type='text/plain'),
    byte_count INTEGER NOT NULL CHECK(byte_count BETWEEN 0 AND 16384),
    content_hash BYTEA NOT NULL CHECK(octet_length(content_hash)=32),
    PRIMARY KEY(company_id,workspace_id,id),
    UNIQUE(company_id,workspace_id,ordinal),
    UNIQUE(company_id,workspace_id,logical_name),
    FOREIGN KEY(company_id,workspace_id) REFERENCES workspace_bindings(company_id,id)
);

CREATE TABLE run_workspace_uses (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    workspace_id UUID NOT NULL,
    manifest_hash BYTEA NOT NULL CHECK(octet_length(manifest_hash)=32),
    store_ref TEXT NOT NULL CHECK(octet_length(store_ref)<=128),
    employee_revision_id UUID NOT NULL,
    employee_lifecycle_epoch BIGINT NOT NULL CHECK(employee_lifecycle_epoch>=0),
    outbox_id UUID NOT NULL,
    admission_lease UUID NOT NULL,
    prepared_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id),
    FOREIGN KEY(company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY(company_id,workspace_id) REFERENCES workspace_bindings(company_id,id),
    FOREIGN KEY(company_id,outbox_id) REFERENCES outbox(company_id,id),
    CHECK(store_ref='workspace-run:'||company_id::text||':'||run_id::text)
);
CREATE INDEX idx_workspace_uses_binding ON run_workspace_uses(company_id,workspace_id,run_id);

CREATE TABLE workspace_tool_actions (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    call_id TEXT NOT NULL CHECK(call_id ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$'),
    file_id UUID NOT NULL,
    arguments_hash BYTEA NOT NULL CHECK(octet_length(arguments_hash)=32),
    ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 1 AND 4),
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN('pending','result_ready','delivered','interrupted')),
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count BETWEEN 0 AND 3),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id,call_id),
    UNIQUE(company_id,run_id,ordinal),
    FOREIGN KEY(company_id,run_id) REFERENCES run_workspace_uses(company_id,run_id),
    CHECK((lease_token IS NULL)=(lease_expires_at IS NULL)),
    CHECK(arguments_hash=sha256(convert_to('{"file_id":"'||file_id::text||'"}','UTF8')))
);
CREATE INDEX idx_workspace_actions_due ON workspace_tool_actions(company_id,next_attempt_at,run_id,ordinal)
    WHERE state IN('pending','result_ready');
CREATE UNIQUE INDEX idx_workspace_actions_one_pending ON workspace_tool_actions(company_id,run_id)
    WHERE state='pending';

CREATE TABLE workspace_tool_receipts (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    call_id TEXT NOT NULL,
    arguments_hash BYTEA NOT NULL CHECK(octet_length(arguments_hash)=32),
    lease_token UUID NOT NULL,
    attempt_count INTEGER NOT NULL CHECK(attempt_count BETWEEN 1 AND 3),
    result_bytes BYTEA NOT NULL CHECK(octet_length(result_bytes) BETWEEN 1 AND 131072),
    result_hash BYTEA NOT NULL CHECK(octet_length(result_hash)=32 AND result_hash=sha256(result_bytes)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id,call_id),
    FOREIGN KEY(company_id,run_id,call_id) REFERENCES workspace_tool_actions(company_id,run_id,call_id)
);

SELECT attach_community_write_fence('workspace_bindings');
SELECT attach_community_write_fence('workspace_files');
SELECT attach_community_write_fence('run_workspace_uses');
SELECT attach_community_write_fence('workspace_tool_actions');
SELECT attach_community_write_fence('workspace_tool_receipts');

CREATE FUNCTION ortak_workspace_binding_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (to_jsonb(NEW)-'revoked_at') IS DISTINCT FROM (to_jsonb(OLD)-'revoked_at')
            OR OLD.revoked_at IS NOT NULL OR NEW.revoked_at IS NULL THEN
            RAISE EXCEPTION 'ortak: workspace revision is immutable except one withdrawal' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.revoked_at IS NOT NULL OR NEW.verified_at>clock_timestamp()
        OR NEW.verified_at<clock_timestamp()-INTERVAL '30 seconds'
        OR NEW.expires_at<=clock_timestamp() OR NEW.expires_at>clock_timestamp()+INTERVAL '30 days' THEN
        RAISE EXCEPTION 'ortak: workspace verification or retention is invalid' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workspace_binding_guard BEFORE INSERT OR UPDATE ON workspace_bindings
    FOR EACH ROW EXECUTE FUNCTION ortak_workspace_binding_guard();
CREATE TRIGGER workspace_binding_authority BEFORE INSERT OR UPDATE OR DELETE ON workspace_bindings
    FOR EACH ROW EXECUTE FUNCTION ortak_fence_office_mutation('company','company_id','revoked_at');

CREATE FUNCTION ortak_workspace_manifest_consistent() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE b workspace_bindings; wire JSONB; files JSONB; file_count INTEGER; total INTEGER;
BEGIN
    IF TG_TABLE_NAME='workspace_bindings' THEN b=NEW;
    ELSE SELECT * INTO b FROM workspace_bindings WHERE company_id=NEW.company_id AND id=NEW.workspace_id; END IF;
    wire=convert_from(b.grant_bytes,'UTF8')::jsonb;
    SELECT count(*),coalesce(sum(byte_count),0),jsonb_agg(jsonb_build_object('file_id',id,'name',logical_name,
        'media_type',media_type,'bytes',byte_count,'sha256',encode(content_hash,'hex')) ORDER BY id)
        INTO file_count,total,files FROM workspace_files WHERE company_id=b.company_id AND workspace_id=b.id AND community_id=b.community_id;
    IF file_count NOT BETWEEN 1 AND 8 OR total>65536
        OR EXISTS(SELECT 1 FROM workspace_files f WHERE f.company_id=b.company_id AND f.workspace_id=b.id AND
            (f.community_id<>b.community_id OR f.ordinal<>(SELECT count(*) FROM workspace_files p
                WHERE p.company_id=f.company_id AND p.workspace_id=f.workspace_id AND p.id<f.id)))
        OR wire IS DISTINCT FROM jsonb_build_object('format','ortak-workspace-read/v1','company_id',b.company_id,
            'project_id',b.project_id,'employee_id',b.employee_id,'workspace_ref',b.workspace_ref,'revision',b.id,
            'manifest_hash',encode(b.manifest_hash,'hex'),'files',files)
        OR b.grant_bytes<>convert_to(ortak_workspace_canonical(wire),'UTF8')
        OR b.manifest_hash<>sha256(convert_to(ortak_workspace_canonical(wire-'manifest_hash'),'UTF8')) THEN
        RAISE EXCEPTION 'ortak: workspace selected manifest differs' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_manifest_consistent AFTER INSERT ON workspace_bindings
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_manifest_consistent();
CREATE CONSTRAINT TRIGGER workspace_files_consistent AFTER INSERT ON workspace_files
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_manifest_consistent();

-- Availability for a Files profile is independent of model identity. The
-- selected project binding must still be current at final activation commit.
CREATE FUNCTION ortak_workspace_profile_available(company UUID, employee TEXT, workspace TEXT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT EXISTS(SELECT 1 FROM workspace_bindings b
        JOIN projects p ON p.company_id=b.company_id AND p.id=b.project_id
        JOIN project_api_bindings pb ON pb.company_id=b.company_id AND pb.project_id=b.project_id AND pb.community_id=b.community_id
        JOIN office_company_bindings ob ON ob.company_id=b.company_id AND ob.community_id=b.community_id
        JOIN companies c ON c.id=b.company_id JOIN communities cm ON cm.id=b.community_id
        WHERE b.company_id=company AND b.employee_id=employee AND b.workspace_ref=workspace
          AND b.revoked_at IS NULL AND b.expires_at>clock_timestamp() AND p.status='active' AND c.status='active'
          AND cm.deletion_state='active' AND cm.deleted_at IS NULL)
$$;

CREATE FUNCTION ortak_workspace_activation_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE manifest JSONB;
BEGIN
    IF NEW.status<>'active' OR (TG_OP='UPDATE' AND NEW.active_revision_id IS NOT DISTINCT FROM OLD.active_revision_id
        AND NEW.status IS NOT DISTINCT FROM OLD.status) THEN RETURN NEW; END IF;
    SELECT r.manifest INTO manifest FROM employee_revisions r WHERE r.company_id=NEW.company_id AND r.employee_id=NEW.id AND r.id=NEW.active_revision_id;
    IF manifest#>>'{runtime,adapter}'='hermes' AND manifest#>'{permissions,allowed_tools}'='["files"]'::jsonb
        AND NOT ortak_workspace_profile_available(NEW.company_id,NEW.id,manifest#>>'{runtime,workspace_ref}') THEN
        RAISE EXCEPTION 'ortak: Files profile requires a current selected workspace at activation' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_activation_at_commit AFTER INSERT OR UPDATE ON employees
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_activation_at_commit();

-- pgschema 1.7.4 can schedule this SQL body before employee_runtime_bindings.
-- Keep admission closed until the mandatory reconciler installs the exact body
-- from immutable74 after every referenced table exists.
CREATE FUNCTION ortak_run_workspace_current(company UUID, run UUID, require_use BOOLEAN DEFAULT true)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT false
$$;

CREATE FUNCTION ortak_lock_run_workspace(company UUID, run UUID, require_use BOOLEAN DEFAULT true)
RETURNS BOOLEAN LANGUAGE plpgsql AS $$
BEGIN
    PERFORM b.id FROM workspace_bindings b JOIN run_workspace_uses u ON u.company_id=b.company_id AND u.workspace_id=b.id
        WHERE u.company_id=company AND u.run_id=run ORDER BY b.id FOR SHARE OF b;
    RETURN coalesce(ortak_run_workspace_current(company,run,require_use),false);
END $$;

CREATE FUNCTION ortak_workspace_use_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT coalesce(ortak_run_workspace_current(NEW.company_id,NEW.run_id),false)
        OR NOT EXISTS(SELECT 1 FROM outbox o JOIN runs r ON r.company_id=o.company_id AND r.id=o.run_id
            WHERE o.company_id=NEW.company_id AND o.id=NEW.outbox_id AND o.run_id=NEW.run_id
              AND o.kind='work_run_dispatch' AND o.state='pending' AND o.lease_token=NEW.admission_lease
              AND o.lease_expires_at>clock_timestamp() AND r.status='queued' AND r.runtime_run_ref IS NULL)
        OR NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.run_id
            AND reader.workspace_id=NEW.workspace_id AND reader.request_key='prepare' AND reader.owner_lease=NEW.admission_lease AND reader.state='stopped'
            AND reader.stop_proof IN('reaped','in_process_returned'))
        OR EXISTS(SELECT 1 FROM run_cancel_requests WHERE company_id=NEW.company_id AND run_id=NEW.run_id)
        OR EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id) THEN
        RAISE EXCEPTION 'ortak: workspace use lacks current dispatch authority' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_use_at_commit AFTER INSERT ON run_workspace_uses
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_use_at_commit();

CREATE FUNCTION ortak_workspace_action_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.state<>'pending' OR NEW.attempt_count<>0 OR NEW.lease_token IS NOT NULL THEN
            RAISE EXCEPTION 'ortak: invalid initial workspace action' USING ERRCODE='check_violation';
        END IF;
    ELSE
        IF (to_jsonb(NEW)-'state'-'lease_token'-'lease_expires_at'-'attempt_count'-'next_attempt_at'-'updated_at')
            IS DISTINCT FROM (to_jsonb(OLD)-'state'-'lease_token'-'lease_expires_at'-'attempt_count'-'next_attempt_at'-'updated_at')
            OR OLD.state IN('delivered','interrupted') OR NEW.attempt_count<OLD.attempt_count
            OR NEW.attempt_count>OLD.attempt_count+1 OR NEW.updated_at<OLD.updated_at
            OR (NEW.state='pending' AND OLD.state<>'pending') THEN
            RAISE EXCEPTION 'ortak: invalid workspace action transition' USING ERRCODE='check_violation';
        END IF;
        IF NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL THEN
            IF OLD.lease_expires_at>clock_timestamp() OR NEW.attempt_count<>OLD.attempt_count+1
                OR NEW.lease_expires_at<=clock_timestamp() OR NEW.lease_expires_at>clock_timestamp()+INTERVAL '30 seconds' THEN
                RAISE EXCEPTION 'ortak: workspace action lease is not claimable' USING ERRCODE='check_violation';
            END IF;
        ELSIF NEW.attempt_count<>OLD.attempt_count OR NEW.lease_expires_at IS DISTINCT FROM OLD.lease_expires_at THEN
            IF NOT (NEW.lease_token IS NULL AND NEW.lease_expires_at IS NULL AND NEW.attempt_count=OLD.attempt_count) THEN
                RAISE EXCEPTION 'ortak: workspace action attempt is not a fresh claim' USING ERRCODE='check_violation';
            END IF;
        END IF;
        IF NEW.state IN('result_ready','delivered') AND NOT EXISTS(SELECT 1 FROM workspace_tool_receipts x
            WHERE x.company_id=NEW.company_id AND x.run_id=NEW.run_id AND x.call_id=NEW.call_id) THEN
            RAISE EXCEPTION 'ortak: workspace action needs its exact result receipt' USING ERRCODE='check_violation';
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workspace_action_guard BEFORE INSERT OR UPDATE ON workspace_tool_actions
    FOR EACH ROW EXECUTE FUNCTION ortak_workspace_action_guard();

CREATE FUNCTION ortak_workspace_action_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' AND NEW.state='interrupted' THEN RETURN NEW; END IF;
    IF NOT EXISTS(SELECT 1 FROM run_workspace_uses u JOIN workspace_files f ON f.company_id=u.company_id AND f.workspace_id=u.workspace_id
        WHERE u.company_id=NEW.company_id AND u.run_id=NEW.run_id AND f.id=NEW.file_id AND u.community_id=NEW.community_id)
        OR (NEW.state='pending' AND NOT coalesce(ortak_run_workspace_current(NEW.company_id,NEW.run_id),false)) THEN
        RAISE EXCEPTION 'ortak: workspace action input is not currently selected' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_action_at_commit AFTER INSERT OR UPDATE ON workspace_tool_actions
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_action_at_commit();

CREATE FUNCTION ortak_workspace_receipt_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE a workspace_tool_actions; f workspace_files; wire JSONB;
BEGIN
    SELECT * INTO a FROM workspace_tool_actions WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND call_id=NEW.call_id;
    SELECT file.* INTO f FROM workspace_files file JOIN run_workspace_uses u ON u.company_id=file.company_id AND u.workspace_id=file.workspace_id
        WHERE u.company_id=NEW.company_id AND u.run_id=NEW.run_id AND file.id=a.file_id;
    wire=convert_from(NEW.result_bytes,'UTF8')::jsonb;
    IF a.call_id IS NULL OR f.id IS NULL OR a.community_id<>NEW.community_id OR a.arguments_hash<>NEW.arguments_hash
        OR a.state<>'result_ready' OR a.lease_token IS DISTINCT FROM NEW.lease_token OR a.attempt_count<>NEW.attempt_count
        OR a.lease_expires_at<=clock_timestamp() OR NOT coalesce(ortak_run_workspace_current(NEW.company_id,NEW.run_id),false)
        OR NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.run_id
            AND reader.request_key='read:'||NEW.call_id AND reader.owner_lease=NEW.lease_token AND reader.state='stopped'
            AND reader.stop_proof IN('reaped','in_process_returned'))
        OR NOT EXISTS(SELECT 1 FROM runs r WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.status IN('running','waiting'))
        OR EXISTS(SELECT 1 FROM run_cancel_requests WHERE company_id=NEW.company_id AND run_id=NEW.run_id)
        OR EXISTS(SELECT 1 FROM runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id) THEN
        RAISE EXCEPTION 'ortak: workspace result has no exact live authority/lease' USING ERRCODE='check_violation';
    END IF;
    IF wire->>'status'='completed' THEN
        IF wire IS DISTINCT FROM jsonb_build_object('status','completed','content',wire->>'content','sha256',encode(f.content_hash,'hex'),
            'bytes',f.byte_count,'name',f.logical_name) OR octet_length(wire->>'content') IS DISTINCT FROM f.byte_count
            OR sha256(convert_to(wire->>'content','UTF8')) IS DISTINCT FROM f.content_hash THEN
            RAISE EXCEPTION 'ortak: workspace result bytes differ from selected input' USING ERRCODE='check_violation';
        END IF;
    ELSIF wire IS DISTINCT FROM jsonb_build_object('status','failed','code',wire->>'code')
        OR wire->>'code' IS NULL OR wire->>'code' NOT IN('authority_changed','workspace_unavailable','file_unavailable','input_changed','deadline_exceeded','cancelled') THEN
        RAISE EXCEPTION 'ortak: invalid workspace failure result' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_receipt_at_commit AFTER INSERT ON workspace_tool_receipts
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_receipt_at_commit();

CREATE FUNCTION ortak_workspace_run_admission() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE run UUID; required BOOLEAN=true;
BEGIN
    IF TG_TABLE_NAME='runs' THEN
        IF NEW.work_admission_token IS NOT DISTINCT FROM OLD.work_admission_token
            AND NEW.runtime_run_ref IS NOT DISTINCT FROM OLD.runtime_run_ref THEN RETURN NEW; END IF;
        -- A confirmed stop can discover the reference of an accepted start
        -- whose response was lost. Restore only that metadata under the live
        -- cancellation lease (or its ACK), never renew execution authority.
        IF OLD.runtime_run_ref IS NULL AND NEW.runtime_run_ref IS NOT NULL
            AND (to_jsonb(NEW)-'runtime_run_ref'-'updated_at') IS NOT DISTINCT FROM (to_jsonb(OLD)-'runtime_run_ref'-'updated_at')
            AND EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.id
                AND (c.state='acknowledged' OR (c.state='pending' AND c.lease_token IS NOT NULL AND c.lease_expires_at>clock_timestamp())))
            AND NOT EXISTS(SELECT 1 FROM workspace_reader_executions reader WHERE reader.company_id=NEW.company_id AND reader.run_id=NEW.id AND reader.state<>'stopped') THEN
            RETURN NEW;
        END IF;
        run=NEW.id; required=NEW.runtime_run_ref IS NOT NULL;
    ELSE run=NEW.run_id;
    END IF;
    IF run IS NULL THEN RETURN NEW; END IF;
    IF NOT coalesce(ortak_run_workspace_current(NEW.company_id,run,required),false) THEN
        RAISE EXCEPTION 'ortak: selected workspace permission changed' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_run_admission AFTER UPDATE ON runs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_run_admission();
CREATE CONSTRAINT TRIGGER workspace_artifact_admission AFTER INSERT ON artifacts
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_run_admission();

CREATE TRIGGER workspace_no_delete BEFORE DELETE ON workspace_bindings FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER workspace_no_truncate BEFORE TRUNCATE ON workspace_bindings FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER workspace_no_delete BEFORE DELETE ON workspace_files FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER workspace_no_truncate BEFORE TRUNCATE ON workspace_files FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER workspace_no_delete BEFORE DELETE ON run_workspace_uses FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER workspace_no_truncate BEFORE TRUNCATE ON run_workspace_uses FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER workspace_no_delete BEFORE DELETE ON workspace_tool_actions FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER workspace_no_truncate BEFORE TRUNCATE ON workspace_tool_actions FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER workspace_no_delete BEFORE DELETE ON workspace_tool_receipts FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER workspace_no_truncate BEFORE TRUNCATE ON workspace_tool_receipts FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER workspace_immutable BEFORE UPDATE ON workspace_files FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER workspace_immutable BEFORE UPDATE ON run_workspace_uses FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER workspace_immutable BEFORE UPDATE ON workspace_tool_receipts FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

-- Preparation precedes the immutable run use; this separate retained journal
-- accounts for both preparation and tool reads without weakening use immutability.
CREATE TABLE workspace_reader_executions (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    id UUID NOT NULL,
    workspace_id UUID NOT NULL,
    request_key TEXT NOT NULL CHECK(octet_length(request_key) BETWEEN 1 AND 160),
    owner_lease UUID NOT NULL,
    owner_deadline TIMESTAMPTZ NOT NULL,
    executable TEXT,
    executable_hash BYTEA,
    operating_uid BIGINT,
    pid BIGINT CHECK(pid BETWEEN 1 AND 4294967295),
    state TEXT NOT NULL DEFAULT 'planned' CHECK(state IN('planned','running','stopped')),
    stop_proof TEXT CHECK(stop_proof IN('reaped','in_process_returned','confirmed_absence')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    stopped_at TIMESTAMPTZ,
    PRIMARY KEY(company_id,id),
    FOREIGN KEY(company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY(company_id,workspace_id) REFERENCES workspace_bindings(company_id,id),
    CHECK((executable IS NULL)=(executable_hash IS NULL) AND (executable IS NULL)=(operating_uid IS NULL)),
    CHECK(executable IS NULL OR (octet_length(executable) BETWEEN 1 AND 4096 AND left(executable,1)='/' AND octet_length(executable_hash)=32 AND operating_uid BETWEEN 0 AND 4294967295)),
    CHECK((state='stopped')=(stopped_at IS NOT NULL) AND (state='stopped')=(stop_proof IS NOT NULL)),
    CHECK(stop_proof IS NULL OR (stop_proof='in_process_returned')=(executable IS NULL))
);
CREATE UNIQUE INDEX idx_workspace_reader_one_unresolved ON workspace_reader_executions(company_id,run_id) WHERE state<>'stopped';
CREATE UNIQUE INDEX idx_workspace_reader_attempt ON workspace_reader_executions(company_id,run_id,request_key,owner_lease);
CREATE INDEX idx_workspace_reader_recovery ON workspace_reader_executions(company_id,owner_deadline,id) WHERE state<>'stopped';
SELECT attach_community_write_fence('workspace_reader_executions');
CREATE TRIGGER workspace_reader_no_delete BEFORE DELETE ON workspace_reader_executions FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER workspace_reader_no_truncate BEFORE TRUNCATE ON workspace_reader_executions FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_workspace_reader_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM id FROM runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    IF TG_OP='INSERT' THEN
        IF NEW.state<>'planned' OR NEW.pid IS NOT NULL OR NEW.owner_deadline<=clock_timestamp()
            OR EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)
            OR EXISTS(SELECT 1 FROM run_cancel_requests c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)
            OR NOT EXISTS(SELECT 1 FROM workspace_bindings b WHERE b.company_id=NEW.company_id AND b.id=NEW.workspace_id AND b.community_id=NEW.community_id)
            OR NOT (EXISTS(SELECT 1 FROM outbox o WHERE o.company_id=NEW.company_id AND o.run_id=NEW.run_id AND o.kind='work_run_dispatch'
                AND o.state='pending' AND o.lease_token=NEW.owner_lease AND o.lease_expires_at=NEW.owner_deadline AND o.lease_expires_at>clock_timestamp() AND NEW.request_key='prepare')
                OR EXISTS(SELECT 1 FROM workspace_tool_actions a WHERE a.company_id=NEW.company_id AND a.run_id=NEW.run_id
                    AND NEW.request_key='read:'||a.call_id AND a.state='pending' AND a.lease_token=NEW.owner_lease
                    AND a.lease_expires_at=NEW.owner_deadline AND a.lease_expires_at>clock_timestamp())) THEN
            RAISE EXCEPTION 'ortak: reader execution needs its exact live owner lease' USING ERRCODE='check_violation';
        END IF;
    ELSE
        IF (to_jsonb(NEW)-'pid'-'state'-'stop_proof'-'stopped_at') IS DISTINCT FROM (to_jsonb(OLD)-'pid'-'state'-'stop_proof'-'stopped_at')
            OR OLD.state='stopped' OR NEW.state='planned' OR (OLD.pid IS NOT NULL AND NEW.pid IS DISTINCT FROM OLD.pid)
            OR (NEW.state='running' AND (OLD.state<>'planned' OR NEW.owner_deadline<=clock_timestamp()
                OR (NEW.executable IS NOT NULL AND NEW.pid IS NULL)
                OR EXISTS(SELECT 1 FROM runtime_cancellations c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)
                OR EXISTS(SELECT 1 FROM run_cancel_requests c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id)))
            OR (NEW.state='stopped' AND NEW.stop_proof='confirmed_absence' AND NEW.owner_deadline>clock_timestamp()) THEN
            RAISE EXCEPTION 'ortak: reader identity or stop proof changed' USING ERRCODE='check_violation';
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workspace_reader_guard BEFORE INSERT OR UPDATE ON workspace_reader_executions
    FOR EACH ROW EXECUTE FUNCTION ortak_workspace_reader_guard();

CREATE FUNCTION ortak_workspace_reader_cancel_fence() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM id FROM runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    IF NEW.state='acknowledged' AND EXISTS(SELECT 1 FROM workspace_reader_executions r
        WHERE r.company_id=NEW.company_id AND r.run_id=NEW.run_id AND r.state<>'stopped') THEN
        RAISE EXCEPTION 'ortak: unresolved workspace reader prevents cancellation acknowledgement' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER workspace_reader_cancel_fence AFTER UPDATE ON runtime_cancellations
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_workspace_reader_cancel_fence();

-- Migration75: conversation storage and current-source observation.
-- SQL scope/publication/runtime eligibility uses closed desired-only stubs;
-- the reconciler restores exact bodies before full catalog parity/admission.
-- D4 source fragment; not an applied migration. Root assembles additive 75.
-- Depends only on immutable 1-74 tables and pgcrypto. No data writes.
-- This is the SQL counterpart of postgres/conversation_memory/{query,resolve}.rs
-- and memory/conversation/wire.rs. Caller ceilings remain in the Rust facade.

-- Compact UTF-8 JSON for the deliberately small conversation wire vocabulary.
-- Object keys use bytewise lexical order; array order and strings are exact.
-- PostgreSQL JSONB::text is used only for closed scalar values, never objects
-- or arrays. to_json(text) matches serde_json string quoting, including control
-- escapes, while retaining Unicode rather than ASCII-escaping it.
-- 524288 accommodates the worst-case JSON escaping of 65536 source bytes plus
-- 16384 encoded tag bytes. Invalid/deeper/oversized values return SQL NULL.
CREATE FUNCTION ortak_conversation_json75(value JSONB, nesting INTEGER DEFAULT 0)
RETURNS TEXT LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE
SET search_path = pg_catalog, public, pg_temp AS $$
DECLARE
    member RECORD;
    encoded TEXT;
    result TEXT;
    separator TEXT := '';
BEGIN
    IF nesting < 0 OR nesting > 4 OR octet_length(value::text) > 524288 THEN
        RETURN NULL;
    END IF;
    CASE jsonb_typeof(value)
    WHEN 'object' THEN
        result := '{';
        FOR member IN SELECT e.key, e.val FROM jsonb_each(value) AS e(key,val)
                      ORDER BY e.key COLLATE "C" LOOP
            encoded := public.ortak_conversation_json75(member.val, nesting + 1);
            IF encoded IS NULL THEN RETURN NULL; END IF;
            result := result || separator || to_json(member.key)::text || ':' || encoded;
            separator := ',';
        END LOOP;
        RETURN result || '}';
    WHEN 'array' THEN
        result := '[';
        FOR member IN SELECT e.val FROM jsonb_array_elements(value)
                      WITH ORDINALITY AS e(val,ordinal) ORDER BY e.ordinal LOOP
            encoded := public.ortak_conversation_json75(member.val, nesting + 1);
            IF encoded IS NULL THEN RETURN NULL; END IF;
            result := result || separator || encoded;
            separator := ',';
        END LOOP;
        RETURN result || ']';
    WHEN 'string' THEN RETURN to_json(value #>> '{}')::text;
    WHEN 'number' THEN
        -- These wires contain only the canonical event's int32 kind, never
        -- arbitrary floating-point values whose encoders could disagree.
        IF value::text !~ '^-?(0|[1-9][0-9]*)$' THEN RETURN NULL; END IF;
        RETURN value::text;
    WHEN 'boolean' THEN RETURN value::text;
    WHEN 'null' THEN RETURN 'null';
    ELSE RETURN NULL;
    END CASE;
END
$$;

-- One/zero row current-read observation, never an approval or retained epoch.
-- STABLE plus the one table-read statement below prevents mixing snapshots
-- while walking ancestry. No routing/delivery-chain root is consulted.
-- Callers doing durable work still need their Office/project/epoch fences and
-- final deadline check; current membership alone cannot prevent old-run revival.
CREATE FUNCTION ortak_conversation_source_observation(
    company UUID, project UUID, employee TEXT, human BYTEA,
    source_id BYTEA, audience_kind TEXT
) RETURNS TABLE(
    community_id UUID,
    channel_id UUID,
    source_event_created_at TIMESTAMPTZ,
    thread_root_event_id BYTEA,
    thread_root_event_created_at TIMESTAMPTZ,
    audience_bytes BYTEA,
    audience_hash BYTEA,
    source_evidence_hash BYTEA,
    source_hash BYTEA,
    provenance_bytes BYTEA,
    observed_at TIMESTAMPTZ,
    valid_before TIMESTAMPTZ
) LANGUAGE plpgsql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path = pg_catalog, public, pg_temp AS $$
DECLARE
    node RECORD;
    first_node RECORD;
    count_nodes INTEGER := 0;
    seen BYTEA[] := ARRAY[]::bytea[];
    expected_parent BYTEA;
    expected_parent_at TIMESTAMPTZ;
    expected_depth INTEGER;
    expected_root BYTEA;
    expected_root_at TIMESTAMPTZ;
    resolved_root BYTEA;
    resolved_root_at TIMESTAMPTZ;
    tag JSONB;
    part JSONB;
    marker TEXT;
    reference_id BYTEA;
    claimed_root BYTEA;
    claimed_parent BYTEA;
    effective_depth INTEGER;
    source_stamp TEXT;
    root_stamp TEXT;
    audience_wire JSONB;
    encoded TEXT;
BEGIN
    IF company IS NULL OR project IS NULL OR employee IS NULL OR human IS NULL
       OR source_id IS NULL OR audience_kind IS NULL
       OR company = '00000000-0000-0000-0000-000000000000'::uuid
       OR project = '00000000-0000-0000-0000-000000000000'::uuid
       OR octet_length(employee) NOT BETWEEN 1 AND 64
       OR employee COLLATE "C" !~ '^[a-z0-9][a-z0-9_-]{0,63}$'
       OR octet_length(human) <> 32 OR octet_length(source_id) <> 32
       OR audience_kind NOT IN ('channel','thread') THEN
        RETURN;
    END IF;

    FOR node IN
      WITH RECURSIVE visible AS MATERIALIZED (
        SELECT office.community_id, a.channel_id, statement_timestamp() AS observed_at,
               least(ch.ttl_deadline,b.valid_until) AS valid_before
        FROM public.companies co
        JOIN public.office_company_bindings office ON office.company_id=co.id
        JOIN public.communities cm ON cm.id=office.community_id
          AND cm.deleted_at IS NULL AND cm.deletion_state='active'
        JOIN public.projects p ON p.company_id=co.id AND p.id=$2 AND p.status='active'
        JOIN public.project_api_bindings a ON a.company_id=p.company_id AND a.project_id=p.id AND a.community_id=cm.id
        JOIN public.project_access_grants g ON g.company_id=p.company_id AND g.project_id=p.id
          AND g.actor_pubkey=encode($4,'hex') AND g.revoked_at IS NULL
        JOIN public.channels ch ON ch.community_id=cm.id AND ch.id=a.channel_id
          AND ch.channel_type='stream' AND ch.deleted_at IS NULL AND ch.archived_at IS NULL
          AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>statement_timestamp())
        JOIN public.channel_members human_member ON human_member.community_id=cm.id AND human_member.channel_id=ch.id
          AND human_member.pubkey=$4 AND human_member.removed_at IS NULL AND human_member.role<>'bot'
        JOIN public.employees emp ON emp.company_id=co.id AND emp.id=$3 AND emp.status='active'
        JOIN public.employee_revisions rev ON rev.company_id=emp.company_id AND rev.employee_id=emp.id AND rev.id=emp.active_revision_id
        JOIN public.employee_office_bindings b ON b.company_id=emp.company_id AND b.employee_id=emp.id
          AND encode(b.public_key,'hex')=rev.manifest #>> '{office,public_key}'
          AND b.signer_ref=rev.manifest #>> '{office,signer_ref}'
          AND b.verified_at IS NOT NULL AND b.valid_from<=statement_timestamp()
          AND (b.valid_until IS NULL OR b.valid_until>statement_timestamp())
        JOIN public.channel_members employee_member ON employee_member.community_id=cm.id AND employee_member.channel_id=ch.id
          AND employee_member.pubkey=b.public_key AND employee_member.removed_at IS NULL
        WHERE co.id=$1 AND co.status='active'
          AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=$4
            AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
          AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings eb WHERE eb.company_id=co.id AND eb.public_key=$4)
          AND NOT EXISTS(SELECT 1 FROM public.channel_members bot WHERE bot.community_id=cm.id AND bot.pubkey=$4 AND bot.role='bot')
          AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=b.public_key AND u.deactivated_at IS NOT NULL)
      ), source AS MATERIALIZED (
        SELECT e.id,e.created_at,e.content,e.pubkey,e.kind,e.sig,v.*
        FROM visible v JOIN public.office_inbox i ON i.company_id=$1 AND i.event_id=$5 AND i.state='decided'
          AND i.channel_id=v.channel_id
        JOIN public.events e ON e.community_id=v.community_id AND e.id=i.event_id AND e.created_at=i.event_created_at
          AND e.pubkey=i.author_pubkey AND e.kind=i.event_kind AND e.channel_id=i.channel_id
        WHERE e.kind IN(9,40002) AND e.deleted_at IS NULL AND octet_length(e.content)<=65536
          AND octet_length(e.pubkey)=32 AND octet_length(e.sig)=64
      ), ancestry AS (
        SELECT 0 AS hop,e.id,e.created_at,
          CASE WHEN octet_length(e.tags::text)<=16384 THEN e.tags END AS tags,
          t.event_id IS NOT NULL AS metadata_present,t.channel_id AS metadata_channel,
          t.parent_event_id,t.parent_event_created_at,t.root_event_id,t.root_event_created_at,t.depth
        FROM source s JOIN public.events e ON e.community_id=s.community_id AND e.id=s.id AND e.created_at=s.created_at
        LEFT JOIN public.thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
        UNION ALL
        SELECT a.hop+1,e.id,e.created_at,
          CASE WHEN octet_length(e.tags::text)<=16384 THEN e.tags END,
          t.event_id IS NOT NULL,t.channel_id,t.parent_event_id,t.parent_event_created_at,
          t.root_event_id,t.root_event_created_at,t.depth
        FROM ancestry a JOIN public.events e ON e.community_id=(SELECT s.community_id FROM source s)
          AND e.id=a.parent_event_id AND e.created_at=a.parent_event_created_at
          AND e.channel_id=(SELECT s.channel_id FROM source s) AND e.deleted_at IS NULL AND e.kind IN(9,40002)
        LEFT JOIN public.thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
        WHERE a.hop<32
      )
      SELECT a.*,s.community_id,s.channel_id,s.observed_at,s.valid_before,
        CASE WHEN a.hop=0 THEN s.content END AS source_content,
        CASE WHEN a.hop=0 THEN s.pubkey END AS source_author,
        CASE WHEN a.hop=0 THEN s.sig END AS source_signature,s.kind AS source_kind
      FROM ancestry a CROSS JOIN source s ORDER BY a.hop LIMIT 33
    LOOP
        IF node.hop <> count_nodes OR octet_length(node.id) <> 32
           OR node.id = ANY(seen)
           OR NOT isfinite(node.created_at)
           OR node.created_at < '1970-01-01 00:00:00+00'::timestamptz
           OR node.created_at >= '10000-01-01 00:00:00+00'::timestamptz
           OR node.tags IS NULL OR jsonb_typeof(node.tags) <> 'array' THEN RETURN; END IF;
        seen := array_append(seen,node.id);
        IF count_nodes=0 THEN
            first_node := node;
            IF node.community_id = '00000000-0000-0000-0000-000000000000'::uuid
               OR node.channel_id = '00000000-0000-0000-0000-000000000000'::uuid THEN RETURN; END IF;
        ELSE
            IF expected_parent IS DISTINCT FROM node.id
               OR expected_parent_at IS DISTINCT FROM node.created_at THEN RETURN; END IF;
        END IF;

        -- Vec<Vec<String>> parity: even non-e tags must be arrays of strings.
        claimed_root := NULL; claimed_parent := NULL;
        FOR tag IN SELECT t.value FROM jsonb_array_elements(node.tags) AS t(value) LOOP
            IF jsonb_typeof(tag) <> 'array' THEN RETURN; END IF;
            FOR part IN SELECT t.value FROM jsonb_array_elements(tag) AS t(value) LOOP
                IF jsonb_typeof(part) <> 'string' THEN RETURN; END IF;
            END LOOP;
            IF tag->>0 IS DISTINCT FROM 'e' THEN CONTINUE; END IF;
            IF jsonb_array_length(tag)<4 OR octet_length(tag->>1)<>64
               OR (tag->>1) COLLATE "C" !~ '^[0-9a-fA-F]{64}$' THEN RETURN; END IF;
            reference_id := decode(tag->>1,'hex');
            marker := tag->>3;
            CASE marker
            WHEN 'root' THEN
                IF claimed_root IS NOT NULL THEN RETURN; END IF;
                claimed_root := reference_id;
            WHEN 'reply' THEN
                IF claimed_parent IS NOT NULL THEN RETURN; END IF;
                claimed_parent := reference_id;
            WHEN 'mention' THEN CONTINUE;
            ELSE RETURN;
            END CASE;
        END LOOP;
        IF claimed_root IS NOT NULL AND claimed_parent IS NULL THEN RETURN; END IF;
        claimed_root := coalesce(claimed_root,claimed_parent);

        -- Both locator halves are required, including exact UTC partition time.
        IF (node.parent_event_id IS NULL) <> (node.parent_event_created_at IS NULL)
           OR (node.root_event_id IS NULL) <> (node.root_event_created_at IS NULL) THEN RETURN; END IF;
        IF node.parent_event_id IS NOT NULL AND (octet_length(node.parent_event_id)<>32
           OR NOT isfinite(node.parent_event_created_at)
           OR node.parent_event_created_at<'1970-01-01 00:00:00+00'::timestamptz
           OR node.parent_event_created_at>='10000-01-01 00:00:00+00'::timestamptz) THEN RETURN; END IF;
        IF node.root_event_id IS NOT NULL AND (octet_length(node.root_event_id)<>32
           OR NOT isfinite(node.root_event_created_at)
           OR node.root_event_created_at<'1970-01-01 00:00:00+00'::timestamptz
           OR node.root_event_created_at>='10000-01-01 00:00:00+00'::timestamptz) THEN RETURN; END IF;

        effective_depth := coalesce(node.depth,0);
        IF node.metadata_present THEN
            IF node.metadata_channel IS DISTINCT FROM first_node.channel_id THEN RETURN; END IF;
            IF node.parent_event_id IS NULL AND node.depth=0 AND claimed_parent IS NULL THEN
                IF node.root_event_id IS NOT NULL AND
                   (node.root_event_id IS DISTINCT FROM node.id OR node.root_event_created_at IS DISTINCT FROM node.created_at) THEN RETURN; END IF;
            ELSIF node.parent_event_id IS NOT NULL AND node.root_event_id IS NOT NULL
                  AND node.depth BETWEEN 1 AND 32
                  AND claimed_parent=node.parent_event_id AND claimed_root=node.root_event_id THEN
                NULL;
            ELSE RETURN;
            END IF;
        ELSIF node.parent_event_id IS NOT NULL OR node.root_event_id IS NOT NULL
              OR node.depth IS NOT NULL OR claimed_parent IS NOT NULL THEN RETURN;
        END IF;
        IF count_nodes>0 AND expected_depth IS DISTINCT FROM effective_depth THEN RETURN; END IF;
        IF node.parent_event_id IS NOT NULL THEN
            IF count_nodes=0 THEN
                expected_root := node.root_event_id;
                expected_root_at := node.root_event_created_at;
            ELSIF node.root_event_id IS DISTINCT FROM expected_root
                  OR node.root_event_created_at IS DISTINCT FROM expected_root_at THEN RETURN;
            END IF;
        ELSE
            IF expected_root IS NOT NULL AND (expected_root IS DISTINCT FROM node.id
               OR expected_root_at IS DISTINCT FROM node.created_at) THEN RETURN; END IF;
            resolved_root := node.id; resolved_root_at := node.created_at;
        END IF;
        expected_parent := node.parent_event_id;
        expected_parent_at := node.parent_event_created_at;
        expected_depth := effective_depth-1;
        count_nodes := count_nodes+1;
    END LOOP;
    -- A missing/deleted/cross-channel parent, cycle or 33rd edge cannot become
    -- a top-level fallback. Every nonterminal depth decreases to an actual root.
    IF count_nodes=0 OR expected_parent IS NOT NULL OR resolved_root IS NULL THEN RETURN; END IF;

    community_id := first_node.community_id;
    channel_id := first_node.channel_id;
    source_event_created_at := first_node.created_at;
    observed_at := first_node.observed_at;
    valid_before := first_node.valid_before;
    source_stamp := to_char(source_event_created_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"');
    IF audience_kind='thread' THEN
        thread_root_event_id := resolved_root;
        thread_root_event_created_at := resolved_root_at;
        root_stamp := to_char(resolved_root_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"');
    END IF;

    audience_wire := jsonb_build_object(
        'channel_id',channel_id,'community_id',community_id,'company_id',company,
        'employee_id',employee,'format','ortak-reviewed-conversation-audience/1',
        'kind',audience_kind,'project_id',project,'thread_root_event_created_at',root_stamp,
        'thread_root_event_id',encode(thread_root_event_id,'hex'));
    encoded := public.ortak_conversation_json75(audience_wire);
    IF encoded IS NULL THEN RETURN; END IF;
    audience_bytes := convert_to(encoded,'UTF8');
    IF octet_length(audience_bytes)>2048 THEN RETURN; END IF;
    audience_hash := public.digest(audience_bytes,'sha256');

    -- Exact SourceEvidence declaration order in Rust is lexical; tags and
    -- source content retain their original strings and array order. No body
    -- is returned, copied into provenance or replaced with a message:<id> hash.
    encoded := public.ortak_conversation_json75(jsonb_build_object(
        'author_pubkey',encode(first_node.source_author,'hex'),'channel_id',channel_id,
        'community_id',community_id,'company_id',company,'content',first_node.source_content,
        'event_created_at',source_stamp,'event_id',encode(source_id,'hex'),
        'format','ortak-reviewed-conversation-evidence/1','kind',first_node.source_kind,
        'sig',encode(first_node.source_signature,'hex'),'tags',first_node.tags));
    IF encoded IS NULL THEN RETURN; END IF;
    source_evidence_hash := public.digest(convert_to(encoded,'UTF8'),'sha256');
    encoded := public.ortak_conversation_json75(jsonb_build_object(
        'audience_hash',encode(audience_hash,'hex'),'format','ortak-reviewed-conversation-source/1',
        'source_evidence_hash',encode(source_evidence_hash,'hex')));
    IF encoded IS NULL THEN RETURN; END IF;
    source_hash := public.digest(convert_to(encoded,'UTF8'),'sha256');
    encoded := public.ortak_conversation_json75(jsonb_build_object(
        'audience',audience_wire,'audience_hash',encode(audience_hash,'hex'),
        'format','ortak-reviewed-conversation-provenance/1','source_event_created_at',source_stamp,
        'source_event_id',encode(source_id,'hex'),'source_evidence_hash',encode(source_evidence_hash,'hex'),
        'source_hash',encode(source_hash,'hex')));
    IF encoded IS NULL THEN RETURN; END IF;
    provenance_bytes := convert_to(encoded,'UTF8');
    IF octet_length(provenance_bytes)>4096 THEN RETURN; END IF;
    RETURN NEXT;
END
$$;

-- D4 storage SOURCE FRAGMENT; not a numbered migration or deployed schema.
-- Root assembles the additive migration after 74. Apply the independently
-- reviewed ortak_conversation_source_observation(...) definition before use.
-- No conversation export/runtime selection or snapshot-v4 admission is added.
-- Before serving, root must exclude conversation facts from the legacy 69/71
-- project predicates and wire the separately reviewed current-use boundary.

ALTER TABLE reviewed_memory_facts
    ADD COLUMN audience_kind TEXT NOT NULL DEFAULT 'project'
        CHECK (audience_kind IN ('project', 'conversation'));
-- The existing 66 fact guard compares every non-revocation column on UPDATE,
-- so this field is immutable without widening the permitted version transition.

CREATE TABLE conversation_memory_authorities (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    project_id UUID NOT NULL,
    channel_id UUID NOT NULL CHECK (channel_id <> '00000000-0000-0000-0000-000000000000'),
    epoch BIGINT NOT NULL DEFAULT 0 CHECK (epoch >= 0),
    last_change_reason TEXT NOT NULL DEFAULT 'registered'
        CHECK (last_change_reason IN ('registered', 'channel_changed',
            'membership_changed', 'project_changed', 'project_grant_changed',
            'event_changed', 'thread_changed', 'identity_changed', 'scope_closed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, project_id, channel_id),
    UNIQUE (company_id, community_id, project_id, channel_id),
    FOREIGN KEY (company_id, project_id) REFERENCES projects(company_id, id),
    CHECK (company_id <> '00000000-0000-0000-0000-000000000000'
        AND community_id <> '00000000-0000-0000-0000-000000000000'
        AND project_id <> '00000000-0000-0000-0000-000000000000'),
    CHECK (changed_at >= created_at)
);
CREATE INDEX idx_conversation_authority_channel
    ON conversation_memory_authorities(community_id, channel_id, company_id, project_id);

-- This checks scope identity, not a human grant, employee, source or permission
-- to consume memory. Those belong to the observation/admission boundaries.
CREATE FUNCTION ortak_conversation_scope_current(
    company UUID, community UUID, project UUID, channel UUID
) RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT false
$$;

CREATE FUNCTION ortak_conversation_authority_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        RAISE EXCEPTION 'ortak: conversation authorities are retained'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF TG_OP='UPDATE' THEN
        IF (NEW.company_id,NEW.community_id,NEW.project_id,NEW.channel_id,NEW.created_at)
            IS DISTINCT FROM
            (OLD.company_id,OLD.community_id,OLD.project_id,OLD.channel_id,OLD.created_at)
            OR OLD.epoch=9223372036854775807 OR NEW.epoch<>OLD.epoch+1
            OR NEW.last_change_reason='registered' THEN
            RAISE EXCEPTION 'ortak: conversation authority only advances'
                USING ERRCODE='object_not_in_prerequisite_state';
        END IF;
        -- Mutation hooks own the Office/project fence and update sorted scope
        -- rows. Do not acquire Office exclusive here: project-grant writers use
        -- the existing project NOWAIT fence under signed shared-Office auth.
        NEW.changed_at=clock_timestamp();
        RETURN NEW;
    END IF;
    PERFORM ortak_lock_office_authority(NEW.company_id);
    PERFORM 1 FROM projects WHERE company_id=NEW.company_id AND id=NEW.project_id
        FOR SHARE NOWAIT;
    IF NOT FOUND OR NEW.epoch<>0 OR NEW.last_change_reason<>'registered'
        OR NOT ortak_conversation_scope_current(
            NEW.company_id,NEW.community_id,NEW.project_id,NEW.channel_id) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration requires current identity'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    -- Both ceilings include retained/removed scopes. Community then company
    -- registration locks are always nonblocking and acquired in that order.
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-community-registration:'||NEW.community_id::text,0)) THEN
        RAISE EXCEPTION 'ortak: community conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-registration:'||NEW.company_id::text,0)) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    IF (SELECT count(*) FROM conversation_memory_authorities WHERE company_id=NEW.company_id)>=128 THEN
        RAISE EXCEPTION 'ortak: retained conversation scope limit reached'
            USING ERRCODE='program_limit_exceeded';
    END IF;
    IF (SELECT count(*) FROM conversation_memory_authorities WHERE community_id=NEW.community_id)>=256 THEN
        RAISE EXCEPTION 'ortak: retained community conversation scope limit reached'
            USING ERRCODE='program_limit_exceeded';
    END IF;
    NEW.created_at=clock_timestamp();
    NEW.changed_at=NEW.created_at;
    RETURN NEW;
END $$;
CREATE TRIGGER conversation_authority_guard
    BEFORE INSERT OR UPDATE OR DELETE ON conversation_memory_authorities
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_authority_guard();
CREATE TRIGGER conversation_authority_no_truncate
    BEFORE TRUNCATE ON conversation_memory_authorities
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
SELECT attach_community_write_fence('conversation_memory_authorities');

-- Called after the shared Office/project locks; never upgrades project SHARE.
-- Existing scopes still require current identity, but do not consume another
-- slot. The returned epoch is locked only for this transaction, not a cache.
CREATE FUNCTION ortak_register_conversation_authority(
    company UUID, community UUID, project UUID, channel UUID
) RETURNS BIGINT LANGUAGE plpgsql AS $$
DECLARE selected BIGINT;
BEGIN
    PERFORM ortak_lock_office_authority(company);
    PERFORM 1 FROM projects p WHERE p.company_id=company AND p.id=project FOR SHARE NOWAIT;
    IF NOT FOUND OR NOT ortak_conversation_scope_current(company,community,project,channel) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration requires current identity'
            USING ERRCODE='object_not_in_prerequisite_state';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-community-registration:'||community::text,0)) THEN
        RAISE EXCEPTION 'ortak: community conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-conversation-authority-registration:'||company::text,0)) THEN
        RAISE EXCEPTION 'ortak: conversation scope registration is busy'
            USING ERRCODE='serialization_failure';
    END IF;
    SELECT a.epoch INTO selected FROM conversation_memory_authorities a
        WHERE a.company_id=company AND a.community_id=community
            AND a.project_id=project AND a.channel_id=channel FOR SHARE;
    IF FOUND THEN RETURN selected; END IF;
    -- A conflicting retained identity is an error, never a rebind/upsert.
    INSERT INTO conversation_memory_authorities(company_id,community_id,project_id,channel_id)
        VALUES(company,community,project,channel) RETURNING epoch INTO selected;
    RETURN selected;
END $$;

CREATE TABLE reviewed_memory_conversation_audiences (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    project_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    channel_id UUID NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('channel','thread')),
    thread_root_event_id BYTEA CHECK (octet_length(thread_root_event_id)=32),
    thread_root_event_created_at TIMESTAMPTZ,
    source_event_id BYTEA NOT NULL CHECK (octet_length(source_event_id)=32),
    source_event_created_at TIMESTAMPTZ NOT NULL,
    audience_bytes BYTEA NOT NULL CHECK (octet_length(audience_bytes) BETWEEN 1 AND 2048),
    audience_hash BYTEA NOT NULL CHECK (octet_length(audience_hash)=32),
    source_evidence_hash BYTEA NOT NULL CHECK (octet_length(source_evidence_hash)=32),
    source_hash BYTEA NOT NULL CHECK (octet_length(source_hash)=32),
    provenance_bytes BYTEA NOT NULL CHECK (octet_length(provenance_bytes) BETWEEN 1 AND 4096),
    PRIMARY KEY (company_id,fact_id),
    FOREIGN KEY (company_id,fact_id) REFERENCES reviewed_memory_facts(company_id,id),
    FOREIGN KEY (company_id,project_id) REFERENCES projects(company_id,id),
    FOREIGN KEY (company_id,employee_id) REFERENCES employees(company_id,id),
    FOREIGN KEY (company_id,community_id,project_id,channel_id)
        REFERENCES conversation_memory_authorities(company_id,community_id,project_id,channel_id),
    CHECK ((kind='channel' AND thread_root_event_id IS NULL AND thread_root_event_created_at IS NULL)
        OR (kind='thread' AND thread_root_event_id IS NOT NULL AND thread_root_event_created_at IS NOT NULL)),
    CHECK (source_event_created_at >= TIMESTAMPTZ '1970-01-01 00:00:00+00'
        AND source_event_created_at < TIMESTAMPTZ '10000-01-01 00:00:00+00'),
    CHECK (thread_root_event_created_at IS NULL OR
        (thread_root_event_created_at >= TIMESTAMPTZ '1970-01-01 00:00:00+00'
         AND thread_root_event_created_at < TIMESTAMPTZ '10000-01-01 00:00:00+00')),
    CHECK (source_event_id IS DISTINCT FROM thread_root_event_id
        OR source_event_created_at=thread_root_event_created_at),
    CHECK (sha256(audience_bytes)=audience_hash),
    CHECK (source_hash=sha256(convert_to(
        '{"audience_hash":"'||encode(audience_hash,'hex')||
        '","format":"ortak-reviewed-conversation-source/1","source_evidence_hash":"'||
        encode(source_evidence_hash,'hex')||'"}','UTF8')))
);
CREATE INDEX idx_conversation_audience_source
    ON reviewed_memory_conversation_audiences(community_id,source_event_id,source_event_created_at,company_id,project_id);
CREATE INDEX idx_conversation_audience_root
    ON reviewed_memory_conversation_audiences(community_id,thread_root_event_id,thread_root_event_created_at,company_id,project_id)
    WHERE thread_root_event_id IS NOT NULL;
CREATE INDEX idx_conversation_audience_scope
    ON reviewed_memory_conversation_audiences(company_id,project_id,channel_id,employee_id,fact_id);
CREATE INDEX idx_conversation_audience_employee
    ON reviewed_memory_conversation_audiences(company_id,employee_id,project_id,channel_id);
CREATE TRIGGER conversation_audience_immutable
    BEFORE UPDATE OR DELETE ON reviewed_memory_conversation_audiences
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();
CREATE TRIGGER conversation_audience_no_truncate
    BEFORE TRUNCATE ON reviewed_memory_conversation_audiences
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
SELECT attach_community_write_fence('reviewed_memory_conversation_audiences');

-- Run on INSERT only. A later Stop, expiry, grant loss or purge must not ask a
-- historical immutable audience to resolve against now-missing source rows.
CREATE FUNCTION ortak_conversation_fact_storage_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE company UUID; fact UUID; f reviewed_memory_facts;
    a reviewed_memory_conversation_audiences; observed RECORD;
BEGIN
    company=NEW.company_id;
    IF TG_TABLE_NAME='reviewed_memory_facts' THEN
        fact=NEW.id;
    ELSE
        fact=NEW.fact_id;
    END IF;
    SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=company AND x.id=fact;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: conversation audience parent fact is missing' USING ERRCODE='check_violation';
    END IF;
    SELECT * INTO a FROM reviewed_memory_conversation_audiences x WHERE x.company_id=company AND x.fact_id=fact;
    IF f.audience_kind='project' THEN
        IF FOUND THEN
            RAISE EXCEPTION 'ortak: project facts cannot acquire a conversation audience' USING ERRCODE='check_violation';
        END IF;
        RETURN NEW;
    END IF;
    IF NOT FOUND OR f.audience_kind<>'conversation' OR f.source_artifact_id IS NOT NULL
        OR (a.company_id,a.community_id,a.project_id,a.employee_id,a.source_event_id)
            IS DISTINCT FROM (f.company_id,f.community_id,f.project_id,f.employee_id,f.source_message_id)
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_facts born
            WHERE born.company_id=company AND born.id=fact
                AND born.xmin::text::bigint=txid_current()%4294967296)
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_conversation_audiences born
            WHERE born.company_id=company AND born.fact_id=fact
                AND born.xmin::text::bigint=txid_current()%4294967296)
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_operations receipt
            WHERE receipt.company_id=f.company_id AND receipt.community_id=f.community_id
                AND receipt.fact_id=f.id AND receipt.project_id=f.project_id
                AND receipt.actor_pubkey=f.approved_by AND receipt.operation_id=f.promotion_operation_id
                AND receipt.action='promote' AND receipt.result_version=1
                AND receipt.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: conversation approval requires one atomic audience and promotion receipt'
            USING ERRCODE='check_violation';
    END IF;
    PERFORM ortak_lock_office_authority(company);
    PERFORM 1 FROM projects p WHERE p.company_id=company AND p.id=f.project_id FOR SHARE NOWAIT;
    PERFORM 1 FROM conversation_memory_authorities authority
        WHERE authority.company_id=a.company_id AND authority.community_id=a.community_id
            AND authority.project_id=a.project_id AND authority.channel_id=a.channel_id FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: conversation authority identity is missing' USING ERRCODE='check_violation';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM project_access_grants grant_row
        WHERE grant_row.company_id=f.company_id AND grant_row.project_id=f.project_id
            AND grant_row.actor_pubkey=f.approved_by AND grant_row.revoked_at IS NULL
            AND grant_row.role IN ('owner','reviewer')) THEN
        RAISE EXCEPTION 'ortak: conversation approval requires current project review authority'
            USING ERRCODE='check_violation';
    END IF;
    BEGIN
        SELECT * INTO STRICT observed FROM ortak_conversation_source_observation(
            f.company_id,f.project_id,f.employee_id,decode(f.approved_by,'hex'),
            f.source_message_id,a.kind);
    EXCEPTION WHEN NO_DATA_FOUND OR TOO_MANY_ROWS THEN
        RAISE EXCEPTION 'ortak: conversation approval source is no longer current'
            USING ERRCODE='check_violation';
    END;
    IF (a.community_id,a.channel_id,a.source_event_created_at,
        a.thread_root_event_id,a.thread_root_event_created_at,a.audience_bytes,
        a.audience_hash,a.source_evidence_hash,a.source_hash,a.provenance_bytes)
        IS DISTINCT FROM
        (observed.community_id,observed.channel_id,observed.source_event_created_at,
        observed.thread_root_event_id,observed.thread_root_event_created_at,observed.audience_bytes,
        observed.audience_hash,observed.source_evidence_hash,observed.source_hash,observed.provenance_bytes)
        OR f.expires_at<=clock_timestamp()
        OR (observed.valid_before IS NOT NULL AND
            (clock_timestamp()>=observed.valid_before OR f.expires_at>observed.valid_before)) THEN
        RAISE EXCEPTION 'ortak: conversation approval bytes or current deadline differ'
            USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER conversation_fact_storage_at_commit
    AFTER INSERT ON reviewed_memory_facts DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_fact_storage_at_commit();
CREATE CONSTRAINT TRIGGER conversation_audience_storage_at_commit
    AFTER INSERT ON reviewed_memory_conversation_audiences DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_fact_storage_at_commit();

ALTER TABLE reviewed_memory_targets
    ADD COLUMN conversation_consumption_enabled BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN conversation_channel_id UUID
        CHECK (conversation_channel_id IS NULL OR conversation_channel_id<>'00000000-0000-0000-0000-000000000000'),
    ADD COLUMN conversation_consumption_epoch BIGINT NOT NULL DEFAULT 0 CHECK (conversation_consumption_epoch>=0),
    ADD CONSTRAINT conversation_target_selection_shape CHECK (
        (NOT conversation_consumption_enabled OR conversation_channel_id IS NOT NULL)
        AND (conversation_channel_id IS NOT NULL OR conversation_consumption_epoch=0));

-- Narrow replacement of 71's target guard: retain its original project epoch
-- transition, immutable binding/receipt identity and <=60s advertisement bound.


ALTER TABLE run_reviewed_memory_uses
    ADD COLUMN conversation_audience_hash BYTEA CHECK (octet_length(conversation_audience_hash)=32),
    ADD COLUMN conversation_authority_epoch BIGINT CHECK (conversation_authority_epoch>=0),
    ADD COLUMN conversation_consumption_epoch BIGINT CHECK (conversation_consumption_epoch>=0),
    ADD CONSTRAINT conversation_use_pin_shape CHECK (
        (conversation_audience_hash IS NULL AND conversation_authority_epoch IS NULL AND conversation_consumption_epoch IS NULL)
        OR (conversation_audience_hash IS NOT NULL AND conversation_authority_epoch IS NOT NULL
            AND conversation_consumption_epoch IS NOT NULL AND consumption_epoch=0));

-- Retained pin consistency only. This does not replace 71/72 current-use,
-- snapshot/admission guards, allocate v4, or permit a conversation runtime use.
CREATE FUNCTION ortak_conversation_use_storage_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE f reviewed_memory_facts; a reviewed_memory_conversation_audiences;
BEGIN
    SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=NEW.company_id AND x.id=NEW.fact_id;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'ortak: reviewed use fact is missing' USING ERRCODE='check_violation';
    END IF;
    IF f.audience_kind='project' THEN
        IF NEW.conversation_audience_hash IS NOT NULL OR NEW.conversation_authority_epoch IS NOT NULL
            OR NEW.conversation_consumption_epoch IS NOT NULL THEN
            RAISE EXCEPTION 'ortak: project use cannot carry conversation pins' USING ERRCODE='check_violation';
        END IF;
        RETURN NEW;
    END IF;
    SELECT * INTO a FROM reviewed_memory_conversation_audiences x WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id;
    IF NOT FOUND OR NEW.consumption_epoch<>0 OR NEW.conversation_audience_hash IS DISTINCT FROM a.audience_hash
        OR NEW.conversation_authority_epoch IS NULL OR NEW.conversation_consumption_epoch IS NULL
        OR NEW.community_id<>a.community_id OR NEW.source_hash<>a.source_hash
        OR NOT EXISTS (SELECT 1 FROM reviewed_memory_targets target
            WHERE target.company_id=NEW.company_id AND target.id=NEW.target_id
                AND target.community_id=a.community_id AND target.project_id=a.project_id
                AND target.employee_id=a.employee_id AND target.conversation_channel_id=a.channel_id) THEN
        RAISE EXCEPTION 'ortak: reviewed conversation use storage pins differ' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER conversation_use_storage_at_commit
    AFTER INSERT ON run_reviewed_memory_uses DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_conversation_use_storage_at_commit();

-- Root assembly dependencies outside this storage fragment:
-- * scoped authority mutation hooks/indexed affected-scope lookup;
-- * explicit legacy project-kind exclusions in 66/69/71 consumers;
-- * conversation source-hash/export dispatch only after separate approval;
-- * reviewed v4 current-use/snapshot/origin/epoch checks before runtime use;
-- * exact+retained deletion inventory and universal fence parity for both new
--   relations, plus G version/table/added-column witnesses.
-- No cleanup columns: permission loss only retires scope epochs. Existing
-- explicit Stop/expiry and real 69 withdrawal receipts remain authoritative.


-- Migration75: scoped authority mutation epochs.
-- D4 SOURCE FRAGMENT, assembled by root after source75 + storage75.
-- No remote I/O, cleanup, per-run scan or mutation of retained use pins.
-- Storage registration retains <=128 scopes/company and <=256/community.
-- One row mutation has at most two old/new communities: inspect <=513 keys,
-- refuse >512 BEFORE updating anything, then advance in deterministic order.

CREATE INDEX idx_conversation_thread_parent_exact
    ON thread_metadata(community_id,parent_event_id,parent_event_created_at)
    WHERE parent_event_id IS NOT NULL;
CREATE INDEX idx_conversation_thread_root_exact
    ON thread_metadata(community_id,root_event_id,root_event_created_at)
    WHERE root_event_id IS NOT NULL;
CREATE INDEX idx_conversation_office_employee_keys
    ON employee_office_bindings(company_id,employee_id,public_key);

-- Neutral INSERT means precisely the same top-level identity as absent
-- metadata. Do NOT consult retained audiences for this Office-fence decision:
-- a concurrent first approval's audience may still be uncommitted/invisible.
-- Desired-only closed stub: events dependencies are installed later by pgschema.
-- The reconciler restores the exact source function before admission/parity.
CREATE FUNCTION ortak_conversation_thread_insert_neutral75(proposed JSONB)
RETURNS BOOLEAN LANGUAGE sql STABLE STRICT AS $$
    SELECT false
$$;

-- Exact 48 body except the parentless metadata skip. The other skip cases,
-- lock ordering, try-lock failures and company/community mappings are retained.



-- The 73 channel trigger, including participant_hash/ttl_seconds/ttl_deadline,
-- remains untouched.

CREATE FUNCTION ortak_advance_conversation_scopes75(
    companies UUID[], communities UUID[], channels UUID[], projects UUID[],
    employees TEXT[], public_keys BYTEA[], selection TEXT, reason TEXT,
    office_fence BOOLEAN
) RETURNS VOID LANGUAGE plpgsql VOLATILE AS $$
DECLARE target UUID; project_key RECORD; keys JSONB; selected JSONB; key_hex TEXT[];
BEGIN
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'Conversation authority requires READ COMMITTED isolation'
            USING ERRCODE='invalid_transaction_state';
    END IF;
    IF companies IS NULL OR communities IS NULL OR channels IS NULL OR projects IS NULL
       OR employees IS NULL OR public_keys IS NULL OR selection IS NULL OR reason IS NULL OR office_fence IS NULL
       OR selection NOT IN('scope','channel','membership','project','identity','employee')
       OR reason NOT IN('channel_changed','membership_changed','project_changed','project_grant_changed',
            'event_changed','thread_changed','identity_changed','scope_closed')
       OR cardinality(companies)>2 OR cardinality(communities)>2 OR cardinality(channels)>2
       OR cardinality(projects)>2 OR cardinality(employees)>2 OR cardinality(public_keys)>2 THEN
        RAISE EXCEPTION 'Conversation mutation selection is invalid' USING ERRCODE='check_violation';
    END IF;
    IF office_fence THEN
        -- Acquire discovery's absent-row fence BEFORE selecting retained keys.
        -- Registration holds the matching Office shared locks. Try-locks retain
        -- 48's reverse-order refusal when the mutating tuple is already locked.
        FOR target IN SELECT DISTINCT v FROM unnest(communities) v ORDER BY v LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_community_lock_key(target)) THEN
                RAISE EXCEPTION 'Conversation community mutation is busy' USING ERRCODE='serialization_failure';
            END IF;
        END LOOP;
        FOR target IN SELECT DISTINCT v FROM unnest(companies) v ORDER BY v LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
                RAISE EXCEPTION 'Conversation company mutation is busy' USING ERRCODE='serialization_failure';
            END IF;
        END LOOP;
    ELSE
        -- Project archive/binding/grant writers can run under signed shared
        -- Office authentication. NEVER upgrade that Office lock. Project row
        -- locking blocks both grant races and newly registered scope phantoms.
        IF selection<>'project' OR cardinality(companies)=0 OR cardinality(projects)=0 THEN
            RAISE EXCEPTION 'Conversation project mutation selection is invalid' USING ERRCODE='check_violation';
        END IF;
        FOR project_key IN SELECT DISTINCT c AS company_id,p AS project_id
            FROM unnest(companies) c CROSS JOIN unnest(projects) p ORDER BY c,p LOOP
            PERFORM 1 FROM public.projects p WHERE p.company_id=project_key.company_id
                AND p.id=project_key.project_id FOR UPDATE NOWAIT;
        END LOOP;
    END IF;
    SELECT coalesce(array_agg(encode(v,'hex')),ARRAY[]::text[]) INTO key_hex FROM unnest(public_keys) v;
    SELECT coalesce(jsonb_agg(to_jsonb(candidate) ORDER BY candidate.company_id,candidate.project_id,candidate.channel_id),'[]'::jsonb)
      INTO keys FROM (
        SELECT a.company_id,a.project_id,a.channel_id
        FROM conversation_memory_authorities a
        JOIN public.communities cm ON cm.id=a.community_id AND cm.deletion_state='active' AND cm.deleted_at IS NULL
        WHERE (CASE WHEN cardinality(companies)>0 THEN a.company_id=ANY(companies)
                    ELSE a.community_id=ANY(communities) END)
          AND (selection='scope'
            OR (selection='project' AND a.project_id=ANY(projects))
            OR (selection IN('channel','membership') AND a.channel_id=ANY(channels))
            OR (selection IN('identity','employee','membership') AND (
                EXISTS(SELECT 1 FROM channel_members m WHERE m.community_id=a.community_id AND m.channel_id=a.channel_id
                    AND m.pubkey=ANY(public_keys))
                OR EXISTS(SELECT 1 FROM project_access_grants g WHERE g.company_id=a.company_id
                    AND g.project_id=a.project_id AND g.actor_pubkey=ANY(key_hex))
                OR EXISTS(SELECT 1 FROM reviewed_memory_conversation_audiences f
                    WHERE f.company_id=a.company_id AND f.project_id=a.project_id AND f.channel_id=a.channel_id
                      AND (f.employee_id=ANY(employees) OR EXISTS(SELECT 1 FROM employee_office_bindings b
                        WHERE b.company_id=f.company_id AND b.employee_id=f.employee_id AND b.public_key=ANY(public_keys))))
                OR EXISTS(SELECT 1 FROM reviewed_memory_targets t
                    WHERE t.company_id=a.company_id AND t.project_id=a.project_id AND t.conversation_channel_id=a.channel_id
                      AND (t.employee_id=ANY(employees) OR EXISTS(SELECT 1 FROM employee_office_bindings b
                        WHERE b.company_id=t.company_id AND b.employee_id=t.employee_id AND b.public_key=ANY(public_keys))))
                OR EXISTS(SELECT 1 FROM employee_office_bindings b JOIN channel_members m
                    ON m.community_id=a.community_id AND m.channel_id=a.channel_id AND m.pubkey=b.public_key
                    WHERE b.company_id=a.company_id AND b.employee_id=ANY(employees)))))
        ORDER BY a.company_id,a.project_id,a.channel_id LIMIT 513
      ) candidate;
    IF jsonb_array_length(keys)>512 THEN
        RAISE EXCEPTION 'Conversation mutation exceeds retained scope bound' USING ERRCODE='program_limit_exceeded';
    END IF;
    -- Retained mappings, not only current office_company_bindings, determine
    -- company identity. Closed communities were retired BEFORE their first
    -- close; later mutations leave that epoch/reason intact. They must not
    -- demand an old deletion lease or bypass the universal community fence.
    IF office_fence THEN
        FOR target IN SELECT DISTINCT (v->>'company_id')::uuid FROM jsonb_array_elements(keys) v ORDER BY 1 LOOP
            IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
                RAISE EXCEPTION 'Conversation retained company mutation is busy' USING ERRCODE='serialization_failure';
            END IF;
        END LOOP;
    END IF;
    FOR selected IN SELECT v FROM jsonb_array_elements(keys) v LOOP
        PERFORM 1 FROM public.projects p WHERE p.company_id=(selected->>'company_id')::uuid
            AND p.id=(selected->>'project_id')::uuid FOR SHARE NOWAIT;
        PERFORM 1 FROM conversation_memory_authorities a
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.project_id=(selected->>'project_id')::uuid
                AND a.channel_id=(selected->>'channel_id')::uuid FOR UPDATE NOWAIT;
        UPDATE conversation_memory_authorities a SET epoch=a.epoch+1,last_change_reason=reason
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.project_id=(selected->>'project_id')::uuid
                AND a.channel_id=(selected->>'channel_id')::uuid;
    END LOOP;
END
$$;

CREATE FUNCTION ortak_conversation_epoch_mutation75() RETURNS TRIGGER LANGUAGE plpgsql VOLATILE AS $$
DECLARE
    previous JSONB := CASE WHEN TG_OP<>'INSERT' THEN to_jsonb(OLD) END;
    proposed JSONB := CASE WHEN TG_OP<>'DELETE' THEN to_jsonb(NEW) END;
    fields TEXT[]; field TEXT; changed BOOLEAN := TG_OP<>'UPDATE';
    companies UUID[]; communities UUID[]; channels UUID[]; projects UUID[];
    employees TEXT[]; public_keys BYTEA[];
    kind TEXT := TG_ARGV[0]; selection TEXT; reason TEXT; office_fence BOOLEAN := true;
    old_manifest JSONB; new_manifest JSONB;
BEGIN
    CASE kind
    WHEN 'channel' THEN fields:=ARRAY['community_id','id','channel_type','visibility','archived_at','deleted_at','participant_hash','ttl_seconds','ttl_deadline']; selection:='channel'; reason:='channel_changed';
    WHEN 'membership' THEN fields:=ARRAY['community_id','channel_id','pubkey','role','removed_at']; selection:='membership'; reason:='membership_changed';
    WHEN 'event' THEN fields:=ARRAY['community_id','id','created_at','pubkey','kind','tags','content','sig','channel_id','deleted_at']; selection:='channel'; reason:='event_changed';
    WHEN 'thread' THEN fields:=ARRAY['community_id','event_id','event_created_at','channel_id','parent_event_id','parent_event_created_at','root_event_id','root_event_created_at','depth']; selection:='channel'; reason:='thread_changed';
    WHEN 'inbox' THEN fields:=ARRAY['company_id','event_id','event_created_at','event_kind','author_pubkey','channel_id','state']; selection:='channel'; reason:='event_changed';
    WHEN 'project' THEN fields:=ARRAY['company_id','id','status','archived_at']; selection:='project'; reason:='project_changed'; office_fence:=false;
    WHEN 'project_binding' THEN fields:=ARRAY['company_id','project_id','community_id','channel_id']; selection:='project'; reason:='project_changed'; office_fence:=false;
    WHEN 'grant' THEN fields:=ARRAY['company_id','project_id','actor_pubkey','role','revoked_at']; selection:='project'; reason:='project_grant_changed'; office_fence:=false;
    WHEN 'user' THEN fields:=ARRAY['community_id','pubkey','agent_type','agent_owner_pubkey','deactivated_at']; selection:='identity'; reason:='identity_changed';
    WHEN 'employee' THEN fields:=ARRAY['company_id','id','status','active_revision_id']; selection:='employee'; reason:='identity_changed';
    WHEN 'office_identity' THEN fields:=ARRAY['company_id','employee_id','public_key','signer_ref','valid_from','valid_until']; selection:='employee'; reason:='identity_changed';
    WHEN 'memory_identity' THEN fields:=ARRAY['company_id','employee_id','revision_id','adapter','endpoint_ref','workspace','user_peer','employee_peer','options']; selection:='employee'; reason:='identity_changed';
    WHEN 'company' THEN fields:=ARRAY['id','status']; selection:='scope'; reason:='scope_closed';
    WHEN 'community' THEN fields:=ARRAY['id','deletion_state','deletion_fence_generation','deleted_at']; selection:='scope'; reason:='scope_closed';
    WHEN 'company_binding' THEN fields:=ARRAY['company_id','community_id']; selection:='scope'; reason:='scope_closed';
    ELSE RAISE EXCEPTION 'Conversation mutation kind is invalid' USING ERRCODE='check_violation';
    END CASE;
    IF TG_OP='UPDATE' THEN
        FOREACH field IN ARRAY fields LOOP
            IF previous->field IS DISTINCT FROM proposed->field THEN changed:=true; EXIT; END IF;
        END LOOP;
        IF kind='office_identity' THEN changed:=changed OR ((previous->>'verified_at' IS NULL)<>(proposed->>'verified_at' IS NULL)); END IF;
        IF kind='memory_identity' THEN changed:=changed OR ((previous->>'validated_at' IS NULL)<>(proposed->>'validated_at' IS NULL)); END IF;
        IF NOT changed THEN RETURN NEW; END IF;
    END IF;
    IF kind='community' AND coalesce(previous->>'deletion_state','')<>'active' THEN
        -- The first transition out of active retired every scope under the
        -- Office exclusive fence. Later closure stages and return to active
        -- never lower that epoch, so no old use can revive. Run this hook
        -- BEFORE the first close while the universal write fence allows it.
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='thread' AND TG_OP='INSERT' THEN
        IF ortak_conversation_thread_insert_neutral75(proposed) THEN RETURN NEW; END IF;
        -- Exclude the just-inserted metadata row from its own reference proof.
        -- Child/root indexes cover a channel fact whose canonical root is not
        -- retained in its channel-wide audience. New unrelated replies have
        -- neither retained anchors nor existing descendants and do not bump.
        IF NOT EXISTS(SELECT 1 FROM reviewed_memory_conversation_audiences a
            WHERE a.community_id=(proposed->>'community_id')::uuid AND
              ((a.source_event_id=(proposed->>'event_id')::bytea AND a.source_event_created_at=(proposed->>'event_created_at')::timestamptz)
                OR (a.thread_root_event_id=(proposed->>'event_id')::bytea AND a.thread_root_event_created_at=(proposed->>'event_created_at')::timestamptz)))
           AND NOT EXISTS(SELECT 1 FROM thread_metadata t WHERE t.community_id=(proposed->>'community_id')::uuid
             AND (t.event_id,t.event_created_at) IS DISTINCT FROM ((proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz)
             AND ((t.parent_event_id=(proposed->>'event_id')::bytea AND t.parent_event_created_at=(proposed->>'event_created_at')::timestamptz)
               OR (t.root_event_id=(proposed->>'event_id')::bytea AND t.root_event_created_at=(proposed->>'event_created_at')::timestamptz))) THEN RETURN NEW; END IF;
    END IF;
    IF kind='inbox' AND coalesce(previous->>'state','')<>'decided'
       AND NOT EXISTS(SELECT 1 FROM reviewed_memory_conversation_audiences a
         WHERE (a.company_id,a.source_event_id,a.source_event_created_at) IN (
           ((previous->>'company_id')::uuid,(previous->>'event_id')::bytea,(previous->>'event_created_at')::timestamptz),
           ((proposed->>'company_id')::uuid,(proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='employee' AND TG_OP='UPDATE' AND previous->>'company_id'=proposed->>'company_id'
       AND previous->>'id'=proposed->>'id' AND previous->>'status'=proposed->>'status' THEN
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO old_manifest
            FROM employee_revisions r WHERE r.company_id=(previous->>'company_id')::uuid AND r.employee_id=previous->>'id'
                AND r.id=(previous->>'active_revision_id')::uuid;
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO new_manifest
            FROM employee_revisions r WHERE r.company_id=(proposed->>'company_id')::uuid AND r.employee_id=proposed->>'id'
                AND r.id=(proposed->>'active_revision_id')::uuid;
        IF old_manifest IS NOT NULL AND old_manifest IS NOT DISTINCT FROM new_manifest THEN RETURN NEW; END IF;
    END IF;
    IF kind='memory_identity' AND NOT EXISTS(SELECT 1 FROM public.employees e
        WHERE (e.company_id,e.id,e.active_revision_id) IN (
          ((previous->>'company_id')::uuid,previous->>'employee_id',(previous->>'revision_id')::uuid),
          ((proposed->>'company_id')::uuid,proposed->>'employee_id',(proposed->>'revision_id')::uuid))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='user' AND TG_OP IN('INSERT','DELETE')
       AND coalesce(proposed,previous)->>'agent_type' IS NULL
       AND coalesce(proposed,previous)->>'agent_owner_pubkey' IS NULL
       AND coalesce(proposed,previous)->>'deactivated_at' IS NULL THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    -- Ordinary first joins cannot invalidate a previously authorized reader;
    -- bot insertion is different: the resolver treats that key as automated
    -- across its entire community, including other retained channels/projects.
    IF kind='membership' AND TG_OP='INSERT' AND proposed->>'role'<>'bot' THEN RETURN NEW; END IF;

    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO companies FROM (VALUES
        (previous->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END),
        (proposed->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO communities FROM (VALUES
        (previous->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END),
        (proposed->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO channels FROM (VALUES
        (previous->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END),
        (proposed->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO projects FROM (VALUES
        (previous->>CASE WHEN kind='project' THEN 'id' ELSE 'project_id' END),
        (proposed->>CASE WHEN kind='project' THEN 'id' ELSE 'project_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v),ARRAY[]::text[]) INTO employees FROM (VALUES
        (previous->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END),
        (proposed->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::bytea),ARRAY[]::bytea[]) INTO public_keys FROM (VALUES
        (previous->>CASE WHEN kind='office_identity' THEN 'public_key' ELSE 'pubkey' END),
        (proposed->>CASE WHEN kind='office_identity' THEN 'public_key' ELSE 'pubkey' END)) t(v) WHERE v IS NOT NULL;
    IF kind='membership' AND coalesce(previous->>'role','')<>'bot' AND coalesce(proposed->>'role','')<>'bot' THEN
        public_keys:=ARRAY[]::bytea[];
    END IF;
    PERFORM ortak_advance_conversation_scopes75(companies,communities,channels,projects,employees,public_keys,selection,reason,office_fence);
    RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
END
$$;

-- AFTER hooks run only after the retained 48/54/73 mutation guards acquired
-- their fences; the helper also covers absent mappings and newly watched data.
-- Community closure is the explicit BEFORE exception, alphabetically after
-- ortak_office_authority_communities and before the universal fence closes.
CREATE TRIGGER conversation_epoch_channels AFTER INSERT OR UPDATE OR DELETE ON channels FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('channel');
CREATE TRIGGER conversation_epoch_members AFTER INSERT OR UPDATE OR DELETE ON channel_members FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('membership');
CREATE TRIGGER conversation_epoch_events AFTER UPDATE OR DELETE ON events FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('event');
CREATE TRIGGER conversation_epoch_threads AFTER INSERT OR UPDATE OR DELETE ON thread_metadata FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('thread');
CREATE TRIGGER conversation_epoch_inbox AFTER UPDATE OR DELETE ON office_inbox FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('inbox');
CREATE TRIGGER conversation_epoch_projects AFTER UPDATE OR DELETE ON projects FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('project');
CREATE TRIGGER conversation_epoch_project_bindings AFTER INSERT OR UPDATE OR DELETE ON project_api_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('project_binding');
CREATE TRIGGER conversation_epoch_grants AFTER INSERT OR UPDATE OR DELETE ON project_access_grants FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('grant');
CREATE TRIGGER conversation_epoch_users AFTER INSERT OR UPDATE OR DELETE ON users FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('user');
CREATE TRIGGER conversation_epoch_employees AFTER INSERT OR UPDATE OR DELETE ON employees FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('employee');
CREATE TRIGGER conversation_epoch_office_identities AFTER INSERT OR UPDATE OR DELETE ON employee_office_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('office_identity');
CREATE TRIGGER conversation_epoch_memory_identities AFTER INSERT OR UPDATE OR DELETE ON employee_memory_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('memory_identity');
CREATE TRIGGER conversation_epoch_companies AFTER UPDATE OR DELETE ON companies FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('company');
CREATE TRIGGER ortak_z_conversation_epoch_communities BEFORE UPDATE OR DELETE ON communities FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('community');
CREATE TRIGGER conversation_epoch_company_bindings AFTER INSERT OR UPDATE OR DELETE ON office_company_bindings FOR EACH ROW EXECUTE FUNCTION ortak_conversation_epoch_mutation75('company_binding');

-- Closed SQL bootstrap bodies are restored exactly by reconciliation76.
CREATE FUNCTION ortak_conversation_run_origin(company UUID, run UUID, project UUID)
RETURNS TABLE(requester_public_key BYTEA, provenance_bytes BYTEA,
    observed_at TIMESTAMPTZ, valid_before TIMESTAMPTZ)
LANGUAGE sql STABLE AS $$
    SELECT NULL::bytea, NULL::bytea, NULL::timestamptz, NULL::timestamptz WHERE false
$$;

-- Closed SQL bootstrap bodies are restored exactly by reconciliation76.
CREATE FUNCTION ortak_conversation_target_eligible76(company UUID, fact UUID, target UUID, publication BOOLEAN)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT false
$$;

-- Closed SQL bootstrap bodies are restored exactly by reconciliation76.
CREATE FUNCTION ortak_conversation_export_eligible(company UUID, fact UUID, target UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT false
$$;

-- Closed SQL bootstrap bodies are restored exactly by reconciliation76.
CREATE FUNCTION ortak_conversation_runtime_eligible(company UUID, run UUID, fact UUID, target UUID,
    authority_epoch BIGINT, consumption_epoch BIGINT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT false
$$;

-- Closed SQL bootstrap bodies are restored exactly by reconciliation76.
CREATE FUNCTION ortak_conversation_effect_admission76() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE effect BOOLEAN=false; previous JSONB; proposed JSONB;
BEGIN
    IF NOT EXISTS(SELECT 1 FROM run_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
        AND u.run_id=NEW.run_id AND u.conversation_audience_hash IS NOT NULL)
        AND NOT EXISTS(SELECT 1 FROM run_employee_reviewed_memory_uses u WHERE u.company_id=NEW.company_id
            AND u.run_id=NEW.run_id) THEN RETURN NEW; END IF;
    previous=CASE WHEN TG_OP='UPDATE' THEN to_jsonb(OLD) END; proposed=to_jsonb(NEW);
    CASE TG_TABLE_NAME
    WHEN 'runtime_work_outputs' THEN effect=NEW.state='materialized';
    WHEN 'runtime_office_outputs' THEN effect=NEW.state='enqueued' OR
        (NEW.office_authority_token IS NOT NULL AND (TG_OP='INSERT'
            OR (proposed->'office_authority_token',proposed->'office_authority_generation',proposed->'office_authority_valid_before')
              IS DISTINCT FROM (previous->'office_authority_token',previous->'office_authority_generation',previous->'office_authority_valid_before')));
    WHEN 'runtime_memory_writes' THEN effect=NEW.state='pending' AND NEW.admission_token IS NOT NULL
        AND (TG_OP='INSERT' OR (proposed->'admission_token',proposed->'admission_generation',proposed->'admission_valid_before')
            IS DISTINCT FROM (previous->'admission_token',previous->'admission_generation',previous->'admission_valid_before'));
    WHEN 'outbox' THEN effect=NEW.kind='office_publish' AND NEW.state='pending'
        AND (TG_OP='INSERT' OR (proposed->'signed_event_id',proposed->'signed_event_bytes')
            IS DISTINCT FROM (previous->'signed_event_id',previous->'signed_event_bytes'));
    ELSE RAISE EXCEPTION 'ortak: unknown conversation effect' USING ERRCODE='check_violation';
    END CASE;
    IF effect AND NOT ortak_run_reviewed_memory_current(NEW.company_id,NEW.run_id) THEN
        RAISE EXCEPTION 'ortak: conversation output authority changed' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

-- Closed SQL bootstrap bodies are restored exactly by reconciliation76.
CREATE FUNCTION ortak_conversation_snapshot76(company UUID, run UUID, wire JSONB)
RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE
    r runs; revision employee_revisions; work work_executions;
    selected_project UUID; origin RECORD; context JSONB; record JSONB; pin JSONB;
    wrapped JSONB; rendered JSONB; expected_pin JSONB; expected_record JSONB;
    u run_reviewed_memory_uses; f reviewed_memory_facts; a reviewed_memory_conversation_audiences;
    used_count INTEGER; scratch_count INTEGER; i INTEGER=0; conversations INTEGER=0;
    reviewed_bytes INTEGER=0; total_bytes INTEGER=0; content TEXT; seen UUID[]=ARRAY[]::uuid[];
BEGIN
    SELECT * INTO r FROM runs x WHERE x.company_id=company AND x.id=run;
    SELECT * INTO revision FROM employee_revisions x WHERE x.company_id=company
        AND x.employee_id=r.employee_id AND x.id=r.employee_revision_id;
    context=wire->'conversation';
    IF r.id IS NULL OR revision.id IS NULL OR r.status NOT IN('queued','running','waiting')
        OR wire->'version' IS DISTINCT FROM '4'::jsonb
        OR wire ? 'reviewed' OR jsonb_typeof(context) IS DISTINCT FROM 'object'
        OR (context-'origin'-'records'-'truncated')<>'{}'::jsonb
        OR jsonb_typeof(context->'truncated') IS DISTINCT FROM 'boolean'
        OR jsonb_typeof(context->'records') IS DISTINCT FROM 'array'
        OR jsonb_typeof(wire#>'{recall,records}') IS DISTINCT FROM 'array'
        OR jsonb_typeof(wire#>'{spec,context,memory_context}') IS DISTINCT FROM 'array'
        OR wire->>'company_id' IS DISTINCT FROM company::text
        OR wire#>>'{spec,run_id}' IS DISTINCT FROM run::text
        OR wire#>>'{spec,employee_id}' IS DISTINCT FROM r.employee_id
        OR wire#>>'{spec,revision_id}' IS DISTINCT FROM r.employee_revision_id::text
        OR wire#>>'{spec,idempotency_key}' IS DISTINCT FROM 'ortak-run:'||company::text||':'||run::text
        OR wire#>'{spec,binding}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'runtime')::json)
        OR wire#>'{spec,permissions}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'permissions')::json)
        OR wire->'memory_binding' IS DISTINCT FROM ortak_snapshot_scratch_jsonb((revision.manifest->'memory')::json) THEN
        RAISE EXCEPTION 'ortak: conversation snapshot shape or run identity differs' USING ERRCODE='check_violation';
    END IF;
    SELECT count(*) INTO used_count FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run;
    scratch_count=jsonb_array_length(wire#>'{recall,records}');
    IF used_count NOT BETWEEN 1 AND 8 OR jsonb_array_length(context->'records')<>used_count
        OR scratch_count+used_count>8
        OR jsonb_array_length(wire#>'{spec,context,memory_context}')<>scratch_count+used_count THEN
        RAISE EXCEPTION 'ortak: conversation snapshot count differs' USING ERRCODE='check_violation';
    END IF;
    -- Select the project from immutable use/fact rows, never from the caller's
    -- JSON provenance. Every reviewed record below must have this same project.
    SELECT min(fact.project_id::text)::uuid INTO selected_project
        FROM run_reviewed_memory_uses used JOIN reviewed_memory_facts fact
            ON fact.company_id=used.company_id AND fact.id=used.fact_id
        WHERE used.company_id=company AND used.run_id=run
        HAVING count(DISTINCT fact.project_id)=1;
    SELECT * INTO origin FROM ortak_conversation_run_origin(company,run,selected_project);
    IF NOT FOUND OR context->'origin' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(
        jsonb_build_object('requester_public_key',encode(origin.requester_public_key,'hex'),
            'provenance',convert_from(origin.provenance_bytes,'UTF8'))::json) THEN
        RAISE EXCEPTION 'ortak: conversation snapshot origin differs' USING ERRCODE='check_violation';
    END IF;
    IF r.work_item_id IS NULL THEN
        IF wire ? 'work_origin' OR wire->>'message_id' IS DISTINCT FROM encode(r.message_id,'hex')
            OR wire->>'root_message_id' IS DISTINCT FROM encode(r.root_message_id,'hex')
            OR wire->>'routing_decision_id' IS DISTINCT FROM r.routing_decision_id::text
            OR wire->'input_truncated' IS DISTINCT FROM 'false'::jsonb
            OR wire#>>'{spec,context,reply_to_message_id}' IS DISTINCT FROM encode(r.message_id,'hex')
            OR wire#>'{spec,context,work_item_id}' IS DISTINCT FROM 'null'::jsonb
            OR NOT EXISTS(SELECT 1 FROM office_inbox inbox
                JOIN office_company_bindings office ON office.company_id=inbox.company_id
                JOIN events event ON event.community_id=office.community_id AND event.id=inbox.event_id
                    AND event.created_at=inbox.event_created_at AND event.kind=inbox.event_kind
                    AND event.channel_id=inbox.channel_id AND event.pubkey=inbox.author_pubkey
                CROSS JOIN LATERAL (SELECT regexp_replace(event.content,
                    U&'[\0001-\0008\000B\000C\000E-\001F\007F-\009F]','','g') AS cleaned) input
                WHERE inbox.company_id=company AND inbox.event_id=r.message_id
                AND wire->'event_kind'=to_jsonb(inbox.event_kind)
                AND wire#>>'{spec,context,conversation_ref}'=inbox.channel_id::text
                -- Source75 already caps the original text at65536 bytes;
                -- control removal cannot require UTF-8 truncation afterwards.
                AND event.deleted_at IS NULL AND octet_length(event.content)<=65536
                AND btrim(input.cleaned,U&'\0009\000A\000B\000C\000D\0020\0085\00A0\1680\2000\2001\2002\2003\2004\2005\2006\2007\2008\2009\200A\2028\2029\202F\205F\3000')<>''
                AND wire#>'{spec,input}'=ortak_snapshot_scratch_jsonb(to_json(input.cleaned))) THEN
            RAISE EXCEPTION 'ortak: conversation Office origin differs' USING ERRCODE='check_violation';
        END IF;
    ELSE
        SELECT * INTO work FROM work_executions x WHERE x.company_id=company AND x.run_id=run;
        IF work.run_id IS NULL OR work.project_id<>selected_project
            OR wire ? 'message_id' OR wire ? 'root_message_id' OR wire ? 'routing_decision_id'
            OR wire->'event_kind' IS DISTINCT FROM '0'::jsonb
            OR wire->'input_truncated' IS DISTINCT FROM 'false'::jsonb
            OR wire->'work_origin' IS DISTINCT FROM jsonb_build_object('run_id',work.run_id,
                'work_item_id',work.work_item_id,'project_id',work.project_id,'execution_version',work.execution_version,
                'definition_hash',encode(work.definition_hash,'hex'))
            OR wire#>'{spec,input}' IS DISTINCT FROM ortak_snapshot_scratch_jsonb(to_json(convert_from(work.definition_bytes,'UTF8')))
            OR wire#>>'{spec,context,work_item_id}' IS DISTINCT FROM r.work_item_id::text
            OR wire#>'{spec,context,reply_to_message_id}' IS DISTINCT FROM 'null'::jsonb
            OR wire#>'{spec,context,conversation_ref}' IS DISTINCT FROM 'null'::jsonb THEN
            RAISE EXCEPTION 'ortak: conversation Work origin differs' USING ERRCODE='check_violation';
        END IF;
    END IF;
    FOR record IN SELECT value FROM jsonb_array_elements(wire#>'{recall,records}') LOOP
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',i::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type','run_scratch_memory','trust','untrusted_data','record',record)
            OR octet_length(wire#>>ARRAY['spec','context','memory_context',i::text])>8192
            OR jsonb_typeof(record->'content') IS DISTINCT FROM 'string' THEN
            RAISE EXCEPTION 'ortak: conversation scratch rendering differs' USING ERRCODE='check_violation';
        END IF;
        content=record->>'content';
        total_bytes=total_bytes+octet_length(content)
            -(octet_length(content)-octet_length(regexp_replace(content,E'\x01[\x01\x02]','','g')))/2;
        i=i+1;
    END LOOP;
    i=0;
    FOR wrapped IN SELECT value FROM jsonb_array_elements(context->'records') LOOP
        record=wrapped->'record'; pin=record->'pin';
        SELECT * INTO u FROM run_reviewed_memory_uses x WHERE x.company_id=company AND x.run_id=run AND x.ordinal=i;
        SELECT * INTO f FROM reviewed_memory_facts x WHERE x.company_id=company AND x.id=u.fact_id;
        IF u.run_id IS NULL OR f.id IS NULL OR f.project_id<>selected_project OR u.fact_id=ANY(seen)
            OR NOT EXISTS(SELECT 1 FROM reviewed_memory_targets target WHERE target.company_id=company
                AND target.id=u.target_id AND ortak_snapshot_scratch_jsonb(target.binding::json)=wire->'memory_binding') THEN
            RAISE EXCEPTION 'ortak: conversation retained record identity differs' USING ERRCODE='check_violation';
        END IF;
        seen=array_append(seen,u.fact_id);
        expected_pin=jsonb_build_object('fact_id',u.fact_id,'target_id',u.target_id,'fact_version',u.fact_version,
            'consumption_epoch',u.consumption_epoch,'content_hash',encode(u.content_hash,'hex'),
            'source_hash',encode(u.source_hash,'hex'),'binding_hash',encode(u.binding_hash,'hex'),
            'approval_id',u.approval_id,'approved_by',u.approved_by,'expires_at',pin->>'expires_at');
        IF wrapped->>'scope'='conversation' AND f.audience_kind='conversation' THEN
            SELECT * INTO a FROM reviewed_memory_conversation_audiences x WHERE x.company_id=company AND x.fact_id=f.id;
            IF NOT FOUND OR u.consumption_epoch<>0 OR u.conversation_audience_hash IS DISTINCT FROM a.audience_hash THEN
                RAISE EXCEPTION 'ortak: conversation audience pin differs' USING ERRCODE='check_violation';
            END IF;
            expected_pin=expected_pin||jsonb_build_object('conversation_audience_hash',encode(u.conversation_audience_hash,'hex'),
                'conversation_authority_epoch',u.conversation_authority_epoch,
                'conversation_consumption_epoch',u.conversation_consumption_epoch);
            expected_record=jsonb_build_object('pin',expected_pin,'content',f.content,'provenance',convert_from(a.provenance_bytes,'UTF8'));
            conversations=conversations+1;
        ELSIF wrapped->>'scope'='project' AND f.audience_kind='project' AND r.work_item_id IS NOT NULL THEN
            expected_record=jsonb_build_object('pin',expected_pin,'content',f.content);
        ELSE RAISE EXCEPTION 'ortak: conversation record scope differs' USING ERRCODE='check_violation';
        END IF;
        IF record IS DISTINCT FROM ortak_snapshot_scratch_jsonb(expected_record::json)
            OR wrapped IS DISTINCT FROM jsonb_build_object('scope',wrapped->>'scope','record',record)
            OR (pin->>'expires_at')::timestamptz IS DISTINCT FROM u.expires_at THEN
            RAISE EXCEPTION 'ortak: conversation record bytes differ from retained use' USING ERRCODE='check_violation';
        END IF;
        rendered=ortak_snapshot_scratch_jsonb((wire#>>ARRAY['spec','context','memory_context',(scratch_count+i)::text])::json);
        IF rendered IS DISTINCT FROM jsonb_build_object('type',CASE WHEN wrapped->>'scope'='project'
                THEN 'reviewed_project_memory' ELSE 'reviewed_conversation_memory' END,'trust','untrusted_data','record',record)
            OR octet_length(wire#>>ARRAY['spec','context','memory_context',(scratch_count+i)::text])>8192 THEN
            RAISE EXCEPTION 'ortak: conversation rendered bytes differ' USING ERRCODE='check_violation';
        END IF;
        reviewed_bytes=reviewed_bytes+octet_length(f.content); i=i+1;
    END LOOP;
    IF conversations=0 OR reviewed_bytes>8192 OR total_bytes+reviewed_bytes>16384
        OR NOT ortak_run_reviewed_memory_current(company,run) THEN
        RAISE EXCEPTION 'ortak: conversation budget or current authority differs' USING ERRCODE='check_violation';
    END IF;
END $$;

-- Closed SQL bootstrap bodies are restored exactly by reconciliation76.

CREATE CONSTRAINT TRIGGER conversation_work_output_at_commit AFTER INSERT OR UPDATE ON runtime_work_outputs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();
CREATE CONSTRAINT TRIGGER conversation_office_output_at_commit AFTER INSERT OR UPDATE ON runtime_office_outputs
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();
CREATE CONSTRAINT TRIGGER conversation_memory_write_at_commit AFTER INSERT OR UPDATE ON runtime_memory_writes
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();
CREATE CONSTRAINT TRIGGER conversation_delivery_at_commit AFTER INSERT OR UPDATE ON outbox
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_conversation_effect_admission76();


-- Reviewed77 desired assembly from immutable 0077.
-- Closed cross-relation SQL bodies are restored exactly by reconciliation77.

CREATE FUNCTION ortak_employee_memory_timestamp(value TIMESTAMPTZ)
RETURNS TEXT LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT CASE WHEN value >= TIMESTAMPTZ '1970-01-01 00:00:00+00'
        AND value < TIMESTAMPTZ '10000-01-01 00:00:00+00'
        THEN to_char(value AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END
$$;

CREATE FUNCTION ortak_employee_memory_observation(
    company UUID, employee TEXT, actor BYTEA, source_id BYTEA,
    source_created_at TIMESTAMPTZ, destination_channel UUID,
    memory_kind TEXT, relationship_human BYTEA
) RETURNS TABLE(community_id UUID, source_channel_id UUID,
    source_author_public_key BYTEA, source_evidence_hash BYTEA,
    employee_revision_id UUID, employee_lifecycle_epoch BIGINT,
    observed_at TIMESTAMPTZ, valid_before TIMESTAMPTZ)
LANGUAGE plpgsql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE node RECORD; first_node RECORD; count_nodes INTEGER:=0;
    seen BYTEA[]:=ARRAY[]::bytea[]; expected_parent BYTEA;
    expected_parent_at TIMESTAMPTZ; expected_depth INTEGER;
    expected_root BYTEA; expected_root_at TIMESTAMPTZ;
    resolved_root BYTEA; resolved_root_at TIMESTAMPTZ;
    tag JSONB; part JSONB; marker TEXT; reference_id BYTEA;
    claimed_root BYTEA; claimed_parent BYTEA; effective_depth INTEGER;
    evidence BYTEA;
BEGIN
    IF company IS NULL OR company='00000000-0000-0000-0000-000000000000'::uuid
        OR employee IS NULL OR employee COLLATE "C" !~ '^[a-z0-9][a-z0-9_-]{0,63}$'
        OR octet_length(employee) NOT BETWEEN 1 AND 64
        OR actor IS NULL OR octet_length(actor)<>32
        OR source_id IS NULL OR octet_length(source_id)<>32
        OR public.ortak_employee_memory_timestamp(source_created_at) IS NULL
        OR destination_channel IS NULL
        OR destination_channel='00000000-0000-0000-0000-000000000000'::uuid
        OR memory_kind IS NULL OR memory_kind NOT IN('experience','relationship')
        OR (memory_kind='experience' AND relationship_human IS NOT NULL)
        OR (memory_kind='relationship' AND relationship_human IS DISTINCT FROM actor) THEN RETURN; END IF;

    FOR node IN
      WITH RECURSIVE selection AS MATERIALIZED (
        SELECT ob.community_id,i.channel_id,i.event_created_at,i.event_kind,i.author_pubkey,
            e.active_revision_id,e.lifecycle_epoch,b.public_key AS employee_key,
            b.valid_until AS identity_valid_before,statement_timestamp() AS observed_at
        FROM public.companies co
        JOIN public.office_company_bindings ob ON ob.company_id=co.id
        JOIN public.communities cm ON cm.id=ob.community_id
            AND cm.deletion_state='active' AND cm.deleted_at IS NULL
        JOIN public.employees e ON e.company_id=co.id AND e.id=$2 AND e.status='active'
        JOIN public.employee_revisions r ON r.company_id=e.company_id AND r.employee_id=e.id AND r.id=e.active_revision_id
        JOIN public.employee_office_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id
            AND encode(b.public_key,'hex')=r.manifest#>>'{office,public_key}'
            AND b.signer_ref=r.manifest#>>'{office,signer_ref}' AND b.verified_at IS NOT NULL
            AND b.valid_from<=statement_timestamp()
            AND (b.valid_until IS NULL OR b.valid_until>statement_timestamp())
        JOIN public.office_inbox i ON i.company_id=co.id AND i.event_id=$4
            AND i.event_created_at=$5 AND i.state='decided' AND i.author_pubkey=$3 AND i.event_kind IN(9,40002)
        WHERE co.id=$1 AND co.status='active' AND e.lifecycle_epoch>=0 AND b.public_key<>$3
            AND octet_length(b.public_key)=32
            AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=$3
                AND (u.deactivated_at IS NOT NULL OR u.agent_type IS NOT NULL OR u.agent_owner_pubkey IS NOT NULL))
            AND NOT EXISTS(SELECT 1 FROM public.employee_office_bindings eb WHERE eb.company_id=co.id AND eb.public_key=$3)
            AND NOT EXISTS(SELECT 1 FROM public.channel_members bot WHERE bot.community_id=cm.id AND bot.pubkey=$3 AND bot.role='bot')
            AND NOT EXISTS(SELECT 1 FROM public.users u WHERE u.community_id=cm.id AND u.pubkey=b.public_key
                AND u.deactivated_at IS NOT NULL)
      ), accepted_channels AS MATERIALIZED (
        SELECT ch.id,ch.ttl_deadline
        FROM selection s JOIN public.channels ch ON ch.community_id=s.community_id AND ch.id IN(s.channel_id,$6)
        JOIN public.channel_members human_member ON human_member.community_id=ch.community_id
            AND human_member.channel_id=ch.id AND human_member.pubkey=$3
            AND human_member.removed_at IS NULL AND human_member.role<>'bot'
        JOIN public.channel_members employee_member ON employee_member.community_id=ch.community_id
            AND employee_member.channel_id=ch.id AND employee_member.pubkey=s.employee_key AND employee_member.removed_at IS NULL
        WHERE ch.archived_at IS NULL AND ch.deleted_at IS NULL
            AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>statement_timestamp())
            AND (ch.channel_type='stream' OR (
                ch.channel_type='dm' AND ch.visibility='private'
                -- Same binary sorted retained-pair recipe as direct_channel_on.
                -- Both exact keys already have current rows above; counting ALL
                -- retained rows (including removed) refuses a third/replaced key.
                AND ch.participant_hash=sha256(CASE WHEN $3<s.employee_key
                    THEN $3||s.employee_key ELSE s.employee_key||$3 END)
                AND (SELECT count(*) FROM (SELECT m.pubkey FROM public.channel_members m
                    WHERE m.community_id=ch.community_id AND m.channel_id=ch.id ORDER BY m.pubkey LIMIT 3) retained)=2))
      ), visible AS MATERIALIZED (
        SELECT s.*,least(src.ttl_deadline,dst.ttl_deadline,s.identity_valid_before) AS valid_before
        FROM selection s JOIN accepted_channels src ON src.id=s.channel_id
        JOIN accepted_channels dst ON dst.id=$6
      ), source AS MATERIALIZED (
        SELECT e.id,e.created_at,e.content,e.pubkey,e.kind,e.sig,v.*
        FROM visible v JOIN public.events e ON e.community_id=v.community_id
            AND e.id=$4 AND e.created_at=v.event_created_at
            AND e.channel_id=v.channel_id AND e.kind=v.event_kind AND e.pubkey=v.author_pubkey
        WHERE e.deleted_at IS NULL AND e.kind IN(9,40002) AND e.pubkey=$3
            AND octet_length(e.content)<=65536 AND octet_length(e.pubkey)=32 AND octet_length(e.sig)=64
      ), ancestry AS (
        SELECT 0 AS hop,e.id,e.created_at,
            CASE WHEN octet_length(e.tags::text)<=16384 THEN e.tags END AS tags,
            t.event_id IS NOT NULL AS metadata_present,t.channel_id AS metadata_channel,
            t.parent_event_id,t.parent_event_created_at,t.root_event_id,t.root_event_created_at,t.depth
        FROM source s JOIN public.events e ON e.community_id=s.community_id AND e.id=s.id AND e.created_at=s.created_at
        LEFT JOIN public.thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
        UNION ALL
        SELECT a.hop+1,e.id,e.created_at,
            CASE WHEN octet_length(e.tags::text)<=16384 THEN e.tags END,
            t.event_id IS NOT NULL,t.channel_id,t.parent_event_id,t.parent_event_created_at,
            t.root_event_id,t.root_event_created_at,t.depth
        FROM ancestry a JOIN public.events e ON e.community_id=(SELECT s.community_id FROM source s)
            AND e.id=a.parent_event_id AND e.created_at=a.parent_event_created_at
            AND e.channel_id=(SELECT s.channel_id FROM source s) AND e.deleted_at IS NULL AND e.kind IN(9,40002)
        LEFT JOIN public.thread_metadata t ON t.community_id=e.community_id AND t.event_id=e.id AND t.event_created_at=e.created_at
        WHERE a.hop<32
      )
      SELECT a.*,s.community_id,s.channel_id,s.active_revision_id,s.lifecycle_epoch,s.observed_at,s.valid_before,
        CASE WHEN a.hop=0 THEN s.content END AS source_content,
        CASE WHEN a.hop=0 THEN s.pubkey END AS source_author,
        CASE WHEN a.hop=0 THEN s.sig END AS source_signature,s.kind AS source_kind
      FROM ancestry a CROSS JOIN source s ORDER BY a.hop LIMIT 33
    LOOP
        IF node.hop <> count_nodes OR octet_length(node.id) <> 32
           OR node.id = ANY(seen)
           OR NOT isfinite(node.created_at)
           OR node.created_at < '1970-01-01 00:00:00+00'::timestamptz
           OR node.created_at >= '10000-01-01 00:00:00+00'::timestamptz
           OR node.tags IS NULL OR jsonb_typeof(node.tags) <> 'array' THEN RETURN; END IF;
        seen := array_append(seen,node.id);
        IF count_nodes=0 THEN
            first_node := node;
            IF node.community_id = '00000000-0000-0000-0000-000000000000'::uuid
               OR node.channel_id = '00000000-0000-0000-0000-000000000000'::uuid THEN RETURN; END IF;
        ELSE
            IF expected_parent IS DISTINCT FROM node.id
               OR expected_parent_at IS DISTINCT FROM node.created_at THEN RETURN; END IF;
        END IF;

        -- Vec<Vec<String>> parity: even non-e tags must be arrays of strings.
        claimed_root := NULL; claimed_parent := NULL;
        FOR tag IN SELECT t.value FROM jsonb_array_elements(node.tags) AS t(value) LOOP
            IF jsonb_typeof(tag) <> 'array' THEN RETURN; END IF;
            FOR part IN SELECT t.value FROM jsonb_array_elements(tag) AS t(value) LOOP
                IF jsonb_typeof(part) <> 'string' THEN RETURN; END IF;
            END LOOP;
            IF tag->>0 IS DISTINCT FROM 'e' THEN CONTINUE; END IF;
            IF jsonb_array_length(tag)<4 OR octet_length(tag->>1)<>64
               OR (tag->>1) COLLATE "C" !~ '^[0-9a-fA-F]{64}$' THEN RETURN; END IF;
            reference_id := decode(tag->>1,'hex');
            marker := tag->>3;
            CASE marker
            WHEN 'root' THEN
                IF claimed_root IS NOT NULL THEN RETURN; END IF;
                claimed_root := reference_id;
            WHEN 'reply' THEN
                IF claimed_parent IS NOT NULL THEN RETURN; END IF;
                claimed_parent := reference_id;
            WHEN 'mention' THEN CONTINUE;
            ELSE RETURN;
            END CASE;
        END LOOP;
        IF claimed_root IS NOT NULL AND claimed_parent IS NULL THEN RETURN; END IF;
        claimed_root := coalesce(claimed_root,claimed_parent);

        -- Both locator halves are required, including exact UTC partition time.
        IF (node.parent_event_id IS NULL) <> (node.parent_event_created_at IS NULL)
           OR (node.root_event_id IS NULL) <> (node.root_event_created_at IS NULL) THEN RETURN; END IF;
        IF node.parent_event_id IS NOT NULL AND (octet_length(node.parent_event_id)<>32
           OR NOT isfinite(node.parent_event_created_at)
           OR node.parent_event_created_at<'1970-01-01 00:00:00+00'::timestamptz
           OR node.parent_event_created_at>='10000-01-01 00:00:00+00'::timestamptz) THEN RETURN; END IF;
        IF node.root_event_id IS NOT NULL AND (octet_length(node.root_event_id)<>32
           OR NOT isfinite(node.root_event_created_at)
           OR node.root_event_created_at<'1970-01-01 00:00:00+00'::timestamptz
           OR node.root_event_created_at>='10000-01-01 00:00:00+00'::timestamptz) THEN RETURN; END IF;

        effective_depth := coalesce(node.depth,0);
        IF node.metadata_present THEN
            IF node.metadata_channel IS DISTINCT FROM first_node.channel_id THEN RETURN; END IF;
            IF node.parent_event_id IS NULL AND node.depth=0 AND claimed_parent IS NULL THEN
                IF node.root_event_id IS NOT NULL AND
                   (node.root_event_id IS DISTINCT FROM node.id OR node.root_event_created_at IS DISTINCT FROM node.created_at) THEN RETURN; END IF;
            ELSIF node.parent_event_id IS NOT NULL AND node.root_event_id IS NOT NULL
                  AND node.depth BETWEEN 1 AND 32
                  AND claimed_parent=node.parent_event_id AND claimed_root=node.root_event_id THEN
                NULL;
            ELSE RETURN;
            END IF;
        ELSIF node.parent_event_id IS NOT NULL OR node.root_event_id IS NOT NULL
              OR node.depth IS NOT NULL OR claimed_parent IS NOT NULL THEN RETURN;
        END IF;
        IF count_nodes>0 AND expected_depth IS DISTINCT FROM effective_depth THEN RETURN; END IF;
        IF node.parent_event_id IS NOT NULL THEN
            IF count_nodes=0 THEN
                expected_root := node.root_event_id;
                expected_root_at := node.root_event_created_at;
            ELSIF node.root_event_id IS DISTINCT FROM expected_root
                  OR node.root_event_created_at IS DISTINCT FROM expected_root_at THEN RETURN;
            END IF;
        ELSE
            IF expected_root IS NOT NULL AND (expected_root IS DISTINCT FROM node.id
               OR expected_root_at IS DISTINCT FROM node.created_at) THEN RETURN; END IF;
            resolved_root := node.id; resolved_root_at := node.created_at;
        END IF;
        expected_parent := node.parent_event_id;
        expected_parent_at := node.parent_event_created_at;
        expected_depth := effective_depth-1;
        count_nodes := count_nodes+1;
    END LOOP;
    -- A missing/deleted/cross-channel parent, cycle or 33rd edge cannot become
    -- a top-level fallback. Every nonterminal depth decreases to an actual root.
    IF count_nodes=0 OR expected_parent IS NOT NULL OR resolved_root IS NULL THEN RETURN; END IF;

    -- Exact original source locator, never the resolved ancestry root. The root
    -- above establishes consistency; it is not an employee audience field.
    evidence=public.ortak_employee_memory_evidence_bytes($1,first_node.community_id,
        first_node.channel_id,first_node.id,first_node.created_at,first_node.source_author,
        first_node.source_kind,first_node.source_signature,first_node.tags,first_node.source_content);
    IF evidence IS NULL OR first_node.created_at IS DISTINCT FROM $5
        OR first_node.source_author IS DISTINCT FROM $3 THEN RETURN; END IF;
    community_id=first_node.community_id;
    source_channel_id=first_node.channel_id;
    source_author_public_key=first_node.source_author;
    source_evidence_hash=sha256(evidence);
    employee_revision_id=first_node.active_revision_id;
    employee_lifecycle_epoch=first_node.lifecycle_epoch;
    observed_at=first_node.observed_at;
    valid_before=first_node.valid_before;
    -- Statement time pins one read snapshot; wall time can pass its deadline
    -- during a bounded ancestry walk. The final caller still checks at commit.
    IF valid_before IS NOT NULL AND valid_before<=clock_timestamp() THEN RETURN; END IF;
    RETURN NEXT;
END $$;

CREATE FUNCTION ortak_employee_memory_command_current(
    company UUID, employee TEXT, actor BYTEA, action TEXT
) RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT false
$$;

CREATE FUNCTION ortak_employee_memory_target_authorized(
    company UUID, employee TEXT, deployment UUID, namespace_bytes BYTEA,
    binding JSONB, creation_receipt JSONB, revision UUID, lifecycle BIGINT,
    destination UUID, valid_until TIMESTAMPTZ
) RETURNS BOOLEAN LANGUAGE sql STABLE SECURITY INVOKER PARALLEL RESTRICTED
SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT false
$$;

CREATE TABLE employee_memory_channel_authorities (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    employee_id TEXT NOT NULL,
    channel_id UUID NOT NULL,
    epoch BIGINT NOT NULL DEFAULT 0,
    reason TEXT NOT NULL DEFAULT 'registered',
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,community_id,employee_id,channel_id),
    FOREIGN KEY(company_id,employee_id) REFERENCES employees(company_id,id),
    CONSTRAINT employee_memory_channel_authorities_channel_id_check CHECK (channel_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CONSTRAINT employee_memory_channel_authorities_check CHECK (company_id <> '00000000-0000-0000-0000-000000000000'::uuid AND community_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CONSTRAINT employee_memory_channel_authorities_check1 CHECK (changed_at >= created_at),
    CONSTRAINT employee_memory_channel_authorities_epoch_check CHECK (epoch >= 0),
    CONSTRAINT employee_memory_channel_authorities_reason_check CHECK (reason = ANY (ARRAY['registered'::text, 'source_changed'::text, 'audience_changed'::text, 'identity_changed'::text, 'scope_closed'::text]))
);

CREATE INDEX employee_memory_authority_community
    ON employee_memory_channel_authorities(community_id,channel_id,company_id,employee_id);

CREATE FUNCTION ortak_employee_memory_authority_guard() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (to_jsonb(NEW)-'epoch'-'reason'-'changed_at') IS DISTINCT FROM
            (to_jsonb(OLD)-'epoch'-'reason'-'changed_at') OR OLD.epoch=9223372036854775807
            OR NEW.epoch<>OLD.epoch+1 OR NEW.reason='registered' THEN
            RAISE EXCEPTION 'employee memory authority only advances' USING ERRCODE='check_violation';
        END IF;
        NEW.changed_at=clock_timestamp();
        RETURN NEW;
    END IF;
    PERFORM ortak_lock_office_authority(NEW.company_id);
    IF NEW.epoch<>0 OR NEW.reason<>'registered' OR NOT EXISTS(
        SELECT 1 FROM companies c JOIN office_company_bindings b ON b.company_id=c.id
        JOIN communities cm ON cm.id=b.community_id
        JOIN employees e ON e.company_id=c.id AND e.id=NEW.employee_id
        JOIN channels ch ON ch.community_id=cm.id AND ch.id=NEW.channel_id
        WHERE c.id=NEW.company_id AND cm.id=NEW.community_id AND c.status='active'
            AND cm.deletion_state='active' AND cm.deleted_at IS NULL AND e.status='active'
            AND ch.archived_at IS NULL AND ch.deleted_at IS NULL
            AND (ch.ttl_deadline IS NULL OR ch.ttl_deadline>clock_timestamp())) THEN
        RAISE EXCEPTION 'employee memory scope is not current' USING ERRCODE='check_violation';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-community-registration:'||NEW.community_id::text,0))
        OR NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-company-registration:'||NEW.company_id::text,0)) THEN
        RAISE EXCEPTION 'employee memory registration busy' USING ERRCODE='serialization_failure';
    END IF;
    IF (SELECT count(*) FROM employee_memory_channel_authorities WHERE company_id=NEW.company_id)>=128
        OR (SELECT count(*) FROM employee_memory_channel_authorities WHERE community_id=NEW.community_id)>=256 THEN
        RAISE EXCEPTION 'retained employee memory scope cap reached' USING ERRCODE='program_limit_exceeded';
    END IF;
    NEW.created_at=clock_timestamp(); NEW.changed_at=NEW.created_at;
    RETURN NEW;
END $$;

CREATE TRIGGER employee_memory_authority_guard BEFORE INSERT OR UPDATE
    ON employee_memory_channel_authorities FOR EACH ROW
    EXECUTE FUNCTION ortak_employee_memory_authority_guard();

CREATE FUNCTION ortak_register_employee_memory_authorities(
    company UUID, community UUID, employee TEXT, source_channel UUID, destination_channel UUID
) RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE channel UUID;
BEGIN
    PERFORM ortak_lock_office_authority(company);
    IF current_setting('transaction_isolation')<>'read committed'
        OR company IS NULL OR community IS NULL OR employee IS NULL
        OR source_channel IS NULL OR destination_channel IS NULL THEN
        RAISE EXCEPTION 'employee memory registration requires current scoped transaction'
            USING ERRCODE='invalid_transaction_state';
    END IF;
    IF NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-community-registration:'||community::text,0))
        OR NOT pg_try_advisory_xact_lock(hashtextextended(
        'ortak-employee-memory-company-registration:'||company::text,0)) THEN
        RAISE EXCEPTION 'employee memory registration busy' USING ERRCODE='serialization_failure';
    END IF;
    FOR channel IN SELECT DISTINCT v FROM unnest(ARRAY[source_channel,destination_channel]) v ORDER BY v LOOP
        -- No rebind/reset of retained keys; INSERT guard independently checks caps.
        PERFORM 1 FROM employee_memory_channel_authorities a WHERE a.company_id=company
            AND a.community_id=community AND a.employee_id=employee AND a.channel_id=channel FOR SHARE;
        IF NOT FOUND THEN
            INSERT INTO employee_memory_channel_authorities(company_id,community_id,employee_id,channel_id)
                VALUES(company,community,employee,channel);
        END IF;
    END LOOP;
END $$;

CREATE TABLE employee_reviewed_memory_facts (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    human_public_key BYTEA,
    destination_channel_id UUID NOT NULL,
    source_channel_id UUID NOT NULL,
    source_event_id BYTEA NOT NULL,
    source_event_created_at TIMESTAMPTZ NOT NULL,
    source_author_public_key BYTEA NOT NULL,
    source_evidence_hash BYTEA NOT NULL,
    audience_bytes BYTEA NOT NULL,
    audience_hash BYTEA NOT NULL,
    source_hash BYTEA NOT NULL,
    provenance_bytes BYTEA NOT NULL,
    sharing_hash BYTEA NOT NULL,
    content TEXT NOT NULL,
    content_hash BYTEA NOT NULL,
    approved_by BYTEA NOT NULL,
    approval_id UUID NOT NULL,
    approved_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    expires_at TIMESTAMPTZ NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    revoked_at TIMESTAMPTZ,
    revoked_by BYTEA,
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,community_id,id),
    UNIQUE(company_id,approved_by,approval_id),
    FOREIGN KEY(company_id,employee_id) REFERENCES employees(company_id,id),
    FOREIGN KEY(company_id,community_id,employee_id,source_channel_id)
        REFERENCES employee_memory_channel_authorities(company_id,community_id,employee_id,channel_id),
    FOREIGN KEY(company_id,community_id,employee_id,destination_channel_id)
        REFERENCES employee_memory_channel_authorities(company_id,community_id,employee_id,channel_id),
    CONSTRAINT employee_reviewed_memory_facts_approval_id_check CHECK (approval_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CONSTRAINT employee_reviewed_memory_facts_audience_bytes_check CHECK (octet_length(audience_bytes) >= 1 AND octet_length(audience_bytes) <= 2048),
    CONSTRAINT employee_reviewed_memory_facts_check CHECK (octet_length(audience_hash) = 32 AND audience_hash = sha256(audience_bytes)),
    CONSTRAINT employee_reviewed_memory_facts_check1 CHECK (octet_length(sharing_hash) = 32 AND sharing_hash = sha256(provenance_bytes)),
    CONSTRAINT employee_reviewed_memory_facts_check2 CHECK (content_hash = sha256(convert_to(content, 'UTF8'::name))),
    CONSTRAINT employee_reviewed_memory_facts_check3 CHECK (octet_length(approved_by) = 32 AND approved_by = source_author_public_key),
    CONSTRAINT employee_reviewed_memory_facts_check4 CHECK (kind = 'experience'::text AND human_public_key IS NULL OR kind = 'relationship'::text AND human_public_key IS NOT NULL AND human_public_key = approved_by),
    CONSTRAINT employee_reviewed_memory_facts_check5 CHECK (ortak_employee_memory_timestamp(approved_at) IS NOT NULL AND ortak_employee_memory_timestamp(expires_at) IS NOT NULL),
    CONSTRAINT employee_reviewed_memory_facts_check6 CHECK (expires_at > approved_at AND expires_at <= (approved_at + '2160:00:00'::interval)),
    CONSTRAINT employee_reviewed_memory_facts_check7 CHECK (version = 1 AND revoked_at IS NULL AND revoked_by IS NULL OR version = 2 AND revoked_at IS NOT NULL AND revoked_by IS NOT NULL AND revoked_by = approved_by AND revoked_at >= approved_at),
    CONSTRAINT employee_reviewed_memory_facts_content_check CHECK (octet_length(content) >= 1 AND octet_length(content) <= 4096 AND btrim(content) <> ''::text),
    CONSTRAINT employee_reviewed_memory_facts_human_public_key_check CHECK (octet_length(human_public_key) = 32),
    CONSTRAINT employee_reviewed_memory_facts_id_check CHECK (id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CONSTRAINT employee_reviewed_memory_facts_kind_check CHECK (kind = ANY (ARRAY['experience'::text, 'relationship'::text])),
    CONSTRAINT employee_reviewed_memory_facts_provenance_bytes_check CHECK (octet_length(provenance_bytes) >= 1 AND octet_length(provenance_bytes) <= 4096),
    CONSTRAINT employee_reviewed_memory_facts_revoked_by_check CHECK (octet_length(revoked_by) = 32),
    CONSTRAINT employee_reviewed_memory_facts_source_author_public_key_check CHECK (octet_length(source_author_public_key) = 32),
    CONSTRAINT employee_reviewed_memory_facts_source_event_created_at_check CHECK (ortak_employee_memory_timestamp(source_event_created_at) IS NOT NULL),
    CONSTRAINT employee_reviewed_memory_facts_source_event_id_check CHECK (octet_length(source_event_id) = 32),
    CONSTRAINT employee_reviewed_memory_facts_source_evidence_hash_check CHECK (octet_length(source_evidence_hash) = 32),
    CONSTRAINT employee_reviewed_memory_facts_source_hash_check CHECK (octet_length(source_hash) = 32),
    CONSTRAINT employee_reviewed_memory_facts_version_check CHECK (version = ANY (ARRAY[1, 2]))
);

CREATE INDEX employee_reviewed_memory_list ON employee_reviewed_memory_facts
    (company_id,employee_id,destination_channel_id,id);

CREATE INDEX employee_reviewed_memory_approver_list ON employee_reviewed_memory_facts
    (company_id,employee_id,approved_by,id);

CREATE INDEX employee_reviewed_memory_source ON employee_reviewed_memory_facts
    (community_id,source_event_id,source_event_created_at,company_id,employee_id);

-- Reconciliation creates ortak_employee_memory_audience after the employee_reviewed_memory_facts row type exists.

-- Reconciliation creates ortak_employee_memory_source after the employee_reviewed_memory_facts row type exists.

CREATE FUNCTION ortak_employee_memory_fact_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE audience JSONB; source JSONB; provenance JSONB;
BEGIN
    IF TG_OP='UPDATE' THEN
        IF OLD.version<>1 OR NEW.version<>2 OR NEW.revoked_at IS NULL OR NEW.revoked_by IS DISTINCT FROM OLD.approved_by
            OR (to_jsonb(NEW)-'version'-'revoked_at'-'revoked_by') IS DISTINCT FROM
                (to_jsonb(OLD)-'version'-'revoked_at'-'revoked_by') THEN
            RAISE EXCEPTION 'employee memory fact only permits Stop' USING ERRCODE='check_violation';
        END IF;
        NEW.revoked_at=clock_timestamp(); RETURN NEW;
    END IF;
    IF NEW.version<>1 OR NEW.revoked_at IS NOT NULL OR NEW.revoked_by IS NOT NULL THEN
        RAISE EXCEPTION 'new employee memory fact must be approved' USING ERRCODE='check_violation';
    END IF;
    NEW.approved_at=clock_timestamp();
    audience=ortak_employee_memory_audience(NEW); source=ortak_employee_memory_source(NEW);
    provenance=jsonb_build_object('format','ortak-reviewed-employee-provenance/1',
        'audience',audience,'audience_hash',encode(NEW.audience_hash,'hex'),
        'source',source,'source_hash',encode(NEW.source_hash,'hex'),
        'approval',jsonb_build_object('format','ortak-reviewed-employee-sharing/1',
            'approval_id',NEW.approval_id,'approved_by',encode(NEW.approved_by,'hex'),
            'content_hash',encode(NEW.content_hash,'hex'),
            'expires_at',ortak_employee_memory_timestamp(NEW.expires_at)));
    IF NEW.audience_bytes IS DISTINCT FROM convert_to(ortak_conversation_json75(audience),'UTF8')
        OR NEW.source_hash IS DISTINCT FROM sha256(convert_to(ortak_conversation_json75(
            jsonb_build_object('audience_hash',encode(NEW.audience_hash,'hex'),
                'format','ortak-reviewed-employee-source/1','source',source)),'UTF8'))
        OR NEW.provenance_bytes IS DISTINCT FROM convert_to(ortak_conversation_json75(provenance),'UTF8') THEN
        RAISE EXCEPTION 'employee memory canonical bytes differ' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER employee_memory_fact_guard BEFORE INSERT OR UPDATE ON employee_reviewed_memory_facts
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_fact_guard();

CREATE TABLE employee_reviewed_memory_operations (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    actor_public_key BYTEA NOT NULL,
    operation_id UUID NOT NULL,
    fact_id UUID NOT NULL,
    action TEXT NOT NULL,
    submitted_bytes BYTEA NOT NULL,
    submitted_hash BYTEA NOT NULL,
    result_version INTEGER NOT NULL,
    auth_event_id BYTEA NOT NULL,
    valid_before TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,actor_public_key,operation_id),
    UNIQUE(company_id,fact_id,action),
    FOREIGN KEY(company_id,community_id,fact_id)
        REFERENCES employee_reviewed_memory_facts(company_id,community_id,id) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT employee_reviewed_memory_operations_action_check CHECK (action = ANY (ARRAY['approve'::text, 'stop'::text])),
    CONSTRAINT employee_reviewed_memory_operations_actor_public_key_check CHECK (octet_length(actor_public_key) = 32),
    CONSTRAINT employee_reviewed_memory_operations_auth_event_id_check CHECK (octet_length(auth_event_id) = 32),
    CONSTRAINT employee_reviewed_memory_operations_check CHECK (submitted_hash = sha256(submitted_bytes)),
    CONSTRAINT employee_reviewed_memory_operations_check1 CHECK (action = 'approve'::text AND result_version = 1 OR action = 'stop'::text AND result_version = 2),
    CONSTRAINT employee_reviewed_memory_operations_operation_id_check CHECK (operation_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CONSTRAINT employee_reviewed_memory_operations_submitted_bytes_check CHECK (octet_length(submitted_bytes) >= 1 AND octet_length(submitted_bytes) <= 32768),
    CONSTRAINT employee_reviewed_memory_operations_valid_before_check CHECK (ortak_employee_memory_timestamp(valid_before) IS NOT NULL)
);

ALTER TABLE employee_reviewed_memory_facts ADD CONSTRAINT employee_memory_original_approval
    FOREIGN KEY(company_id,approved_by,approval_id)
    REFERENCES employee_reviewed_memory_operations(company_id,actor_public_key,operation_id)
    DEFERRABLE INITIALLY DEFERRED;

-- Reconciliation creates ortak_employee_memory_submission after the employee_reviewed_memory_facts row type exists.

CREATE FUNCTION ortak_employee_memory_fact_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE CONSTRAINT TRIGGER employee_memory_fact_at_commit AFTER INSERT OR UPDATE
    ON employee_reviewed_memory_facts DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_fact_at_commit();

CREATE FUNCTION ortak_employee_memory_operation_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
        WHERE f.company_id=NEW.company_id AND f.community_id=NEW.community_id AND f.id=NEW.fact_id
            AND f.approved_by=NEW.actor_public_key AND f.version=NEW.result_version
            AND (NEW.action='stop' OR f.approval_id=NEW.operation_id)
            AND NEW.submitted_bytes=ortak_employee_memory_submission(f,NEW.operation_id,NEW.action)
            AND f.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'employee memory receipt lacks its atomic effect' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE CONSTRAINT TRIGGER employee_memory_operation_at_commit AFTER INSERT
    ON employee_reviewed_memory_operations DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_operation_at_commit();

CREATE TABLE employee_reviewed_memory_targets (
    registration_receipt JSONB NOT NULL,
    runtime_consumption_enabled BOOLEAN NOT NULL DEFAULT false,
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    destination_channel_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    deployment_id UUID NOT NULL,
    namespace_bytes BYTEA NOT NULL,
    namespace_hash BYTEA NOT NULL,
    protocol TEXT NOT NULL,
    binding JSONB NOT NULL,
    creation_receipt JSONB NOT NULL,
    binding_hash BYTEA NOT NULL,
    employee_revision_id UUID NOT NULL,
    employee_lifecycle_epoch BIGINT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT false,
    consumption_epoch BIGINT NOT NULL DEFAULT 0,
    valid_until TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,id),
    UNIQUE(company_id,destination_channel_id,employee_id,deployment_id,binding_hash),
    FOREIGN KEY(company_id,community_id,employee_id,destination_channel_id)
        REFERENCES employee_memory_channel_authorities(company_id,community_id,employee_id,channel_id),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id),
    CONSTRAINT employee_reviewed_memory_targets_binding_check CHECK (jsonb_typeof(binding) = 'object'::text AND octet_length(binding::text) <= 8192),
    CONSTRAINT employee_reviewed_memory_targets_binding_hash_check CHECK (octet_length(binding_hash) = 32),
    CONSTRAINT employee_reviewed_memory_targets_check CHECK (namespace_hash = sha256(namespace_bytes)),
    CONSTRAINT employee_reviewed_memory_targets_check1 CHECK (COALESCE((creation_receipt ->> 'company_id'::text) = company_id::text AND (creation_receipt ->> 'employee_id'::text) = employee_id AND (creation_receipt ->> 'deployment_id'::text) = deployment_id::text AND (creation_receipt -> 'binding'::text) = binding AND (creation_receipt ->> 'protocol'::text) = protocol AND (creation_receipt ->> 'namespace_hash'::text) = encode(namespace_hash, 'hex'::text) AND (creation_receipt ->> 'request_hash'::text) ~ '^[0-9a-f]{64}$'::text AND jsonb_typeof(creation_receipt -> 'native_ids'::text) = 'object'::text, false)),
    CONSTRAINT employee_reviewed_memory_targets_consumption_epoch_check CHECK (consumption_epoch >= 0),
    CONSTRAINT employee_reviewed_memory_targets_creation_receipt_check CHECK (jsonb_typeof(creation_receipt) = 'object'::text AND octet_length(creation_receipt::text) <= 16384),
    CONSTRAINT employee_reviewed_memory_targets_deployment_id_check CHECK (deployment_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CONSTRAINT employee_reviewed_memory_targets_employee_lifecycle_epoch_check CHECK (employee_lifecycle_epoch >= 0),
    CONSTRAINT employee_reviewed_memory_targets_id_check CHECK (id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CONSTRAINT employee_reviewed_memory_targets_namespace_bytes_check CHECK (octet_length(namespace_bytes) >= 1 AND octet_length(namespace_bytes) <= 2048),
    CONSTRAINT employee_reviewed_memory_targets_protocol_check CHECK (protocol = 'reviewed-employee/1'::text),
    CONSTRAINT employee_reviewed_memory_targets_registration_receipt_check CHECK (jsonb_typeof(registration_receipt) = 'object'::text AND octet_length(registration_receipt::text) <= 4096),
    CONSTRAINT employee_reviewed_memory_targets_valid_until_check CHECK (ortak_employee_memory_timestamp(valid_until) IS NOT NULL)
);

CREATE TABLE employee_reviewed_memory_exports (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    destination_channel_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    target_id UUID NOT NULL,
    employee_revision_id UUID NOT NULL,
    employee_lifecycle_epoch BIGINT NOT NULL,
    content_hash BYTEA NOT NULL,
    source_hash BYTEA NOT NULL,
    sharing_hash BYTEA NOT NULL,
    requested_by TEXT NOT NULL,
    operation_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,fact_id),
    FOREIGN KEY(company_id,fact_id) REFERENCES employee_reviewed_memory_facts(company_id,id),
    FOREIGN KEY(company_id,community_id,employee_id,destination_channel_id)
        REFERENCES employee_memory_channel_authorities(company_id,community_id,employee_id,channel_id),
    FOREIGN KEY(company_id,target_id) REFERENCES employee_reviewed_memory_targets(company_id,id),
    FOREIGN KEY(company_id,employee_id,employee_revision_id) REFERENCES employee_revisions(company_id,employee_id,id),
    CONSTRAINT employee_reviewed_memory_exports_content_hash_check CHECK (octet_length(content_hash) = 32),
    CONSTRAINT employee_reviewed_memory_exports_employee_lifecycle_epoch_check CHECK (employee_lifecycle_epoch >= 0),
    CONSTRAINT employee_reviewed_memory_exports_operation_id_check CHECK (operation_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CONSTRAINT employee_reviewed_memory_exports_requested_by_check CHECK (requested_by ~ '^[0-9a-f]{64}$'::text),
    CONSTRAINT employee_reviewed_memory_exports_sharing_hash_check CHECK (octet_length(sharing_hash) = 32),
    CONSTRAINT employee_reviewed_memory_exports_source_hash_check CHECK (octet_length(source_hash) = 32)
);

CREATE TABLE employee_reviewed_memory_export_jobs (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    action TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_hash BYTEA NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    total_attempts INTEGER NOT NULL DEFAULT 0,
    retry_version INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL,
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    last_error_code TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,fact_id,action),
    UNIQUE(company_id,idempotency_key),
    FOREIGN KEY(company_id,fact_id) REFERENCES employee_reviewed_memory_exports(company_id,fact_id),
    CONSTRAINT employee_reviewed_memory_export_jobs_action_check CHECK (action = ANY (ARRAY['publish'::text, 'withdraw'::text])),
    CONSTRAINT employee_reviewed_memory_export_jobs_attempt_count_check CHECK (attempt_count >= 0 AND attempt_count <= 20),
    CONSTRAINT employee_reviewed_memory_export_jobs_check CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL)),
    CONSTRAINT employee_reviewed_memory_export_jobs_check1 CHECK (total_attempts >= attempt_count AND total_attempts <= (20 * (retry_version + 1))),
    CONSTRAINT employee_reviewed_memory_export_jobs_check2 CHECK (state <> 'failed'::text OR lease_token IS NULL),
    CONSTRAINT employee_reviewed_memory_export_jobs_idempotency_key_check CHECK (idempotency_key ~ '^[a-z0-9:-]{1,200}$'::text),
    CONSTRAINT employee_reviewed_memory_export_jobs_last_error_code_check CHECK (last_error_code = ANY (ARRAY['authority_refused'::text, 'target_unavailable'::text, 'service_retry'::text, 'service_refused'::text, 'database_retry'::text, 'deadline'::text, 'lease_exhausted'::text])),
    CONSTRAINT employee_reviewed_memory_export_jobs_request_hash_check CHECK (octet_length(request_hash) = 32),
    CONSTRAINT employee_reviewed_memory_export_jobs_retry_version_check CHECK (retry_version >= 0 AND retry_version <= 8),
    CONSTRAINT employee_reviewed_memory_export_jobs_state_check CHECK (state = ANY (ARRAY['pending'::text, 'acknowledged'::text, 'failed'::text])),
    CONSTRAINT employee_reviewed_memory_export_jobs_total_attempts_check CHECK (total_attempts >= 0 AND total_attempts <= 180)
);

CREATE INDEX employee_reviewed_memory_export_due ON employee_reviewed_memory_export_jobs(company_id,next_attempt_at,fact_id,action)
    WHERE state='pending';

CREATE TABLE employee_reviewed_memory_export_commands (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    actor_pubkey TEXT NOT NULL,
    operation_id UUID NOT NULL,
    fact_id UUID NOT NULL,
    action TEXT NOT NULL,
    request_hash BYTEA NOT NULL,
    result_version INTEGER NOT NULL,
    auth_event_id BYTEA NOT NULL,
    valid_before TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,actor_pubkey,operation_id),
    UNIQUE(company_id,fact_id,action,result_version),
    FOREIGN KEY(company_id,fact_id) REFERENCES employee_reviewed_memory_exports(company_id,fact_id) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT employee_reviewed_memory_export_commands_action_check CHECK (action = ANY (ARRAY['publish'::text, 'retry_publish'::text, 'retry_withdraw'::text])),
    CONSTRAINT employee_reviewed_memory_export_commands_actor_pubkey_check CHECK (actor_pubkey ~ '^[0-9a-f]{64}$'::text),
    CONSTRAINT employee_reviewed_memory_export_commands_auth_event_id_check CHECK (octet_length(auth_event_id) = 32),
    CONSTRAINT employee_reviewed_memory_export_commands_check CHECK (action = 'publish'::text AND result_version = 0 OR (action = ANY (ARRAY['retry_publish'::text, 'retry_withdraw'::text])) AND result_version >= 1 AND result_version <= 8),
    CONSTRAINT employee_reviewed_memory_export_commands_operation_id_check CHECK (operation_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CONSTRAINT employee_reviewed_memory_export_commands_request_hash_check CHECK (octet_length(request_hash) = 32),
    CONSTRAINT employee_reviewed_memory_export_commands_valid_before_check CHECK (ortak_employee_memory_timestamp(valid_before) IS NOT NULL)
);

ALTER TABLE employee_reviewed_memory_exports ADD CONSTRAINT employee_reviewed_export_instruction
    FOREIGN KEY(company_id,requested_by,operation_id)
    REFERENCES employee_reviewed_memory_export_commands(company_id,actor_pubkey,operation_id) DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE employee_reviewed_memory_export_receipts (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL REFERENCES communities(id),
    fact_id UUID NOT NULL,
    action TEXT NOT NULL,
    request_hash BYTEA NOT NULL,
    binding_hash BYTEA NOT NULL,
    content_hash BYTEA,
    remote_status TEXT NOT NULL,
    erased_from_reviewed_store BOOLEAN NOT NULL,
    tombstone_at TIMESTAMPTZ,
    lease_token UUID NOT NULL,
    total_attempts INTEGER NOT NULL,
    acknowledged_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,fact_id,action),
    FOREIGN KEY(company_id,fact_id,action) REFERENCES employee_reviewed_memory_export_jobs(company_id,fact_id,action),
    CONSTRAINT employee_reviewed_memory_export_receipts_action_check CHECK (action = ANY (ARRAY['publish'::text, 'withdraw'::text])),
    CONSTRAINT employee_reviewed_memory_export_receipts_binding_hash_check CHECK (octet_length(binding_hash) = 32),
    CONSTRAINT employee_reviewed_memory_export_receipts_check CHECK (erased_from_reviewed_store = (tombstone_at IS NOT NULL)),
    CONSTRAINT employee_reviewed_memory_export_receipts_check2 CHECK (action <> 'withdraw'::text OR erased_from_reviewed_store AND remote_status <> 'active'::text),
    CONSTRAINT employee_reviewed_memory_export_receipts_content_hash_check CHECK (octet_length(content_hash) = 32),
    CONSTRAINT employee_reviewed_memory_export_receipts_remote_status_check CHECK (remote_status = ANY (ARRAY['active'::text, 'expired'::text, 'withdrawn'::text])),
    CONSTRAINT employee_reviewed_memory_export_receipts_request_hash_check CHECK (octet_length(request_hash) = 32),
    CONSTRAINT employee_reviewed_memory_export_receipts_total_attempts_check CHECK (total_attempts >= 1 AND total_attempts <= 180),
    CONSTRAINT employee_reviewed_receipt_erasure_state CHECK ((remote_status = 'withdrawn'::text) = erased_from_reviewed_store)
);

CREATE FUNCTION ortak_employee_memory_target_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE expected_namespace BYTEA; expected_binding BYTEA; registration JSONB; diagnostic JSONB;
    observed TIMESTAMPTZ; cleanup_hash TEXT; recovery_only BOOLEAN=false;
BEGIN
    PERFORM ortak_lock_office_authority(NEW.company_id);
    expected_namespace=convert_to(ortak_conversation_json75(jsonb_build_object(
        'format','ortak-reviewed-employee-namespace/1','company_id',NEW.company_id,'employee_id',NEW.employee_id)),'UTF8');
    expected_binding=sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
        'binding',NEW.binding,'namespace_hash',encode(NEW.namespace_hash,'hex'),'protocol',NEW.protocol)),'UTF8'));
    IF NEW.namespace_bytes IS DISTINCT FROM expected_namespace OR NEW.binding_hash IS DISTINCT FROM expected_binding THEN
        RAISE EXCEPTION 'employee memory target namespace differs' USING ERRCODE='check_violation';
    END IF;
    IF TG_OP='INSERT' THEN
        registration=NEW.registration_receipt; diagnostic=registration->'diagnostic';
        IF jsonb_typeof(registration)<>'object' OR (SELECT count(*) FROM jsonb_object_keys(registration))<>3
            OR registration->>'format' IS DISTINCT FROM 'ortak-employee-namespace-registration/1'
            OR jsonb_typeof(diagnostic)<>'object' OR (SELECT count(*) FROM jsonb_object_keys(diagnostic))<>8
            OR diagnostic->>'operation_id' IS NULL OR diagnostic->>'employee_revision_id' IS DISTINCT FROM NEW.employee_revision_id::text
            OR diagnostic->>'employee_lifecycle_epoch' IS DISTINCT FROM NEW.employee_lifecycle_epoch::text
            OR diagnostic->>'erased' IS DISTINCT FROM 'true'
            OR NOT coalesce(diagnostic->>'challenge_hash' ~ '^[0-9a-f]{64}$',false)
            OR NOT coalesce(diagnostic->>'write_request_hash' ~ '^[0-9a-f]{64}$',false)
            OR NOT coalesce(diagnostic->>'withdraw_request_hash' ~ '^[0-9a-f]{64}$',false)
            OR diagnostic->>'tombstone_at' IS NULL OR registration->>'validated_at' IS NULL THEN
            RAISE EXCEPTION 'employee namespace registration metadata invalid' USING ERRCODE='check_violation';
        END IF;
        observed=(registration->>'validated_at')::timestamptz;
        IF (diagnostic->>'operation_id')::uuid='00000000-0000-0000-0000-000000000000'::uuid
            OR ortak_employee_memory_timestamp(observed) IS DISTINCT FROM registration->>'validated_at'
            OR ortak_employee_memory_timestamp((diagnostic->>'tombstone_at')::timestamptz) IS NULL
            OR observed>clock_timestamp()+interval '5 seconds' OR observed<=clock_timestamp()-interval '55 seconds'
            OR NEW.valid_until<=clock_timestamp() OR NEW.valid_until>observed+interval '90 days'
            OR NEW.consumption_epoch<>0 OR NEW.runtime_consumption_enabled THEN
            RAISE EXCEPTION 'employee namespace initial witness expired or selection invalid' USING ERRCODE='check_violation';
        END IF;
        cleanup_hash=encode(sha256(convert_to(ortak_conversation_json75(jsonb_build_object(
            'format','ortak-reviewed-employee-diagnostic-withdraw/1','operation_id',(diagnostic->>'operation_id')::uuid,
            'namespace_hash',encode(NEW.namespace_hash,'hex'),'binding_hash',encode(NEW.binding_hash,'hex'),
            'employee_revision_id',NEW.employee_revision_id,'employee_lifecycle_epoch',NEW.employee_lifecycle_epoch,
            'challenge_hash',diagnostic->>'challenge_hash')),'UTF8')),'hex');
        IF diagnostic->>'withdraw_request_hash' IS DISTINCT FROM cleanup_hash THEN
            RAISE EXCEPTION 'employee namespace cleanup commitment differs' USING ERRCODE='check_violation';
        END IF;
    ELSE
        recovery_only=OLD.runtime_consumption_enabled AND NOT NEW.runtime_consumption_enabled
            AND (to_jsonb(NEW)-'runtime_consumption_enabled'-'updated_at')=(to_jsonb(OLD)-'runtime_consumption_enabled'-'updated_at');
        -- Includes registration receipt and original selection expiry. A model
        -- refresh cannot create ownership, renew an expired selection or rewrite
        -- the original I/O evidence. Explicit future renewal is a separate API.
        IF (to_jsonb(NEW)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'runtime_consumption_enabled'-'updated_at'-'consumption_epoch')
            IS DISTINCT FROM (to_jsonb(OLD)-'employee_revision_id'-'employee_lifecycle_epoch'-'enabled'-'runtime_consumption_enabled'-'updated_at'-'consumption_epoch')
            OR NEW.consumption_epoch<>OLD.consumption_epoch THEN
            RAISE EXCEPTION 'employee memory target identity is immutable' USING ERRCODE='check_violation';
        END IF;
        IF (NEW.enabled,NEW.runtime_consumption_enabled,NEW.employee_lifecycle_epoch) IS DISTINCT FROM (OLD.enabled,OLD.runtime_consumption_enabled,OLD.employee_lifecycle_epoch) THEN
            IF OLD.consumption_epoch=9223372036854775807 THEN
                RAISE EXCEPTION 'employee memory target epoch exhausted' USING ERRCODE='program_limit_exceeded';
            END IF;
            NEW.consumption_epoch=OLD.consumption_epoch+1;
        END IF;
    END IF;
    IF NOT recovery_only AND (TG_OP='INSERT' OR NEW.enabled) AND NOT coalesce(ortak_employee_memory_target_authorized(
        NEW.company_id,NEW.employee_id,NEW.deployment_id,NEW.namespace_bytes,NEW.binding,NEW.creation_receipt,
        NEW.employee_revision_id,NEW.employee_lifecycle_epoch,NEW.destination_channel_id,NEW.valid_until),false) THEN
        RAISE EXCEPTION 'employee namespace current binding unavailable' USING ERRCODE='check_violation';
    END IF;
    NEW.updated_at=clock_timestamp(); RETURN NEW;
END $$;

CREATE TRIGGER employee_memory_target_guard BEFORE INSERT OR UPDATE ON employee_reviewed_memory_targets
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_target_guard();

CREATE FUNCTION ortak_employee_reviewed_export_eligible(company UUID, fact UUID, target UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT false
$$;

CREATE FUNCTION ortak_employee_reviewed_request_hash(company UUID, fact UUID, action TEXT)
RETURNS BYTEA LANGUAGE sql STABLE AS $$
    SELECT NULL::bytea
$$;

CREATE FUNCTION ortak_employee_reviewed_export_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE CONSTRAINT TRIGGER employee_reviewed_export_at_commit AFTER INSERT ON employee_reviewed_memory_exports
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_at_commit();

CREATE FUNCTION ortak_employee_reviewed_export_stop() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    UPDATE employee_reviewed_memory_export_jobs SET next_attempt_at=least(next_attempt_at,NEW.revoked_at),updated_at=clock_timestamp()
        WHERE company_id=NEW.company_id AND fact_id=NEW.id AND action='withdraw' AND state='pending';
    RETURN NEW;
END $$;

CREATE TRIGGER employee_reviewed_export_stop AFTER UPDATE ON employee_reviewed_memory_facts FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_stop();

CREATE FUNCTION ortak_employee_reviewed_export_job_guard() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE allowed BOOLEAN:=false;
BEGIN
    IF (NEW.company_id,NEW.community_id,NEW.fact_id,NEW.action,NEW.idempotency_key,NEW.request_hash)
        IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.fact_id,OLD.action,OLD.idempotency_key,OLD.request_hash)
        OR OLD.state='acknowledged' OR NEW.total_attempts<OLD.total_attempts OR NEW.total_attempts>OLD.total_attempts+1
        OR NEW.retry_version<OLD.retry_version OR NEW.retry_version>OLD.retry_version+1 THEN
        RAISE EXCEPTION 'ortak: reviewed job identity and progress are retained' USING ERRCODE='check_violation';
    END IF;
    IF NEW.retry_version=OLD.retry_version+1 THEN
        allowed:=OLD.state='failed' AND OLD.lease_token IS NULL AND NEW.state='pending' AND NEW.attempt_count=0
            AND NEW.total_attempts=OLD.total_attempts AND NEW.lease_token IS NULL AND NEW.last_error_code IS NULL
            AND NEW.next_attempt_at<=clock_timestamp();
    ELSIF NEW.attempt_count=OLD.attempt_count+1 AND NEW.total_attempts=OLD.total_attempts+1 THEN
        allowed:=OLD.state='pending' AND NEW.state='pending' AND OLD.next_attempt_at<=clock_timestamp()
            AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at<=clock_timestamp())
            AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token
            AND NEW.lease_expires_at>clock_timestamp() AND NEW.lease_expires_at<=clock_timestamp()+INTERVAL '60 seconds'
            AND NEW.next_attempt_at=OLD.next_attempt_at AND NEW.last_error_code IS NOT DISTINCT FROM OLD.last_error_code;
    ELSIF NEW.attempt_count=OLD.attempt_count AND NEW.total_attempts=OLD.total_attempts AND OLD.state='pending' THEN
        IF NEW.state='acknowledged' THEN
            allowed:=OLD.lease_token IS NOT NULL AND OLD.lease_expires_at>clock_timestamp()
                AND NEW.lease_token=OLD.lease_token AND NEW.lease_expires_at=OLD.lease_expires_at
                AND NEW.next_attempt_at=OLD.next_attempt_at AND NEW.last_error_code IS NULL;
        ELSIF NEW.state='failed' AND NEW.last_error_code='lease_exhausted' THEN
            allowed:=OLD.attempt_count=20 AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at<=clock_timestamp())
                AND NEW.lease_token IS NULL AND NEW.next_attempt_at=OLD.next_attempt_at;
        ELSIF NEW.state='pending' AND NEW.action='withdraw' AND NEW.next_attempt_at<=OLD.next_attempt_at THEN
            allowed:=(NEW.lease_token,NEW.lease_expires_at,NEW.last_error_code)
                IS NOT DISTINCT FROM (OLD.lease_token,OLD.lease_expires_at,OLD.last_error_code)
                AND EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f WHERE f.company_id=NEW.company_id AND f.id=NEW.fact_id
                    AND f.revoked_at IS NOT NULL AND NEW.next_attempt_at=least(OLD.next_attempt_at,f.revoked_at)
                    AND f.xmin::text::bigint=txid_current()%4294967296);
            IF NOT coalesce(allowed,false) THEN
                allowed:=OLD.attempt_count=0 AND OLD.lease_token IS NULL
                    AND NEW.lease_token IS NULL AND NEW.last_error_code IS NOT DISTINCT FROM OLD.last_error_code
                    AND NEW.next_attempt_at<=clock_timestamp()
                    AND EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x
                        WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id
                        AND NOT ortak_employee_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id));
            END IF;
        ELSIF NEW.lease_token IS NULL AND NEW.last_error_code IS NOT NULL THEN
            allowed:=OLD.lease_token IS NOT NULL AND OLD.lease_expires_at>clock_timestamp()
                AND NEW.next_attempt_at>clock_timestamp() AND NEW.next_attempt_at<=clock_timestamp()+INTERVAL '301 seconds'
                AND (NEW.state='failed' OR NEW.state='pending' AND OLD.attempt_count<20);
        END IF;
    END IF;
    IF NOT coalesce(allowed,false) THEN
        RAISE EXCEPTION 'ortak: reviewed job transition lacks a due claim, live lease, stop or audited retry' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER employee_reviewed_export_job_guard BEFORE UPDATE ON employee_reviewed_memory_export_jobs FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_job_guard();

CREATE FUNCTION ortak_employee_reviewed_export_job_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x JOIN employee_reviewed_memory_facts f ON f.company_id=x.company_id AND f.id=x.fact_id
            WHERE x.company_id=NEW.company_id AND x.fact_id=NEW.fact_id AND x.community_id=NEW.community_id
            AND x.xmin::text::bigint=txid_current()%4294967296 AND NEW.state='pending' AND NEW.attempt_count=0
            AND NEW.total_attempts=0 AND NEW.retry_version=0 AND NEW.last_error_code IS NULL
            AND NEW.idempotency_key='employee-reviewed:'||NEW.action||':'||NEW.company_id::text||':'||NEW.fact_id::text
            AND NEW.request_hash=ortak_employee_reviewed_request_hash(NEW.company_id,NEW.fact_id,NEW.action)
            AND NEW.lease_token IS NULL AND ((NEW.action='withdraw' AND NEW.next_attempt_at=f.expires_at)
                OR (NEW.action='publish' AND NEW.next_attempt_at<=clock_timestamp()))) THEN
            RAISE EXCEPTION 'ortak: reviewed job requires atomic publication' USING ERRCODE='check_violation';
        END IF;
    ELSIF NEW.retry_version<>OLD.retry_version THEN
        IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_commands o WHERE o.company_id=NEW.company_id AND o.fact_id=NEW.fact_id
            AND o.action='retry_'||NEW.action AND o.result_version=NEW.retry_version AND o.xmin::text::bigint=txid_current()%4294967296) THEN
            RAISE EXCEPTION 'ortak: reviewed retry requires atomic human command' USING ERRCODE='check_violation';
        END IF;
    END IF;
    IF NEW.state='acknowledged' AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_receipts r
        WHERE r.company_id=NEW.company_id AND r.fact_id=NEW.fact_id AND r.action=NEW.action AND r.request_hash=NEW.request_hash
          AND r.community_id=NEW.community_id AND r.lease_token=NEW.lease_token AND r.total_attempts=NEW.total_attempts AND NEW.lease_expires_at>clock_timestamp()
          AND r.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed acknowledgement requires atomic live-lease receipt' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE CONSTRAINT TRIGGER employee_reviewed_export_job_at_commit AFTER INSERT OR UPDATE ON employee_reviewed_memory_export_jobs DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_job_at_commit();

CREATE FUNCTION ortak_employee_reviewed_export_command_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE CONSTRAINT TRIGGER employee_reviewed_export_command_at_commit AFTER INSERT ON employee_reviewed_memory_export_commands DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_command_at_commit();

CREATE FUNCTION ortak_employee_reviewed_export_receipt_at_commit() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_jobs j
        JOIN employee_reviewed_memory_exports x ON x.company_id=j.company_id AND x.fact_id=j.fact_id
        JOIN employee_reviewed_memory_targets t ON t.company_id=x.company_id AND t.id=x.target_id
        WHERE j.company_id=NEW.company_id AND j.fact_id=NEW.fact_id AND j.action=NEW.action AND j.community_id=NEW.community_id
        AND j.state='acknowledged' AND j.request_hash=NEW.request_hash AND t.binding_hash=NEW.binding_hash
        AND (NEW.content_hash=x.content_hash OR NEW.content_hash IS NULL AND NEW.action='withdraw'
            AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_export_receipts p
                WHERE p.company_id=NEW.company_id AND p.fact_id=NEW.fact_id AND p.action='publish'))
        AND j.lease_token=NEW.lease_token AND j.total_attempts=NEW.total_attempts AND j.lease_expires_at>clock_timestamp()
        AND j.xmin::text::bigint=txid_current()%4294967296) THEN
        RAISE EXCEPTION 'ortak: reviewed receipt requires its exact live job' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE CONSTRAINT TRIGGER employee_reviewed_export_receipt_at_commit AFTER INSERT ON employee_reviewed_memory_export_receipts DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_reviewed_export_receipt_at_commit();

CREATE FUNCTION ortak_employee_memory_schedule_cleanup(company UUID, fact UUID)
RETURNS BOOLEAN LANGUAGE plpgsql AS $$
DECLARE affected INTEGER;
BEGIN
    PERFORM ortak_lock_office_authority(company);
    UPDATE employee_reviewed_memory_export_jobs j SET next_attempt_at=clock_timestamp(),updated_at=clock_timestamp()
        WHERE j.company_id=company AND j.fact_id=fact AND j.action='withdraw'
            AND j.state='pending' AND j.attempt_count=0 AND j.lease_token IS NULL
            AND j.next_attempt_at>clock_timestamp()
            AND EXISTS(SELECT 1 FROM employee_reviewed_memory_exports x WHERE x.company_id=j.company_id
                AND x.fact_id=j.fact_id AND NOT ortak_employee_reviewed_export_eligible(x.company_id,x.fact_id,x.target_id));
    GET DIAGNOSTICS affected=ROW_COUNT;
    RETURN affected=1;
END $$;

CREATE FUNCTION ortak_employee_memory_epoch_mutation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE prior JSONB; proposed JSONB; kind TEXT:=TG_ARGV[0]; reason TEXT:=TG_ARGV[1];
    changed BOOLEAN:=TG_OP<>'UPDATE'; field TEXT; co UUID[]; cm UUID[];
    channels UUID[]; employee_keys TEXT[]; target UUID; keys JSONB; selected JSONB;
    old_identity JSONB; new_identity JSONB;
BEGIN
    IF TG_OP<>'INSERT' THEN prior=to_jsonb(OLD); END IF;
    IF TG_OP<>'DELETE' THEN proposed=to_jsonb(NEW); END IF;
    -- Only plaintext Office events can be a canonical employee-memory source
    -- or ancestor. Native NIP-RS (30078) replaces its old encrypted read-state
    -- payload by deletion; a NULL channel there is not a company-wide source
    -- revocation. Check BOTH sides so changing into or out of 9/40002 still
    -- retires the old use. The existing Office mutation fence remains intact.
    IF kind='event' AND NOT (
        coalesce((prior->>'kind')::integer IN (9,40002),false)
        OR coalesce((proposed->>'kind')::integer IN (9,40002),false)
    ) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF TG_OP='UPDATE' THEN
        FOREACH field IN ARRAY TG_ARGV[2:TG_NARGS-1] LOOP
            IF prior->field IS DISTINCT FROM proposed->field THEN changed=true; EXIT; END IF;
        END LOOP;
        IF kind='office_identity' THEN
            changed=changed OR ((prior->>'verified_at' IS NULL)<>(proposed->>'verified_at' IS NULL));
        END IF;
        IF kind='memory_identity' THEN
            changed=changed OR ((prior->>'validated_at' IS NULL)<>(proposed->>'validated_at' IS NULL));
        END IF;
        IF NOT changed THEN RETURN NEW; END IF;
    END IF;
    IF kind='community' AND coalesce(prior->>'deletion_state','')<>'active' THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='thread' AND TG_OP='INSERT' THEN
        IF ortak_conversation_thread_insert_neutral75(proposed) THEN RETURN NEW; END IF;
        -- A new unrelated reply cannot revoke a running memory consumer while
        -- that consumer is delivering it. Restoration of a referenced anchor
        -- is different, and the existing parent/root indexes bound that lookup.
        IF NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
            WHERE f.community_id=(proposed->>'community_id')::uuid
                AND f.source_event_id=(proposed->>'event_id')::bytea
                AND f.source_event_created_at=(proposed->>'event_created_at')::timestamptz)
            AND NOT EXISTS(SELECT 1 FROM thread_metadata t
                WHERE t.community_id=(proposed->>'community_id')::uuid
                    AND (t.event_id,t.event_created_at) IS DISTINCT FROM
                        ((proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz)
                    AND ((t.parent_event_id=(proposed->>'event_id')::bytea
                        AND t.parent_event_created_at=(proposed->>'event_created_at')::timestamptz)
                        OR (t.root_event_id=(proposed->>'event_id')::bytea
                        AND t.root_event_created_at=(proposed->>'event_created_at')::timestamptz))) THEN
            RETURN NEW;
        END IF;
    END IF;
    IF kind='inbox' AND coalesce(prior->>'state','')<>'decided'
        AND NOT EXISTS(SELECT 1 FROM employee_reviewed_memory_facts f
            WHERE (f.company_id,f.source_event_id,f.source_event_created_at) IN(
                ((prior->>'company_id')::uuid,(prior->>'event_id')::bytea,(prior->>'event_created_at')::timestamptz),
                ((proposed->>'company_id')::uuid,(proposed->>'event_id')::bytea,(proposed->>'event_created_at')::timestamptz))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='user' AND TG_OP IN('INSERT','DELETE')
        AND coalesce(proposed,prior)->>'agent_type' IS NULL
        AND coalesce(proposed,prior)->>'agent_owner_pubkey' IS NULL
        AND coalesce(proposed,prior)->>'deactivated_at' IS NULL THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    IF kind='employee' AND TG_OP='UPDATE'
        AND (prior->'company_id',prior->'id',prior->'status',prior->'lifecycle_epoch')
            IS NOT DISTINCT FROM
            (proposed->'company_id',proposed->'id',proposed->'status',proposed->'lifecycle_epoch') THEN
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO old_identity
            FROM employee_revisions r WHERE r.company_id=(prior->>'company_id')::uuid
                AND r.employee_id=prior->>'id' AND r.id=(prior->>'active_revision_id')::uuid;
        SELECT jsonb_build_array(r.manifest->'office',r.manifest->'memory') INTO new_identity
            FROM employee_revisions r WHERE r.company_id=(proposed->>'company_id')::uuid
                AND r.employee_id=proposed->>'id' AND r.id=(proposed->>'active_revision_id')::uuid;
        IF old_identity IS NOT NULL AND old_identity IS NOT DISTINCT FROM new_identity THEN RETURN NEW; END IF;
    END IF;
    IF kind='memory_identity' AND NOT EXISTS(SELECT 1 FROM employees e
        WHERE (e.company_id,e.id,e.active_revision_id) IN(
            ((prior->>'company_id')::uuid,prior->>'employee_id',(prior->>'revision_id')::uuid),
            ((proposed->>'company_id')::uuid,proposed->>'employee_id',(proposed->>'revision_id')::uuid))) THEN
        RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
    END IF;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO co FROM (VALUES
        (prior->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END),
        (proposed->>CASE WHEN kind='company' THEN 'id' ELSE 'company_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO cm FROM (VALUES
        (prior->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END),
        (proposed->>CASE WHEN kind='community' THEN 'id' ELSE 'community_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v::uuid),ARRAY[]::uuid[]) INTO channels FROM (VALUES
        (prior->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END),
        (proposed->>CASE WHEN kind='channel' THEN 'id' ELSE 'channel_id' END)) t(v) WHERE v IS NOT NULL;
    SELECT coalesce(array_agg(DISTINCT v),ARRAY[]::text[]) INTO employee_keys FROM (VALUES
        (prior->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END),
        (proposed->>CASE WHEN kind='employee' THEN 'id' ELSE 'employee_id' END)) t(v) WHERE v IS NOT NULL;
    IF kind='office_identity' THEN
        -- A new/removed employee key also changes the community-wide human
        -- classification of that key in other employees' approved sources.
        -- Retire bounded company scopes, not only the binding's employee.
        employee_keys=ARRAY[]::text[];
    END IF;
    IF kind='membership' AND (prior->>'role'='bot' OR proposed->>'role'='bot') THEN
        channels=ARRAY[]::uuid[];
    END IF;
    IF current_setting('transaction_isolation')<>'read committed' THEN
        RAISE EXCEPTION 'employee memory authority requires READ COMMITTED' USING ERRCODE='invalid_transaction_state';
    END IF;
    -- Do not rely only on currently visible retained rows: a first registration
    -- may be in flight. These exclusive try-locks conflict with that shared read.
    FOR target IN SELECT unnest(cm) ORDER BY 1 LOOP
        IF NOT pg_try_advisory_xact_lock(ortak_office_community_lock_key(target)) THEN
            RAISE EXCEPTION 'employee memory community fence busy' USING ERRCODE='serialization_failure';
        END IF;
    END LOOP;
    FOR target IN SELECT unnest(co) ORDER BY 1 LOOP
        IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
            RAISE EXCEPTION 'employee memory company fence busy' USING ERRCODE='serialization_failure';
        END IF;
    END LOOP;
    SELECT coalesce(jsonb_agg(to_jsonb(k) ORDER BY company_id,community_id,employee_id,channel_id),'[]'::jsonb)
        INTO keys FROM (
            SELECT a.company_id,a.community_id,a.employee_id,a.channel_id
            FROM employee_memory_channel_authorities a JOIN communities c ON c.id=a.community_id
            WHERE (a.company_id=ANY(co) OR a.community_id=ANY(cm))
                AND (cardinality(channels)=0 OR a.channel_id=ANY(channels))
                AND (cardinality(employee_keys)=0 OR a.employee_id=ANY(employee_keys))
                AND c.deletion_state='active' AND c.deleted_at IS NULL
            ORDER BY a.company_id,a.community_id,a.employee_id,a.channel_id LIMIT 769
        ) k;
    IF jsonb_array_length(keys)>768 THEN
        RAISE EXCEPTION 'employee memory mutation scope cap exceeded' USING ERRCODE='program_limit_exceeded';
    END IF;
    FOR target IN SELECT DISTINCT (v->>'company_id')::uuid FROM jsonb_array_elements(keys) v ORDER BY 1 LOOP
        IF NOT pg_try_advisory_xact_lock(ortak_office_company_lock_key(target)) THEN
            RAISE EXCEPTION 'retained employee memory company fence busy' USING ERRCODE='serialization_failure';
        END IF;
    END LOOP;
    FOR selected IN SELECT value FROM jsonb_array_elements(keys) LOOP
        PERFORM 1 FROM employee_memory_channel_authorities a
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.community_id=(selected->>'community_id')::uuid
                AND a.employee_id=selected->>'employee_id' AND a.channel_id=(selected->>'channel_id')::uuid
            FOR UPDATE NOWAIT;
        UPDATE employee_memory_channel_authorities a SET epoch=epoch+1,reason=TG_ARGV[1]
            WHERE a.company_id=(selected->>'company_id')::uuid AND a.community_id=(selected->>'community_id')::uuid
                AND a.employee_id=selected->>'employee_id' AND a.channel_id=(selected->>'channel_id')::uuid;
    END LOOP;
    RETURN CASE WHEN TG_OP='DELETE' THEN OLD ELSE NEW END;
END $$;

CREATE TRIGGER employee_memory_epoch_channels AFTER INSERT OR UPDATE OR DELETE ON channels
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('channel','audience_changed',
        'community_id','id','channel_type','visibility','archived_at','deleted_at','participant_hash','ttl_seconds','ttl_deadline');

CREATE TRIGGER employee_memory_epoch_members AFTER INSERT OR UPDATE OR DELETE ON channel_members
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('membership','audience_changed',
        'community_id','channel_id','pubkey','role','removed_at');

CREATE TRIGGER employee_memory_epoch_events AFTER UPDATE OR DELETE ON events
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('event','source_changed',
        'community_id','id','created_at','pubkey','kind','tags','content','sig','channel_id','deleted_at');

CREATE TRIGGER employee_memory_epoch_threads AFTER INSERT OR UPDATE OR DELETE ON thread_metadata
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('thread','source_changed',
        'community_id','event_id','event_created_at','channel_id','parent_event_id','parent_event_created_at',
        'root_event_id','root_event_created_at','depth');

CREATE TRIGGER employee_memory_epoch_inbox AFTER UPDATE OR DELETE ON office_inbox
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('inbox','source_changed',
        'company_id','event_id','event_created_at','event_kind','author_pubkey','channel_id','state');

CREATE TRIGGER employee_memory_epoch_users AFTER INSERT OR UPDATE OR DELETE ON users
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('user','identity_changed',
        'community_id','pubkey','agent_type','agent_owner_pubkey','deactivated_at');

CREATE TRIGGER employee_memory_epoch_employees AFTER INSERT OR UPDATE OR DELETE ON employees
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('employee','identity_changed',
        'company_id','id','status','active_revision_id','lifecycle_epoch');

CREATE TRIGGER employee_memory_epoch_office_identity AFTER INSERT OR UPDATE OR DELETE ON employee_office_bindings
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('office_identity','identity_changed',
        'company_id','employee_id','public_key','signer_ref','valid_from','valid_until');

CREATE TRIGGER employee_memory_epoch_memory_identity AFTER INSERT OR UPDATE OR DELETE ON employee_memory_bindings
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('memory_identity','identity_changed',
        'company_id','employee_id','revision_id','adapter','endpoint_ref','workspace','user_peer','employee_peer','options');

CREATE TRIGGER employee_memory_epoch_companies AFTER UPDATE OR DELETE ON companies
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('company','scope_closed','id','status');

CREATE TRIGGER ortak_z_employee_memory_epoch_communities BEFORE UPDATE OR DELETE ON communities
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('community','scope_closed',
        'id','deletion_state','deletion_fence_generation','deleted_at');

CREATE TRIGGER employee_memory_epoch_company_bindings AFTER INSERT OR UPDATE OR DELETE ON office_company_bindings
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_memory_epoch_mutation('company_binding','scope_closed','company_id','community_id');

CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_memory_channel_authorities FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_memory_channel_authorities FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

SELECT attach_community_write_fence('employee_memory_channel_authorities');

CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_facts FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_facts FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

SELECT attach_community_write_fence('employee_reviewed_memory_facts');

CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_operations FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_operations FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

SELECT attach_community_write_fence('employee_reviewed_memory_operations');

CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_targets FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_targets FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

SELECT attach_community_write_fence('employee_reviewed_memory_targets');

CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_exports FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_exports FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

SELECT attach_community_write_fence('employee_reviewed_memory_exports');

CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_export_jobs FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_export_jobs FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

SELECT attach_community_write_fence('employee_reviewed_memory_export_jobs');

CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_export_commands FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_export_commands FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

SELECT attach_community_write_fence('employee_reviewed_memory_export_commands');

CREATE TRIGGER employee_memory_no_delete BEFORE DELETE ON employee_reviewed_memory_export_receipts FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_no_truncate BEFORE TRUNCATE ON employee_reviewed_memory_export_receipts FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

SELECT attach_community_write_fence('employee_reviewed_memory_export_receipts');

CREATE TRIGGER employee_memory_immutable BEFORE UPDATE ON employee_reviewed_memory_operations FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_immutable BEFORE UPDATE ON employee_reviewed_memory_exports FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_immutable BEFORE UPDATE ON employee_reviewed_memory_export_commands FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_immutable BEFORE UPDATE ON employee_reviewed_memory_export_receipts FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE FUNCTION ortak_employee_memory_evidence_bytes(
    company UUID, community UUID, channel UUID, event_id BYTEA,
    event_created_at TIMESTAMPTZ, author BYTEA, event_kind INTEGER,
    signature BYTEA, tags JSONB, content TEXT
) RETURNS BYTEA LANGUAGE plpgsql IMMUTABLE SECURITY INVOKER PARALLEL SAFE
SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE tag JSONB; part JSONB; encoded TEXT;
BEGIN
    IF company IS NULL OR community IS NULL OR channel IS NULL
        OR company='00000000-0000-0000-0000-000000000000'::uuid
        OR community='00000000-0000-0000-0000-000000000000'::uuid
        OR channel='00000000-0000-0000-0000-000000000000'::uuid
        OR event_id IS NULL OR octet_length(event_id)<>32
        OR public.ortak_employee_memory_timestamp(event_created_at) IS NULL
        OR author IS NULL OR octet_length(author)<>32
        OR event_kind IS NULL OR event_kind NOT IN(9,40002)
        OR signature IS NULL OR octet_length(signature)<>64
        OR tags IS NULL OR jsonb_typeof(tags)<>'array' OR octet_length(tags::text)>16384
        OR content IS NULL OR octet_length(content)>65536 THEN RETURN NULL; END IF;
    FOR tag IN SELECT value FROM jsonb_array_elements(tags) LOOP
        IF jsonb_typeof(tag)<>'array' THEN RETURN NULL; END IF;
        FOR part IN SELECT value FROM jsonb_array_elements(tag) LOOP
            IF jsonb_typeof(part)<>'string' THEN RETURN NULL; END IF;
        END LOOP;
    END LOOP;
    encoded=public.ortak_conversation_json75(jsonb_build_object(
        'author_public_key',encode(author,'hex'),'channel_id',channel,
        'community_id',community,'company_id',company,'content',content,
        'event_created_at',public.ortak_employee_memory_timestamp(event_created_at),
        'event_id',encode(event_id,'hex'),'format','ortak-reviewed-employee-evidence/1',
        'kind',event_kind,'sig',encode(signature,'hex'),'tags',tags));
    IF encoded IS NULL OR octet_length(encoded)>524288 THEN RETURN NULL; END IF;
    RETURN convert_to(encoded,'UTF8');
END $$;

CREATE TABLE encrypted_dm_selections (
    company_id UUID NOT NULL REFERENCES companies(id),
    selection_id UUID NOT NULL,
    community_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    human_public_key BYTEA NOT NULL,
    employee_public_key BYTEA NOT NULL,
    office_binding_id UUID NOT NULL,
    key_version BIGINT NOT NULL,
    decrypt_ref TEXT NOT NULL,
    purpose TEXT NOT NULL DEFAULT 'dm_decrypt',
    enabled BOOLEAN NOT NULL DEFAULT false,
    generation BIGINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    enabled_at TIMESTAMPTZ,
    PRIMARY KEY(company_id,selection_id),
    CONSTRAINT encrypted_dm_selections_check CHECK (human_public_key <> employee_public_key),
    CONSTRAINT encrypted_dm_selections_check1 CHECK (NOT enabled OR enabled_at IS NOT NULL),
    CONSTRAINT encrypted_dm_selections_decrypt_ref_check CHECK (ortak_is_credential_ref(decrypt_ref)),
    CONSTRAINT encrypted_dm_selections_employee_public_key_check CHECK (octet_length(employee_public_key) = 32),
    CONSTRAINT encrypted_dm_selections_generation_check CHECK (generation > 0),
    CONSTRAINT encrypted_dm_selections_human_public_key_check CHECK (octet_length(human_public_key) = 32),
    CONSTRAINT encrypted_dm_selections_key_version_check CHECK (key_version >= 0),
    CONSTRAINT encrypted_dm_selections_purpose_check CHECK (purpose = 'dm_decrypt'::text),
    CONSTRAINT encrypted_dm_selections_selection_id_check CHECK (selection_id <> '00000000-0000-0000-0000-000000000000'::uuid)
);

CREATE UNIQUE INDEX encrypted_dm_one_enabled_pair
 ON encrypted_dm_selections(company_id,employee_id) WHERE enabled;

SELECT attach_community_write_fence('encrypted_dm_selections');

-- Reconciliation creates ortak_encrypted_dm_pair_current after the encrypted_dm_selections row type exists.

CREATE FUNCTION ortak_encrypted_dm_selection_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF TG_OP='DELETE' THEN
  RAISE EXCEPTION 'Encrypted DM selection is retained' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' THEN
  IF (to_jsonb(NEW)-ARRAY['enabled','generation','changed_at','enabled_at']) IS DISTINCT FROM
     (to_jsonb(OLD)-ARRAY['enabled','generation','changed_at','enabled_at'])
     OR NEW.generation<>OLD.generation OR NEW.changed_at<>OLD.changed_at
     OR NEW.enabled_at IS DISTINCT FROM OLD.enabled_at THEN
   RAISE EXCEPTION 'Encrypted DM selection identity is immutable' USING ERRCODE='check_violation';
  END IF;
  IF NEW.enabled=OLD.enabled THEN RETURN OLD; END IF;
 END IF;
 -- Config changes are Office mutations. Try-lock fails rather than upgrading
 -- across another signed reader; no caller holds this fence through crypto.
 PERFORM public.ortak_advance_office_authority(NEW.company_id,'encrypted_dm_selections');
 PERFORM public.ortak_lock_office_authority(NEW.company_id);
 IF TG_OP='INSERT' THEN
  IF NEW.generation<>1 OR (SELECT count(*) FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id)>=128 THEN
   RAISE EXCEPTION 'Encrypted DM retained selection bound' USING ERRCODE='check_violation';
  END IF;
  NEW.created_at:=clock_timestamp();
 ELSE NEW.generation:=OLD.generation+1;
 END IF;
 IF (TG_OP='INSERT' OR NEW.enabled) AND NOT public.ortak_encrypted_dm_pair_current(NEW) THEN
  RAISE EXCEPTION 'Encrypted DM selected pair unavailable' USING ERRCODE='check_violation';
 END IF;
 NEW.changed_at:=clock_timestamp();
 IF NEW.enabled THEN NEW.enabled_at:=NEW.changed_at; END IF;
 RETURN NEW;
END
$$;

CREATE TRIGGER encrypted_dm_selection_guard BEFORE INSERT OR UPDATE OR DELETE ON encrypted_dm_selections
FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_selection_guard();

CREATE FUNCTION ortak_encrypted_dm_selection_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE CONSTRAINT TRIGGER encrypted_dm_selection_current_at_commit AFTER INSERT OR UPDATE ON encrypted_dm_selections
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_selection_commit_guard();

CREATE FUNCTION ortak_encrypted_dm_outer(target UUID, community UUID, source BYTEA, at_time TIMESTAMPTZ, recipient BYTEA)
RETURNS BYTEA LANGUAGE plpgsql VOLATILE STRICT
SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE ev RECORD; canonical TEXT;
BEGIN
 SELECT e.id,e.pubkey,e.created_at,e.kind,e.tags,e.content,e.sig INTO ev
 FROM public.office_inbox i JOIN public.events e
  ON e.community_id=community AND e.id=i.event_id AND e.created_at=i.event_created_at
 WHERE i.company_id=target AND i.event_id=source AND i.event_created_at=at_time
  AND i.event_kind=1059 AND e.kind=1059 AND e.channel_id IS NULL AND i.channel_id IS NULL
  AND i.author_pubkey=e.pubkey AND e.deleted_at IS NULL
  AND i.state='pending' AND i.claim_generation=0 AND i.attempt_count=0 AND i.finalized_at IS NULL
  AND e.created_at>=timestamptz '1970-01-01 00:00:00+00' AND e.created_at<timestamptz '10000-01-01 00:00:00+00'
  AND date_trunc('second',e.created_at)=e.created_at
  AND octet_length(e.pubkey)=32 AND octet_length(e.sig)=64
  AND octet_length(e.content) BETWEEN 132 AND 60000 AND e.content~'^[A-Za-z0-9+/]*={0,2}$'
  AND octet_length(e.tags::text)<=256 AND e.tags=jsonb_build_array(jsonb_build_array('p',encode(recipient,'hex')));
 IF NOT FOUND THEN RETURN NULL; END IF;
 canonical:=public.ortak_conversation_json75(jsonb_build_object(
  'id',encode(ev.id,'hex'),'pubkey',encode(ev.pubkey,'hex'),'created_at',extract(epoch FROM ev.created_at)::bigint,
  'kind',1059,'tags',ev.tags,'content',ev.content,'sig',encode(ev.sig,'hex')));
 IF canonical IS NULL OR octet_length(canonical)>65536 THEN RETURN NULL; END IF;
 RETURN convert_to(canonical,'UTF8');
END
$$;

CREATE TABLE encrypted_dm_decrypt_jobs (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL,
    source_id BYTEA NOT NULL,
    source_created_at TIMESTAMPTZ NOT NULL,
    source_author BYTEA NOT NULL,
    source_hash BYTEA NOT NULL,
    source_received_at TIMESTAMPTZ NOT NULL,
    selection_id UUID NOT NULL,
    selection_generation BIGINT NOT NULL,
    employee_id TEXT NOT NULL,
    employee_revision_id UUID NOT NULL,
    employee_lifecycle_epoch BIGINT NOT NULL,
    office_generation BIGINT NOT NULL,
    valid_before TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    deadline TIMESTAMPTZ NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    claim_generation BIGINT NOT NULL DEFAULT 0,
    claim_token UUID,
    worker_id UUID,
    claimed_at TIMESTAMPTZ,
    claim_expires_at TIMESTAMPTZ,
    crypto_deadline TIMESTAMPTZ,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    terminal_at TIMESTAMPTZ,
    error_code TEXT,
    seal_id BYTEA,
    seal_created_at TIMESTAMPTZ,
    rumor_id BYTEA,
    rumor_created_at TIMESTAMPTZ,
    rumor_hash BYTEA,
    reply_to BYTEA,
    verified_at TIMESTAMPTZ,
    PRIMARY KEY(company_id,source_id),
    FOREIGN KEY(company_id,selection_id) REFERENCES encrypted_dm_selections(company_id,selection_id),
    CONSTRAINT encrypted_dm_decrypt_jobs_attempts_check CHECK (attempts >= 0 AND attempts <= 3),
    CONSTRAINT encrypted_dm_decrypt_jobs_check CHECK (isfinite(deadline) AND isfinite(valid_before) AND deadline > source_received_at AND deadline <= (source_received_at + '00:02:00'::interval) AND valid_before <= deadline),
    CONSTRAINT encrypted_dm_decrypt_jobs_check1 CHECK (isfinite(next_attempt_at) AND next_attempt_at <= (deadline + '00:00:05'::interval)),
    CONSTRAINT encrypted_dm_decrypt_jobs_check10 CHECK (state <> 'verified'::text OR verified_at IS NOT NULL),
    CONSTRAINT encrypted_dm_decrypt_jobs_check2 CHECK (claim_generation = attempts),
    CONSTRAINT encrypted_dm_decrypt_jobs_check3 CHECK ((state = ANY (ARRAY['claimed'::text, 'verified'::text])) = (claim_token IS NOT NULL)),
    CONSTRAINT encrypted_dm_decrypt_jobs_check4 CHECK ((claim_token IS NULL) = (worker_id IS NULL) AND (claim_token IS NULL) = (claimed_at IS NULL) AND (claim_token IS NULL) = (claim_expires_at IS NULL) AND (claim_token IS NULL) = (crypto_deadline IS NULL)),
    CONSTRAINT encrypted_dm_decrypt_jobs_check5 CHECK (claim_token IS NULL OR claim_token <> '00000000-0000-0000-0000-000000000000'::uuid AND worker_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CONSTRAINT encrypted_dm_decrypt_jobs_check6 CHECK (claim_token IS NULL OR claimed_at < crypto_deadline AND crypto_deadline <= (claimed_at + '00:00:05'::interval) AND crypto_deadline <= claim_expires_at AND claim_expires_at <= (claimed_at + '00:00:30'::interval) AND claim_expires_at <= valid_before),
    CONSTRAINT encrypted_dm_decrypt_jobs_check7 CHECK ((state = ANY (ARRAY['failed'::text, 'cancelled'::text])) = (terminal_at IS NOT NULL)),
    CONSTRAINT encrypted_dm_decrypt_jobs_check8 CHECK ((verified_at IS NULL) = (rumor_id IS NULL) AND (verified_at IS NULL) = (seal_id IS NULL) AND (verified_at IS NULL) = (seal_created_at IS NULL) AND (verified_at IS NULL) = (rumor_created_at IS NULL) AND (verified_at IS NULL) = (rumor_hash IS NULL)),
    CONSTRAINT encrypted_dm_decrypt_jobs_check9 CHECK (verified_at IS NOT NULL OR reply_to IS NULL),
    CONSTRAINT encrypted_dm_decrypt_jobs_claim_generation_check CHECK (claim_generation >= 0 AND claim_generation <= 3),
    CONSTRAINT encrypted_dm_decrypt_jobs_employee_lifecycle_epoch_check CHECK (employee_lifecycle_epoch >= 0),
    CONSTRAINT encrypted_dm_decrypt_jobs_error_code_check CHECK (error_code = ANY (ARRAY['material_unavailable'::text, 'crypto_invalid'::text, 'authority_changed'::text, 'source_unavailable'::text, 'deadline_exceeded'::text, 'attempts_exhausted'::text, 'cancelled'::text])),
    CONSTRAINT encrypted_dm_decrypt_jobs_office_generation_check CHECK (office_generation >= 0),
    CONSTRAINT encrypted_dm_decrypt_jobs_reply_to_check CHECK (octet_length(reply_to) = 32),
    CONSTRAINT encrypted_dm_decrypt_jobs_rumor_created_at_check CHECK (rumor_created_at IS NULL OR rumor_created_at >= '1970-01-01 00:00:00+00'::timestamp with time zone AND rumor_created_at < '10000-01-01 00:00:00+00'::timestamp with time zone AND date_trunc('second'::text, rumor_created_at) = rumor_created_at),
    CONSTRAINT encrypted_dm_decrypt_jobs_rumor_hash_check CHECK (octet_length(rumor_hash) = 32),
    CONSTRAINT encrypted_dm_decrypt_jobs_rumor_id_check CHECK (octet_length(rumor_id) = 32),
    CONSTRAINT encrypted_dm_decrypt_jobs_seal_created_at_check CHECK (seal_created_at IS NULL OR seal_created_at >= '1970-01-01 00:00:00+00'::timestamp with time zone AND seal_created_at < '10000-01-01 00:00:00+00'::timestamp with time zone AND date_trunc('second'::text, seal_created_at) = seal_created_at),
    CONSTRAINT encrypted_dm_decrypt_jobs_seal_id_check CHECK (octet_length(seal_id) = 32),
    CONSTRAINT encrypted_dm_decrypt_jobs_selection_generation_check CHECK (selection_generation > 0),
    CONSTRAINT encrypted_dm_decrypt_jobs_source_author_check CHECK (octet_length(source_author) = 32),
    CONSTRAINT encrypted_dm_decrypt_jobs_source_hash_check CHECK (octet_length(source_hash) = 32),
    CONSTRAINT encrypted_dm_decrypt_jobs_source_id_check CHECK (octet_length(source_id) = 32),
    CONSTRAINT encrypted_dm_decrypt_jobs_state_check CHECK (state = ANY (ARRAY['pending'::text, 'claimed'::text, 'verified'::text, 'failed'::text, 'cancelled'::text]))
);

CREATE INDEX encrypted_dm_jobs_due ON encrypted_dm_decrypt_jobs(company_id,next_attempt_at,source_received_at,source_id)
 WHERE state IN('pending','claimed','verified');

CREATE INDEX encrypted_dm_jobs_live ON encrypted_dm_decrypt_jobs(company_id,claim_expires_at)
 WHERE state IN('claimed','verified');

CREATE INDEX encrypted_dm_verified_rumor ON encrypted_dm_decrypt_jobs(company_id,employee_id,rumor_id)
 WHERE verified_at IS NOT NULL;

SELECT attach_community_write_fence('encrypted_dm_decrypt_jobs');

CREATE FUNCTION ortak_encrypted_dm_job_consumed(company UUID,source BYTEA)
RETURNS BOOLEAN LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT false
$$;

-- Reconciliation creates ortak_encrypted_dm_job_current after the encrypted_dm_decrypt_jobs row type exists.

CREATE FUNCTION ortak_encrypted_dm_job_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE fresh BOOLEAN:=false;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Encrypted DM job is retained' USING ERRCODE='check_violation'; END IF;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.verified_at IS NOT NULL OR NEW.error_code IS NOT NULL THEN
   RAISE EXCEPTION 'Encrypted DM job initial state' USING ERRCODE='check_violation';
  END IF;
  fresh:=true;
 ELSE
  IF (to_jsonb(NEW)-ARRAY['state','attempts','claim_generation','claim_token','worker_id','claimed_at','claim_expires_at','crypto_deadline','next_attempt_at','terminal_at','error_code','seal_id','seal_created_at','rumor_id','rumor_created_at','rumor_hash','reply_to','verified_at']) IS DISTINCT FROM
     (to_jsonb(OLD)-ARRAY['state','attempts','claim_generation','claim_token','worker_id','claimed_at','claim_expires_at','crypto_deadline','next_attempt_at','terminal_at','error_code','seal_id','seal_created_at','rumor_id','rumor_created_at','rumor_hash','reply_to','verified_at']) THEN
   RAISE EXCEPTION 'Encrypted DM job source is immutable' USING ERRCODE='check_violation';
  END IF;
  IF OLD.state IN('failed','cancelled') THEN
   IF NEW IS DISTINCT FROM OLD THEN RAISE EXCEPTION 'Encrypted DM terminal job retained' USING ERRCODE='check_violation'; END IF;
   RETURN OLD;
  END IF;
  IF OLD.verified_at IS NOT NULL AND
   (NEW.seal_id,NEW.seal_created_at,NEW.rumor_id,NEW.rumor_created_at,NEW.rumor_hash,NEW.reply_to,NEW.verified_at) IS DISTINCT FROM
   (OLD.seal_id,OLD.seal_created_at,OLD.rumor_id,OLD.rumor_created_at,OLD.rumor_hash,OLD.reply_to,OLD.verified_at) THEN
   RAISE EXCEPTION 'Encrypted DM verified metadata is immutable' USING ERRCODE='check_violation';
  END IF;
  IF OLD.verified_at IS NULL AND NEW.verified_at IS NOT NULL
    AND NOT(OLD.state='claimed' AND NEW.state='verified') THEN
   RAISE EXCEPTION 'Encrypted DM metadata requires current verification' USING ERRCODE='check_violation';
  END IF;
  -- Identical in-budget receipt replay has no new effect and cannot renew a
  -- token or deadline. Deferred current checks still apply to the result row.
  IF OLD.state='verified' AND NEW IS NOT DISTINCT FROM OLD THEN RETURN OLD; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.claim_generation=OLD.claim_generation+1 AND NEW.state='claimed'
   AND (OLD.state='pending' OR OLD.claim_expires_at+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)<=clock_timestamp()) AND OLD.next_attempt_at<=clock_timestamp()
   AND NEW.claim_token IS NOT NULL AND NEW.claim_token IS DISTINCT FROM OLD.claim_token THEN fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.claim_generation=OLD.claim_generation THEN
   IF NEW.state='verified' AND OLD.state='claimed' AND OLD.crypto_deadline>clock_timestamp()
    AND (OLD.verified_at IS NOT NULL OR NEW.verified_at>=OLD.claimed_at) AND NEW.verified_at<=clock_timestamp()
    AND (NEW.claim_token,NEW.worker_id,NEW.claimed_at,NEW.claim_expires_at,NEW.crypto_deadline) IS NOT DISTINCT FROM
        (OLD.claim_token,OLD.worker_id,OLD.claimed_at,OLD.claim_expires_at,OLD.crypto_deadline) THEN fresh:=true;
   ELSIF NEW.state IN('failed','cancelled') AND NEW.error_code IS NOT NULL THEN NULL;
   ELSIF NEW.state='pending' AND OLD.state IN('claimed','verified') AND OLD.claim_expires_at>clock_timestamp()
    AND NEW.error_code='material_unavailable' AND OLD.attempts<3
    AND NEW.next_attempt_at>=statement_timestamp()+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END) THEN NULL;
   ELSE RAISE EXCEPTION 'Encrypted DM job transition refused' USING ERRCODE='check_violation';
   END IF;
  ELSE RAISE EXCEPTION 'Encrypted DM claim generation refused' USING ERRCODE='check_violation';
  END IF;
 END IF;
 IF fresh THEN
  PERFORM public.ortak_lock_office_authority(NEW.company_id);
  PERFORM 1 FROM public.encrypted_dm_selections WHERE company_id=NEW.company_id AND selection_id=NEW.selection_id FOR SHARE;
  -- Inbox claim-state changes deliberately do not advance Office generation.
  -- Retain its row lock through commit as well as comparing canonical facts.
  PERFORM 1 FROM public.office_inbox WHERE company_id=NEW.company_id AND event_id=NEW.source_id FOR SHARE;
  IF NEW.state='claimed' THEN
   IF NOT pg_try_advisory_xact_lock(hashtextextended('ortak-encrypted-dm-claims:'||NEW.company_id::text,0))
     OR NEW.claimed_at>clock_timestamp() OR NEW.crypto_deadline<=clock_timestamp()
     OR (SELECT count(*) FROM public.encrypted_dm_decrypt_jobs j WHERE j.company_id=NEW.company_id
          AND j.source_id<>NEW.source_id AND j.state IN('claimed','verified') AND NOT public.ortak_encrypted_dm_job_consumed(j.company_id,j.source_id) AND j.claim_expires_at>clock_timestamp())>=2 THEN
    RAISE EXCEPTION 'Encrypted DM finite claim slot unavailable' USING ERRCODE='serialization_failure';
   END IF;
  END IF;
  IF NOT public.ortak_encrypted_dm_job_current(NEW) THEN
   RAISE EXCEPTION 'Encrypted DM job authority changed' USING ERRCODE='serialization_failure';
  END IF;
  IF NEW.state='verified' AND NEW.reply_to IS NOT NULL AND NOT EXISTS(
    SELECT 1 FROM public.encrypted_dm_decrypt_jobs previous
    JOIN public.encrypted_dm_selections p ON p.company_id=previous.company_id AND p.selection_id=previous.selection_id
    JOIN public.encrypted_dm_selections s ON s.company_id=NEW.company_id AND s.selection_id=NEW.selection_id
    WHERE previous.company_id=NEW.company_id AND previous.employee_id=NEW.employee_id
      AND previous.rumor_id=NEW.reply_to AND previous.verified_at IS NOT NULL AND previous.source_id<>NEW.source_id
      AND (p.community_id,p.channel_id,p.human_public_key,p.employee_public_key)=(s.community_id,s.channel_id,s.human_public_key,s.employee_public_key)) THEN
   RAISE EXCEPTION 'Encrypted DM reply lacks same-pair verified provenance' USING ERRCODE='check_violation';
  END IF;
 END IF;
 RETURN NEW;
END
$$;

CREATE TRIGGER encrypted_dm_job_guard BEFORE INSERT OR UPDATE OR DELETE ON encrypted_dm_decrypt_jobs
FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_job_guard();

CREATE FUNCTION ortak_encrypted_dm_job_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE CONSTRAINT TRIGGER encrypted_dm_job_current_at_commit AFTER INSERT OR UPDATE ON encrypted_dm_decrypt_jobs
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_encrypted_dm_job_commit_guard();

CREATE TRIGGER encrypted_dm_selections_no_truncate BEFORE TRUNCATE ON encrypted_dm_selections
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();
CREATE TRIGGER encrypted_dm_decrypt_jobs_no_truncate BEFORE TRUNCATE ON encrypted_dm_decrypt_jobs
FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_confidential_runtime_binding(company UUID,revision UUID)
RETURNS JSONB LANGUAGE SQL VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT NULL::jsonb
$$;

CREATE FUNCTION ortak_confidential_dm_run_id(company UUID,source BYTEA)
RETURNS UUID LANGUAGE SQL IMMUTABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT NULL::uuid
$$;

CREATE FUNCTION ortak_confidential_dm_source(company UUID,source BYTEA)
RETURNS BYTEA LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT NULL::bytea
$$;

CREATE FUNCTION ortak_confidential_dm_identity(company UUID,source BYTEA,run UUID,key UUID)
RETURNS BYTEA LANGUAGE SQL STABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT NULL::bytea
$$;

CREATE TABLE confidential_runs (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL,
    run_id UUID NOT NULL,
    source_id BYTEA NOT NULL,
    selection_id UUID NOT NULL,
    employee_id TEXT NOT NULL,
    human_public_key BYTEA NOT NULL,
    rumor_id BYTEA NOT NULL,
    key_id UUID NOT NULL,
    identity_bytes BYTEA NOT NULL,
    source_bytes BYTEA NOT NULL,
    wrapped_key BYTEA NOT NULL,
    start_key TEXT NOT NULL,
    admitted_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    admission_deadline TIMESTAMPTZ NOT NULL,
    execution_deadline TIMESTAMPTZ NOT NULL,
    claim_generation BIGINT NOT NULL,
    claim_token UUID NOT NULL,
    claim_worker UUID NOT NULL,
    PRIMARY KEY(company_id,run_id),
    UNIQUE(company_id,source_id),
    UNIQUE(company_id,key_id),
    -- Independent of wrapper, Office key version, pair re-enable and model revision.
 UNIQUE(company_id,employee_id,human_public_key,rumor_id),
    FOREIGN KEY(company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY(company_id,source_id) REFERENCES encrypted_dm_decrypt_jobs(company_id,source_id),
    FOREIGN KEY(company_id,selection_id) REFERENCES encrypted_dm_selections(company_id,selection_id),
    CONSTRAINT confidential_runs_check CHECK (start_key = ((('ortak-run:'::text || company_id::text) || ':'::text) || run_id::text)),
    CONSTRAINT confidential_runs_check1 CHECK (isfinite(admitted_at) AND isfinite(admission_deadline) AND isfinite(execution_deadline) AND admission_deadline > admitted_at AND execution_deadline > admitted_at AND execution_deadline <= (admitted_at + '00:10:00'::interval)),
    CONSTRAINT confidential_runs_claim_generation_check CHECK (claim_generation >= 1 AND claim_generation <= 3),
    CONSTRAINT confidential_runs_human_public_key_check CHECK (octet_length(human_public_key) = 32),
    CONSTRAINT confidential_runs_identity_bytes_check CHECK (octet_length(identity_bytes) >= 1 AND octet_length(identity_bytes) <= 2048),
    CONSTRAINT confidential_runs_rumor_id_check CHECK (octet_length(rumor_id) = 32),
    CONSTRAINT confidential_runs_source_bytes_check CHECK (octet_length(source_bytes) >= 1 AND octet_length(source_bytes) <= 4096),
    CONSTRAINT confidential_runs_source_id_check CHECK (octet_length(source_id) = 32),
    CONSTRAINT confidential_runs_wrapped_key_check CHECK (octet_length(wrapped_key) >= 1 AND octet_length(wrapped_key) <= 12288)
);

SELECT attach_community_write_fence('confidential_runs');

CREATE TABLE confidential_run_payloads (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL,
    run_id UUID NOT NULL,
    purpose TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    envelope_bytes BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id,purpose,ordinal),
    UNIQUE(company_id,run_id,purpose,nonce),
    FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
    CONSTRAINT confidential_run_payloads_check CHECK ((purpose = ANY (ARRAY['snapshot'::text, 'reply_draft'::text])) AND ordinal = 0 OR purpose = 'runtime_event'::text AND ordinal >= 1 AND ordinal <= 512),
    CONSTRAINT confidential_run_payloads_envelope_bytes_check CHECK (octet_length(envelope_bytes) >= 1 AND octet_length(envelope_bytes) <= 98304),
    CONSTRAINT confidential_run_payloads_nonce_check CHECK (octet_length(nonce) = 12),
    CONSTRAINT confidential_run_payloads_purpose_check CHECK (purpose = ANY (ARRAY['snapshot'::text, 'runtime_event'::text, 'reply_draft'::text]))
);

SELECT attach_community_write_fence('confidential_run_payloads');

CREATE TABLE confidential_dm_receipts (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL,
    source_id BYTEA NOT NULL,
    run_id UUID NOT NULL,
    duplicate_rumor BOOLEAN NOT NULL,
    claim_generation BIGINT NOT NULL,
    claim_token UUID NOT NULL,
    claim_worker UUID NOT NULL,
    committed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,source_id),
    FOREIGN KEY(company_id,source_id) REFERENCES encrypted_dm_decrypt_jobs(company_id,source_id),
    FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id)
);

SELECT attach_community_write_fence('confidential_dm_receipts');

CREATE TABLE confidential_run_dispatches (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL,
    run_id UUID NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    generation BIGINT NOT NULL DEFAULT 0,
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    error_code TEXT,
    finished_at TIMESTAMPTZ,
    PRIMARY KEY(company_id,run_id),
    FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
    CONSTRAINT confidential_run_dispatches_attempts_check CHECK (attempts >= 0 AND attempts <= 3),
    CONSTRAINT confidential_run_dispatches_check CHECK (generation = attempts),
    CONSTRAINT confidential_run_dispatches_check1 CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL)),
    CONSTRAINT confidential_run_dispatches_check2 CHECK ((state <> 'pending'::text) = (finished_at IS NOT NULL)),
    CONSTRAINT confidential_run_dispatches_check3 CHECK (state = 'pending'::text OR lease_token IS NULL),
    CONSTRAINT confidential_run_dispatches_check4 CHECK (isfinite(next_attempt_at) AND (lease_expires_at IS NULL OR isfinite(lease_expires_at))),
    CONSTRAINT confidential_run_dispatches_error_code_check CHECK (error_code = ANY (ARRAY['unavailable'::text, 'authority_changed'::text, 'deadline_exceeded'::text, 'cancelled'::text])),
    CONSTRAINT confidential_run_dispatches_state_check CHECK (state = ANY (ARRAY['pending'::text, 'delivered'::text, 'failed'::text, 'cancelled'::text]))
);

SELECT attach_community_write_fence('confidential_run_dispatches');

CREATE FUNCTION ortak_confidential_dm_current(company UUID,run UUID)
RETURNS BOOLEAN LANGUAGE SQL VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
    SELECT false
$$;

CREATE FUNCTION ortak_lock_confidential_dm(company UUID,run UUID) RETURNS BOOLEAN
LANGUAGE plpgsql VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE target RECORD;
BEGIN
 PERFORM public.ortak_lock_office_authority(company);
 SELECT selection_id,source_id INTO target FROM public.confidential_runs WHERE company_id=company AND run_id=run;
 IF NOT FOUND THEN RETURN false; END IF;
 PERFORM 1 FROM public.encrypted_dm_selections WHERE company_id=company AND selection_id=target.selection_id FOR SHARE;
 PERFORM 1 FROM public.encrypted_dm_decrypt_jobs WHERE company_id=company AND source_id=target.source_id FOR SHARE;
 PERFORM 1 FROM public.office_inbox WHERE company_id=company AND event_id=target.source_id FOR SHARE;
 RETURN public.ortak_confidential_dm_current(company,run);
END
$$;

CREATE FUNCTION ortak_confidential_payload_valid(bytes BYTEA,identity BYTEA,purpose TEXT,ordinal INTEGER)
RETURNS BOOLEAN LANGUAGE plpgsql IMMUTABLE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE wire JSONB; header JSONB; size INTEGER; nonce BYTEA; cipher BYTEA; maximum INTEGER;
BEGIN
 IF octet_length(bytes)>98304 OR octet_length(identity)>2048 THEN RETURN false; END IF;
 wire:=convert_from(bytes,'UTF8')::jsonb;
 IF jsonb_typeof(wire) IS DISTINCT FROM 'object' OR NOT wire ?& ARRAY['ciphertext','header','nonce'] OR wire-ARRAY['ciphertext','header','nonce']<>'{}'::jsonb
  OR convert_to(public.ortak_conversation_json75(wire),'UTF8')<>bytes THEN RETURN false; END IF;
 header:=wire->'header';
 IF jsonb_typeof(header) IS DISTINCT FROM 'object' OR NOT header ?& ARRAY['algorithm','format','identity','ordinal','plaintext_bytes','purpose'] OR header-ARRAY['algorithm','format','identity','ordinal','plaintext_bytes','purpose']<>'{}'::jsonb
  OR header->>'algorithm' IS DISTINCT FROM 'A256GCM' OR header->>'format' IS DISTINCT FROM 'ortak-confidential-payload/1'
  OR header->>'purpose' IS DISTINCT FROM purpose OR header->'ordinal' IS DISTINCT FROM to_jsonb(ordinal)
  OR convert_to(public.ortak_conversation_json75(header->'identity'),'UTF8') IS DISTINCT FROM identity
  OR jsonb_typeof(header->'plaintext_bytes')<>'number' THEN RETURN false; END IF;
 maximum:=CASE purpose WHEN 'snapshot' THEN 49152 WHEN 'runtime_event' THEN 32768 WHEN 'reply_draft' THEN 16384 END;
 IF maximum IS NULL OR (purpose='runtime_event' AND ordinal NOT BETWEEN 1 AND 512)
  OR (purpose<>'runtime_event' AND ordinal<>0) THEN RETURN false; END IF;
 IF (header->>'plaintext_bytes')!~'^(0|[1-9][0-9]{0,5})$' THEN RETURN false; END IF;
 size:=(header->>'plaintext_bytes')::integer;
 IF size>maximum OR jsonb_typeof(wire->'nonce')<>'string' OR length(wire->>'nonce')<>16
  OR jsonb_typeof(wire->'ciphertext')<>'string' OR length(wire->>'ciphertext')>65560 THEN RETURN false; END IF;
 nonce:=decode(wire->>'nonce','base64'); cipher:=decode(wire->>'ciphertext','base64');
 RETURN octet_length(nonce)=12 AND octet_length(cipher)=size+16
  AND replace(encode(nonce,'base64'),E'\n','')=wire->>'nonce'
  AND replace(encode(cipher,'base64'),E'\n','')=wire->>'ciphertext';
END
$$;

CREATE FUNCTION ortak_confidential_run_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE TRIGGER confidential_run_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_runs
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_guard();

CREATE FUNCTION ortak_confidential_payload_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE TRIGGER confidential_payload_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_run_payloads
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_payload_guard();

CREATE FUNCTION ortak_confidential_receipt_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE TRIGGER confidential_receipt_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_dm_receipts
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_receipt_guard();

CREATE FUNCTION ortak_confidential_consumed_job() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM public.confidential_dm_receipts WHERE company_id=OLD.company_id AND source_id=OLD.source_id)
  AND NEW IS DISTINCT FROM OLD THEN
  RAISE EXCEPTION 'Consumed decrypt job cannot be reclaimed' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;

CREATE TRIGGER confidential_consumed_job BEFORE UPDATE ON encrypted_dm_decrypt_jobs
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_consumed_job();

CREATE FUNCTION ortak_confidential_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE CONSTRAINT TRIGGER confidential_run_at_commit AFTER INSERT ON confidential_runs
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard();

CREATE CONSTRAINT TRIGGER confidential_payload_at_commit AFTER INSERT ON confidential_run_payloads
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard();

CREATE CONSTRAINT TRIGGER confidential_receipt_at_commit AFTER INSERT ON confidential_dm_receipts
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_commit_guard();

CREATE FUNCTION ortak_confidential_run_mode_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF TG_OP='UPDATE' AND NEW.payload_mode IS DISTINCT FROM OLD.payload_mode THEN
  RAISE EXCEPTION 'Run payload mode is immutable' USING ERRCODE='check_violation';
 END IF;
 IF NEW.payload_mode='ordinary' THEN RETURN NEW; END IF;
 IF NEW.work_item_id IS NOT NULL OR NEW.routing_decision_id IS NULL OR NEW.message_id IS NULL OR NEW.root_message_id<>NEW.message_id
  OR NEW.error_message IS NOT NULL OR (NEW.error_code IS NOT NULL AND NEW.error_code NOT IN('confidential_failed','confidential_cancelled'))
  OR (NEW.cancel_reason IS NOT NULL AND NEW.cancel_reason NOT IN('office_revoked','human_requested'))
  OR (NEW.runtime_run_ref IS NOT NULL AND NEW.runtime_run_ref!~'^[A-Za-z0-9][A-Za-z0-9:._/-]{0,255}$') THEN
  RAISE EXCEPTION 'Confidential run permits bounded metadata only' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND
  (to_jsonb(NEW)-ARRAY['status','runtime_run_ref','started_at','finished_at','updated_at','delivery_intent','cancel_reason','error_code']) IS DISTINCT FROM
  (to_jsonb(OLD)-ARRAY['status','runtime_run_ref','started_at','finished_at','updated_at','delivery_intent','cancel_reason','error_code']) THEN
  RAISE EXCEPTION 'Confidential run authority is immutable' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND OLD.status IN('completed','failed','cancelled') AND NEW.status<>OLD.status THEN
  RAISE EXCEPTION 'Confidential terminal status cannot revive' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND OLD.runtime_run_ref IS NOT NULL AND NEW.runtime_run_ref IS DISTINCT FROM OLD.runtime_run_ref THEN
  RAISE EXCEPTION 'Confidential start correlation cannot change' USING ERRCODE='check_violation';
 END IF;
 IF TG_OP='UPDATE' AND NEW.status IS DISTINCT FROM OLD.status AND NEW.status IN('running','waiting','completed')
  AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.id) THEN
  RAISE EXCEPTION 'Confidential fresh execution authority retired' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;

CREATE TRIGGER confidential_run_mode_guard BEFORE INSERT OR UPDATE ON runs
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_mode_guard();

CREATE FUNCTION ortak_confidential_reject_ordinary() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM public.runs r WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.payload_mode='confidential_dm_v1') THEN
  RAISE EXCEPTION 'Confidential run cannot use an ordinary content path' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;

CREATE TRIGGER confidential_no_ordinary_snapshot BEFORE INSERT OR UPDATE ON run_context_snapshots FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_ordinary_events BEFORE INSERT OR UPDATE ON run_events FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_ordinary_office BEFORE INSERT OR UPDATE ON runtime_office_outputs FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_ordinary_work BEFORE INSERT OR UPDATE ON runtime_work_outputs FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_ordinary_memory BEFORE INSERT OR UPDATE ON runtime_memory_writes FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_reviewed_use BEFORE INSERT OR UPDATE ON run_reviewed_memory_uses FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_workspace_use BEFORE INSERT OR UPDATE ON run_workspace_uses FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_workspace_action BEFORE INSERT OR UPDATE ON workspace_tool_actions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_workspace_receipt BEFORE INSERT OR UPDATE ON workspace_tool_receipts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_workspace_reader BEFORE INSERT OR UPDATE ON workspace_reader_executions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_work_execution BEFORE INSERT OR UPDATE ON work_executions FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_artifact BEFORE INSERT OR UPDATE ON artifacts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_work_attachment BEFORE INSERT OR UPDATE ON work_attachments FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE TRIGGER confidential_no_ordinary_outbox BEFORE INSERT OR UPDATE ON outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reject_ordinary();

CREATE FUNCTION ortak_confidential_dispatch_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE deadline TIMESTAMPTZ; fresh BOOLEAN:=false;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'Confidential dispatch is retained' USING ERRCODE='check_violation'; END IF;
 SELECT execution_deadline INTO STRICT deadline FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.generation<>0 OR NEW.lease_token IS NOT NULL OR NEW.error_code IS NOT NULL THEN
   RAISE EXCEPTION 'Confidential dispatch initial state' USING ERRCODE='check_violation';
  END IF;
 ELSE
  IF (NEW.company_id,NEW.community_id,NEW.run_id) IS DISTINCT FROM (OLD.company_id,OLD.community_id,OLD.run_id)
   OR OLD.state<>'pending' THEN RAISE EXCEPTION 'Confidential dispatch identity or terminal result changed' USING ERRCODE='check_violation'; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.generation=OLD.generation+1 AND NEW.state='pending'
   AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token
   AND OLD.next_attempt_at<=clock_timestamp() AND (OLD.lease_expires_at IS NULL OR OLD.lease_expires_at+(CASE WHEN OLD.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)<=clock_timestamp())
   AND NEW.lease_expires_at>clock_timestamp() AND NEW.lease_expires_at<=least(deadline,clock_timestamp()+interval '30 seconds') THEN
   fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.generation=OLD.generation AND NEW.lease_token IS NULL THEN
   -- Exact lease accounting remains possible after source/Office revocation.
   -- A delivered result requires a retained start reference; it grants no start.
   IF NEW.state='delivered' AND (OLD.lease_expires_at<=clock_timestamp() OR OLD.lease_token IS NULL
      OR NOT EXISTS(SELECT 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND runtime_run_ref IS NOT NULL)) THEN
    RAISE EXCEPTION 'Confidential delivery needs retained start receipt' USING ERRCODE='check_violation';
   ELSIF NEW.state='pending' AND (NEW.error_code<>'unavailable' OR OLD.lease_token IS NULL OR OLD.lease_expires_at<=clock_timestamp()
     OR NEW.attempts>=3 OR NEW.next_attempt_at<statement_timestamp()+(CASE WHEN NEW.attempts=1 THEN interval '1 second' ELSE interval '5 seconds' END)) THEN
    RAISE EXCEPTION 'Confidential retry is not bounded lease accounting' USING ERRCODE='check_violation';
   END IF;
  ELSE RAISE EXCEPTION 'Confidential dispatch lease transition refused' USING ERRCODE='check_violation';
  END IF;
 END IF;
 IF NEW.next_attempt_at>deadline+interval '5 seconds' THEN RAISE EXCEPTION 'Confidential retry deadline exceeded' USING ERRCODE='check_violation'; END IF;
 IF fresh AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential dispatch authority retired' USING ERRCODE='check_violation';
 END IF;
 RETURN NEW;
END
$$;

CREATE TRIGGER confidential_dispatch_guard BEFORE INSERT OR UPDATE OR DELETE ON confidential_run_dispatches
FOR EACH ROW EXECUTE FUNCTION ortak_confidential_dispatch_guard();

CREATE FUNCTION ortak_confidential_dispatch_commit_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL THEN
  IF NEW.lease_expires_at<=clock_timestamp() OR NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.run_id) THEN
   RAISE EXCEPTION 'Confidential dispatch expired before commit' USING ERRCODE='serialization_failure';
  END IF;
 END IF;
 RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER confidential_dispatch_at_commit AFTER UPDATE ON confidential_run_dispatches
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_dispatch_commit_guard();

CREATE FUNCTION ortak_commit_confidential_dm(company UUID,source BYTEA,run UUID,key UUID,identity BYTEA,wrapped BYTEA,snapshot BYTEA,nonce BYTEA)
RETURNS TABLE(committed_run_id UUID,duplicate_rumor BOOLEAN)
LANGUAGE plpgsql VOLATILE STRICT SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE INDEX confidential_dispatch_due ON confidential_run_dispatches(company_id,next_attempt_at,run_id) WHERE state='pending';

CREATE INDEX confidential_runs_selection ON confidential_runs(company_id,selection_id,run_id);

CREATE TRIGGER confidential_runs_no_truncate BEFORE TRUNCATE ON confidential_runs FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE TRIGGER confidential_payloads_no_truncate BEFORE TRUNCATE ON confidential_run_payloads FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE TRIGGER confidential_receipts_no_truncate BEFORE TRUNCATE ON confidential_dm_receipts FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE TRIGGER confidential_dispatches_no_truncate BEFORE TRUNCATE ON confidential_run_dispatches FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

CREATE FUNCTION ortak_confidential_run_complete_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE CONSTRAINT TRIGGER confidential_run_complete_at_commit AFTER INSERT ON runs
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_complete_guard();

CREATE FUNCTION ortak_confidential_run_transition_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
 IF NEW.payload_mode='confidential_dm_v1' AND NEW.status IS DISTINCT FROM OLD.status AND NEW.status IN('running','waiting','completed')
  AND NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.id) THEN
  RAISE EXCEPTION 'Confidential execution expired before commit' USING ERRCODE='serialization_failure';
 END IF;
 RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER confidential_run_transition_at_commit AFTER UPDATE ON runs
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_run_transition_guard();

CREATE FUNCTION ortak_employee_memory_run_origin(company UUID, run UUID, destination UUID)
RETURNS TABLE(origin_bytes BYTEA,observed_at TIMESTAMPTZ,valid_before TIMESTAMPTZ)
LANGUAGE sql STABLE AS $$
    SELECT NULL::bytea, NULL::timestamptz, NULL::timestamptz WHERE false
$$;

CREATE FUNCTION ortak_employee_reviewed_runtime_eligible(company UUID, run UUID, fact UUID, target UUID,
    source_epoch BIGINT,destination_epoch BIGINT,target_epoch BIGINT)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT false
$$;

CREATE TABLE run_employee_reviewed_memory_uses (
    company_id UUID NOT NULL REFERENCES companies(id),
    community_id UUID NOT NULL REFERENCES communities(id),
    run_id UUID NOT NULL,
    ordinal INTEGER NOT NULL,
    fact_id UUID NOT NULL,
    target_id UUID NOT NULL,
    fact_version BIGINT NOT NULL,
    content_hash BYTEA NOT NULL,
    source_hash BYTEA NOT NULL,
    sharing_hash BYTEA NOT NULL,
    audience_hash BYTEA NOT NULL,
    binding_hash BYTEA NOT NULL,
    namespace_hash BYTEA NOT NULL,
    approval_id UUID NOT NULL,
    approved_by TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    source_authority_epoch BIGINT NOT NULL,
    destination_authority_epoch BIGINT NOT NULL,
    consumption_epoch BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id,ordinal),
    UNIQUE(company_id,run_id,fact_id),
    FOREIGN KEY(company_id,run_id) REFERENCES runs(company_id,id),
    FOREIGN KEY(company_id,run_id) REFERENCES run_context_snapshots(company_id,run_id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(company_id,fact_id) REFERENCES employee_reviewed_memory_facts(company_id,id),
    FOREIGN KEY(company_id,fact_id) REFERENCES employee_reviewed_memory_exports(company_id,fact_id),
    FOREIGN KEY(company_id,target_id) REFERENCES employee_reviewed_memory_targets(company_id,id),
    CONSTRAINT run_employee_reviewed_memory__destination_authority_epoch_check CHECK (destination_authority_epoch >= 0),
    CONSTRAINT run_employee_reviewed_memory_uses_approved_by_check CHECK (approved_by ~ '^[0-9a-f]{64}$'::text),
    CONSTRAINT run_employee_reviewed_memory_uses_audience_hash_check CHECK (octet_length(audience_hash) = 32),
    CONSTRAINT run_employee_reviewed_memory_uses_binding_hash_check CHECK (octet_length(binding_hash) = 32),
    CONSTRAINT run_employee_reviewed_memory_uses_consumption_epoch_check CHECK (consumption_epoch >= 0),
    CONSTRAINT run_employee_reviewed_memory_uses_content_hash_check CHECK (octet_length(content_hash) = 32),
    CONSTRAINT run_employee_reviewed_memory_uses_fact_version_check CHECK (fact_version = 1),
    CONSTRAINT run_employee_reviewed_memory_uses_namespace_hash_check CHECK (octet_length(namespace_hash) = 32),
    CONSTRAINT run_employee_reviewed_memory_uses_ordinal_check CHECK (ordinal >= 0 AND ordinal <= 7),
    CONSTRAINT run_employee_reviewed_memory_uses_sharing_hash_check CHECK (octet_length(sharing_hash) = 32),
    CONSTRAINT run_employee_reviewed_memory_uses_source_authority_epoch_check CHECK (source_authority_epoch >= 0),
    CONSTRAINT run_employee_reviewed_memory_uses_source_hash_check CHECK (octet_length(source_hash) = 32)
);

CREATE INDEX employee_memory_use_fact ON run_employee_reviewed_memory_uses(company_id,fact_id,run_id);

CREATE INDEX employee_memory_use_expiry ON run_employee_reviewed_memory_uses(company_id,expires_at,run_id);

CREATE TRIGGER employee_memory_use_immutable BEFORE UPDATE OR DELETE ON run_employee_reviewed_memory_uses
    FOR EACH ROW EXECUTE FUNCTION ortak_reject_row_mutation();

CREATE TRIGGER employee_memory_use_no_truncate BEFORE TRUNCATE ON run_employee_reviewed_memory_uses
    FOR EACH STATEMENT EXECUTE FUNCTION ortak_reject_office_truncate();

SELECT attach_community_write_fence('run_employee_reviewed_memory_uses');

CREATE FUNCTION ortak_employee_use_ordinary() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS(SELECT 1 FROM runs r WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id
        AND coalesce(to_jsonb(r)->>'payload_mode','ordinary')='ordinary') THEN
        RAISE EXCEPTION 'employee memory requires ordinary run' USING ERRCODE='check_violation';
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER employee_memory_use_ordinary BEFORE INSERT ON run_employee_reviewed_memory_uses
    FOR EACH ROW EXECUTE FUNCTION ortak_employee_use_ordinary();

CREATE FUNCTION ortak_run_employee_memory_current(company UUID, run UUID)
RETURNS BOOLEAN LANGUAGE sql STABLE AS $$
    SELECT false
$$;

CREATE FUNCTION ortak_employee_snapshot_v5(company UUID, run UUID, wire JSONB)
RETURNS VOID LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE CONSTRAINT TRIGGER employee_memory_snapshot_at_commit AFTER INSERT ON run_employee_reviewed_memory_uses
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_reviewed_snapshot_consistent();

CREATE TABLE confidential_execution_leases (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL,
    run_id UUID NOT NULL,
    state TEXT NOT NULL DEFAULT 'observing',
    generation BIGINT NOT NULL DEFAULT 0,
    failures INTEGER NOT NULL DEFAULT 0,
    cancel_attempts INTEGER NOT NULL DEFAULT 0,
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    error_code TEXT,
    finished_at TIMESTAMPTZ,
    PRIMARY KEY(company_id,run_id),
    FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
    CONSTRAINT confidential_execution_leases_cancel_attempts_check CHECK (cancel_attempts >= 0 AND cancel_attempts <= 3),
    CONSTRAINT confidential_execution_leases_check CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL)),
    CONSTRAINT confidential_execution_leases_check1 CHECK ((state = ANY (ARRAY['observing'::text, 'sealing'::text, 'cancelling'::text])) OR lease_token IS NULL),
    CONSTRAINT confidential_execution_leases_check2 CHECK ((state = ANY (ARRAY['complete'::text, 'stopped'::text, 'unconfirmed'::text])) = (finished_at IS NOT NULL)),
    CONSTRAINT confidential_execution_leases_check3 CHECK (isfinite(next_attempt_at) AND (lease_expires_at IS NULL OR isfinite(lease_expires_at))),
    CONSTRAINT confidential_execution_leases_error_code_check CHECK (error_code = ANY (ARRAY['unavailable'::text, 'authority_changed'::text, 'protocol'::text, 'deadline_exceeded'::text, 'cancelled'::text])),
    CONSTRAINT confidential_execution_leases_failures_check CHECK (failures >= 0 AND failures <= 3),
    CONSTRAINT confidential_execution_leases_generation_check CHECK (generation >= 0 AND generation <= 128),
    CONSTRAINT confidential_execution_leases_state_check CHECK (state = ANY (ARRAY['observing'::text, 'sealing'::text, 'cancelling'::text, 'complete'::text, 'stopped'::text, 'unconfirmed'::text]))
);

SELECT attach_community_write_fence('confidential_execution_leases');

CREATE INDEX confidential_execution_due ON confidential_execution_leases(company_id,next_attempt_at,run_id)
 WHERE state IN('observing','sealing','cancelling');

CREATE TABLE confidential_event_receipts (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL,
    run_id UUID NOT NULL,
    ordinal INTEGER NOT NULL,
    purpose TEXT NOT NULL DEFAULT 'runtime_event',
    occurred_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY(company_id,run_id,ordinal),
    FOREIGN KEY(company_id,run_id,purpose,ordinal) REFERENCES confidential_run_payloads(company_id,run_id,purpose,ordinal),
    CONSTRAINT confidential_event_receipts_occurred_at_check CHECK (isfinite(occurred_at)),
    CONSTRAINT confidential_event_receipts_ordinal_check CHECK (ordinal >= 1 AND ordinal <= 512),
    CONSTRAINT confidential_event_receipts_purpose_check CHECK (purpose = 'runtime_event'::text)
);

SELECT attach_community_write_fence('confidential_event_receipts');

CREATE TABLE confidential_reply_bundles (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL,
    run_id UUID NOT NULL,
    rumor_id BYTEA NOT NULL,
    rumor_hash BYTEA NOT NULL,
    recipient_id BYTEA NOT NULL,
    history_id BYTEA NOT NULL,
    recipient_bytes BYTEA NOT NULL,
    history_bytes BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,run_id),
    UNIQUE(company_id,recipient_id),
    UNIQUE(company_id,history_id),
    FOREIGN KEY(company_id,run_id) REFERENCES confidential_runs(company_id,run_id),
    CONSTRAINT confidential_reply_bundles_check CHECK (recipient_id <> history_id),
    CONSTRAINT confidential_reply_bundles_history_bytes_check CHECK (octet_length(history_bytes) >= 1 AND octet_length(history_bytes) <= 65536),
    CONSTRAINT confidential_reply_bundles_history_id_check CHECK (octet_length(history_id) = 32),
    CONSTRAINT confidential_reply_bundles_recipient_bytes_check CHECK (octet_length(recipient_bytes) >= 1 AND octet_length(recipient_bytes) <= 65536),
    CONSTRAINT confidential_reply_bundles_recipient_id_check CHECK (octet_length(recipient_id) = 32),
    CONSTRAINT confidential_reply_bundles_rumor_hash_check CHECK (octet_length(rumor_hash) = 32),
    CONSTRAINT confidential_reply_bundles_rumor_id_check CHECK (octet_length(rumor_id) = 32)
);

SELECT attach_community_write_fence('confidential_reply_bundles');

CREATE TABLE confidential_reply_outbox (
    company_id UUID NOT NULL,
    community_id UUID NOT NULL,
    run_id UUID NOT NULL,
    copy INTEGER NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    generation BIGINT NOT NULL DEFAULT 0,
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    error_code TEXT,
    acknowledged_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    PRIMARY KEY(company_id,run_id,copy),
    FOREIGN KEY(company_id,run_id) REFERENCES confidential_reply_bundles(company_id,run_id),
    CONSTRAINT confidential_reply_outbox_attempts_check CHECK (attempts >= 0 AND attempts <= 3),
    CONSTRAINT confidential_reply_outbox_check CHECK (generation = attempts),
    CONSTRAINT confidential_reply_outbox_check1 CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL)),
    CONSTRAINT confidential_reply_outbox_check2 CHECK (state = 'pending'::text OR lease_token IS NULL),
    CONSTRAINT confidential_reply_outbox_check3 CHECK ((state <> 'pending'::text) = (finished_at IS NOT NULL)),
    CONSTRAINT confidential_reply_outbox_check4 CHECK ((state = 'acked'::text) = (acknowledged_at IS NOT NULL)),
    CONSTRAINT confidential_reply_outbox_check5 CHECK (isfinite(next_attempt_at) AND (lease_expires_at IS NULL OR isfinite(lease_expires_at))),
    CONSTRAINT confidential_reply_outbox_copy_check CHECK (copy = ANY (ARRAY[0, 1])),
    CONSTRAINT confidential_reply_outbox_error_code_check CHECK (error_code = ANY (ARRAY['unavailable'::text, 'authority_changed'::text, 'deadline_exceeded'::text])),
    CONSTRAINT confidential_reply_outbox_state_check CHECK (state = ANY (ARRAY['pending'::text, 'acked'::text, 'failed'::text, 'retired'::text]))
);

SELECT attach_community_write_fence('confidential_reply_outbox');

CREATE INDEX confidential_reply_due ON confidential_reply_outbox(company_id,next_attempt_at,run_id,copy) WHERE state='pending';

CREATE FUNCTION ortak_confidential_execution_immutable() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN RAISE EXCEPTION 'Confidential execution history is retained' USING ERRCODE='check_violation'; END
$$;

CREATE TRIGGER confidential_event_immutable BEFORE UPDATE OR DELETE ON confidential_event_receipts FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();

CREATE TRIGGER confidential_reply_immutable BEFORE UPDATE OR DELETE ON confidential_reply_bundles FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();

CREATE TRIGGER confidential_execution_retain BEFORE DELETE ON confidential_execution_leases FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();

CREATE TRIGGER confidential_outbox_retain BEFORE DELETE ON confidential_reply_outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_immutable();

CREATE TRIGGER confidential_event_no_truncate BEFORE TRUNCATE ON confidential_event_receipts FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();

CREATE TRIGGER confidential_reply_no_truncate BEFORE TRUNCATE ON confidential_reply_bundles FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();

CREATE TRIGGER confidential_execution_no_truncate BEFORE TRUNCATE ON confidential_execution_leases FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();

CREATE TRIGGER confidential_outbox_no_truncate BEFORE TRUNCATE ON confidential_reply_outbox FOR EACH STATEMENT EXECUTE FUNCTION ortak_confidential_execution_immutable();

CREATE FUNCTION ortak_confidential_execution_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
BEGIN
    RAISE EXCEPTION 'ortak: schema77 bootstrap requires reconciliation' USING ERRCODE='object_not_in_prerequisite_state';
END
$$;

CREATE TRIGGER confidential_execution_guard BEFORE INSERT OR UPDATE ON confidential_execution_leases FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_guard();

CREATE FUNCTION ortak_confidential_execution_commit() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE fresh BOOLEAN:=TG_OP='INSERT';
BEGIN
 IF NOT EXISTS(SELECT 1 FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id) THEN
  RAISE EXCEPTION 'Confidential execution community mismatch' USING ERRCODE='check_violation'; END IF;
 IF TG_TABLE_NAME='confidential_execution_leases' THEN
  fresh:=NEW.state IN('observing','sealing') AND (TG_OP='INSERT' OR NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL);
  IF NEW.state='stopped' AND NOT EXISTS(SELECT 1 FROM public.runtime_cancellations WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND state='acknowledged') THEN
   RAISE EXCEPTION 'Confidential stopped state lacks containment acknowledgement' USING ERRCODE='check_violation'; END IF;
  IF NEW.state IN('complete','sealing') AND NOT EXISTS(SELECT 1 FROM public.runs r
     WHERE r.company_id=NEW.company_id AND r.id=NEW.run_id AND r.status='completed'
      AND ((r.delivery_intent='silent' AND NEW.state='complete'
          AND (SELECT count(*) FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id)=3)
       OR (r.delivery_intent='reply'
          AND (SELECT count(*) FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id)=4
          AND EXISTS(SELECT 1 FROM public.confidential_run_payloads WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND purpose='reply_draft' AND ordinal=0)
          AND (NEW.state='sealing' OR EXISTS(SELECT 1 FROM public.confidential_reply_bundles WHERE company_id=NEW.company_id AND run_id=NEW.run_id))))) THEN
   RAISE EXCEPTION 'Confidential terminal projection is incomplete' USING ERRCODE='check_violation'; END IF;
 ELSIF TG_TABLE_NAME='confidential_reply_outbox' AND TG_OP='UPDATE' THEN
  fresh:=NEW.lease_token IS DISTINCT FROM OLD.lease_token AND NEW.lease_token IS NOT NULL;
 END IF;
 IF fresh AND NOT public.ortak_confidential_dm_current(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential execution authority expired at commit' USING ERRCODE='serialization_failure'; END IF;
 IF TG_TABLE_NAME='confidential_reply_bundles' THEN
  IF (SELECT count(*) FROM public.confidential_reply_outbox WHERE company_id=NEW.company_id AND run_id=NEW.run_id)<>2
   OR NOT EXISTS(SELECT 1 FROM public.confidential_run_payloads WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND purpose='reply_draft' AND ordinal=0) THEN
   RAISE EXCEPTION 'Confidential reply freeze is incomplete' USING ERRCODE='check_violation'; END IF;
 END IF;
 IF TG_TABLE_NAME='confidential_run_payloads' THEN
  IF NEW.purpose='runtime_event' AND NOT EXISTS(SELECT 1 FROM public.confidential_event_receipts WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND ordinal=NEW.ordinal) THEN
   RAISE EXCEPTION 'Confidential event time receipt absent' USING ERRCODE='check_violation'; END IF;
 END IF;
 RETURN NEW;
END
$$;

CREATE CONSTRAINT TRIGGER confidential_execution_at_commit AFTER INSERT OR UPDATE ON confidential_execution_leases DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();

CREATE CONSTRAINT TRIGGER confidential_event_at_commit AFTER INSERT ON confidential_event_receipts DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();

CREATE CONSTRAINT TRIGGER confidential_reply_at_commit AFTER INSERT ON confidential_reply_bundles DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();

CREATE CONSTRAINT TRIGGER confidential_outbox_at_commit AFTER INSERT OR UPDATE ON confidential_reply_outbox DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();

CREATE CONSTRAINT TRIGGER confidential_event_payload_at_commit AFTER INSERT ON confidential_run_payloads DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ortak_confidential_execution_commit();

CREATE FUNCTION ortak_confidential_reply_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE identity JSONB; wire JSONB; bytes BYTEA; target TEXT; expected BYTEA; n INTEGER;
BEGIN
 IF NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) OR NOT EXISTS(SELECT 1 FROM public.runs WHERE company_id=NEW.company_id AND id=NEW.run_id AND status='completed' AND delivery_intent='reply') THEN
  RAISE EXCEPTION 'Confidential reply has no current completion' USING ERRCODE='check_violation'; END IF;
 SELECT convert_from(identity_bytes,'UTF8')::jsonb INTO STRICT identity FROM public.confidential_runs WHERE company_id=NEW.company_id AND run_id=NEW.run_id AND community_id=NEW.community_id;
 FOR n IN 0..1 LOOP
  bytes:=CASE n WHEN 0 THEN NEW.recipient_bytes ELSE NEW.history_bytes END;
  expected:=CASE n WHEN 0 THEN NEW.recipient_id ELSE NEW.history_id END;
  target:=identity->>(CASE n WHEN 0 THEN 'human_public_key' ELSE 'employee_public_key' END);
  wire:=convert_from(bytes,'UTF8')::jsonb;
  IF jsonb_typeof(wire)<>'object' OR NOT wire ?& ARRAY['id','pubkey','created_at','kind','tags','content','sig']
   OR wire-ARRAY['id','pubkey','created_at','kind','tags','content','sig']<>'{}'::jsonb
   OR wire->>'id' IS DISTINCT FROM encode(expected,'hex') OR wire->'kind' IS DISTINCT FROM '1059'::jsonb
   OR wire->'tags' IS DISTINCT FROM jsonb_build_array(jsonb_build_array('p',target))
   OR ((wire->>'pubkey')~'^[0-9a-f]{64}$') IS DISTINCT FROM true OR ((wire->>'sig')~'^[0-9a-f]{128}$') IS DISTINCT FROM true
   OR jsonb_typeof(wire->'created_at') IS DISTINCT FROM 'number'
   OR ((wire->>'created_at')~'^(0|[1-9][0-9]{0,11})$') IS DISTINCT FROM true
   OR jsonb_typeof(wire->'content') IS DISTINCT FROM 'string' OR octet_length(wire->>'content') NOT BETWEEN 132 AND 60000 THEN
   RAISE EXCEPTION 'Confidential reply copy mismatch' USING ERRCODE='check_violation'; END IF;
 END LOOP;
 RETURN NEW;
END
$$;

CREATE TRIGGER confidential_reply_guard BEFORE INSERT ON confidential_reply_bundles FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reply_guard();

CREATE FUNCTION ortak_confidential_reply_lease_guard() RETURNS TRIGGER
LANGUAGE plpgsql SET search_path=pg_catalog,public,pg_temp AS $$
DECLARE deadline TIMESTAMPTZ; fresh BOOLEAN:=false;
BEGIN
 SELECT c.execution_deadline INTO STRICT deadline FROM public.confidential_runs c WHERE c.company_id=NEW.company_id AND c.run_id=NEW.run_id AND c.community_id=NEW.community_id;
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'pending' OR NEW.attempts<>0 OR NEW.generation<>0 OR NEW.lease_token IS NOT NULL THEN RAISE EXCEPTION 'Invalid confidential output admission' USING ERRCODE='check_violation'; END IF;
 ELSE
  IF (NEW.company_id,NEW.community_id,NEW.run_id,NEW.copy) IS DISTINCT FROM(OLD.company_id,OLD.community_id,OLD.run_id,OLD.copy) OR OLD.state<>'pending' THEN
   RAISE EXCEPTION 'Confidential output identity or terminal result changed' USING ERRCODE='check_violation'; END IF;
  IF NEW.attempts=OLD.attempts+1 AND NEW.generation=OLD.generation+1 AND NEW.lease_token IS NOT NULL AND NEW.lease_token IS DISTINCT FROM OLD.lease_token THEN
   IF NEW.state<>'pending' OR OLD.next_attempt_at>clock_timestamp()
    OR (OLD.lease_expires_at IS NOT NULL AND OLD.lease_expires_at+interval '5 seconds'>clock_timestamp())
    OR NEW.lease_expires_at<=clock_timestamp() OR NEW.lease_expires_at>least(deadline,clock_timestamp()+interval '30 seconds') THEN
    RAISE EXCEPTION 'Confidential output lease refused' USING ERRCODE='check_violation'; END IF;fresh:=true;
  ELSIF NEW.attempts=OLD.attempts AND NEW.generation=OLD.generation AND NEW.lease_token IS NULL THEN
   -- A known ACK for the unchanged locked owner is receipt-only after expiry.
   -- Pending retry still needs a live lease and cannot gain new authority here.
   IF NEW.state='acked' AND OLD.lease_token IS NULL
    OR NEW.state='pending' AND (OLD.lease_token IS NULL OR OLD.lease_expires_at<=clock_timestamp())
    OR NEW.state='pending' AND (NEW.attempts>=3 OR NEW.next_attempt_at<statement_timestamp()+interval '5 seconds') THEN
    RAISE EXCEPTION 'Confidential output settlement refused' USING ERRCODE='check_violation'; END IF;
  ELSE RAISE EXCEPTION 'Confidential output generation mismatch' USING ERRCODE='check_violation'; END IF;
 END IF;
 IF fresh AND NOT public.ortak_lock_confidential_dm(NEW.company_id,NEW.run_id) THEN
  RAISE EXCEPTION 'Confidential output authority retired' USING ERRCODE='check_violation'; END IF;
 RETURN NEW;
END
$$;

CREATE TRIGGER confidential_reply_lease_guard BEFORE INSERT OR UPDATE ON confidential_reply_outbox FOR EACH ROW EXECUTE FUNCTION ortak_confidential_reply_lease_guard();

CREATE FUNCTION ortak_routing_notify() RETURNS TRIGGER AS $$
DECLARE
    message TEXT;
BEGIN
    IF TG_TABLE_NAME = 'routing_decisions' THEN
        message := encode(NEW.message_id, 'hex');
    ELSIF TG_TABLE_NAME <> 'office_authority_generations' THEN
        RAISE EXCEPTION 'invalid routing notification source' USING ERRCODE='55000';
    END IF;
    PERFORM pg_notify('ortak_routing_v1', json_build_object(
        'company_id', NEW.company_id, 'message_id', message)::TEXT);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_routing_decisions_notify AFTER INSERT ON routing_decisions
    FOR EACH ROW EXECUTE FUNCTION ortak_routing_notify();

CREATE TRIGGER trg_routing_authority_notify AFTER INSERT OR UPDATE ON office_authority_generations
    FOR EACH ROW EXECUTE FUNCTION ortak_routing_notify();
