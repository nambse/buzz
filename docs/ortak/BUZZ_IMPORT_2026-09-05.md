# Buzz Selective Import — 2026-09-05

Status: cherry-picks applied on `ortak/buzz-import-2026-09-05`, intended destination `ortak/main`; scoped verification recorded below

Policy: selective, user-approved imports only. Ortak remains an independent product with no automated upstream tracking and no compatibility promise. See `BUZZ_BASELINE.md` § Code-import policy.

## Reference points

| Point | Commit |
|---|---|
| Pinned Buzz baseline (unchanged) | [`b1f6b7ef770dddbb7f33c9f5861c379a47bca1d6`](https://github.com/block/buzz/commit/b1f6b7ef770dddbb7f33c9f5861c379a47bca1d6) |
| Ortak pre-import HEAD | `d4a8d4ccaab8375a117fd8e79bbffe906f6effa6` (merge: integrate Work and Projects foundation) |
| Upstream HEAD inspected | [`f038cbbb0d4092a72ffd93f17916f84d2b39bb43`](https://github.com/block/buzz/commit/f038cbbb0d4092a72ffd93f17916f84d2b39bb43) |
| Upstream commits in range `b1f6b7ef..f038cbbb` | 13 |
| Accepted | 8 |
| Deferred | 5 |

The `buzz-reference` remote stays fetch-only with push `DISABLED`. Imports were cherry-picked with original authors preserved; each local commit records its source SHA. Any short SHA below expands with `git rev-parse`.

## Accepted

| Upstream | Local | Author | Change | Why it fits Ortak |
|---|---|---|---|---|
| [`cd02b69`](https://github.com/block/buzz/commit/cd02b693aae86444b0b5636474a3d202c6fc8f26) | `0590b50` | Luke Tornquist | refactor(relay): extract NIP-29 membership authorization (#7285) | Pure policy split of channel membership authority in `crates/buzz-relay/src/handlers/channel_authz.rs`. Keeps every DB guard in `side_effects.rs`. Touches no Office-ingress or `office_inbox` path. |
| [`ee883d7`](https://github.com/block/buzz/commit/ee883d73fb84752d26f52c6606ad3cadc9f12cc7) | `dce967e` | Logan Johnson | fix(desktop): bind duplicate mention selections to exact recipients (#7133) | Exact recipient identity for same-name mentions in send, edit, and copy. Aligns with the Ortak rule that ambiguous names must fail visibly, never fan out. Extends the pre-existing `docs/mention-editor.md`. |
| [`d595806`](https://github.com/block/buzz/commit/d595806fc3b9c9758992e39b9b51cbb5f55791b0) | `b203bd8` | Logan Johnson | fix(desktop): authorize remote mentions at publication (#7124) | Authorization is checked at actual publication rather than at draft time; async draft persistence and cancellation fixes. Adds `docs/remote-mention-routing.md`. |
| [`b17c077`](https://github.com/block/buzz/commit/b17c0776b7438d59904e8c38926148bf692fa5f5) | `bd6eca9` | Will Pfleger | fix(buzz-acp): bound busy-owner hold to prevent cross-channel starvation (#7337) | Legacy ACP path maintenance only. Bounds a hold that could starve other channels. Retained until the central routing cutover removes `buzz-acp` (Remaining Work slice G). |
| [`e09f715`](https://github.com/block/buzz/commit/e09f715c9d0ee2cb7bf8a39061e601f3a502f588) | `866d6dc` | Jordan Mecom | Verify ACP relay events before prompt routing (#7010) | Relay and storage responses are verified (`buzz_core::verify_event`) before prompt routing, membership, and dedup in `crates/buzz-acp/src/relay.rs`. Legacy path hardening. |
| [`5d10783`](https://github.com/block/buzz/commit/5d107836c6bd6a57e2da28560ec0acc0d016b8d9) | `4ab1b58` | morgmart | Persist video playback speed preference (#7336) | Small shared desktop UI improvement (`desktop/src/shared/lib/videoPlaybackSpeedPreference.ts`). No product-model impact. |
| [`4afef86`](https://github.com/block/buzz/commit/4afef8649ab11d60b423893e9e4f8cd36868d35a) | `6594844` | Logan Johnson | fix(desktop): restore mention chip identity icons (#7338) | Follow-up to `ee883d7`; mention identity icons and clipboard normalization. Depends on `dce967e`. |
| [`f038cbb`](https://github.com/block/buzz/commit/f038cbbb0d4092a72ffd93f17916f84d2b39bb43) | `71d281c` | Taylor Ho | fix(sidebar): simplify unread indicators and emphasize priority activity (#7134) | Sidebar unread clarity plus native/live priority parity (`desktop/src-tauri/src/unread_catch_up.rs`). Shared Office shell behavior Ortak keeps. |

Local short SHAs expand with `git rev-parse <short>` in this repository. The upstream objects are present locally because the range was fetched from `buzz-reference`.

## Deferred

| Upstream | Change | Reason |
|---|---|---|
| [`01bacb8`](https://github.com/block/buzz/commit/01bacb8df3d2f5718e0a468828e07ae874a38eae) | chore(release): release Buzz Desktop version 0.5.22 (#7308) | Release metadata only (`CHANGELOG.md`, `desktop/package.json`, `tauri.conf.json`, candidate JSON). Ortak must not claim a Buzz release. |
| [`ce9decb`](https://github.com/block/buzz/commit/ce9decb235f628c484631ac923db96466460fc3f) | fix(acp): rename system tag to agent-instructions (#7332) | Self-contained prompt-tag rename in the legacy ACP path (`prompt_framing.rs`, `queue.rs`, `pool.rs`, transcript helpers). No accepted fix depends on it. Revisit only if the legacy path outlives slice G. |
| [`e7e2993`](https://github.com/block/buzz/commit/e7e29937a145aca7a3c7f5436b07e32c6c20087f) | feat(desktop): invite owned agents from standalone forums (#7125) | Scope expansion of the Buzz forum/owned-agent model. Not a dependency of `d595806`. Ortak Employees are not owned agents invited per forum. |
| [`4beffef`](https://github.com/block/buzz/commit/4beffef6979347f7ebdc760705865bdd04d23508) | feat(buzz-acp): update base prompt; add buzz context and skills to Pi agents (#7335) | Pi-specific launcher (`pi_launcher.rs`) and managed-agent preset/discovery changes. Hermes is the first Ortak runtime target; Pi integration is out of scope. |
| [`4d447b9`](https://github.com/block/buzz/commit/4d447b9c20a23fb33c94778e6cf309424abea6c8) | Add generic information-flow control core (#7293) | New `ifc-core` crate plus a design paper. Unwired in upstream and not security enforcement. Useful reference for later audience/provenance/egress checks (Remaining Work slice D and the slice G security review), not needed now. |

## What the eight imported commits did not touch

Verified with `git diff --name-only d4a8d4c..HEAD` over the eight imported commits only. The documentation commit that follows (this file, `REMAINING_WORK_V1.md`, and status notes in the plan and baseline) is separate and is not part of that range.

- No `crates/ortak-*` source changed.
- No `migrations/` file and no `schema/schema.sql` change. Ortak migrations 0045–0047 are as before.
- No `docs/ortak/` file changed by the imports.
- No `config/employees/` fixture changed.
- No dependency lockfile changed.
- The Office-ingress seam (`crates/buzz-relay/src/handlers/office_ingress.rs`, the `ORTAK_CENTRAL_ROUTING_ENABLED` flag, and the hook in `ingest.rs`) is unchanged.

Imported files that land in retained-Buzz areas and gain an Ortak owner by this import:

- `crates/buzz-relay/src/handlers/channel_authz.rs` (Office membership policy; owner: Office adapter, disposition "reuse then rename").
- `docs/mention-editor.md` (extended) and `docs/remote-mention-routing.md` (new) (Office composer contract; owner: Office desktop feature).
- `crates/buzz-acp/src/relay.rs` and `pool.rs` changes (legacy path; disposition unchanged: remove after cutover).

## Verification (scoped, as run by Codex)

Scope was limited to the areas the eight imports touch. Not run: full `just ci`, the full desktop unit suite, the full desktop Playwright suite, relay/DB integration lanes (`just test`), any Hermes or Honcho smoke, and any fresh install. Those remain open and are owned by the relevant Remaining Work slices.

Rust:

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml -- --check` | pass |
| `cargo clippy -q -p buzz-relay -p buzz-acp --lib --bins -- -D warnings` | pass |
| `cargo test -q -p buzz-relay --lib handlers::channel_authz::` | 6 passed |
| `cargo test -q -p buzz-acp --lib` | 910 passed; includes the `relay::tests` signature-validation tests because `relay` is a lib module |
| `cargo test -q -p ortak-domain -p ortak-router` | 41 passed (13 lib, 6 fixtures, 22 router) |
| `cargo test -q --manifest-path desktop/src-tauri/Cargo.toml unread_catch_up` | 3 passed, after the documented `just _ensure-sidecar-stubs` test setup |

A mistaken `--bin`-filtered `buzz-acp` test command ran zero tests and is not evidence; the `--lib` run above is.

Desktop:

| Check | Result |
|---|---|
| `pnpm --dir desktop typecheck` | pass |
| 16 targeted Node test files (mention identity, drafts, authorization, clipboard, video preference, sidebar helpers) | 268 passed |
| `pnpm --dir desktop build:e2e` (tsc + Vite mock-bridge build) | pass |
| Focused Chromium mock suite: mention-recipients, remote-owned-mentions, mention-clipboard, video-attachment, badge, thread-unread | 104 passed initially; one pre-existing hover-colour timing failure passed on a focused rerun (1 passed, 1.6s) with no source or test change |
| `pnpm --dir desktop exec biome check --changed --since=d4a8d4c` | exit 0, 99 files checked, one upstream optional-chain style warning at `useAgentAddressLockPicker.ts:162`, no errors |

The hover-colour failure is in a test and inactive hover code that the imports do not change; the theme applies asynchronously after the initial sample. This is recorded as one timing failure plus a passing focused rerun, not as a single all-green run.

Repository:

| Check | Result |
|---|---|
| `CHECK_FILE_SIZES_BASE=d4a8d4c just file-size-check` | pass; 10 policy tests passed |
| `./scripts/test-ci-required-context-isolation.sh` | pass |
| `git diff d4a8d4c..HEAD` review | no changes to Ortak crates, `config/employees`, migrations/schema, or dependency lockfiles |

Environment notes: the first Rust run exhausted disk; only reproducible build caches from the completed `m4-activity-queries` and `m2-office-delivery` worktrees (about 12.6 GiB) were cleared, and the retry passed. The pinned Playwright browser (v1223) was missing and was installed before the UI suite ran. Test-only sidecar placeholders were needed for the native checks; this was not a packaged application build.

Relay/DB integration lanes (`just test` with Postgres and Redis) still apply because `buzz-relay` changed. The Ortak Postgres suites need `ORTAK_TEST_DATABASE_URL` as documented in `crates/buzz-relay/tests/postgres_office_ingress.rs`.

## Merge

Import branch: `ortak/buzz-import-2026-09-05`. Intended destination: `ortak/main`. The merge follows this documentation commit.

## Discrepancies noticed during the audit

- `scripts/run-tests.sh` and `Justfile` gained upstream lanes for `channel_authz` and ACP relay verification. They reference tests that exist in this tree, but `.github/workflows/_ci-relay.yml` was also edited by `cd02b69`. Ortak has not decided which inherited GitHub workflows remain live; the workflow edit is harmless but is Buzz CI, not Ortak CI.
- `Cargo.toml` still sets `repository = "https://github.com/block/buzz"` for retained `buzz-*` packages by design (see the workspace comment). Imports did not change this.
- `README.md` lists only `ortak-domain` and `ortak-router` as delivered crates. Five more `ortak-*` crates exist (`control`, `office`, `runtime`, `observability`, `work`). This predates the import and is tracked in `REMAINING_WORK_V1.md`.
