# Ortak Architecture v0

Status: Accepted for implementation  
Date: 2026-09-04  
Companion documents: `BUZZ_BASELINE.md`, `IMPLEMENTATION_PLAN_V0.md`

## 1. Product boundary

Ortak is the operating workspace for an AI-native company. It is where a human
talks with the company, hires/configures employees, assigns and observes work,
reviews artifacts, and manages durable company memory.

Top-level product surfaces are:

- **Office**: channels, DMs, threads, attachments, and employee conversation.
- **Employees**: durable identities, role/persona, capabilities, permissions,
  runtime, model, memory, availability, and provisioning.
- **Work**: tasks, assignments, approvals, deliverables, dependencies, and runs.
- **Projects**: context and policy boundary for related work and artifacts.
- **Activity**: routing decisions, runs, turns, tools, commands, files, memory,
  errors, and audit history.
- **Memory**: company truth, employee memory, project context, recall/write
  visibility, and retention controls.
- **Settings**: company, integrations, credentials, policies, and deployment.

“Agent” is an implementation term. The product object is an **Employee**.

## 2. Invariants

These are architecture constraints, not aspirations:

1. **Employee is not a session.** An employee survives process restarts, model
   changes, profile moves, and individual runs.
2. **One inbound message gets one dispatching routing decision.** The unique
   key is `(company_id, message_id)` and the row pins the policy/scorer version
   used. A later re-evaluation is a separate, non-dispatching audit record.
   Employee runtimes do not independently subscribe to Office and decide
   whether to wake.
3. **Deterministic rules run before semantics.** DMs, explicit targets, replies,
   structured dispatch, system events, and loop guards do not require a model.
4. **Silence is valid.** The router may choose zero recipients; a runtime may
   choose `silent` delivery.
5. **Employee-origin content cannot trigger text parsing or semantic fan-out.**
   It may wake another employee only through a server-resolved DM participant,
   authorized structured dispatch/mention, or direct reply.
6. **Every wake is explainable.** Policy version, candidates, scores, exclusions,
   reason, and final recipients are durable.
7. **Delivery-chain authority is durable and serialized.** Every root has one
   database row and one unique reservation per visited employee. Pure
   `DeliveryChain` values are snapshots for early rejection and tests, never
   the concurrency authority for spending hop or wake budgets.
8. **Accepted Office input cannot fall between stores.** The signed event and a
   durable Ortak inbox row are committed in one database transaction before
   acknowledgement. Routing state and delivery work are then committed through
   idempotent transactions and an outbox.
9. **Runtime and memory are adapters.** Hermes and Honcho are first backends,
   not Employee identity or the Ortak domain model.
10. **Secrets are referenced, not copied.** Git and normal database rows contain
   credential references, never provider tokens or private keys.
11. **Observability is a product contract.** A run never disappears into an
    opaque process; waiting, work, failure, cancellation, and silence are states.

## 3. System context

```text
┌──────────────────────────────── ORTAK DESKTOP ───────────────────────────────┐
│ Office │ Employees │ Work │ Projects │ Activity │ Memory │ Settings          │
└───────────────────────────────┬───────────────────────────────────────────────┘
                                │ signed commands + queries + event stream
                                ▼
┌──────────────────────────── ORTAK SERVER ────────────────────────────────────┐
│ Auth / Policy                                                             │
│     │                                                                      │
│ Office ingest ──► Conversation Router ──► Run Dispatcher ──► Delivery      │
│     │                    │                     │                 │           │
│     ├──► Work / Projects │                     │                 └─► Office  │
│     ├──► Employee Control│                     │                             │
│     └──► Activity        │                     │                             │
│                          └── Routing Decision  └── RuntimeAdapter            │
│                                                        │                    │
│                                        ┌───────────────┴──────────────┐     │
│                                        │ Hermes adapter (first)       │     │
│                                        │ future runtime adapters      │     │
│                                        └───────────────┬──────────────┘     │
│                                                        │ run events          │
│ Memory policy ─────────────────────────────────────────┤                    │
│        │                                               │                    │
│        └── MemoryAdapter ──► Honcho (first)             │                    │
└───────────────┬─────────────────────┬───────────────────┴────────────────────┘
                │                     │
         PostgreSQL + outbox     Redis / realtime       Object storage
```

