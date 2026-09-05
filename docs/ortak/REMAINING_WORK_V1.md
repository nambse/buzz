# Ortak Remaining Work v1

Status: Active

Date: 2026-09-05

Supersedes the per-milestone "Implementation state" notes in `IMPLEMENTATION_PLAN_V0.md` as the working plan. Architecture v0 is unchanged.

Planning style: dependency-ordered slices with concrete acceptance gates; no percentages, no dates.

## 1. Where the code actually is

The working tree contains solid library foundations and one default-off production seam. There is no end-to-end company loop yet. Every path below was confirmed by reading the source on 2026-09-05. Deployed state (the Ortak Runtime host, Hermes, Honcho, signer) was inspected read-only by the operator on 2026-09-05; the observed facts are in `RUNTIME_DISCOVERY_2026-09-05.md` and are partial slice A discovery, not adoption acceptance.

Update 2026-09-05 (later the same day): the B1a channel-normalization substep is delivered as verified library code and documented in `B1_CHANNEL_NORMALIZATION.md`. It is **unwired** and stays unwired until the B1b gate (mutable Office authority fencing inside the routing commit) in that document closes.

Deployment decision 2026-09-05: the existing Coolify Hermes/Honcho/Buzz stack is disposable **test** infrastructure, not a production or migration target, and Cem and Zeynep remain test employee definitions. Ortak may build a clean, separate stack; adopting the existing Cem/Zeynep resources is optional. See `DEPLOYMENT_STRATEGY_V0.md` for the sequence and `UPSTREAM_MAINTENANCE.md` for Buzz/Hermes/Honcho pinning and upgrade rules. The slices below are adjusted accordingly: nothing in slice B requires adopting the old resources first.

### Shipped vs. port-only

| Area | State | Evidence |
|---|---|---|
| Domain types, routing policy, fixtures | Shipped, tested | `crates/ortak-domain`, `crates/ortak-router`, `config/employees/{cem,zeynep}.yaml` (draft/adopt, secret-free) |
| Durable control-plane schema | Shipped | migrations 0045–0047, `schema/schema.sql` parity |
| Inbox claim, authoritative routing commit, outbox leases | Shipped as library + Postgres adapter, tested | `crates/ortak-control/src/{service,routing,postgres}` |
| Atomic Office event + `office_inbox` insert | Shipped, **the only production wiring**, default off | `crates/buzz-relay/src/handlers/office_ingress.rs`, `ORTAK_CENTRAL_ROUTING_ENABLED` in `config.rs` |
| Central-routing flag scope | Global on/off for the relay process; there is no per-channel or per-cohort selector | `config.rs`, `office_ingress.rs` |
| Inbox reconciler (backfill stored events lacking inbox rows) | Not implemented; only referenced in doc comments | `office_ingress.rs` header |
| `MessageNormalizer` | `PgChannelNormalizer` implemented and tested as unwired snapshot library; B1b authority fence still required | `crates/ortak-office/src/normalizer/`, `B1_CHANNEL_NORMALIZATION.md` |
| `SemanticScorer` | Production `DisabledSemanticScorer` records honest silence; unwired. Real semantic scoring still absent | `crates/ortak-control/src/scorer.rs` |
| Office signer / publisher / delivery service | Library + Postgres repo; `FakeOfficeSigner`, `FakeOfficePublisher` only | `crates/ortak-office` |
| Run dispatch authority, supervisor, cancellation | Library + Postgres repo; `FakeRuntimeAdapter` only | `crates/ortak-runtime`, `crates/ortak-control/src/fakes.rs` |
| Hermes `RuntimeAdapter` | Absent. The desktop "Hermes Agent" entry is a Buzz ACP preset, not an Ortak adapter | `desktop/src-tauri/src/managed_agents/discovery/presets.rs` |
| Runtime completion → Office publish | Not enqueued. Supervisor stops at terminal run state | `crates/ortak-runtime/src/supervisor.rs` header |
| Permission policy on runs | Not transported. `RunSpec` has no `PermissionPolicy`; `run_spec` sets `work_item_id = None`, `memory_context = []` | `crates/ortak-runtime/src/authority.rs` |
| `MemoryAdapter` (recall/remember/health/ensure) | Port + fake only; inspect/forget/retention deferred; not called pre/post run | `crates/ortak-control/src/memory.rs` |
| Honcho adapter | Absent | — |
| `RunEvent` kinds | Lifecycle, assistant delta, tool, terminal, file, usage, error, delivery intent. **No** memory, permission, or artifact kinds | `crates/ortak-control/src/run_event.rs` |
| Activity queries (list, detail, timeline) | Library + Postgres, tested; no transport, no UI, no realtime | `crates/ortak-observability` |
| Work/Projects aggregates + repository + service | Library + Postgres, tested; `WorkActor::Human` is a validated string, not an authenticated principal | `crates/ortak-work`, `crates/ortak-domain/src/work.rs` |
| Provisioning saga | Library + Postgres, tested against fakes only | `crates/ortak-control/src/provisioning.rs` |
| Credential resolver | Port + fake; existence-only contract | `crates/ortak-control/src/credentials.rs` |
| Composition root / workers / server | Absent. No `ortak-server`, no binary spawns the routing worker, supervisor, or delivery service | `Cargo.toml` workspace members |
| Ortak desktop surfaces | Absent. `desktop/src/features/projects` is the Buzz git forge, not Ortak Projects | `desktop/src/features/` |
| DM support | Ingress can store gift wraps; B1a refuses them explicitly before normalization reads content, and dispatch has a second non-channel guard. Trusted decryption remains absent | `crates/ortak-office/src/normalizer/`, `crates/ortak-runtime/src/postgres.rs` |
| Legacy ACP wake path | The only wired agent path in source; independently subscribes per profile. Whether it is running anywhere was not inspected | `crates/buzz-acp` |

