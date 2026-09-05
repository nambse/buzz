# CLI continuation checkpoint — 2026-09-05

Current checkpoint: **12:00 Istanbul**. The private stack is running migration
**0056**, with a verified database-only backup and manual Work workflow. **The
real employee MVP is not complete:** Ada remains draft, central routing is off,
and no real model/provider is selected or has been called.

## Ownership and source

- Worktree: `/Users/nambse/.codex/worktrees/a5ed/ortak.dev`.
- Branch: `codex/ortak-private-mvp`; latest signed implementation checkpoint
  **`1dea0d0`** adds the bounded credential-reference resolver. Six focused tests,
  scoped all-target Clippy and formatting passed. It remains uncomposed and
  undeployed. Running backend binaries contain **`5c285d2`**, the validated
  migration56/activation and synthetic-check checkpoint. Earlier signed commits
  are `d07f55c`, `f23c9fd` and `2eac15f`. Staged audits of26 then4 source files
  found no fresh private values, PEM material or binary artifacts. Subsequent
  commits may contain documentation only; verify HEAD/diff before resuming.
- Session: `01a06f05-497a-7380-a611-75b7d9432d60`. The owner extended work to
  **12:00 Istanbul today**. New unattended coding stopped at noon. The existing
  `ortak-morning-delivery` heartbeat was updated through the app to **PAUSED**
  at12:00:14 and its saved status was verified. This turn only finishes the
  handoff; CLI ownership is not assumed released until the desktop writer exits.
- Root currently has full access with approval policy `never`; older subagents
  retained restricted runtimes, so root ran infrastructure tests centrally.
  Read the actual permissions of any resumed session.
- CLI continuation is `codex resume 01a06f05-497a-7380-a611-75b7d9432d60 --yolo`.
  Start it only after the current writer releases ownership and the existing
  `ortak-morning-delivery` heartbeat is confirmed paused. An earlier concurrent
  resume failed with an active-writer conflict. If the desktop still owns the
  writer after this turn ends, quit Codex completely before retrying the CLI;
  do not delete lock/session files or start a second writer.

Read [AGENTS.md](../../AGENTS.md), [Architecture v0](ARCHITECTURE_V0.md),
[remaining work](REMAINING_WORK_V1.md), and the
[validation ledger](OVERNIGHT_DELIVERY_PLAN_2026-09-05.md). Preserve all current
work and the external Cem/Zeynep test resources. No private credentials belong
in manifests, logs, commits or this handoff. No full `just ci`, PR or push has
been completed for this checkpoint.

## Latest evidence

| Gate | Actual result and limits |
| --- | --- |
| Activation freshness | 25 saga and 25 control unit tests passed; 14 distinct provisioning PostgreSQL cases passed (13 together, plus the same-key Office-binding reuse case). Real repository/locking/deferred-commit behavior, **synthetic external adapters**. |
| Schema56 | Actual migration-built vs `pgschema` desired-state parity passed, including reconciliation and activation guards. Private migration56 applied at 11:34; backend rebuilt in 2m33s. |
| Work and authenticated APIs | 19 core Work PostgreSQL and 12 signed API PostgreSQL tests passed. Four headless UI tests passed, with inspected distinct screenshots. Native queue package built. |
| Private manual workflow | Nine signed API checks and Work replay passed after migration56. Original create→review→completion advanced version 1→7; replay remains 7. One project, one completed item, 8 operation receipts, 7 history entries, zero assignments/runs/outbox/routing decisions; Ada draft. |
| Runtime recovery | Production-seam PostgreSQL tests cover authority changes, lost-start cancellation, immutable context/output, bounded row waits and retries. Earlier 38-case runtime suite passed; this is not a deployed provider-backed loop. |
| Hermes synthetic HTTP | Real bridge HTTP, pinned Hermes/SDK, journal and contained worker passed with 5 HTTP requests: 3 synthetic Responses calls and 2 catalog 404s; zero real-provider requests, owned cleanup verified. Explicit endpoint and SDK OS-header test seams remain; no production provider-health claim. |
| Honcho | Fresh pinned native extension, atomic remember receipts, scoped recall and real local HTTP/Rust checks passed. Full-text memory I/O is proven for its explicit test/diagnostic resources; embedding/derivation provider health is not. |

Do not add overlapping suites into a new aggregate. Detailed receipts and limits
are in [activation gaps](ACTIVATION_COMPOSITION_GAPS.md),
[Work E1](WORK_API_E1.md), [Hermes synthetic HTTP](../../runtime/hermes-bridge/SYNTHETIC_HTTP.md),
and the [Honcho validation record](../../runtime/honcho-adapter/VALIDATION.md).

Schema56 parity receipt:
`/private/tmp/ortak-private-20260905/logs/schema-parity-b9c1c3fbe80944e6b827b41a7428bbbd/receipt.json`.
Desired schema file SHA256:
`8acafb2213bc3bdf7406064cfa1b20ff342919722a9ff3a90896b24b17d360a0`.

Verified schema56 backup manifest:
`/private/tmp/ortak-private-20260905/backups/20260905T083500Z_952d0c34d48f462ba1d3268d872a5438/manifest.json`.
Archive: 537977 bytes; SHA256
`e737171d4fa1177edba41c26d03b98a0dc48ec0a23952550e1ca2948ee6b9154`.
Fresh retained restore database:
`ortak_verify_7a359a24f12a4a8795768df594c74f84`.
All 103 table counts, migrations 1–56 and semantic schema matched; catalog schema
SHA256 `8c78de1551cd2bba299b7919cdf3e2cccff4749f4113231c46f0050a8c9c42d8`.
This catalog hash and the desired SQL file hash measure different things.
Earlier successful and failed backups/verification databases are retained.
This is **database-only**, not coordinated MinIO/Honcho/bridge/secret recovery.