The current Buzz relay remains the Office transport during migration. For every
routable accepted event, the relay transaction also inserts an idempotent
`office_inbox` row before acknowledging the sender. The Ortak control plane
consumes that durable inbox; it is never invoked by a best-effort post-commit
callback. A reconciliation job backfills accepted message events missing an
inbox row before any cohort is cut over. The relay does not call employee
gateways directly.

Buzz `community_id` and Ortak `company_id` are joined only through a unique,
server-owned deployment mapping resolved from the authenticated host. Unknown
mappings are rejected, and client-supplied company identifiers are never
trusted or copied into Ortak records.

## 4. Component model

### 4.1 `ortak-domain`

Pure domain types and validation; no database, network, process, or UI imports.

Primary aggregates:

- `Company`
- `Employee` and immutable `EmployeeRevision`
- `Conversation`, `MessageEnvelope`, and the pure `DeliveryChain` snapshot
- `RoutingPolicy` and `RoutingDecision`
- `Project`, `WorkItem`, `Assignment`
- `Run` and `RunEvent`
- `ProvisioningOperation`

Employee revisions contain role/persona, responsibilities, domains, capability
and permission policies, runtime binding, memory binding, aliases, and routing
policy. A run records the exact revision it started with.

### 4.2 `ortak-router`

A pure policy engine. It receives normalized messages and an already-filtered
employee roster. If semantics are needed, it emits a least-privilege scoring
request and later applies a bounded score set or failure. The async control
layer owns deadlines/cancellation and adapter calls; policy code never calls an
LLM SDK or blocks on network I/O.

Routing order:

```text
normalize
  → consume idempotent durable Office inbox row
  → reject duplicate / non-routable event
  → read message, policy, roster, and delivery-chain snapshot
  → resolve deterministic candidates
  → if human-origin and still untargeted: semantic scoring outside a DB tx
  → begin a short authoritative transaction
  → fence the inbox claim and one-decision key
  → lock/create the durable root chain row and refresh authoritative inputs
  → reapply eligibility, thresholds, recipient cap, hop and wake budgets
  → reserve unique employee visits and advance the chain counters
  → persist decision, recipients, and dispatch outbox in the same commit
```

Deterministic rules, in priority order:

1. System, reaction-only, and delivery acknowledgement events: `DROP`.
2. DM: server-resolved employee participants.
3. Authorized structured dispatch target: the named employee IDs.
4. Trusted structured mention: the mentioned employee IDs.
5. Direct reply to an employee-authored message: that employee.
6. Unique explicit `@alias` or leading vocative name such as `Cem, ...`: that
   employee, but only for human-origin text. Alias uniqueness is validated when
   an EmployeeRevision is saved and the longest boundary-valid alias wins.
7. Human-authorized structured Work assignment: the assigned employees.

DM participants, structured targets, structured mentions, reply context, and
Work assignments are trusted only when derived by the authenticated Office
adapter, Work service, or a validated typed runtime delivery. Arbitrary model
text cannot populate these fields. Runtime output cannot mint a system origin,
reset delivery-chain state, or invent an unapproved target.

Authoritative v0 origin matrix:

| Origin | Server-resolved DM | Structured dispatch / mention / reply | Raw alias / vocative | Work assignment | Semantic |
|---|---:|---:|---:|---:|---:|
| Human | allow | allow | allow | allow | allow |
| Employee | allow | allow | deny | deny | deny |
| Integration | deny by default | capability required | deny | capability required | deny |
| System | deny by default | capability required | deny | capability required | deny |

If no deterministic target exists:

- Human-origin Office messages may enter semantic routing.
- Employee-origin, integration-origin, and system-origin messages without one
  of the trusted deterministic routes resolve to silence. They never enter
  raw-alias parsing or semantic routing. Integration and system routes require
  an explicit server-side capability; v0 grants neither by default.
- The semantic scorer returns a bounded score and evidence labels for eligible
  candidates. Policy applies the configured threshold and recipient cap.

