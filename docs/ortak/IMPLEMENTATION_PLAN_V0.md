# Ortak Implementation Plan v0

Status: Active  
Date: 2026-09-04  
Planning style: dependency-ordered milestones; no calendar estimates

## Delivery strategy

Build vertical company loops, not isolated screens. Each milestone must leave a
working path with production-seam tests. The fork stays runnable while new
Ortak modules take over one dependency boundary at a time.

The first loop is:

```text
employee fixtures
  → message
  → deterministic/semantic decision
  → idempotent run request
  → normalized activity
  → typed delivery intent
```

Hermes, Honcho, database, relay, and desktop integration are attached to that
loop in subsequent milestones. Upstream Buzz updates are not a workstream.

## Milestone 0 — Baseline and executable routing core

Outcome: the product boundary exists in the repo and the most important safety
policy runs as tested code.

Implementation state: complete in the current working tree; focused tests
exercise the domain fixtures and production router entry point.

- Pin and document the Buzz source snapshot and fork policy.
- Write Architecture v0 and this implementation plan before product code.
- Add `ortak-domain` with Employee, message origin/context, routing policy,
  routing decision, and validation types.
- Add `ortak-router` with deterministic rules, a two-phase bounded semantic
  request/result contract, eligibility filters, recipient caps, hop/visited
  guards, and stable explanations. Remote scoring is owned by async control,
  never called inline by the pure router.
- Add secret-free Cem and Zeynep `adopt` fixtures.
- Add typed credential/tool/approval fields, secret-safe validation errors,
  canonical policy fingerprints, bounded evidence labels, and absolute routing
  safety ceilings.
- Unit-test the real route entry point for direct name, structured mention,
  human semantic routing, employee-origin silence, caps, and loop prevention.
- Record the exact baseline and keep the Buzz remote fetch-only.

Exit gate:

- `cargo test -p ortak-domain -p ortak-router` is green.
- Removing a production routing guard makes at least one test fail.
- Fixtures contain no secrets and cannot request create/delete operations.

## Milestone 1 — Durable company control plane

Outcome: Employee and routing state is durable and idempotent.

- Add Ortak migrations for company, employee/revision/alias/bindings, routing
  decision/recipient, Office inbox/company/signer bindings, run/run-event,
  per-root delivery chain/unique employee visits, provisioning operation, and
  outbox tables.
- Implement repository ports and Postgres adapters.
- Enforce unique normalized aliases and immutable employee revisions.
- Add an idempotent Office inbox claim/finalize application service; semantic
  scoring occurs outside long database transactions.
- Implement an authoritative short routing transaction that fences the inbox
  claim generation, locks or creates the `(company_id, root_message_id)` chain
  row, refreshes policy/roster/message state, reapplies routing guards, reserves
  unique employee visits, advances hop/wake counters, and commits the decision,
  recipients, and dispatch outbox atomically.
- Treat pure `DeliveryChain` snapshots as early-rejection and test inputs only;
  they cannot authorize or spend a durable chain budget.
- Persist exactly one dispatching decision per `(company_id, message_id)` and
  pin policy, candidate revisions, scorer adapter/model/prompt versions, and
  input hash on that decision.
- Store any later what-if/re-evaluation as a separate non-dispatching audit row.
- Define `hop_count` as committed dispatch batches, including the initial human
  batch. Increment once only when the transaction reserves at least one WAKE;
  v0 `max_hops = 2` permits at most one later employee-delegation batch.
- Add outbox leases, bounded retries, terminal failure state, and operator retry.
- Extend hash-chain audit for employee, routing, permission, and provisioning
  control actions.

Exit gate:

- Replaying a message does not create a second decision or dispatch.
- Concurrent sibling branches cannot both consume the final hop, exceed the
  root wake budget, or reserve the same employee; the losing decision records
  the refreshed exclusion reason without a dispatch outbox row.
- Partial adapter failure leaves retryable outbox/provisioning state.
- Postgres integration tests exercise the production transaction path.

## Milestone 2 — Office, semantic scorer, and router cutover

