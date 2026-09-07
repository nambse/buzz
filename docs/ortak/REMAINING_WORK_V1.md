# Ortak Remaining Work v1

Status: Active goal in task `01a078f7-e36b-7d92-b157-f1c0919b9af2` from
2026-09-07, with no deadline or token budget. The old heartbeat is removed.
[The new goal](GOAL_CONVERSATION_AND_V0_2026-09-07.md) and the top of
[the continuation ledger](CONTINUATION_PROGRESS_2026-09-05.md) govern current
scope, ownership and evidence. The historical A–G inventory below remains
useful for dependency order; it is not the current deployment inventory.

Inherited work is preserved in three pushed DCO checkpoints (ebc11ca, e4d02b2,
ecd9c1b). Context79 is pushed in993dc9d, deployed with verified installed artifacts, and
actual native Ada→Bora translation passed without a re-paste. Disposable schema
parity passed; live services now use79.

Current remaining gates: bounded authorized
conversation/team/thread/Work context and Ada→Bora acceptance; conversational
Work/artifact revision flow; truthful Employee identity and availability;
repeatable service lifecycle and durable state location; changed-schema
install/recovery validation; required integration and full CI; final GitHub
review link and Turkish operating notes. Existing schema78 and single-turn
acceptance do not close these conversation gates.

This is the dependency-ordered working plan. It supersedes stale per-milestone
implementation notes, not [Architecture v0](ARCHITECTURE_V0.md) or its acceptance
criteria. The owner's deadline and execution authority are recorded in the
[overnight plan](OVERNIGHT_DELIVERY_PLAN_2026-09-05.md); current source/process
ownership is in the [CLI handoff](CLI_HANDOFF_2026-09-05.md).

**The private stack now demonstrates real employee activation, Office replies,
cancellation, model changes, completed assigned Work and actual reviewed-memory
consumption/publication/withdrawal, plus a real selected-file read through saved
deliverable and native operator review/completion. Full v0 remains incomplete.** Ada is active with
GPT-5.6 Sol/high through Hermes and the selected ChatGPT OAuth enrollment.
The inventory below records the earlier baseline, not current deployment.
Current evidence and remaining acceptance boundaries are in the continuation
ledger. Preserved external Cem/Zeynep resources remain optional adoption targets,
not prerequisites or default credentials.

## 1. Current implementation and evidence

The latest signed implementation checkpoint on `codex/ortak-private-mvp` is
`1dea0d0`, adding the bounded environment credential resolver in source. Six
focused tests passed in0.05s, scoped all-target Clippy passed in39.51s, and
formatting passed. The25 earlier control tests were filtered, not rerun for
this increment. This resolver is not yet composed or deployed.

Running private backend binaries remain at `5c285d2`, with validated migration56;
the native package is the earlier `d07f55c` queue build. Other signed checkpoints
are `f23c9fd` and `2eac15f`. Subsequent commits may contain documentation only.
Use the current handoff for noon stop and heartbeat status.

