# Ortak overnight delivery plan

Decision: 2026-09-05, Europe/Istanbul. This is the execution overlay on
`REMAINING_WORK_V1.md`, not a replacement for Architecture v0 or its safeguards.

## Outcome and deadline

The owner asked to continue proactively and have Ortak ready by morning.
The owner extended unattended work to **12:00 Istanbul on 2026-09-05** after
the original08:00 planning checkpoint. Target a usable private MVP
first, then progress through the rest of v0. This is an ambitious target, not
a guarantee that all production and v0 acceptance gates fit the available time.
Do not call a mocked dashboard or a happy-path-only reply a ready product.

The private MVP must demonstrate:

- One isolated stack, company, server-selected channel and disposable employee.
- Human message → durable routing decision → real profile-scoped Hermes run →
  ordered activity → one signed Office reply.
- Office, Employees and Activity UI showing real data, honest errors and
  authenticated cancellation. Existing chat/tool/terminal/file renderers may
  be reused; unbuilt actions stay hidden or explicitly unavailable.
- Actual permission enforcement at the runtime tool boundary; unsupported
  policies or approval resume fail closed. Policy transport is not enforcement.
- Cancellation, worker restart around run correlation and terminal delivery,
  cursor reconnect, and frozen-event retry without duplicated work or replies.
- Honcho health and valid Office/signer/runtime bindings before activation.
- Untargeted messages record disabled-semantic silence; DMs record an explicit
  unsupported decision until trusted normalization exists.
- No independently subscribed employee Office gateways in the selected cohort.

## Parallel lanes, one integrator

| Lane | Immediate work | Ownership boundary |
| --- | --- | --- |
| A: authorization | B1b coordinated Office mutation/commit/admission fencing, including absent-row insertions | Control routing and Office authority protocol; coordinate runtime-authority edits after B2 lands |
| B: execution | Pinned permission transport, profile-isolated Hermes adapter/bridge, durable event journal, enforced policy and cancellation | Runtime contracts and bridge; transport-only B2 is already assigned separately |
| C: product | Authenticated APIs and minimum Office/Employees/Activity screens over real contracts | UI and API transport; actors/audiences/roles derived server-side |
| Integrator | Composition, cohort selector, reconciler, delivery target, atomic terminal→publish, real signer/publisher, isolated stack, verification | Shared migrations/interfaces and merges; only one integration owner |

Codex is now authorized to write code directly and delegate coding, not merely
review Claude output. Use **GPT-6 Astra with ultra thinking** for Codex coding
and integration as requested. Claude Code/Fable 5.1 remains an optional parallel
writer; retry recoverable launch failures, then use Codex rather than abandoning
the task. Do not silently change the requested Codex model/effort.

Before multiple writers change shared code, agree on `RunSpec`, permission
events, delivery-target persistence, cursor/cancellation semantics and migration
numbers. Keep separate branches/worktrees. Workers own bounded changes and
tests; the integrator owns review, commits, merges and pushes. No competing
writers on the main checkout or shared files. Serialize Cargo builds using the
existing shared caches to avoid disk exhaustion.

## Checkpoints, not fictional completion dates

1. **First checkpoint:** close shared contracts and select B1b's real fencing
   protocol. Finish/review the bounded pinned-permission transport slice.
2. **Runtime checkpoint:** choose a reviewed pinned Hermes build, verify profile
   selection and prove durable event capture before wiring a live loop. A
   subscriber which journals only after receiving an in-memory SSE event is not
   durable across disconnects. Journal at the execution boundary or demonstrate
   equivalent durable reconstruction. Explicitly handle finite idempotency
   retention and restart reconciliation.
3. **Integration checkpoint:** compose the isolated deterministic loop; enforce
   a server-owned cohort, scoped credentials, permissions, and signed delivery.
   No central-routing activation before B1b and runtime admission gates pass.
4. **Usability checkpoint:** expose authenticated Office/Employees/Activity and
   cancel controls; connect them to the proven loop. Verify the actual UI.
5. **Morning checkpoint:** report the real URL or launch instructions, exact
   tested/deployed revisions, demonstrated workflows, remaining gaps and
   blockers. Retain rollback data. A failed gate means not ready, not a lowered
   security standard.