The production semantic adapter sends only the bounded message, candidate
identity/name/title/biography/responsibilities/domains, and an explicitly
allowed project or conversation summary. It excludes credentials, raw memory,
tool output, and
out-of-scope private-channel context. It must return exactly one valid score per
eligible candidate. Missing, duplicate, malformed, or timed-out results fail
closed to a recorded silent decision; a late response never dispatches work.
Each decision pins adapter, model, prompt, and scorer versions plus latency and
usage metadata. Safe caching keys include the content hash, candidate revision
set, policy version, and scorer version. Remote scoring runs outside database
transactions; only the bounded request/result metadata enters persistence.

The inbox worker first reads a scorer-input snapshot and releases its short
claim transaction before calling the remote scorer. When the result returns,
the worker opens the authoritative commit transaction, fences the inbox claim
generation, locks the root delivery-chain row, and reloads the message, policy,
candidate revisions, and eligibility. If an input that affected scoring changed,
the transaction rolls back and the worker re-scores outside a transaction with a
bounded retry count. If only chain counters or visited reservations changed, the
scores remain evidence, but policy is reapplied against the refreshed chain and
may drop recipients. A late scorer response, stale claim generation, or losing
worker cannot reserve a visit or create a second dispatching decision. Exhausting
the bounded refresh/re-score attempts produces an explainable silent decision;
no abandoned or later scorer response may wake an employee.

Loop prevention is structural:

- `root_message_id` identifies a delivery chain. One `delivery_chains` row per
  `(company_id, root_message_id)` is the authority for its pinned limits,
  `hop_count`, and `wake_count`.
- `delivery_chain_visits` reserves each employee at most once under a unique
  `(company_id, root_message_id, employee_id)` constraint. A reservation is
  consumed when the decision/outbox transaction commits and is not released
  merely because runtime delivery later retries or fails.
- Every decision that may wake an employee locks the root chain row before
  applying guards. Reservations, counter updates, the decision, recipients,
  and dispatch outbox rows commit atomically in one short transaction. Sibling
  branches therefore serialize; the database uniqueness constraint is the
  final backstop against two stale workers visiting the same employee.
- `hop_count` counts successfully committed dispatch batches, including the
  initial human-to-employee batch. A batch with one or more newly reserved WAKE
  recipients increments it once; silent/drop-only decisions do not. With the v0
  default `max_hops = 2`, the initial human batch consumes hop 1 and at most one
  subsequent employee-delegation batch can consume hop 2. Concurrent sibling
  delegations race for that remaining batch; only the transaction that locks and
  commits first may reserve recipients.
- `wake_count` increases by the number of newly reserved employees in the
  committed batch. `max_recipients` and the root's pinned wake budget bound
  both per-decision and cumulative fan-out.
- The dispatch idempotency key is `(routing_decision_id, employee_id)`.
- Runtime output is typed as `reply`, `channel`, or `silent`; plain model text
  is not automatically rebroadcast as a new employee request.
- Delivery-chain state is constructed and advanced only by the server. A pure
  `DeliveryChain` snapshot is materialized from the durable root and visit rows
  and may let the router reject obviously exhausted work early, but it is
  defense-in-depth only and cannot authorize a dispatch. Chain state is never
  accepted from a client or model-produced content.

The durable decision shape is conceptually:

```json
{
  "message_id": "...",
  "mode": "deterministic | semantic | silent",
  "policy_version": "routing-v0",
  "policy_fingerprint": "sha256:...",
  "recipients": [
    {
      "employee_id": "cem",
      "action": "wake",
      "reason": "explicit_alias",
      "score": null
    }
  ]
}
```

### 4.3 `ortak-control`

Application services and transaction boundaries:

- employee create/adopt/update/disable;
- provisioning saga and health checks;
- inbox claim, semantic preflight, and authoritative routing commit;
- delivery-chain row locking and unique employee-visit reservation;
- routing-to-run dispatch;
- run cancellation and delivery;
- work/project mutations;
- credential-reference and permission validation.

