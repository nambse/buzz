# Buzz Baseline Assessment

Status: Accepted for Ortak Architecture v0  
Assessment date: 2026-09-04  
Source: `https://github.com/block/buzz`  
Pinned source commit: `b1f6b7ef770dddbb7f33c9f5861c379a47bca1d6`  
License: Apache-2.0

## Decision

Ortak starts from this Buzz source snapshot, but it is not a downstream Buzz
distribution. The repository is now on `ortak/main`; the original remote is
named `buzz-reference` and has no push URL. We will not merge upstream releases
or preserve Buzz API, database, event-kind, configuration, or UI compatibility.

The fork is useful because it contains mature implementations of signed events,
realtime chat, authentication, persistence, search, audit, desktop networking,
and agent activity rendering. It is not the product model we want. Ortak owns
every retained line and may replace it when the new company model requires it.

## What Buzz is today

The inspected snapshot is a Rust workspace with a Tauri 2 / React 19 desktop
client and additional web and Flutter clients. Its relay is the canonical
ingress/read and orchestration boundary; PostgreSQL's `events` table is the
durable event store. Auth and ephemeral event ranges are fan-out-only:

- `crates/buzz-core`: Nostr event types, signature verification, filters, kind
  registry, channels, presence, and tenant types. It is deliberately I/O-free.
- `crates/buzz-relay`: Axum WebSocket/HTTP server and the event-ingestion
  pipeline. It coordinates auth, database, fan-out, search, audit, media, git,
  workflow, and subscriptions.
- `crates/buzz-auth`: NIP-42 and NIP-98 authentication and authorization scopes.
- `crates/buzz-db`: Postgres event store plus relational channel, member, user,
  thread, reaction, workflow, git, and administration projections. Messages
  themselves remain signed rows in the event store.
- `crates/buzz-pubsub`: Redis fan-out, presence, and typing state.
- `crates/buzz-search`: Postgres full-text search with caller-owned permission
  filtering.
- `crates/buzz-audit`: append-only, hash-chained audit log.
- `crates/buzz-sdk`, `crates/buzz-ws-client`, `crates/buzz-cli`: typed event
  builders and tested client transport.
- `crates/buzz-acp`: a per-agent ACP harness that listens for relay events and
  starts agent subprocesses.
- `desktop`: a feature-organized Tauri/React application. Relevant foundations
  include shared relay networking, chat/messages, agent activity, terminal/file
  renderers, design-system primitives, and update/reconnect behavior.

The key product mismatch is that Buzz treats humans and independently connected
agents as peers in a communication community. Ortak treats employees as managed
company resources. Their identity is durable, their runtime is replaceable, and
one company router decides whether a message becomes work for an employee.

## Reuse, rewrite, and removal matrix

The labels describe ownership, not upstream compatibility.

| Buzz area | v0 disposition | Ortak use | Evidence in snapshot |
|---|---|---|---|
| Signed event envelope and verification | Reuse, then rename | Durable Office message identity and integrity | `crates/buzz-core/src/event.rs`, `verification.rs`, `filter.rs` |
| Kind registry and channel primitives | Adapt | Keep chat-compatible kinds where useful; add Ortak domain events without promising Buzz compatibility | `crates/buzz-core/src/kind.rs`, `channel.rs` |
| NIP-42/NIP-98 auth | Reuse | Human/client authentication at the Office boundary | `crates/buzz-auth/src/nip42.rs`, `nip98.rs`, `scope.rs` |
| Relay WebSocket, ingest, subscriptions | Reuse infrastructure; rewrite orchestration | Office realtime transport; route accepted messages through the Ortak application pipeline | `crates/buzz-relay/src/handlers`, `subscription.rs` |
| Postgres event store, channels, DMs, reactions, threads | Reuse and adapt | Office history and projections | `crates/buzz-db/src/store/event.rs`, `crates/buzz-db/src/store/channel.rs`, `crates/buzz-db/src/store/dm.rs`, `crates/buzz-db/src/store/reaction.rs`, `crates/buzz-db/src/store/thread.rs` |
| Redis fan-out, presence, typing | Reuse | Ephemeral UI state and multi-node fan-out | `crates/buzz-pubsub` |
| Search | Reuse and extend | Permission-aware Office search; later Work and artifacts | `crates/buzz-search` |
| Hash-chain audit | Reuse and extend | Company control actions, routing, provisioning, permissions | `crates/buzz-audit` |
| Media/Blossom storage | Defer, then reuse | Office attachments and run artifacts | `crates/buzz-media` |
| Desktop shared UI and networking | Reuse selectively | Tauri shell, websocket/reconnect, message timeline, accessible primitives | `desktop/src/shared`, `desktop/src/features/chat`, `messages` |
| Agent session activity renderers | Adapt heavily | Runs, tool calls, terminal, files, raw event rail | `desktop/src/features/agents/ui/AgentSession*`, `FileEditDiffView.tsx`, `RawEventRail.tsx` |
| Existing agent/persona model | Replace | Ortak Employee aggregate, revisions, permissions, runtime and memory bindings | `desktop/src/features/agents`, `desktop/src-tauri/src/managed_agents` |
| ACP mention harness | Do not use as steady-state architecture | Reference its queueing, cancellation, and process-safety tests; central dispatcher calls Hermes | `crates/buzz-acp` |
| Buzz workflow engine | Remove after Work cutover | Ortak Work owns tasks, assignments, approvals, and runs | `crates/buzz-workflow`, DB workflow projections |
| Buzz project/git forge | Remove from v0 | Ortak Project is a business/work container, not a hosted git forge | relay git handlers, `crates/git-*`, `web` |
| Multi-community hosting | Remove from v0 | One company deployment first; preserve explicit `company_id` in new tables | `crates/buzz-core/src/tenant.rs`, community DB code |
| Mesh compute and Kubernetes agent providers | Remove from v0 | Runtime placement belongs behind `RuntimeAdapter`; Hermes is first | `crates/buzz-relay-mesh`, `crates/buzz-backend-kubernetes`, desktop mesh code |
| Huddles/voice | Remove from v0 | Not part of the AI-company control loop | relay audio/huddle and `crates/buzz-voice` |
| Mobile, pairing, push gateway, moderation, culture extras | Remove/defer | Re-evaluate after the desktop company workflow is complete | `mobile`, pairing crates, push gateway, moderation features |
| Buzz branding, onboarding, community UX | Rewrite | Office, Employees, Work, Projects, Memory, Activity, Settings | desktop routes and navigation |