At each checkpoint continue safe unblocked work without waiting for a new user
message. If a runtime capability remains blocked, keep implementing adapters,
UI and isolated tests that do not pretend the missing capability works. Keep
the integrated path ahead of additional abstraction, unrelated upstream imports,
branding polish, or broad test expansion.

## Full v0 remains in scope after the first loop

Complete bounded semantic routing, scoped Honcho recall/write with provenance
and idempotent receipts, Work/Projects dispatch and review/artifacts, employee
create/adopt/update dashboard, and legacy pruning/install/upgrade/backup work.
These remain `REMAINING_WORK_V1.md` slices D–G; they are not silently deleted
from the product scope or claimed complete by reaching the private MVP.

## Operational boundaries

- Work in `/Users/nambse/dev/ortak.dev` and its isolated sibling worktrees.
- Existing Hetzner/Coolify Hermes/Buzz/Honcho services are preserved test
  infrastructure. A separate clean stack is allowed; do not overwrite old
  volumes, credentials, identities or memory. Do not use desktop Hermes.
- Cem and Zeynep remain test employee definitions. Old-resource adoption is
  optional. Their gateways are intentionally stopped; conditional restart was
  authorized only if a controlled test needs it and cannot restore the old loop.
- Build private/local first. Publishing `ortak.dev` requires verified DNS,
  TLS, authentication, network exposure and rollback configuration. The morning
  target does not authorize an unreviewed public deployment or a paid server.
- Follow `UPSTREAM_MAINTENANCE.md`: selective Buzz imports, pinned tested
  Hermes/Honcho artifacts, milestone checks rather than repeated full audits.
- Preserve secrets. Resolve existing references only for the intended service;
  never copy keys into Git, task prompts, browser screenshots or logs.
- Focus verification on the changed seams, then a real end-to-end smoke. Keep
  known safeguard tests; do not spend the night expanding unrelated suites.
- No usage-reset credit is authorized by the earlier question about whether
  resets are possible. Do not redeem one or buy credits without explicit consent.

## Continuity

The existing task has a long history. Continue implementation in a fresh Codex
task after reading `TAKEOVER_2026-09-05.md`; do not fork the entire conversation.
Keep a concise checkpoint ledger here (commit, completed gate, next gate,
unresolved facts), and use the app's heartbeat mechanism for continued overnight
work rather than promising background work with no scheduler. Keep quiet when
state is unchanged; notify on completion, meaningful failure or needed input.

## Integration checkpoint — 2026-09-05, early morning

### Noon handoff — 12:00 Istanbul

New unattended coding stopped at the owner-selected deadline. The app confirmed
`ortak-morning-delivery` updated to PAUSED at12:00:14; its saved status was checked.
All agent lanes finished, and no build/test/synthetic fixture remains running.
The private API/relay/native app and all retained databases/evidence stay in place.

Latest implementation commits are1dea0d0 (explicit credential references) and
5c285d2 (fresh activation admission plus synthetic HTTP evidence). Private backend
binaries remain at5c285d2 and native at the earlierd07f55c queue build. No push,
PR, full repository CI or real employee/provider activation is claimed. The
current CLI handoff supplies launch instructions, exact evidence and remaining
production activation/cohort/reconciliation dependencies. Manual Work and the
103-table database-only restore are proven; the real employee MVP remains open.

### Explicit credential references — approximately11:52 Istanbul

The control plane now has an environment-backed CredentialResolver source
implementation with a finite caller-authorized reference/name map. Construction
validates the entire allowlist without reading values. Verification denies
unknown references before lookup and reads current selected availability without
returning, logging or caching values. Six focused tests passed, including real
public-path checks in fresh bounded child environments. The trait has no company
or caller argument; all-target control-plane Clippy with warnings denied and
workspace formatting also passed. Signed checkpoint1dea0d0 records this slice.
The future runner must select an authorized resolver instance and align its
mappings with the owning adapters. This port is not yet composed or
deployed, and does not prove provider health or activate Ada.