| Area | Present and tested | Remaining acceptance boundary |
| --- | --- | --- |
| Durable routing and Office authority | Atomic accepted event/inbox seam; canonical channel normalizer; company mutation generation fence; root reservations; immutable decision input; runtime/delivery revalidation; deferred claim/admission expiry. | Deployed server-owned channel cohort and stored-event inbox reconciliation; actual live deterministic loop. |
| Runtime and worker | Opt-in composition, pinned permissions, Hermes adapter/bridge, durable start keys/events/cancellation, recovery-only mode, bounded operations and shutdown. | Real selected-provider profile execution and integrated cancellation/restart; supported policy/approval behavior beyond the empty-tool-policy candidate. |
| Office delivery | Canonical immutable completion draft, signer mapping, exact signed-event replay, fresh authorization and durable bounded delivery. | One actual employee result published by the integrated live worker. |
| Memory | Pinned native Honcho extension, atomic scoped remember receipts, real Rust transport, explicit owned-resource I/O witness, frozen RunSpec recall, post-Office-ack write jobs and scoped UI provenance. | Activation acquisition composition, broader permitted scopes/redaction/retention, provider-dependent memory features and real-run acceptance. Current automatic recall/write is RunScratch only. |
| Semantic routing | Optional default-off HTTP scorer, sealed inputs, strict response parsing, bounded cache/circuit/deadline and no-late-wake commit guards. | Selected provider/model, relevance-quality and deployed bounded multi-employee acceptance. |
| Product surfaces | Authenticated Office/Employees/Activity, cancel, memory provenance; manual Work/Projects and read-only employee assignment queue with current audience filters. | Live run/reconnect evidence, remaining authorized retry/approval UX, Work execution/artifacts and provisioning dashboard. |
| Provisioning | Durable saga and immutable revision/bindings; fresh sealed preparation/probes/final revalidation; migration56 deferred activation receipt and expiry guards. A bounded environment-backed credential existence port is now frozen in source, six focused tests and scoped Clippy passed in signed1dea0d0. | Real Office identity port, explicit caller-authorized credential composition, consistent acquisition mode and production saga runner. Activation tests currently use synthetic external adapters. |
| Private operations | Isolated local stores, API/relay/native package; schema parity through56; real manual Work flow; verified database-only fresh restore. | Coordinated lifecycle/install/upgrade/full-stack backup, legacy pruning and one deployed v0 acceptance flow. |

Recent evidence, without summing overlapping suites:

- Work: 19 core PostgreSQL cases; signed product API: 12 PostgreSQL cases;
  headless product UI: 4 cases, with visual inspection of distinct screenshots.
  Native queue package built; no native UI/OS-interaction evidence.
- Private signed API checks: 9 passed after migration56. Manual Work originally advanced
  version 1→7 and replay stays 7: 1 project, 1 completed item, 8 operation receipts,
  7 history entries, zero assignments/runs/outbox/routing decisions.
- Activation: 25 saga and 25 control unit cases, 14 distinct provisioning
  PostgreSQL cases (13 together plus same-key Office reuse). Fresh commit,
  replay, changed authority, pre-lock/final-commit expiry and audit immutability
  bind the real repository; external adapter facts are synthetic.
- Schema56 migration/desired-state parity passed. Private schema56 database backup
  restored into a new retained database with 103 table counts, migration
  checksums 1–56 and semantic schema equal. See the
  [backup record](../../runtime/private-stack/DATABASE_BACKUP.md).
- Pinned Hermes actual HTTP check passed with 5 requests: 3 synthetic Responses
  calls and 2 catalog 404s, zero real-provider calls and verified owned cleanup.
  Its endpoint/SDK OS-header seams are explicit; see
  [the exact limits](../../runtime/hermes-bridge/SYNTHETIC_HTTP.md).
- Honcho native extension and Rust HTTP/owned-resource roundtrip checks prove
  scoped full-text memory behavior, not embedding/derivation provider health.
  No full `just ci`, PR or push is claimed for the latest checkpoint.

### Product levels

- **Runnable private baseline:** the fresh local stack, migrations, authenticated
  manual Work and rebuilt desktop exist. This replaces the earlier library-only
  inventory; it is not a running employee system.
- **Usable MVP:** one selected isolated channel, one freshly prepared employee,
  a real Hermes run and signed reply, ordered Activity, human cancellation and
  restart recovery. Requires A, B and the minimum of C below.
- **Full v0:** all Architecture v0 acceptance criteria in one deployed flow,
  together with the approved clean-stack/optional-adoption deployment strategy.
  Requires A–G. Manual status changes and synthetic providers do not substitute.

## 2. Dependency-ordered remaining slices

### A. Production activation composition — next critical dependency

Outcome: an explicit operation can activate a fresh employee through real ports
and current evidence, without changing preserved test resources.

The [source gap report](ACTIVATION_COMPOSITION_GAPS.md) specifies the smallest
coherent slice. Office delivery transports do not implement the provisioning
`OfficeIdentityAdapter`; that provisioning port still has only a fake
implementation. The new `EnvCredentialResolver` source implements credential
existence through at most 128 explicitly authorized opaque-reference/env-name
mappings, validated before lookup. Every check reads current selected presence;
values are not returned or cached. Company/principal authorization belongs to
the caller selecting this instance, because the port has no scope parameter.
This is not credential-manager discovery, format/provider-health verification,
or a composed production saga. All six focused tests passed centrally,
including isolated subprocess checks of the public environment lookup path.
Hermes adopts prepared profiles, while Honcho
executable I/O requires original extension-created ownership plus explicit
validation. The saga's single acquisition mode cannot currently compose them.