Outcome: Office messages wake only router-selected employees.

- Define the normalized Office message adapter from existing signed events.
- Add a unique authenticated-host/Buzz-community to Ortak-company binding;
  reject unknown or client-supplied company identifiers.
- Extend Buzz event insertion so the signed event and `office_inbox` row commit
  atomically before acknowledgement. Add an idempotent reconciler for stored
  eligible events that lack inbox rows.
- Keep inbox consumption and semantic scoring outside the relay transaction;
  persist retryable/terminal failures instead of fire-and-forget callbacks.
- Stop the selected employee path from independently subscribing through one
  ACP/gateway per profile.
- Add trusted structured target and delivery-intent event shapes. Derive origin,
  DM membership, reply author, Work assignment, and chain state from canonical
  server data; never from arbitrary model/event JSON.
- Add `OfficeSigner` backed by opaque signer references, public-key proof,
  least-privilege resolution, adoption checks, rotation, and historical key
  mapping. Persist exact signed event bytes/id before publish and reuse them on
  retry.
- Implement `ortak-routing-semantic` with a bounded/redacted request contract,
  exactly-one-score-per-candidate validation, hard timeout/circuit breaker,
  fail-silent behavior, and no late dispatch.
- Run semantic scoring after an inbox snapshot and outside every database
  transaction. On return, lock the root chain and refresh authoritative inputs;
  re-score outside the transaction when the scorer input fingerprint changed,
  otherwise reapply current eligibility/hop/wake/visited guards and reserve
  recipients in the decision/outbox commit.
- Pin scorer adapter/model/prompt version; add safe content/revision/policy cache
  keys plus latency, usage, cost, and privacy telemetry.
- Add Office delivery outbox consumer with idempotent publish.
- Expose routing decision queries and a realtime decision stream.
- Add a feature flag that switches a channel/employee cohort to central routing.

Exit gate:

- `Cem, ...` wakes Cem once and not Zeynep.
- A general human message can wake a policy-bounded relevant set.
- An employee reply creates no semantic wake.
- Explicit employee-to-employee dispatch observes hop and wake budgets.
- The initial human dispatch increments `hop_count` to 1; with `max_hops = 2`,
  only one concurrent employee sibling can commit a subsequent delegation batch.
- Missing, malformed, duplicate, or timed-out semantic scores produce one
  explainable silent decision and never dispatch later.
- Office signer adoption proves the expected public key; a publish retry sends
  the identical signed event id.
- A crash after sender acknowledgement cannot lose routing work; reconciliation
  neither misses nor duplicates dispatch.
- Unknown/cross-community company mappings fail closed.
- The legacy path can be disabled for the cohort without message loss.

## Milestone 3 — Hermes runtime adapter

Outcome: selected decisions become supervised Hermes runs.

Implementation state: the runtime-independent dispatch and supervision
foundation exists in `crates/ortak-runtime` over the `RuntimeAdapter` port and
the fake adapter. A leased `run_dispatch` row is treated as a hint; the
authority is re-derived from company-scoped durable rows into a sealed
`DispatchAuthority`, one `queued` run is created per decision recipient under
the lease fence, `start_run` runs outside any transaction with the stable
per-run idempotency key, correlation is a compare-and-set that completes the
lease in the same commit, events resume from the last durable cursor and move
the run through its states only from typed events, and cancellation is
supervised and idempotent by run id. Next boundary: persist the run's
server-derived delivery target (thread root, channel, reply parent) so a
completed `reply`/`channel` run can enqueue `office_publish` authoritatively;
then the Hermes adapter itself.

- Capability-probe the deployed Hermes version and record supported API/schema.
- Implement `RuntimeAdapter` validation, start, events, cancel, and health.
- Map EmployeeRevision to external profile ref, workspace, model, tool policy,
  and credential references.
- Keep existing Cem/Zeynep profiles in `adopt` mode; never copy their auth files.
- Add runtime-run correlation and resume-from-cursor behavior.
- Normalize terminal/process errors; bound output and process lifetime.
- Implement typed runtime completion: `reply`, `channel`, or `silent`.