The private relay and API still run the5c285d2 backend; native runs thed07f55c
queue package. After relay restart, a separate authenticated CLI channel listing
passed against the fresh owner. The native TCP connection seen immediately after
relaunch was absent at11:47; keep its UI/session/reconnect acceptance unverified.
All backend listeners remain bound to127.0.0.1, and final Docker inventory found
no remaining synthetic fixture containers or networks.

### Fresh activation admission — approximately11:36 Istanbul

Migration56 and the provisioning repository now commit activation only against
a fresh database-issued target, exact running attempt and current Office/employee
authority. Every activation attempt rechecks real adapter/signer evidence within
one bounded lifetime; deferred commit-time expiry rolls back the revision,
bindings and success receipts atomically. Success records are immutable. Reusing
the same Office identity keeps its original binding provenance.

Verification passed25 saga unit cases,25 control unit cases and14 distinct
PostgreSQL provisioning cases (13 together, then the new reuse regression).
Scoped four-crate all-target Clippy, both Rust formatting roots and actual
pgschema/migrator56 parity passed. No full repository `just ci`, PR or push is
claimed. Signed previous checkpoints are2eac15f,f23c9fd,d07f55c.

The rebuilt private backend upgraded the selected database through56. All nine
live signed API checks and the unchanged manual Work replay7→7 passed. The
11:35 database-only backup restored into a fresh retained database with all103
table counts, migration checksums and schema matching; see the backup runbook.

The final synthetic Hermes check joins the actual controller, journal, executor,
pinned Hermes AIAgent and OpenAI SDK with an explicit local synthetic Responses
endpoint and a test-only OS-header helper. It passed four gate groups: three
synthetic model requests plus two authenticated catalog404s, zero real-provider
requests and confirmed fixture cleanup. Eight local tests passed. Exact seams,
pins and evidence are in `runtime/hermes-bridge/SYNTHETIC_HTTP.md`.

Ada remains draft and routing stays disabled. Real provider selection, production
provisioning composition and the signed Office roundtrip remain open. A live
native process alone is not visual UI evidence. The app was gracefully relaunched
as PID18023/session64511 after the56 relay restart, with TCP to3038 established.
The previous process did not recover that connection during observation; source
review found an existing retry path but no proven bug without authenticated
session/UI state. Automatic reconnect and native OS interaction remain unverified.
The synthetic SDK metadata refusal belongs only to the fixture's extra audit
hook, not a demonstrated production request failure; no SDK patch was added.
The implementation is signed as5c285d2. Work continues to the authorized12:00 stop.

### Employee queue and safe community detach — approximately11:00 Istanbul

The final55 candidate adds scoped read-only employee assignments, with all
permission/source filters before pagination and current-state checks after row
waits. Full Work19PG, signed API12PG, four headless scenarios and scoped
Clippy passed. The assigned-work screenshot was inspected. Three real deletion
regressions plus20 retained deletion cases prove approved detach, independent
company history retention and expired-executor rollback.

Actual pgschema and migration55 catalogs matched in two fresh retained databases;
reconciliation ran twice. The private relay/API/admin were rebuilt, the private
DB upgraded through55, and nine live API checks including Ada's empty manual
queue passed. The unchanged Work replay retained version7. The55 database-only
backup restored into a fresh verification DB with matching103 tables/schema/
migration checksums. Receipt details are in the Work, schema parity and database
backup runbooks. The private native queue package built in43.77s after removing
only regenerable task artifacts to resolve disk pressure; its production
frontend matrix passed. No native OS visual interaction is claimed.

The complete real-provider employee loop remains open. An independent review
recorded the missing production provisioning identity/credential ports and the
Hermes/Honcho acquisition-mode mismatch in `ACTIVATION_COMPOSITION_GAPS.md`.
Cached activation-evidence freshness is the next bounded implementation being
prepared; a separate real-SDK/controller test uses an explicit synthetic local
provider and will remain fixture evidence. Ada is draft; no activation occurred.

### Manual Work and private upgrade checkpoint — approximately10:10 Istanbul