### Product levels

- **Retained runnable baseline**: the inherited relay, Postgres, and Redis stack plus the Ortak migrations is the same stack the migration and Postgres-backed tests exercise, with central routing off and no employee runs. That is test evidence over the retained baseline. A fresh install or deployed local demo was **not** exercised in this turn; slice G owns proving it.
- **Usable MVP**: one isolated channel cohort routed centrally, one employee (a disposable test employee on the clean stack, or an adopted Cem revision if adoption is chosen) completes a real Hermes run and replies, Activity shows it, humans can cancel. Requires slices A, B, and the minimum of C.
- **Full v0**: all eleven acceptance criteria in `ARCHITECTURE_V0.md` §11, including semantic routing, Honcho visibility, Work linkage, provisioning UI, and legacy-path removal. Requires slices A–G.

## 2. Slices

Slices are ordered by dependency. UI shell work in C, E, and F may overlap with adapter work, but the live run in B is the first critical path and G gates completion.

### A. Runtime capability discovery and optional read-only adoption

Outcome: adapters target verified Hermes, Honcho, signer, and credential interfaces. The existing test deployment supplies reference evidence; the selected clean-stack revisions must be verified independently. If old-resource adoption is chosen, rehearse it without external writes.

Status 2026-09-05: partial. `RUNTIME_DISCOVERY_2026-09-05.md` records the read-only probes (root Hermes capabilities, profile capabilities returning 404 with both gateways intentionally stopped, Honcho workspace/peers, Office key matches). The adoption dry run is now **optional**: it is required only if the clean-stack path in `DEPLOYMENT_STRATEGY_V0.md` chooses to adopt the existing resources rather than provision disposable test employees.

Scope:

- Read-only probes for Hermes profiles/run API version and permission resume semantics, the Honcho workspace/peer/session API, the selected Office signer backend, and the credential-reference store. Probes may not create, mutate, or delete anything.
- A **read-only `OfficeIdentityAdapter`** behind the existing signer/identity port: proves that the configured opaque reference resolves to the selected employee's recorded public key without signing or publishing. Existing Cem/Zeynep keys are required only for their adoption path.
- A **real credential-reference resolver** with the existing existence-only contract: answers whether each `credential://ortak-runtime/...` reference resolves, never returns a value.
- **Honcho health and adopt support** in a real `MemoryAdapter` limited to `health` and `ensure_resources` in adopt mode: confirms the existing workspace/peer resources and refuses to create.
- **Optional adoption branch:** run the existing provisioning saga in dry-run `adopt` mode for Cem and Zeynep with these read-only adapters. Use local scratch or in-memory bookkeeping: the saga dry run writes its own repository, but must make no external writes. **Dry-run adoption does not activate any employee.** Existing profiles remain unchanged.
- Record each missing endpoint, permission, or credential as an external prerequisite in this document. Do not invent an API shape to fill a gap.

Prerequisites: access to the selected isolated runtime environment; access to the old test stack only if its adoption branch is chosen. `BUZZ_BASELINE.md` external facts describe reference resources, not a required production topology.