The control layer owns idempotency and the outbox. Adapters cannot mutate domain
tables directly.

### 4.4 `ortak-runtime`

Runtime port:

```text
validate(binding) -> capabilities + health
provision(employee_revision, mode=create|adopt) -> binding result
start_run(run_spec, context) -> runtime_run_ref
stream_events(runtime_run_ref, cursor) -> ordered runtime events
cancel(runtime_run_ref, reason) -> terminal acknowledgement
status(binding or runtime_run_ref) -> normalized health/status
```

The first adapter targets Hermes. It maps an employee revision to a Hermes
profile reference, model configuration, workspace, tool policy, and credential
references. It does not make Hermes the source of Employee truth.

Hermes run/profiles APIs must be capability-probed at startup. API shapes are
versioned in the adapter and are not assumed from a dashboard version string.
Where the deployed Hermes version lacks a stable API, a narrowly scoped provider
process may bridge the operation; this remains behind the same port.

### 4.5 `ortak-memory`

Memory port:

```text
recall(employee, actor, project, conversation, query, budget) -> memory context
remember(employee, run, facts, provenance) -> write receipts
inspect(scope, cursor) -> normalized memory records
forget(scope, record, reason) -> audited outcome
health(binding) -> status
```

The first adapter targets Honcho. Namespaces are explicit:

- company truth;
- project context;
- employee experiential memory;
- human/employee relationship memory;
- run/session scratch context.

Canonical company files remain authoritative for policy and facts that require
review. Honcho memory is experiential context, not a replacement for those
files. Every recalled item carries source/provenance; every write records its
employee, run, scope, and adapter receipt.

### 4.6 `ortak-observability`

Runtime-specific streams are normalized into durable `RunEvent` records:

- run queued/started/waiting/completed/failed/cancelled;
- turn started/completed;
- message/thought summary;
- tool call started/completed/failed;
- terminal command started/output/completed;
- file read/edit and diff summary;
- artifact created;
- memory recall/write;
- permission requested/resolved;
- usage/model telemetry;
- heartbeat, silence, timeout, and runtime disconnect.

Payloads are bounded and redacted before persistence. Large output and files go
to object storage with size/hash/media metadata. The Activity UI renders
semantics first and exposes raw payloads only on demand.

### 4.7 Office and delivery

Office retains signed, realtime conversations. The normalized input boundary
decouples routing from Nostr/Buzz shapes. A message adapter translates existing
events into `MessageEnvelope`; a later transport change will not rewrite the
router.

Office ingestion and delivery have separate durable seams:

```text
accepted signed event + office_inbox row  --one transaction, then ACK-->
inbox claim + snapshot --short transaction-->
optional semantic scoring --outside any database transaction-->
refresh + root-chain lock + visit reservations + decision + dispatch outbox
  --one short idempotent transaction-->
run/delivery outbox --idempotent publish--> signed Office event
```

The first implementation must extend the existing Postgres event insertion
transaction to write `office_inbox`; central routing remains disabled until
that atomic path and the reconciliation scan are deployed. Semantic scoring
never holds the Buzz event transaction open. Unknown or failed inbox rows keep
a durable terminal/retryable state for operator inspection.

Employee-authored Office events are signed through an `OfficeSigner` port:

```text
sign(employee_id, employee_revision, unsigned_event, signer_ref)
  -> verified signed event
```

`signer_ref` is an opaque credential-manager/KMS/remote-signer reference. Ortak
never stores private keys in manifests or ordinary database rows. Provisioning
proves that the signer produces the configured public key before activation.
Rotation creates a new binding revision, keeps historical signatures
verifiable, and supports an overlap/revocation window. Delivery persists the
exact signed event bytes and event id before its first publish; retries reuse
that event rather than re-signing with a new timestamp or event id.

Runtime completion returns a typed delivery intent:

- `reply`: reply to the triggering message/thread;
- `channel`: publish a new channel message with explicit context;
- `silent`: complete the run without publishing a message.

Delivery is idempotent and outbox-backed. An Office publish failure leaves a
retryable record; it never converts a successful run into an invisible success.

## 5. Data ownership