Manual E1 now has authenticated project creation, durable project roles,
channel/source visibility, atomic operation receipts, manual status, criteria
and human review, plus the desktop Projects/Work surface. Core14 PostgreSQL
tests passed (4.50s), all10 signed API PostgreSQL tests passed (5.53s), and the
runtime suite passed38 cases (58.79s). The routing suite passed14 cases,
including live claim-expiry contention. The13 ordinary migration checks passed;
their7 PostgreSQL-only cases were not part of that invocation. Five integration
crates passed all-target Clippy with warnings denied before the final small
test/document refinements. Full repository `just ci` has not run.

The full headless surface suite passed3 cases. After the final abort cleanup
regression, the Work-only smoke passed again (2.9s), and its project/review/done
screenshots were visually checked, readable and distinct. Six focused Work
UI unit cases and11 existing client/render cases passed; TypeScript and Biome
passed. These screenshots use the isolated browser fixture.

The selected private database upgraded through0054 using the rebuilt native
migrator under the schema lock. Stable SQLx builds now watch the migrations
directory. The sole fresh private owner explicitly received manual project
creation capability; the exact previous configuration remains protected in
`api-config.before-work.json`. No employee was activated. The refreshed API
is PID49598/session38127 and its7 existing signed network checks passed. The
private native package rebuilt successfully (43.53s Rust build plus production
frontend), restarted as PID49854/session82657, and its connection to the private
relay3038 is established. OS-level visual interaction remains unverified.

Follow-up at approximately10:15: signed checkpoint `2eac15f` records the
297-file integration. Both workspace and Tauri formatting checks pass; the
295 present source files passed the pre-stage private-value/binary audit.
Generated browser assets and all private state stay outside Git. No push occurred.

The actual private manual Work script passed first traversal (version1→7) and
an unchanged replay (7→7). It observed draft assignment refusal, acceptance and
required-human-approval completion guards, exact-operation replays/conflicts,
and seven dense attributed history rows. Read-only SQL confirmed one project,
one item, eight operation receipts, zero runs/outbox/routing decisions, and
Ada draft with no active revision. Script interruption/transport fixtures pass
all five tests. These are explicit retained manual test records.

The upgraded database backup restored into a new retained verification database:
all103 public table counts, migration checksums through0054 and the selected
schema catalog matched. Receipt directory is
`/private/tmp/ortak-private-20260905/backups/20260905T071413Z_9605df7a4ddc4795a342e62090b381fd`;
the archive is523,975 bytes with SHA256
`1cb330c41326efbaac1179661e542061bf9917134a7997034feb940def0bf265`.
This remains a database-only backup. Ada remains draft, central routing remains disabled, and
no real model/provider or semantic run has occurred. Work execution, artifacts,
sharing, realtime delivery and provisioning remain open. The next bounded E
substep is an authorized read-only employee assignment queue. This entry does
not claim a push or full-v0 completion. The older sections below retain their
historical evidence.

### Semantic evidence and operations checkpoint

Latest follow-up, approximately09:25 Istanbul: the shared API/worker database
connector now enforces500ms lock,5s statement and10s idle-transaction bounds.
All three signed HTTP/PG suites passed2.39s, including cancellation lock failure,
complete rollback, release of all eight connections and a fresh signed retry.
The private API was replaced after exact process verification: PID16903 listens
on127.0.0.1:8787 and all seven signed private network checks passed. It now uses
the verified shared shutdown/database code; no worker or provider was started.
The existing native desktop remains on its previously verified memory build.

Status tests now total12 and include opaque optional semantic configuration.
The operations script run passed32 tests with one explicitly live SQL test
skipped; the Hermes fixture suite passed46 on Python3.12.8. Seven isolated
Honcho build-helper tests passed after fixing the local daemon/builder selection,
fresh Docker environment and bytecode exclusion. Only build-tooling/context
source changed; previous Honcho image IDs remain historical tested artifacts,
and this revised build context has not been rebuilt.

Work/Projects E1 implementation is in progress: authenticated manual project and
task operations, explicit durable project roles, canonical channel promotion,
idempotent operation receipts and safe DTOs. This is not yet a verified delivery
gate. Work dispatch/artifacts, project-sharing UI, and full-v0 acceptance remain
open. A separately reviewed routing lease-expiry fence is being tested before
the integration checkpoint is committed.