### Private relay source boundary (2026-09-06)

The inherited relay mesh/huddle, workflow engine, and git forge are archived
behind the default-on `legacy-mesh`, `legacy-workflow`, and `legacy-git` Cargo
features. Ortak private relay artifacts select `--no-default-features`; ordinary
development builds retain their inherited defaults. The private source graph
omits workflow construction, cron/event wake hooks, workflow commands/webhooks,
and git HTTP/policy routes, git cache/store construction and conformance probes.
The shared ingest path refuses writes to those workflow and NIP-34 families.
Explicit git GUI/probe or mesh/huddle activation settings fail before startup
I/O when their feature is absent.

This boundary retains canonical Office ingest, central routing, membership and
identity recovery, media storage, historical event/DB projections, and authorized
cleanup of historical workflow definitions. It does not reclassify a Buzz
project event as an Ortak Project. The direct relay `buzz-workflow`,
`buzz-relay-mesh`, git S3/tempfile/compression, and mesh postcard edges become
optional; the unused direct `buzz-admin` workflow edge is removed. Shared media
may still require S3 and test fixtures may still require tempfile. Workspace
source and historical data are preserved. The integration owner deployed this
private boundary on 2026-09-06 with selected normal/build graph evidence,
four production-seam tests and actual404 workflow/git route observations.
The current artifact/owner references are in `CONTINUATION_PROGRESS_2026-09-05.md`;
this private cutover does not claim full repository CI or a public release.

## Code-import policy

1. The pinned Buzz commit remains recorded in this document and in git history.
2. Apache-2.0 license and required attribution remain in the repository.
3. No automated upstream merge or compatibility test will be added.
4. During the transition, existing `buzz-*` crates may remain as implementation
   names. New product-domain crates use `ortak-*` names.
5. A retained Buzz module must have an explicit Ortak owner and a target
   disposition in this document. Unowned modules are candidates for deletion.
6. Deletion happens after the replacement path is exercised, not as a cosmetic
   mass rename. The build graph, not directory names, determines the cutover.
7. Individual upstream commits may be cherry-picked when the user approves
   them, preserving author and source SHA. Each import is recorded in a dated
   `BUZZ_IMPORT_<date>.md` with accepted and deferred lists. The pinned
   snapshot above does not move. First record: `BUZZ_IMPORT_2026-09-05.md`
   (8 accepted, 5 deferred, from upstream `f038cbbb`); see that record for
   the import scope and the verification actually run.

## Important gaps that Ortak must not inherit

- Relay fan-out is not a routing policy. Allowing every employee runtime to
  subscribe independently recreates wake storms and employee-to-employee loops.
- Buzz addresses an agent by a Nostr public key backed by signing material. The
  ACP harness/runtime is replaceable but operates as that transport identity.
  Ortak Employee identity must remain separate from both signing credentials
  and runtime instances, and survive runtime, model, profile, and key rotation.
- Activity is mostly reconstructed client-side from agent protocol traffic.
  Ortak needs a normalized, durable run-event model for supervision and audit.
  Relevant inherited seams are `crates/buzz-core/src/observer.rs`,
  `desktop/src-tauri/src/archive/retention.rs`, and
  `desktop/src/features/agents/observerRelayStore.ts`.
- Buzz workflows are channel automation, not company work management.
- Secrets and runtime configuration are managed as agent launch details. Ortak
  needs credential references, versioned employee configurations, provisioning
  operations, health checks, and rollback/adoption behavior.

## External operational facts to verify before migration

- No Cem or Zeynep Hermes profiles were found on this Mac. They must not be
  fabricated from local state.
- The currently documented deployed profiles are referenced as
  `/opt/data/profiles/cem` and `/opt/data/profiles/zeynep` in the Ortak Runtime
  environment. The v0 seed manifests use these as external references only.
- Existing private keys and `auth.json` content are never copied into git.
- Cem and Zeynep are intended to be adopted, not recreated or deleted. These
  operational facts do not come from the Buzz source snapshot; provisioning
  remains in draft/dry-run mode until the deployed environment, signing
  identity, and backup/recovery path have been verified.
