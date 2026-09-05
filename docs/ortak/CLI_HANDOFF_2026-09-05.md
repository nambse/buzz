# CLI continuation checkpoint — 2026-09-05

Current validation ledger: see the approximately10:10 Istanbul manual Work/private upgrade checkpoint in `OVERNIGHT_DELIVERY_PLAN_2026-09-05.md`. It supersedes the historical pending Docker, browser and output-contention gates below; work remains in this desktop session with approval policy never.

## Resumed after handoff attempt — 05:53 Istanbul

The user-started CLI reported an active-writer conflict and did not acquire
this session. The current task now receives `danger-full-access` with approval
policy `never`; the owner explicitly asked to continue here and extended work
until **12:00 Istanbul today**. The pause notes below are historical.
The existing heartbeat was reactivated with the new deadline and this worktree
as its current source. Do not start another writer for the same session.

Root executed the actual Playwright smoke successfully under the new profile.
The four resulting screenshots were visually inspected and have distinct
SHA256 hashes. The existing subagent processes retained their earlier restricted
tool runtime, so root is centrally running commands requiring the new access.
Office output contention correction and the pinned Hermes image build are now
in progress; no fresh deployment or real model roundtrip is claimed.

## Why this task paused

The owner is going to sleep and explicitly requested a pause so this same
session can be resumed in a user-started CLI with `--yolo`. The desktop task
continued to receive `workspace-write` and restricted network permissions even
though the owner reported Full access selected. The cause of that mismatch was
not established. Textual authorization did not alter the effective tool policy.
The agent did not launch a nested unrestricted CLI or modify permission files.

Session: `01a06f05-497a-7380-a611-75b7d9432d60` (verified from CODEX_THREAD_ID).
Working root: `/Users/nambse/.codex/worktrees/a5ed/ortak.dev`.
Local CLI accepts `codex resume <session-id> --yolo` (verified with `--help`).
Read the effective permissions supplied by the resumed CLI; do not infer them
solely from the old desktop history or a UI selection.

The app heartbeat `ortak-morning-delivery` has been set to **PAUSED** through
the automation tool. Keep it paused during CLI ownership to prevent duplicate
desktop writers. Do not treat this intentional handoff as cancellation of the
authorized overnight product work. Resume that work only after the owner's CLI
continuation prompt.

## Scope and ownership

Read `OVERNIGHT_DELIVERY_PLAN_2026-09-05.md`, `TAKEOVER_2026-09-05.md`, local
AGENTS.md, and relevant vision/testing guidance before further implementation.
The original planning checkpoint is 08:00 Europe/Istanbul on September5.
The owner authorized coding and subagents, signed reviewed integration commits
and pushing the owner fork, and a separate fresh private Hetzner/Coolify stack.
Preserve old services, profiles, credentials, volumes and Honcho memory. No
unreviewed public deployment, paid infrastructure, or usage-reset redemption.
Requested coding model/effort remains GPT-6 Astra / ultra.

Root branch is `codex/ortak-private-mvp`, last committed HEAD `6280436`.
The large integration patch remains **uncommitted**; no mainline push or fresh
deployment is claimed. Canonical checkout is `/Users/nambse/dev/ortak.dev`,
branch `ortak/main`; owner origin is `git@github.com:nambse/buzz.git`. Avoid
overwriting existing checkout work. Commits require `git commit -s`; activate
Hermit before Git/hooks. Review the final diff before integration.

Agents were instructed to finish saving current work and stop for this handoff.
The API and Office agents reported no active processes. No Docker build or
browser smoke is running. Re-inventory agents before spawning replacements.

## Verified integrated evidence

The four-crate actual integration run exited0 against the explicitly disposable
`ortak_api_20260905` database on localhost55432:

- 17 control/provisioning PostgreSQL cases.
- 12 canonical channel-normalization PostgreSQL cases.
- 6 Office PostgreSQL cases, including freeze recovery and community-host guard.
- 25 runtime PostgreSQL cases, including cancellation and all5 output cases.
- 1 authenticated API PostgreSQL case.
- 1 real loopback Office HTTP retry case, outside that61 PostgreSQL total.

All **61 PostgreSQL +1 real HTTP** cases passed. The only failure in the first
run was a test fixture which expired a zero-duration lease before freezing;
the corrected test freezes with a30s lease and explicitly expires it afterward.
Earlier all four crates' regular Rust tests passed (**74** total).
Focused SQL fencing/output probes passed on both direct desired-schema and
migration-built disposable databases. No generic DATABASE_URL fallback is
allowed for these runtime tests; do not use5432 or existing service databases.