- New `ortak-routing-semantic` supplies strict, bounded Chat Completions scores
  only when explicitly selected; the private stack has no such selection.
  Real loopback HTTP/transport and cache/circuit tests:6 passed0.31s. They cover
  secret redaction, exact model/candidate coverage, duplicated JSON control
  fields, refusal/tools/redirects/oversized replies, cache invalidation, two-call
  concurrency and cancelled requests without detached cache updates.
- Control semantic input is sealed to company, source, policy and exact active
  revisions. A five-second total budget spans refreshes and is capped by claim
  expiry. All25 control unit tests and11 PostgreSQL routing tests passed
  (0.13s/3.29s), including late first-poll rejection, durable timeout silence,
  revision refresh and a reclaimed request that cannot dispatch.
- Worker semantic selection remains disabled when absent; malformed selection
  records unavailable semantics without preventing existing recovery. Worker
  unit tests:3 passed. No provider credential was loaded or model called.
- Shared process shutdown registers SIGINT/SIGTERM before startup I/O, drops
  in-flight worker cycles while preserving durable recovery and caps API drain
  at15s. Six tests passed0.02s, including real signals to owned test children.
  This does not prove coordinated remote process shutdown.
- Focused Clippy over control/router/semantic/server, all targets with warnings
  denied, passed1m09s. Source and tests compiled together after integration.
- Database backup/restore verified100 table counts, migration52, schema and
  component hashes. Archive502264bytes remains private alongside the new
  verification database and earlier failed comparison receipt. Nine backup tests
  passed0.602s, including the real dropped-column ordinal regression. See
  `runtime/private-stack/DATABASE_BACKUP.md`; full-stack backup remains open.
- Eleven status tests passed; its unauthenticated observations explicitly leave
  authority and workflow readiness unverified. All documented Docker recipes
  now pin the local daemon and dated project with a reconstructed environment.
- To avoid duplicate build caches, subsequent root Cargo commands use the
  original pinned registry at
  `/Users/nambse/dev/ortak.dev-worktrees/buzz-import-2026-09-05/.hermit/rust`,
  target `/private/tmp/ortak-root-build-target`, pinned Rust1.95, jobs2 and
  incremental0, with no profile overrides. Only redundant task-owned artifacts
  tied to the other private registry path were removed; source, state, current
  builds and unrelated caches were preserved. Free disk recovered to3.8GiB.

The API replacement above supersedes the earlier memory-only API binary; its
semantic adapter remains unselected. The native desktop remains unchanged.
All integration changes remain uncommitted; no PR/public deployment, selected
provider credential, real model reply, usable-MVP or full-v0 completion claim.

### Verified private package at approximately08:23 Istanbul

- Final integrated runtime PostgreSQL suite:37 passed46.73s, including the35s
  cancellation backlog deadline and preserved cursor, unrelated stop completion,
  exact reply replay, admission fences and memory-context/write invariants.
- Signed HTTP memory projection passed0.71s after its fixture gained the actual
  delivery-chain reservation. It proves bounded/redacted recall, original-source
  pins for both recall and writes, private-field omission, current audience
  revocation, corrupt data refusal and same-channel retarget rejection.
- Selected worker pool's real held-inbox routing test passed1.28s. Worker memory
  composition passed two unit tests and its actual private Honcho compatibility
  test0.17s, including restart readiness, unchanged original configuration and
  refused create/delete operations. This uses the real readonly company lookup
  and explicit diagnostic memory I/O; it starts no runtime worker or provider.
- New native package rebuilt44.40s. Its API and desktop processes were replaced
  only after verifying their exact executable paths. Current API PID40959 listens
  on127.0.0.1:8787; private desktop PID40990 has an actual TCP connection to
  relay PID5000 on localhost3038. Native visual interaction remains unverified
  because OS Computer Use permissions are pending; browser visual checks passed.
- Rebuilt buzz-admin applied migration52 to the private app DB; actual
  _sqlx_migrations reports52/success. Current private DB has one draft employee
  and zero runs. All seven signed network API checks passed again after restart.