Postgres is the durable source of truth. Redis contains only rebuildable
ephemeral state and fan-out. Object storage contains content-addressed large
artifacts.

New tables/projections planned for the Ortak schema:

| Table | Purpose / key invariant |
|---|---|
| `companies` | Company boundary; v0 has one row but all new records carry `company_id` |
| `employees` | Stable identity and lifecycle status |
| `employee_revisions` | Immutable effective configuration; runs pin one revision |
| `employee_aliases` | Company-unique normalized aliases for deterministic routing |
| `employee_runtime_bindings` | Adapter and external profile reference; no secrets |
| `employee_memory_bindings` | Adapter, workspace, peer mapping, and policy |
| `employee_office_bindings` | Public key, opaque signer reference, validity window, and rotation metadata |
| `office_company_bindings` | Unique authenticated-host/community to Ortak company mapping |
| `office_inbox` | One durable accepted-message handoff keyed by company/event; inserted with the Office event |
| `delivery_chains` | Authoritative per-company/root pinned hop/wake limits and serialized counters |
| `delivery_chain_visits` | Unique employee reservation per company/root with decision and batch provenance |
| `routing_decisions` | Exactly one dispatching decision per company/input message; pins policy and scorer versions |
| `routing_re_evaluations` | Optional non-dispatching what-if/audit evaluations |
| `routing_recipients` | Candidate/action/reason/score/evidence for audit |
| `runs` | One employee execution, linked to message/work/revision |
| `run_events` | Ordered normalized activity with per-run sequence uniqueness |
| `projects` | Company work/context boundary |
| `work_items` | Task lifecycle, acceptance criteria, priority, parent/dependency data |
| `work_assignments` | Employee ownership and assignment history |
| `artifacts` | Content-addressed output and provenance |
| `provisioning_operations` | Durable multi-step create/adopt/update state machine |
| `outbox` | Transactional dispatch/delivery work; signed Office payload is frozen before publish |

Existing Buzz event/channel/message tables remain the Office store during the
transition. New domain code accesses them through an Office repository port,
not by importing arbitrary Buzz database modules.

## 6. Employee provisioning

Provisioning is a resumable saga, not a UI sequence of unrelated calls:

```text
validate manifest
  → reserve employee identity
  → resolve credential references
  → create or adopt Hermes profile
  → validate SOUL/config/workspace
  → create or adopt Honcho workspace/peers/identity
  → create or adopt Office signing identity and membership
  → publish Office employee profile
  → health probe runtime + memory + Office
  → activate employee revision
```

Every step records `pending/running/succeeded/failed/compensating` and an
idempotency key. Retry resumes at the failed step. Existing resources are tagged
as adopted and are never deleted by compensation.

Cem and Zeynep enter v0 as `adopt` fixtures in `draft` state:

- Cem: Co-Founder, Hermes profile `/opt/data/profiles/cem`, Office pubkey
  `345fe1a23fc0cc492f78b8c90535414e46536eb58b38d6170dabbd949969f1a6`.
- Zeynep: Mobile Ventures Lead, Hermes profile
  `/opt/data/profiles/zeynep`, Office pubkey
  `178af33ce482b480346598c45828d6d2d1a83df1494c4babfe424c227ae88e4d`.

The fixtures intentionally omit private keys, OAuth material, `auth.json`, and
Honcho/API secrets. The provisioning saga must validate runtime, memory, Office
membership, and signer/public-key correspondence before creating a new active
revision. Seed import cannot activate, remove, or recreate their profiles.

## 7. Work and Projects

`Project` groups goals, context, policies, work, artifacts, and relevant Office
conversations. It is not equivalent to a git repository.

`WorkItem` supports:

- goal, description, acceptance criteria, priority, state;
- parent/child decomposition and dependency links;
- human and employee assignments;
- related conversations, runs, artifacts, and approvals;
- append-only status history.

A routing decision may create a conversational run without a WorkItem. Longer
work can be promoted to Work and subsequent runs attach to it. This keeps casual
Office conversation lightweight without making serious work invisible.

## 8. Permissions and security

Permissions are evaluated before dispatch and again at the tool boundary.