The integrator's binary hashes and observed source attribution are retained in
`/private/tmp/ortak-private-20260905/logs/artifact-observation-1145.json`.
Backend binaries contain the5c285d2 source. The native package is the earlier
d07f55c queue build, not a rebuilt migration56 native package. This is an
observed build record, not a reproducible-build attestation.

## Running private services

State directory: `/private/tmp/ortak-private-20260905`. Use the read-only status
helper and [operator runbook](../../runtime/private-stack/OPERATIONS.md):

```sh
python3 scripts/ortak/private_status.py --state-dir /private/tmp/ortak-private-20260905
```

Last observed processes after the schema56 backend restart: relay PID 17426
(session 78300), API PID 17461 (session 99640). Native PID 85310 was gracefully
stopped after it did not retain a TCP connection across that relay restart;
relaunch session 64511 now runs PID 18023 from the exact private bundle. At 11:37
its loopback TCP connection to 3038 was established, but was absent at11:47.
A separate authenticated CLI channel listing passed against the same fresh
owner and relay. The native observation is **not authenticated
WebSocket, native UI/OS-interaction or automatic-reconnect proof**. Source review
found no concrete reconnect bug; the earlier UI/auth state was unobserved. Recheck
process identity and exact executable before any stop/restart; PIDs can be reused.

The private relay uses 3038, health 8089 and metrics 9198; API 8787, PostgreSQL 55433,
Redis 56382, MinIO 9008 and Honcho 8009. Hermes 8650 is optional and does not imply
an enabled employee worker. PostgreSQL 55432 is the separate disposable test
service. Never point reset/seeding tests at private 55433 or the preserved old
services. Docker operations use the explicitly selected local socket and owned
project from the runbook; never delete volumes or retained verification databases.

The already-running app is **Ortak Private**. If it has been closed, launch its
verified bundle with the private identity/environment helper from this worktree:

```sh
python3 scripts/ortak/private_native_services.py \
  --state-dir /private/tmp/ortak-private-20260905 \
  --binary-dir 'desktop/src-tauri/target/ortak-private-native/debug/bundle/macos/Ortak Private.app/Contents/MacOS' \
  desktop
```

Backend launches use the same helper with
`--binary-dir /private/tmp/ortak-root-build-target/debug` and action `relay` or
`api`, each in its own terminal. Check for an existing exact process first.
Opening the raw API URL without signed authorization correctly returns401;
it is not a browser dashboard URL.

## Next implementation dependency

[Activation composition gaps](ACTIVATION_COMPOSITION_GAPS.md) is the concrete
next slice. Freshness is implemented and tested; the remaining composition is:

1. A real provisioning Office identity adapter. Existing Office signer/publisher
   implementations serve completed-run delivery. A new source-only
   `EnvCredentialResolver` implements the existence port using at most 128
   explicitly authorized opaque-reference/environment-name mappings; its caller
   must select the correctly authorized instance because the trait carries no
   company scope. It checks current selected environment presence, returns no
   secret and does not prove provider health or act as a credential manager.
   All six focused tests passed centrally in0.05s after a27.47s compile; scoped
   all-target Clippy passed in39.51s. The resolver
   is not composed into a production saga or deployed by this increment.
2. A coherent acquisition path: Hermes currently supports Adopt only, while
   executable Honcho memory uses extension-owned Create receipts and explicit
   I/O validation. The saga currently assigns one mode to all resources.
3. An explicit default-off saga runner over those real adapters, preserving
   exact native IDs, original ownership and retry receipts. Prefer adopting
   explicitly prepared fresh resources; never hand-insert an active revision
   or turn old health receipts into activation evidence.

Then select an explicitly authorized fresh provider/profile, close server-owned
channel cohort selection and inbox reconciliation, and prove one human message
→ one real run → ordered Activity → one signed reply. Exercise cancellation,
restart around lost acknowledgements, exact-byte delivery retry and scoped
memory. Keep routing off until the gates pass. The synthetic Hermes test's
SDK OS-header override satisfies an additional test-only process audit;
production does not install that audit hook. No production SDK failure or
process escape was established by that fixture finding.

Full v0 still includes broader semantic/memory behavior, Work execution and
artifacts, provisioning UI, legacy pruning, upgrade and full-stack recovery;
see [the dependency inventory](REMAINING_WORK_V1.md).

## Resume discipline

Confirm single-writer/heartbeat ownership, current HEAD/diff and the latest
process/backup receipts first. Activate Hermit before Git/hooks and use signed
commits. Root's writable Cargo target is `/private/tmp/ortak-root-build-target`,
with `CARGO_HOME=/Users/nambse/dev/ortak.dev-worktrees/buzz-import-2026-09-05/.hermit/rust`
and pinned Rust1.95.0 at
`/Users/nambse/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin`. The older
`/private/tmp/ortak-local-cargo-cache` was retired; do not rely on it. Serialize
builds; separate worktrees need separate targets. New migrations require a fresh
explicit disposable database and a refreshed `buzz-db` build so SQLx embeds the
correct migration set. Do not rerun broad tests without a relevant change.

**Noon handoff completed:** new unattended coding stopped and the existing
heartbeat is paused, confirmed by the app update and saved automation state.
All three agent lanes finished; no build, test or synthetic fixture is left
running. Private databases, API/relay and native app remain available. Their
process and TCP observations above are timestamped evidence, not a claim of
native visual or real-provider acceptance. Latest observed host free space was
approximately1.5GiB; keep builds serialized and preserve all source/private state.