- Read-only changed-file audit found no build artifacts, likely secret literals,
  new production unsafe/unwrap/expect, or whitespace errors. Differential
  desktop/web/mobile size gates passed using origin/ortak/main (origin/main is
  absent). One desktop script format-only issue was corrected. Focused Clippy
  and private database backup/restore work are in progress.

All source changes are still uncommitted. No provider key or real model reply
has been demonstrated, and the private MVP/full v0 acceptance gates remain open.

### Memory integration at approximately08:16 Istanbul

- Immutable pre-run context, same-run bounded recall, fresh memory/Office
  admission and exact-spec retries compiled together. All34 runtime PostgreSQL
  cases passed7.47s, including four supervisor context cases, two snapshot
  persistence/deadline cases and two real signed Office→memory scheduler cases.
- Two additional actual Office delivery contention cases passed10.00s: stalled
  publisher timeout and held acknowledgement row both preserved cancellation,
  finite retry state and exact signed event bytes. Runtime replay fixtures are
  still not real model-provider responses.
- Unicode-whitespace partition review found a valid32767-byte reply that needed
  both memory fact boundaries moved. The bounded linear splitter fix passed
  19control unit tests and all10 PostgreSQL memory-job cases1.88s. Test fixture
  migration initialization now runs once, avoiding concurrent schema-fence
  collisions. Migration0052 is unchanged and remains tested only on the
  disposable0052b database; the private app database still needs migration.
- The fresh Ada Honcho bootstrap completed on the actual selectedcc8 runtime
  image. Stable original creation/diagnostic keys, native resource IDs and the
  server receipt are saved in the protected private state. A second invocation
  only inspected the owned resources and preserved the configuration. Ada is
  still draft/inactive and no runtime worker was started.
- Read-only run memory API projection and desktop Memory panel are implemented.
  The projection validates original source/revision/channel pins, redacts text,
  and excludes runtime configuration/credentials/prompts. Its new signed-PG test
  is still being completed; do not claim that gate passed yet.
- UI18 focused tests and typechecking passed; rebuilt private E2E frontend plus
  both real browser scenarios passed15.8s. The browser fixtures prove display,
  keyboard expansion and continued polling until a memory receipt. The pending
  and confirmed Memory screenshots were visually inspected and differ.
- Worker review identified unbounded routing/database locks and long terminal
  replay while cancelling. A selected worker pool with SQL deadlines and a35s
  whole stop-attempt deadline are integrated; their focused PG checks are pending.

No actual model response or end-to-end usable MVP is claimed. The new Memory UI
has not yet been rebuilt into the native package. No PR, push or commit occurred.

### Integrated local services at 07:42 Istanbul

Terminal execution now proceeds with actual full access and no command approval
requests. All services below belong to the fresh private stack. Central routing
and employee activation remain off; no fresh model-provider credential is available.

- Fresh source-built MinIO image `sha256:e1d7f7262c86498b45f869bcc7e3bbe7c11b3c026d9aad25f7759b053fd60a41`
  passed version, live readiness, signed bucket create/replay and the retained
  relay's startup object-store conformance gate. It binds only localhost9008.
- Relay is running with actual listeners127.0.0.1:3038/8089/9198.
  Health uses `/_liveness` and `/_readiness` on8089; metrics use9198.
  Private community `55bebe0f-90f0-44a2-a021-3b69fbb520a6` and private Office
  channel `f6bcbca6-9974-4792-8f2c-e19718f6bc11` were created through the real relay.
- `bootstrap_private_control.py` verified the fresh owner membership and
  atomically created the company/Office binding and draft `ada-private` employee.
  Rerun preserved identity/config. API8787 is running: the real signed directory,
  draft/health projection, NIP98 replay rejection, exact signed URL, audience
  denial and private CORS checks all passed through the network and real DB/Redis.
- Full private Tauri package built successfully; a subsequent42s rebuild pins
  automatic connection to the selected localhost Office. Actual bundle identity
  is `dev.ortak.private20260905`, private deep link and data namespace verified.
  The explicit `desktop` launcher selects the fresh test owner from private state,
  preserving existing desktop/keyring identities. Actual native→relay TCP
  connection is established. Native visual interaction is not verified: Computer
  Use stopped at pending macOS Accessibility/Screen Recording permissions.
  First native boot also downloaded pinned inherited voice model assets into
  only `.buzz-demo-ortak-private-20260905/models`.