- Employee policy specifies allowed projects, workspaces, tools, network scopes,
  credential refs, and approval requirements.
- Runtime credentials are least-privilege and scoped per employee where the
  provider supports it.
- Tool calls that exceed policy fail closed and emit a permission event.
- Secrets are redacted at ingestion and excluded from semantic-routing prompts,
  activity payloads, memory writes, and logs.
- Router inputs use bounded message/context sizes.
- Semantic output is treated as untrusted scoring data; deterministic policy,
  eligibility, caps, and loop guards remain authoritative.
- Semantic evidence is a bounded stable-code taxonomy, not model prose; adapter
  redaction still runs before persistence.
- The policy version is paired with a canonical content fingerprint so two
  different thresholds/limits cannot share one audit identity.
- Tool and approval identifiers are typed by the manifest schema. Adapter
  option schemas reject secret-like keys/values and keep credentials in opaque
  reference fields.
- Administrative mutations and provisioning steps are audited.

## 9. Desktop information architecture

Target navigation:

```text
Office
Work
Employees
Projects
Activity
Memory
Settings
```

Employee detail:

```text
Overview | Work | Memory | Skills | Tools | Activity | Terminal | Settings
```

The main status view shows structured state: available, routing, queued,
working, waiting for approval, blocked, failed, or offline. Terminal/raw logs
are debugging surfaces, not the primary explanation of work.

## 10. Repository target shape

The fork stays buildable while Ortak takes ownership by dependency boundary:

```text
crates/
  ortak-domain/          pure product types and validation
  ortak-router/          deterministic + semantic routing policy
  ortak-routing-semantic/ production scorer adapter, versioning, cache, telemetry
  ortak-control/         application services, transactions, outbox
  ortak-office/          signed event adapter, inbox, signer, and delivery port
  ortak-runtime/         runtime port + shared normalized events
  ortak-runtime-hermes/  Hermes adapter
  ortak-memory/          memory port
  ortak-memory-honcho/   Honcho adapter
  ortak-observability/   run-event normalization and redaction
  ortak-server/          composition root and APIs

desktop/src/features/
  office/
  employees/
  work/
  projects/
  activity/
  memory/

config/employees/        secret-free employee manifests and fixtures
docs/ortak/              product architecture and implementation decisions
```

Existing `buzz-*` crates remain only until their retained behavior is behind an
Ortak port or their replacement is exercised. The final codebase need not keep
their names or public interfaces.

## 11. v0 acceptance criteria

Architecture v0 is proven when all of these work in one deployed flow:

1. Cem and Zeynep are visible as adopted test employees without changing their
   existing profiles.
2. `Cem, selam nasılsın?` creates one deterministic decision and one Cem run.
3. An untargeted human message can semantically wake zero, one, or bounded
   multiple relevant employees, with scores/reasons visible.
4. Employee output cannot cause a Cem↔Zeynep loop without explicit dispatch.
5. A Hermes run streams normalized run/tool/terminal/file events into Activity.
6. Honcho recall/write activity is visible with scope and provenance.
7. A run can attach to Work, produce an artifact, and deliver `reply`, `channel`,
   or `silent` exactly once.
8. Provisioning can dry-run and adopt an employee; retries are resumable and do
   not delete pre-existing resources.
9. An acknowledged Office message survives process failure between ingest and
   routing, and reconciliation produces no duplicate dispatch.
10. Semantic scorer timeout or malformed output produces an explainable silent
    decision and never a late wake.
11. Two concurrent employee sibling branches cannot both spend the final hop or
    reserve the same employee. The initial human dispatch is hop 1, so the v0
    default permits at most one subsequent employee-delegation batch.

## 12. Deferred decisions

- Whether Office keeps Nostr as a long-term public contract or only as an
  internal signed-event envelope.
- Exact Hermes API/provider bridge after capability probing the deployed
  version.
- Honcho retention and deletion semantics after verifying the deployed API.
- Multi-company hosting, remote runtime placement, mobile, git forge, voice,
  workflow DSL, and public protocol interoperability.

These do not block the first end-to-end company loop.