Desktop:15 client/render tests, TypeScript check, scoped Biome and a fresh
E2E-mode build passed. Playwright smoke discovery passed; **actual browser
execution and screenshots are still unverified** because binding localhost4177
required escalation. Source changes for the complete smoke have been copied
from the API worktree into root.

Migration0051 final SHA256:
`37af7c4e6af2500a081584b872713cd5c41f3a5ce0ac1311cea201adf4dcbbaa`.
Root buzz-db was cleaned and rebuilt before the actual passing suite, so the
static SQLx migrator includes final51. When adding/changing migrations, remember
that the existing SQLx migrate macro did not automatically rebuild this crate.

Exact passing command (from this root):

```sh
. ./bin/activate-hermit && ORTAK_TEST_DATABASE_URL=postgres://ortak:ortak@127.0.0.1:55432/ortak_api_20260905 CARGO_HOME=/private/tmp/ortak-local-cargo-cache CARGO_TARGET_DIR=/private/tmp/ortak-root-build-target CARGO_INCREMENTAL=0 RUSTC=/Users/nambse/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin/rustc /Users/nambse/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin/cargo test --offline -p ortak-control -p ortak-office -p ortak-runtime -p ortak-server -- --ignored --test-threads=1
```

Do not repeat this whole suite without a relevant change or concern. Root owns
`/private/tmp/ortak-root-build-target`; the API worktree has its own
`/private/tmp/ortak-api-build-target`. Never share one Cargo target between
worktrees: this already caused stale pre-B2 artifacts. The original large
import/toolchain caches are preserved and not writable build targets.

## Implementation now present

-0048: Office mutation generations, shared authority fences and fresh runtime
  admission tokens. Canonical facts, current identity, company/community state,
  scope and deadlines are revalidated across routing/admission/delivery.
-0049/API: NIP98 authentication, explicit server-derived human grants, bounded
  Activity, durable cancellation requests, narrow CORS and account signing UI.
-0050/runtime: adapter-scoped durable cancellation with bounded retries, lost
  start-key cancellation, revocation reconciliation and terminal preservation.
-0051/output: terminal completion creates a durable output job atomically;
  canonical text/tags/source facts freeze, then authorization and idempotent
  Office enqueue occur under the appropriate fences. Failure is durable.
- Office publisher: environment-backed explicit signer mappings; exact frozen
  signed bytes on retries and fresh NIP98 auth; expected community host checked
  before HTTP; bounded no-redirect HTTP response and matching accepted event ID.
- Worker binary: opt-in dispatch, activation/recovery capability checks,
  cancellation reconciliation, ordered event pumping, output preparation and
  delivery. Missing activation or Office credentials suppress new work while
  recovery capabilities can drain known cancellation. Initialization errors are
  explicit; leases and retry state remain durable.
- UI/API: a completed runtime and Office delivery have distinct states. Pending
  delivery keeps polling after runtime completion; failed/delivered stop polling,
  with Reload available. Only persisted relay acceptance becomes delivered.

## Concrete next correction found during the final review

The Office agent identified a remaining output scheduling concern; **it has
not been fixed or tested**. `crates/ortak-runtime/src/office_output.rs` claims a
batch of jobs with60s leases and processes them serially. The canonical target
lookup takes a run `FOR UPDATE` without a timeout. One held row can delay the
worker's next cancellation pass indefinitely and expire later leases before
processing, consuming their20-attempt budgets on repeated contention.

Inspect the production path and address this before treating worker recovery as
proven: claim one job immediately before each bounded phase and/or apply bounded
database/operation timeouts; add a falsifiable lock-contention regression. Do not
mistake the existing green output tests for evidence of bounded lock waits.

## Hermes candidate and next real runtime gate

Hermes agent worktree: `/private/tmp/ortak-hermes-bridge`.
API/UI worktree: `/private/tmp/ortak-product-api`.
The reviewed source candidate is
`29112bef099274229cadff79cdff7bf7b99c4b77`, not yet an accepted deployment digest.
Official archive exists at `/private/tmp/ortak-hermes-29112bef.tar.gz`:
SHA256 `76b99a8be9b77d66833c3cfe2b35c6d6f6a58e4ff9637ef8effcfc1f420ab35a`.
Verified extracted source:
`/private/tmp/ortak-hermes-source-29112bef/hermes-agent-29112bef099274229cadff79cdff7bf7b99c4b77`.
Extraction checked paths and bounded uncompressed size. No further source
download is needed. The lock manifest verifies12 critical source-file hashes.