- Real Hermes controller HTTP recovery passed12 assertions in the selected
  controller image, including401 auth, honest disabled-start/health responses,
  cancel-before-start tombstones, SIGKILL/restart, dense durable replay and
  idempotent terminal cancellation. This used no provider call or Docker socket.
- Honcho ORM authority-refresh fix passed12PG tests and actual native HTTP smoke.
  Rust memory adapter first candidate passed3unit/4HTTP/1live-extension test,
  including scoped full-text write/read/replay and restart gating. Independent
  review then found and fixed witness expiry during inspection and mutable-name
  replacement being mistaken for the original native resources.
- Latest Honcho extension adds read-only immutable resource inspection; Rust
  `resume_created_resources` requires the original create receipt/key and never
  creates missing resources. Latest gate tests:4Rust unit +7HTTP passed;
  extension14PG passed3.71s on test image
  `sha256:9c685bb4bc2bca526cc097c1e23a9d5f8b012dd7e85d0dfe453b41f5f7a08d8c`.
  Runtime image `sha256:cc8b4a29c0adda08978886e205ff5c5ff0a13923e4ed15e1626b24194d0c0c21`
  then passed actual native HTTP smoke and the updated Rust live-extension test
  (0.23s), including read-only restart recovery. These are memory I/O proofs in
  full-text mode, not embedding/derivation/model-provider health.
- Migration0052 introduces immutable run-context snapshots, memory-binding
  mutation fencing and durable post-acknowledgement RunScratch memory jobs.
  Control16unit and9PG cases passed, including delivery lock contention,
  repeated admission clock expiry, lease-deadline expiry at commit and exact
  trailing-newline preservation. Production runtime context integration and
  its complete workflow tests are still being completed.
- Worker memory composition, canonical post-delivery memory scheduler, bounded
  Office delivery and transaction-local outbox deadlines are written but await
  the remaining context types and integrated compile/PG checks. Do not mark these
  new worker paths verified merely because their dependency suites are green.

All integration changes remain uncommitted. No actual model response, complete
human-message→Hermes→Office reply, public deployment, or finished v0 is claimed.
The earlier checkpoint below is historical and contains superseded pending gates.

### Resumed validation at approximately06:40 Istanbul

The desktop session now has actual danger-full-access with approval policy
never. Root executes Docker/TCP/PG checks directly. The earlier CLI resume
failed because this session already had an active writer; work continued here.
The existing heartbeat remains active until the12:00 owner deadline.

- The bounded Office output fix passed all six focused PostgreSQL output cases,
  including a held-row contention test and unrelated-run cancellation.
- The private UI passed actual first-run and Activity browser scenarios. Four
  screenshots were inspected and have distinct hashes. Seventeen focused UI
  tests, typechecking and the private build passed. The six-file isolated native
  package recipe also passed three command tests, eight real native identity
  module tests and its production frontend build; full Tauri build/launch has
  not run.
- The exact Hermes source29112bef099274229cadff79cdff7bf7b99c4b77 now has17
  locked source hashes and46 bridge tests. Real constructor and real run-loop
  checks passed with zero network/provider calls. A discovered unknown-tool
  retry was fixed by rejecting tool intent before transport normalization.
- Final local worker OCI image:
  `sha256:623fae9e3b38c75bc3cb94f73bc3d1c303bc3ed6a77765eb51fc17b54cc90b18`.
  Controller image:
  `sha256:ef9a9d2a7446d9e13cdbf94cf1a2152011b5a72050e450d500356f059852d7b1`.
  Actual containment passed eight cases: running/failed/completed process stop,
  cancel-before-start, controller SIGKILL recovery, no blind rerun, dense durable
  replay, and a child deadline while the controller is absent. SQLite3.53.4 is
  in both images. Evidence directory:
  `/private/tmp/ortak-hermes-checks-20260905/containment-381ebc6a-210b-4730-b2f5-d47e57e42094`.