Exit gate:

- Cem and Zeynep each complete a real smoke run using their existing profiles.
- Cancellation reaches a durable terminal state.
- Runtime disconnect/restart resumes event ingestion without duplicate delivery.
- A missing/invalid credential reference fails before work starts and is redacted.

## Milestone 4 — Activity and supervision

Outcome: a human can understand and control ongoing work without reading raw
gateway logs.

Implementation state: the server-side Activity read foundation exists in
`crates/ortak-observability` over the existing `runs`/`run_events` rows and
the `RunEvent`/`RunStatus` contracts (no duplicate normalization, no
lifecycle change). It delivers, behind the `ActivityQueries` port on
`PgControlPlane`: a company-scoped run list with deterministic keyset paging
on `(queued_at DESC, run_id DESC)` (hard cap 100, employee/status/time
filters, opaque cursor; migration 0046 adds the supporting index); one run
detail with bounded/redacted terminal text and a fixed-query aggregate
summary (tool start/complete/fail, terminal commands and non-zero/abnormal
exits, file changes by kind, usage totals, terminal state, last event); and a
typed `ActivityEntry` timeline for lifecycle, assistant output, tool,
terminal, file, usage, delivery-intent, and error events with
`after_sequence` incremental paging (hard cap 500, `has_more`, next cursor,
gap signal). Company scope is a separate argument, never a filter field;
unknown and cross-company runs are one `RunNotFound`; closed vocabularies
fail closed; runtime run references and cursors surface only as presence
booleans, and the opt-in raw view is the already-bounded normalized payload
with the run reference scrubbed. Not yet delivered: desktop Activity
list/detail/rail rendering, API or WebSocket transport composition, realtime
push (clients poll the sequence cursor for now), retry/cancel actions from
Activity, an operator-only raw model, employee current-run summary, and
large-payload artifact offload beyond the existing `artifact_ref` slot.

- Add normalized RunEvent ingestion, ordering, redaction, and large-payload
  artifact offload.
- Adapt the existing activity/tool/file/terminal renderers to Ortak RunEvents.
- Add Activity list, run detail, employee current-run summary, retry/cancel, and
  raw event rail.
- Render waiting, silent, timeout, permission request, and disconnect states.
- Add usage/model metadata when available without making it authoritative.

Exit gate:

- A real run shows message → tool/terminal/file → delivery outcome in order.
- UI refresh/reconnect does not lose, duplicate, or reorder durable events.
- Sensitive fixture strings are absent from stored/rendered payloads in tests.

## Milestone 5 — Honcho memory adapter

Outcome: employee recall and learning is scoped, visible, and attributable.

- Capability-probe the deployed Honcho services.
- Implement memory binding health and employee/workspace/peer reconciliation.
- Recall bounded context before a run using company/project/conversation scope.
- Write reviewed/extracted facts after a run with provenance and receipts.
- Add Memory inspection UI and per-run recall/write activity.
- Keep canonical company files separate and visibly authoritative.
- Add retention/forget only after deployed API semantics are verified.

Exit gate:

- Cem's known cross-session memory behavior remains intact.
- Zeynep identity/peer health is explicitly verified; a failed identity seed is
  visible and retryable rather than treated as success.
- Memory from one employee/project cannot leak to another disallowed scope.

## Milestone 6 — Work and Projects

Outcome: conversations can become durable, assignable company work.

- Implement Project and WorkItem aggregates, histories, dependencies, and
  assignments.
- Add Work and Projects APIs and realtime projections.
- Add Work/Projects desktop surfaces and employee work queues.
- Attach conversations, decisions, runs, artifacts, and approvals.
- Support promote-from-conversation and dispatch-from-work.
- Add acceptance-criteria and approval completion gates.

Exit gate:

- A conversation is promoted to Work, assigned to an employee, executed,
  reviewed, and completed with linked artifacts and full history.

## Milestone 7 — Employee provisioning dashboard

Outcome: adding/adopting an employee is a safe product workflow, not a terminal
ritual.