- Implement the company/community/cohort-bound production identity port: exact
  signer proof, current membership, stable profile publication receipt and
  fail-closed unsupported create/delete behavior. Compose the finite environment
  credential resolver only after the caller authorizes every mapping for that
  instance; owning adapters still validate credential format and usability.
  Never expose secret values or silently read old profile credentials.
- Prefer explicit adoption of prepared **fresh** Hermes/Office resources and
  an extension-owned Honcho bundle. Preserve native IDs and original create
  receipts separately from saga acquisition ownership. Ordinary list-only
  adoption/health must not grant Recall/Remember or perform writes.
- Add a default-off production saga runner with durable operation/step keys.
  Reuse the sealed fresh activation protocol and actual I/O witnesses; do not
  insert active revisions or successful health receipts by hand.
- Select an authorized fresh provider credential/profile. The synthetic HTTP
  check uses an explicit SDK OS-header override for its extra test-only process
  audit; this is neither an established production failure nor provider-health
  evidence. Unsupported permission policies and approval resume remain explicit
  refusals.

Acceptance: real adapters complete one operation with exact identity/binding
receipts; missing signer/membership/provider/memory evidence refuses it. Lost
acknowledgement and restart reuse identities; compensation cannot delete adopted
resources. Old Cem/Zeynep adoption remains optional and, if selected, needs its
own read-only dry run and current capability/identity evidence.

### B. Demonstrated deterministic Office loop

Outcome: one authorized human message becomes exactly one real run and one
signed reply, with durable recovery.

The canonical normalizer, mutation fences, runtime supervisor, cancellation,
immutable outputs and delivery transports are implemented. Remaining work is to
close the production selection/reconciliation seams and run the complete path:

- Implement a server-owned channel cohort consulted at ingress and routing;
  the relay's central-routing flag alone is global. Keep independently
  subscribing legacy employee gateways disabled for that selected cohort.
- Add bounded idempotent reconciliation of stored routable events missing
  inbox rows, then prove no gaps or duplicate dispatch around process failure.
- Configure the real activation result and worker on the isolated stack. Keep
  routing disabled until A and the authority/cohort gates pass.
- Prove direct-name dispatch, untargeted disabled-semantic silence, explicit
  unsupported gift-wrap DM decisions, and no employee-origin wake loop.
  Ciphertext must never reach a runtime.
- Exercise human cancellation, lost start acknowledgement, worker/bridge
  restart, dense cursor replay and terminal→Office retry with exact signed bytes.

Acceptance: one decision/run, ordered events and one visible reply; no run for
non-cohort, unsupported DM, employee-origin or disabled-semantic inputs.
Cancellation reaches the real runtime; restart duplicates neither run nor reply.

### C. Live Office, Employees and Activity usability

Outcome: a human sees why an employee woke, what it did and whether delivery or
memory succeeded, and can stop it without reading logs.

Authenticated scoped APIs, cancellation, Activity polling, memory provenance and
current employee status are implemented. Remaining acceptance includes real
run/tool/terminal/file data, reload and cursor reconnect, explicit disconnected
states, and authorized retry/approval actions with durable audit. Approval UI
must use a verified runtime pause/resume capability or show a closed refusal;
the current empty-tool-policy runtime is not proof of approval resume.

Acceptance: the complete B run appears in order through reload/reconnect;
wrong-company/audience/role reads and mutations refuse; cancellation and any
supported approval resolution are visible and attributable. Realtime push with
cursor recovery remains distinct from current polling. A native package/TCP
check is not native interaction or automatic reconnect evidence.

### D. Semantic routing and broader memory

Outcome: bounded relevant semantic wakes and scoped, attributable employee
memory in the deployed flow.