Bridge foundation: SQLite WAL/FULL journal, permanent idempotency fingerprints
and cancellation tombstones, dense durable cursors, exact profile ownership,
empty four-field permission policy only, and all five real tool/delegation
entrypoints overridden before AIAgent construction. A fatal policy denial is
persisted. No tools or approval resume are advertised. Fake-executor/unit
evidence does not prove actual Hermes/provider behavior or containment.

The candidate Docker executor constrains the complete process tree, uses an
immutable image identity, bounded CLI output/timeouts, dedicated network,
read-only image/profile and one writable journal, labels/ownership fences,
bounded active containers and restart cleanup. It does not run upstream /init
or old employee gateways. Actual source is hash-checked before import. Service
execution remains opt-in behind `--enable-validated-docker-executor`; default
executor is unavailable.

The image recipe uses the pinned uv base digest and pinned SQLite3.53.4 source
to include the upstream WAL-reset fix. Its build-time `--network=none` smoke
constructs the real pinned AIAgent and tests all five denial boundaries. That
build has **not run**. The minimal worker image is not a packaged bridge
controller: the controller also needs the patched SQLite runtime, Docker CLI
and daemon access, separate from the unprivileged worker's mounts.

Build next, under the resumed session's actual permissions:

```sh
docker buildx build --load --build-context hermes_source=/private/tmp/ortak-hermes-source-29112bef/hermes-agent-29112bef099274229cadff79cdff7bf7b99c4b77 -f runtime/hermes-bridge/Dockerfile -t ortak-hermes-candidate:29112bef runtime/hermes-bridge
```

A mutable tag is only a local build handle; capture and validate the immutable
image ID/digest before selecting execution. The fresh profile currently supports
OpenAI/OpenRouter API-key providers only, not Codex OAuth. An actual fresh
provider credential and profile binding are still needed for model execution;
do not repurpose old profile credentials or print secrets. Old Honcho health
was inspected earlier; no memory was read or written.

After build/guard proof, demonstrate real profile-scoped start, lost receipt,
cancel, restart/replay, exact-byte reply retry and a complete human-message to
signed Office reply on the separate private stack. No real model call, complete
message roundtrip or new deployment has been proven yet.

Actual browser smoke command (already built in the API worktree):

```sh
cd /private/tmp/ortak-product-api/desktop
/Users/nambse/Library/Caches/hermit/pkg/node-24.15.0/bin/node node_modules/@playwright/test/cli.js test --config src/features/ortak/smoke/playwright.config.mjs
```

It covers keyboard cancellation failure, pointer cancellation, ordered activity,
and completed-run Office pending → failed → delivered, with four distinct
screenshots after animation completion. Port4177 is this disposable smoke only.

## Resume sequence

1. Confirm effective CLI permissions and that old agents/processes are stopped;
   retain this worktree and all uncommitted integration changes.
2. Read the final Hermes handoff addendum below and verify copied files. Keep
   desktop heartbeat paused while the CLI owns work.
3. Fix the concrete bounded-output-wait concern, then run appropriate tests.
4. Execute actual browser and pinned Hermes image gates, continuing useful work
   while any genuine external prerequisite remains unresolved.
5. Review/integrate signed commits and prove the private loop before readiness
   claims. At the morning checkpoint report actual evidence and remaining gaps.

## Final Hermes handoff receipt

The agent finished and stopped with no active processes or pending tools.
Its final **37 Python tests passed**, compileall passed, and all12 critical
source hashes verified against the actual extracted archive. No Docker build,
real-class image smoke, provider call or live runtime was executed.

The following final files were copied from the agent worktree into this root
and checked byte-for-byte; earlier bridge foundation files remain in place:

- `runtime/hermes-bridge/Dockerfile`, `.dockerignore`, `hermes-source-lock.json`.
- `ortak_hermes_bridge/__main__.py`, `docker_executor.py`, `worker.py`,
  `verify_source.py`, `candidate_smoke.py` under that bridge directory.
- `runtime/hermes-bridge/tests/test_source_and_cli.py`.
- `docs/ortak/HERMES_BRIDGE_V0.md`.

Worker and smoke environment sanitization retain only the fixed patched SQLite
library path; old host/provider environment is still cleared. Service startup
requires SQLite at least3.51.3. The controller packaging requirement described
above remains open. All three subagents have completed their handoff and stopped.