Acceptance gate: a checked-in probe report listing supported operations, exact tested runtime revision, and permission pause/resume behavior. The optional adoption branch additionally requires a Cem/Zeynep dry-run log with zero `created` outcomes, external writes, and activations. The clean-stack branch instead provisions a disposable employee in slice B using explicit create mode.

### B. Runnable deterministic Office loop (first critical path)

Outcome: one human message in one isolated channel cohort becomes one real Hermes run, its normalized events, and one signed reply, with cancellation and crash/retry behavior proven.

Scope:

1. **Composition root**: an `ortak-server` binary (or a relay-hosted worker set) that constructs `PgControlPlane`, the inbox routing worker, `RunSupervisor`, `OfficeDeliveryService`, and the inbox reconciler, with bounded concurrency and graceful shutdown.
2. **Real `MessageNormalizer`** over the stored event: server-derived origin (human vs. employee pubkey via `employee_office_bindings`), channel context, reply parent, and trusted mention targets. Channel messages only.
3. **DM handling**: a gift-wrap (kind 1059) inbox item is durably held or rejected with the explicit reason `unsupported: dm_normalization_pending`, recorded as a routing decision, until trusted participant resolution and decryption normalization land in slice D. Ciphertext is never passed to a runtime, and held items are never queued indefinitely without a visible decision.
4. **Production `SemanticScorer` stand-in that is disabled**: for untargeted input the routing commit records a silent decision with reason `semantic_scorer_disabled`. No fake or placeholder scores are produced in production until slice D replaces it.
5. **Hermes `RuntimeAdapter`** implementing `probe_capabilities`, `health`, `ensure_profile` (explicit create/adopt modes; adopt never creates), `start_run` with the stable idempotency key, `next_events` from a cursor, and `cancel_run`, mapped from slice A's probe results. New profiles are isolated disposable resources until their gates pass.
6. **Permission transport and enforcement**: add the pinned `PermissionPolicy` from the employee revision to `RunSpec`, enforce it at the adapter tool boundary, and emit a permission `RunEvent` kind when a tool call is refused.
7. **Persisted delivery target**: store the server-derived thread root, channel, and reply parent on the run row at dispatch time; a completed `reply`/`channel` run enqueues an `office_publish` outbox row in the same commit as its terminal event, linked to the terminal assistant output.
8. **Real `OfficeSigner` and `OfficePublisher`** behind the existing ports, built on the slice A identity adapter: signer proves the configured public key; publisher submits the frozen bytes to the relay and reuses the frozen event on retry.
9. **Reconciler**: idempotent scan for stored routable events without inbox rows, using `is_office_routable_kind`.
10. **Server-owned cohort selection**: the existing `ORTAK_CENTRAL_ROUTING_ENABLED` flag is a global switch and is not a channel selector. Implement a server-owned cohort (a durable allow-list of channel ids owned by the control plane), make ingress consult it, and ensure the legacy ACP path is disabled for the selected cohort **before** central routing is enabled for it. No client-supplied selector.
11. **Employee activation**: after fresh evidence for the chosen employee revision passes the runtime health, Honcho health, Office membership, and signer gates, activate that revision. On the clean-stack path this is a clearly disposable test employee created with fresh identities and test data. If adoption of the existing Cem resources is chosen instead, the same gates apply and only Ortak's adopted revision is activated; the existing external Cem and Zeynep profiles remain unchanged. Dispatch requires an active employee revision and a validated Office binding; anything else is a recorded refusal.

Prerequisites: slice A probe facts for the runtime and memory APIs, the identity adapter, and the credential resolver; a reproducible isolated Ortak stack per `DEPLOYMENT_STRATEGY_V0.md` (separate data volumes, explicit secret references, pinned Hermes/Honcho revisions per `UPSTREAM_MAINTENANCE.md`). Adoption of the existing Cem profile is **not** a prerequisite; a disposable test employee provisioned in `create` mode on the clean stack satisfies the loop. B1b (authority fencing in the routing commit) must close before central routing is enabled for any cohort.

Acceptance gate:

- A direct-name message to the chosen disposable employee (or `Cem, selam nasılsın?` if adoption is chosen) yields exactly one `routing_decisions` row, one `runs` row, ordered `run_events`, and one signed reply visible in the cohort channel.
- An untargeted message in the cohort channel yields one recorded silent decision with the disabled-scorer reason and no run.
- A gift-wrap DM yields one recorded held/rejected decision with the unsupported reason, no run, and no ciphertext in any run input.
- A message in a non-cohort channel is not routed centrally. A clean stack has no legacy subscriber; any deliberate migration retains only the explicitly selected non-cohort legacy behavior.
- Cancelling the run from the CLI or a temporary admin command reaches `cancelled` durably and Hermes stops.
- Killing the worker between run start and correlation, and between terminal event and publish, resumes without a duplicate run or duplicate reply.
- An employee-authored reply creates no wake. No second employee is woken; if the old resources are adopted, Zeynep is never woken and never activated.