- Select and validate a semantic model/provider, test relevance-quality and
  visible scores for zero/one/capped recipients. Preserve timeout/malformed
  fail-silence, current authority and no late dispatch.
- Extend current RunScratch-only memory deliberately to permitted conversation,
  project and employee scopes. Define extracted/reviewed/redacted facts and
  scope-specific provenance; do not promote raw output across scopes implicitly.
- Prove real-run recall visibility, cross-company/employee/project isolation and
  one remember receipt across retry. Keep full-text functionality separate
  from optional embedding/derivation provider health.
- Implement inspect/forget/retention only after verifying actual selected-server
  semantics. Add trusted server-side DM participant resolution/decryption;
  replace the current explicit unsupported decision only when that gate passes.

Acceptance: real relevance results remain bounded and explainable; memory
cannot leak another identity/project/company, and retries do not double-write.

### E. Work execution, artifacts and complete editing

Manual E1 and the employee's read-only queue are demonstrated. The queue includes
active owner/contributor/reviewer assignments on currently visible nonterminal
work, including for draft/suspended employees; `execution_available:false`
remains accurate. See [Work E1](WORK_API_E1.md).

- Dispatch assigned work with `runs.work_item_id` and current authority; link
  terminal output and move successful execution toward **REVIEW**, never
  directly to completed or automatically satisfied human acceptance criteria.
- Add artifacts/storage/provenance and authorized human review.
- Complete title/description/criteria editing, reassignment/release, dependency
  removal and parent/child decomposition as applicable to the domain contract.

Acceptance: conversation→assigned work→real run→artifact→review→authorized human
completion, with history and links. The current manual version 1→7 workflow
proves the manual API only and produced zero runtime dispatches.

### F. Provisioning dashboard

After A's real runner and B/D behavior, expose create/adopt/update with opaque
credential-reference selection, immutable runtime/model/memory/tools/skills/
permission revisions, step progress, retry and compensation. Surface adopted
resource protection and actual capability refusals; do not offer placeholder
controls over fake adapters.

Acceptance: a fresh employee can be prepared, health-checked, activated,
disabled and re-enabled through audited product operations. Any offered old
resource adoption is a no-op externally until explicitly authorized and never
permits compensation to delete adopted resources.

### G. Product cutover and operational acceptance

The local private stack, bounded database connector, signal handling, schema
parity and database-only backup/restore are implemented. Remaining work:

- Coordinated service ownership, startup/restart/quiescence and authenticated
  readiness; fresh install and upgrade from baseline `b1f6b7ef`, exercised from
  the retained pinned artifacts rather than inferred from compilation.
- Consistent full-stack recovery covering PostgreSQL, object storage, Honcho,
  bridge journals/profiles and secret-reference restoration. Keep all retained
  database verification targets; current backups do not satisfy this gate.
- Disable/remove legacy per-profile ACP wake paths after central-routing soak;
  prune unowned Buzz surfaces/build dependencies per `BUZZ_BASELINE.md`, then
  verify branding/onboarding/settings and retained CI workflows.
- Review secrets, permissions, routing prompts, memory scope, SSRF, containment
  and audit coverage; track reviewed/deployed Buzz/Hermes/Honcho pins under
  `UPSTREAM_MAINTENANCE.md`. The deferred `ifc-core` paper remains review input.

Acceptance: fresh install and baseline upgrade work, full recovery is rehearsed,
no reachable legacy wake path remains, and Architecture v0 passes in one deployed
flow. Public hosting additionally needs reviewed DNS/TLS/auth/exposure/rollback;
the morning target is not authorization for unreviewed public or paid deployment.

## 3. Execution and testing posture

Prioritize A, then the live B loop; independent C–G work may proceed without
claiming the missing provider/activation gates. Use real production-seam tests
that fail when the guarded behavior is removed. Preserve current regressions,
add only relevant tests, and record fixture vs deployed evidence separately.

Use explicit disposable PostgreSQL 55432 URLs for destructive fixtures; private
55433 and old services are not test reset targets. Serialize Cargo targets and
retain failed operation/backup receipts. Capability declarations, green unit
suites, synthetic HTTP and a running desktop process each prove their stated
boundary; none alone proves employee activation or the usable MVP.
