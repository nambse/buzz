# Ortak overnight delivery plan

Decision: 2026-09-05, Europe/Istanbul. This is the execution overlay on
`REMAINING_WORK_V1.md`, not a replacement for Architecture v0 or its safeguards.

## Outcome and deadline

The owner asked to continue proactively and have Ortak ready by morning.
Use **08:00 Istanbul on 2026-09-05** as a planning checkpoint, an assumption
to revise if the owner supplies another time. Target a usable private MVP
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