### C. Usable Office, Employees, and Activity surfaces with authenticated APIs

Outcome: a human can see why an employee woke, what it did, and stop it, without reading logs.

Scope:

- Authenticated read **and mutation** APIs for routing decisions, run list/detail/timeline (over `ActivityQueries`), employee status, current run, cancel, retry, and approval resolution. Every request is authorized by company, by channel or project audience, and by role. Human and system actors are derived server-side from NIP-42/NIP-98 principals; no client-supplied actor.
- Durable administrative audit records for cancel, retry, approval resolution, and provisioning actions, attributable to the derived principal.
- Realtime decision/run-event push with cursor reconnect that neither drops nor duplicates events.
- Desktop `activity/` and `employees/` features: decision explanation panel, run timeline reusing the existing tool/terminal/file renderers, cancel action, waiting/silent/timeout/disconnect states.
- Human approval enforcement: a permission `RunWaiting` pauses the run until an authenticated human resolves it; resolution is a durable event. Whether the runtime can actually pause and resume is a **slice A capability-probe result**, not an assumption. If Hermes exposes no resume API, the approval UI fails closed (the run ends as refused and the refusal is recorded) or uses the architecture-approved bridge; it must not pretend a resume happened.
- Rename navigation to the Architecture v0 target set for the surfaces that exist; leave unbuilt surfaces hidden rather than stubbed.

Prerequisites: slice B for real data; slice A permission-capability result; API transport choice (extend `POST /query` style HTTP or new event kinds per `AGENTS.md` guidance).

Acceptance gate: one real run shows message → decision → tool/terminal/file → delivery in order after a UI reload and a reconnect; a cross-company or wrong-role read and mutation are both refused with an audit record; a permission request is resolved from the UI and either resumes the run (if the probe confirmed resume) or records a closed refusal.

### D. Semantic routing and Honcho memory

Outcome: untargeted human messages can wake a bounded relevant set, and employee memory is scoped, visible, and attributable.

Scope:

- `ortak-routing-semantic`: bounded/redacted request builder, exactly-one-score validation, timeout and circuit breaker, version pinning, cache keys, telemetry; wired into the existing preflight/refresh commit path, replacing the disabled stand-in from slice B. Fail silent; no late wake; employee-origin messages never enter scoring.
- Honcho `MemoryAdapter` completed beyond slice A's health/adopt: bounded pre-run `recall` composed into `RunContext.memory_context` with provenance, scoped by the employee, project, and conversation permissions of the run; post-run `remember` writes only extracted, reviewed, redacted facts with provenance, using idempotent durable receipts so a retried run never double-writes.
- Add memory recall/write `RunEvent` kinds and persist receipts; Memory inspection surface listing scope, source, and run.
- `inspect`/`forget`/retention only after slice A verified the deployed semantics.
- **Trusted DM normalization**: server-side gift-wrap participant resolution and decryption through the Office adapter so the runtime never sees ciphertext or a client-supplied recipient list. This replaces the slice B hold/reject decision.

Prerequisites: slice B loop; slice A Honcho probe.

Acceptance gate: a general human message wakes zero, one, or a capped set with visible scores; scorer timeout or malformed output yields one silent decision and no later dispatch; Cem's recall is visible with provenance and cannot read Zeynep's scope, another project's scope, or another company's scope; a retried run produces one memory receipt.

### E. Work and Projects execution

Outcome: conversations become assignable, executable, reviewable work with linked runs and artifacts.

Scope:

- Authenticated Work/Projects APIs over `WorkService`; `WorkActor::Human` derived from the authenticated principal; approval resolution authorized by role.
- Employee work queues (the `work_assignments` active index exists; the query does not).
- Dispatch-from-work: assignment creates a run linked through `runs.work_item_id`. Successful runtime completion moves the item **toward `REVIEW`** and links the terminal output; it does not complete the item and does not satisfy acceptance criteria or approval gates. Only an authorized human review does.
- `artifacts` relation and storage (Blossom/S3 via `buzz-media` port), artifact-created `RunEvent` kind, review permissions.
- Editing titles/descriptions/criteria, reassignment, assignment release, dependency removal, parent/child decomposition.
- Desktop `work/` and `projects/` features (Ortak Projects, distinct from the retained Buzz git forge).