- Fresh PostgreSQL/Redis project `ortak-private-20260905` is healthy on loopback
  ports55433/56382. All native migrations passed. Fresh identities remain in a
  private file outside Git. Real Redis unauthenticated denial/authenticated
  acceptance and identity-initialization idempotency passed. Relay diagnostic
  loopback regression and native binary rebuild passed. Relay boot correctly
  stopped at its missing object-store conformance gate; MinIO source build is
  being prepared, and central routing remains disabled.
- Honcho3.1.1 atomic receipt extension first candidate passed11 real PG/HTTP
  cases after native migrations on a separate pgvector store, with no provider
  calls. Final replay-integrity refinements require a new artifact/test pass.
  Rust MemoryAdapter implementation is underway in the API agent worktree.

There is still no actual model call, complete human-message→signed-Office-reply
workflow, deployed endpoint, signed integration commit/push, or completed v0.
No selected fresh provider credential is available. Do not turn those gaps into
capability/activation success based on fixture evidence. The notes below record
earlier checkpoints and may contain superseded counts or pending gates.

The private integration branch has B2 pinned permission transport applied, plus
uncommitted B1b/API/runtime/UI integration work. No live routing or external
employee execution has been enabled by this checkpoint.

Implemented and being integrated:

- Migration0048: company/community Office mutation generations, shared admission
  fencing, clock deadlines, and a fresh token for every explicit re-admission.
  Independent review found and fixed suspended-company admission, pinned Office
  identity rotation, and unchanged-witness expiry while waiting on an existing
  run. Routing snapshots and commits carry the witness; runtime admission
  re-normalizes canonical Office input while retaining pinned permissions.
- Migration0049 and `ortak-server`: signed NIP98, explicit human/channel/employee
  grants, durable cancellation requests, bounded Activity reads and CORS.
- Migration0050: adapter-filtered durable cancellation queue, retry budget,
  lost-start-key cancellation and restart-safe Office revocation review. Terminal
  stop acknowledgement also settles the corresponding dispatch and human request.
- A bounded Hermes HTTP transport and explicit opt-in `ortak-worker` executable
  now compose canonical routing, dispatch, cancellation, event pumping and Office
  delivery. Recovery remains available without activation capabilities or valid
  Office delivery configuration when the bridge has the required recovery
  capabilities; new routing and dispatch remain paused.
- Migration0051 atomically creates completion output jobs. The scheduler freezes
  canonical source facts and the last assistant turn, then idempotently enqueues
  the immutable draft. The real signer and HTTP publisher recheck live authority
  and the configured community host, and retry byte-identical signed events.
  The actual external Hermes execution boundary still requires deployment
  evidence; transport and persistence tests are not a real runtime smoke test.
- Desktop Employees/Activity screens use the existing account signing path,
  community-specific API configuration, resumed event cursors and honest pending
  cancellation state.

Evidence so far: focused SQL fencing suites passed on both direct desired-schema
and migration-built disposable databases; cancellation SQL probes passed on both.
The four-crate Rust suite passed 74 non-PostgreSQL tests. The subsequent explicit
ignored-test run passed 61 PostgreSQL tests: 17 control/provisioning, 12 channel
normalization, 6 Office delivery, 25 runtime supervision and 1 authenticated API
route test. One real loopback HTTP transport test also passed. This run used the
final embedded migrations through 0051 and covered cancellation recovery,
canonical output freezing, delivery retry, stale admission and publisher host
validation. Desktop type checking, scoped formatting, E2E-mode build and14
client/render tests passed in the UI lane; browser smoke remains a separate gate.
All executed PostgreSQL checks used explicit disposable localhost:55432
databases. No generic DATABASE_URL fallback is permitted for the new runtime
tests.

Still required before claiming the private workflow works: exercise the browser
seam, inspect and deploy the actual pinned Hermes execution boundary, demonstrate
start/lost-ack/cancel/restart/replay against that runtime, then prove one real
message roundtrip on the isolated stack. No fresh deployment, mainline
integration push, or full-v0 completion is claimed by this checkpoint.