- Implement manifest validation and plan/dry-run output.
- Implement durable create/adopt/update provisioning saga.
- Add credential-reference picker without exposing secret values.
- Add runtime, model, memory, Office identity, workspace, permissions, routing,
  skills, and tools configuration.
- Add step-level progress, logs, retry, rollback where safe, and adopt semantics.
- Add Overview, Work, Memory, Skills, Tools, Activity, Terminal, Settings tabs.

Exit gate:

- Cem/Zeynep adoption dry-run is no-op safe.
- A disposable test employee can be provisioned, health-checked, disabled, and
  re-enabled through the UI.
- Compensation never deletes an externally adopted profile or credential.

## Milestone 8 — Product cutover and prune

Outcome: Ortak is the only visible product and the unused Buzz surface is gone.

- Replace navigation, branding, onboarding, settings, and product vocabulary.
- Remove ACP per-profile relay gateway path after central routing soak.
- Remove or archive unowned modules: mobile, voice/huddles, mesh, git forge,
  pairing, push gateway, Buzz workflow UI/engine, community/catalog surfaces,
  and unused personas.
- Rename retained transitional `buzz-*` modules only when their public boundary
  is owned and tests pin behavior.
- Produce clean deployment compose/manifests and migration/backup runbooks.
- Run security review of secrets, tool permissions, routing prompts, memory
  scopes, SSRF, process containment, and audit coverage.

Exit gate:

- Fresh install and upgrade from the pinned baseline both work.
- The built product contains no reachable legacy agent wake path.
- Full company loop acceptance criteria in Architecture v0 pass.
- The unused code removal is demonstrated by the build/dependency graph.

## Cross-cutting test plan

### Pure policy tests

- full origin × target type × reply × hop-limit matrix;
- alias normalization/collision and Turkish/Unicode boundaries;
- deterministic precedence over semantic scores;
- semantic threshold, stable ordering, cap, and zero-recipient result;
- disabled/paused/unhealthy/out-of-scope/visited/self exclusions;
- typed delivery and loop-budget invariants.

### Integration tests

- Postgres transaction + outbox idempotency;
- atomic Office event + inbox insert, crash recovery, and reconciliation;
- root-chain row locking, unique visit reservations, and concurrent sibling
  races for the same employee, different employees, final hop, and wake budget,
  including semantic results that return in the opposite order from their
  sibling inbox claims;
- committed-batch hop semantics: initial human batch is hop 1, silent decisions
  do not increment, and default hop 2 is the only employee delegation batch;
- authenticated community/company mapping and cross-company denial;
- Office signer adoption/rotation plus byte-identical publish retry;
- semantic scorer fake/deployed contract, timeout, malformed output, and cache;
- Hermes fake server contract and deployed smoke test;
- Honcho fake server contract and deployed smoke test;
- reconnect/cursor replay and duplicate external events;
- provisioning retry/adopt/compensation.

### Desktop tests

- router explanation and silent state;
- employee status/current run;
- activity ordering and progressive disclosure;
- provisioning dry-run and failure recovery;
- keyboard, screen-reader, reconnect, and stale-state paths.

### Safety tests

- secret redaction corpus;
- prompt/semantic scorer output injection cannot bypass deterministic guards;
- bounded payload, output, retry, hop, recipient, and process limits;
- pure delivery-chain snapshots cannot authorize dispatch or bypass durable
  root locks, visit uniqueness, or refreshed budgets;
- cross-employee/project/company authorization denial;
- untrusted event/model fields cannot forge origin, targets, reply context, or
  delivery-chain budgets;
- adopted resources survive rollback and deletion attempts.

## First implementation slice

The first code change after these documents is intentionally narrow:

1. `ortak-domain` types and validation.
2. `ortak-router` deterministic and semantic policy engine.
3. Secret-free `config/employees/cem.yaml` and `zeynep.yaml` adopt fixtures.
4. Production-seam unit tests for the routing examples and loop failure that
   motivated the architecture.

This slice creates the contract that later database, Office, Hermes, Honcho,
and desktop adapters must obey. It does not mutate the deployed runtime.