Prerequisites: slices B and C.

Acceptance gate: promote a conversation, assign Cem, run executes, artifact attached, item lands in review rather than done, human reviews and completes; full history and links visible.

### F. Provisioning dashboard

Outcome: adding or adopting an employee is a safe product workflow.

Scope:

- The slice A/B/D adapters (identity, credential resolver, Hermes runtime, Honcho memory) behind the existing saga, no fakes in production.
- Create/adopt/update flows, credential-reference picker without values, runtime/model/memory/tools/skills/permission settings editing that produces a new immutable revision.
- Step-level progress, retry, and compensation display; adopted-resource protection surfaced explicitly.
- Employee detail tabs from Architecture v0 §9 for the data that exists.

Prerequisites: slices A, B, D.

Acceptance gate: a disposable test employee is created, health-checked, disabled, and re-enabled through the UI with audit records; if adoption is offered, the Cem/Zeynep adoption dry-run is a no-op and compensation never touches an adopted resource.

### G. Product cutover, prune, and deployment

Outcome: Ortak is the only visible product; the legacy wake path is gone; install and upgrade are documented and demonstrated.

Scope:

- Disable, then remove, the per-profile ACP wake path only after slice B has soaked on the cohort and every channel is on central routing.
- Remove or archive unowned Buzz surfaces per `BUZZ_BASELINE.md` (mobile, voice, mesh, git forge, pairing, push gateway, Buzz workflow engine, community/catalog UX), demonstrated by the build graph.
- Branding, onboarding, settings vocabulary; README crate list and verification commands updated.
- Fresh install, upgrade-from-baseline, backup/restore, and operator runbook, each actually exercised; decide which inherited GitHub workflows remain.
- Security review: secrets, tool permissions, routing prompts, memory scopes, SSRF, process containment, audit coverage. The deferred upstream `ifc-core` paper is reference input here.

Prerequisites: slices B–F exercised.

Acceptance gate: fresh install and upgrade from `b1f6b7ef` both work; no reachable legacy wake path in the built product; Architecture v0 §11 criteria pass in one deployed flow.

## 3. First next task

B1b: **mutable Office authority fencing**, as specified in `B1_CHANNEL_NORMALIZATION.md` §8. Choose and implement a locking/version-fencing protocol that also coordinates Office mutation writers and absent-row insertions; the root lock and transaction-local rereads alone are insufficient. Revalidate channel, author, target, binding, and provenance facts at routing commit and runtime admission, with adversarial concurrent tests. `PgChannelNormalizer` and `DisabledSemanticScorer` stay unwired until this gate closes.

In parallel, and independent of the old test stack: the reproducible isolated Ortak development stack from `DEPLOYMENT_STRATEGY_V0.md` step 2, the composition root skeleton, the persisted delivery target on the run row, and `PermissionPolicy` transport on `RunSpec`. The remaining slice A probes (permission pause/resume, durable event capture) are recorded as found in `RUNTIME_DISCOVERY_2026-09-05.md`; none are invented and no credential value is recorded.

## 4. External prerequisites (record as discovered)

- A clean, isolated Ortak stack (Hermes, Honcho, relay, Postgres, Redis) with pinned revisions per `UPSTREAM_MAINTENANCE.md`; the existing Coolify stack is optional reference infrastructure only.
- Only if adoption is chosen: access to the existing test environment hosting `/opt/data/profiles/cem` and `/opt/data/profiles/zeynep` (probed read-only on 2026-09-05; both gateways intentionally stopped).
- A profile-scoped Hermes adapter/bridge with durable replay and policy enforcement. Root 0.21.0 capabilities were observed on the old stack; exact source revision and employee-specific behavior remain unverified. Recheck the selected clean build.
- Honcho scope and API compatibility on the selected clean build. Old-stack API 3.1.1 and workspace/peers were observed; retention/delete semantics remain unverified.
- A signer backend for the selected employee identity through an opaque reference; matching existing Cem/Zeynep keys is required only if adopting them.
- Credential manager that answers existence for `credential://ortak-runtime/...` references.

## 5. Testing posture

Existing safeguard and foundation tests are valuable and stay. New work adds production-seam tests at each slice gate (the live loop, cohort selection, DM hold/reject, disabled-scorer decision, reconnect/cursor, permission refusal, publish retry, audit records) rather than another broad unit-test expansion. Runtime evidence from the real Hermes run is the acceptance record for slice B, not green CI alone.
