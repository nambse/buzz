# Continuation operations audit — 2026-09-05

Observation window: 14:00–14:13 UTC (17:00–17:13 Istanbul). This is a
read-only checkpoint from the new `14b1` checkout. It does not establish v0
acceptance, authenticated native use, model health, activation or recovery.
No service was started/stopped, no private state or old employee resource was
changed, no cleanup/build/import/deployment was performed, and no credential
value or OAuth file content was read or copied by this audit.

## Checkout and continuation ownership

- HEAD: `afbb891732a584404e9c21ffc7b5028da2c389d5`, branch
  `codex/ortak-v0-delivery`; `b9b00a379ee9ae6a967cf40241820048f37adbd9` is an
  ancestor. Git ran after Hermit activation. Concurrent Office/memory/root
  integration edits are expected; this audit owns only this document.
- Saved `ortak-morning-delivery` is a **PAUSED heartbeat**, already targeting
  `01a071d1-f830-7e82-80cf-9cb0fc9687d0`. The record was inspected without
  changing its status, prompt or schedule. No second automation was created.
- After the user's disk cleanup, `df -h /private/tmp` reports **61 GiB free**
  (372 GiB used). Earlier sub-GiB observations are superseded. Further cleanup
  is unnecessary for this checkpoint.

## Private state and health

`python3 scripts/ortak/private_status.py --state-dir
/private/tmp/ortak-private-20260905` completed at
`2026-09-05T14:00:48.169754Z`. Its exact marker validation passed. The root,
`memory` and `object-store` directories are nonsymlink directories owned by
UID 501 with mode 0700. Marker, API config, runtime environment file,
identities file, memory bootstrap/config, object-store image selection and
credentials files are nonsymlink regular files owned by UID 501 with mode
0600. Secret-bearing files received metadata-only `lstat` checks.

The public API audience config and original memory bootstrap receipts passed
the helper's local checks. No independent expected company/owner arguments were
supplied. Current membership, native memory ownership and an execution
roundtrip witness remain unverified here. The receipt's historical roundtrip
is not a newly established activation witness.

| Surface | Current observation | Limit |
| --- | --- | --- |
| Relay 3038, health 8089, metrics 9198 | UID 501 process owns loopback listeners; both health paths return 200 | No signed Office transaction was exercised |
| API 8787 | Loopback listener; employees endpoint returns 401 | Authentication fence only |
| PostgreSQL 55433 | Private Compose container running and healthy, loopback publish | No SQL reset, migration or data mutation |
| Redis 56382 | Private Compose container running and healthy, loopback publish | No queue/replay authority probe |
| MinIO 9008 | Both health paths return 200; selected image equals running image | No fresh authenticated bucket/write check |
| Honcho 8009 | API container running, endpoint returns 401 | No new current resource/roundtrip proof |
| Hermes 8650 | No listener; protected `worker-config.json` absent | No private executor is configured |

Disposable test PostgreSQL 55432 remains the distinct
`ortak-m1-schema-check` container. It is not the private 55433 service.

## Exact native processes and artifacts

`ps` supplied UID/start time/executable, while `lsof` independently matched each
loaded executable and its working directory to private state. PID numbers are
observations, never future stop authority. Whole process arguments/environments
were not printed.

| Process | PID / start time (Istanbul) | Exact loaded executable |
| --- | --- | --- |
| Relay | 17426 / 11:34:31 | `/private/tmp/ortak-root-build-target/debug/buzz-relay` |
| API | 17461 / 11:34:32 | `/private/tmp/ortak-root-build-target/debug/ortak-server` |
| Desktop | 18023 / 11:36:07 | `/Users/nambse/.codex/worktrees/a5ed/ortak.dev/desktop/src-tauri/target/ortak-private-native/debug/bundle/macos/Ortak Private.app/Contents/MacOS/buzz-desktop` |

All three are UID 501, parent PID 1, cwd
`/private/tmp/ortak-private-20260905`. No `ortak-worker` process was found.
The desktop owns a loopback listener on 52332, but no established connection to
3038 was observed. Its presence is not native UI, authenticated WebSocket or
automatic reconnect evidence.

Current on-disk hashes match the retained 08:43:35 UTC
`logs/artifact-observation-1145.json` for relay, API and native:

| Artifact | Bytes | SHA256 |
| --- | ---: | --- |
| Relay | 146672952 | `07248188450b135d58c464d82b05d4a0ced9afbaac5f755606c87f370fe3b17a` |
| API | 28624064 | `8ee9f28468edc70c17ebc5bf5176eac4e2d8a9a1bfe216fd69491e287222422e` |
| Native | 184533408 | `ad5c4ca8a2b30cd9f978f64bccb3361b61ac02b830bc89e1aeaaa7d1096e520e` |
| Worker (present, not running) | 31410544 | `ed2fe2961739afb3b2397e3dd90266afa995feaba087ec1dfb4852efe77a3776` |

The previous integrator attributed relay/API to `5c285d2` and native to
`d07f55c`. Hash equality preserves that observation; it does not turn it into a
reproducible build attestation or attribute current source changes to these
binaries.

### New-checkout path implications

`private_status.py` deliberately checks the bundle relative to its own
checkout, so its `private_desktop_bundle: missing` is correct in `14b1` even
while the old `a5ed` bundle is loaded. The desktop launch helper also requires
the exact checkout-relative bundle and verifies bundle ID
`dev.ortak.private20260905`; the old absolute launch path cannot be substituted
into the new helper. A newly built bundle is required before claiming the new
checkout's native artifact is installed.

Retained task build inputs still exist:

- `/private/tmp/ortak-root-build-target`: 17 GiB, contains running backend
  artifacts. Preserve while replacing them deliberately.
- Old `a5ed/desktop/src-tauri/target`: 12 GiB, contains the loaded private app.
- Cargo cache
  `/Users/nambse/dev/ortak.dev-worktrees/buzz-import-2026-09-05/.hermit/rust`:
  1.2 GiB; registry and git caches remain.
- Pinned Rust 1.95.0 binaries remain in
  `/Users/nambse/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin`.
- `/private/tmp/ortak-hermes-worker-oci`: 463 MiB, retained worker export.
- The new checkout's `.hermit` currently contains Node, not its own Rust cache.

No path above is a cleanup recommendation. The root integrator must choose a
new serialized build target explicitly and preserve loaded rollback artifacts.

## Container identities

Inspection used the explicit local Docker socket, a reconstructed environment,
and only selected `ortak` containers. Environment values, full configuration
and command lines were not emitted.

| Container | Running immutable image |
| --- | --- |
| `ortak-private-20260905-postgres-1` | `sha256:ef257d85f76e48da1c64832459b59fcaba1a4dac97bf5d7450c77753542eee94` |
| `ortak-private-20260905-redis-1` | `sha256:ff02b58f971e7d7d156a1267e283fcbbeee91773b6aa36c49dac28ecfe28eadf` |
| `ortak-private-20260905-minio-1` | `sha256:e1d7f7262c86498b45f869bcc7e3bbe7c11b3c026d9aad25f7759b053fd60a41` |
| `ortak-honcho-api-check-20260905` | `sha256:cc8b4a29c0adda08978886e205ff5c5ff0a13923e4ed15e1626b24194d0c0c21` |
| `ortak-honcho-test-db-20260905` | `sha256:cf134a767f474095eeba57e0117be8e568e011a63f33fbf252f14c9b760f8e6f` |

The Honcho database has no published host port. Honcho API/database are separate
from the dated Compose project; their names containing `check` or `test` are
not permission to reset them. They currently serve retained private memory.

Hermes worker `sha256:623fae9e3b38c75bc3cb94f73bc3d1c303bc3ed6a77765eb51fc17b54cc90b18`
and controller `sha256:ef9a9d2a7446d9e13cdbf94cf1a2152011b5a72050e450d500356f059852d7b1`
remain in the image store. Both have the explicit
`org.ortak.hermes.revision=29112bef099274229cadff79cdff7bf7b99c4b77` label;
the controller's worker label equals the selected worker digest. The generic
`org.opencontainers.image.revision` is an inherited base-image label and must
not be mistaken for Hermes source identity. These images are available tested
artifacts, not an enabled private worker deployment.

## Upstream integration checkpoint

Official GitHub commit, release, annotated-tag and bounded compare endpoints
were read. Public security-advisory endpoints returned empty arrays for all
three repositories; this is only the published repository-advisory observation,
not a comprehensive vulnerability scan.

| Dependency | Observed upstream | Reviewed/selected boundary | Current artifact boundary |
| --- | --- | --- | --- |
| Buzz | `f038cbbb0d4092a72ffd93f17916f84d2b39bb43`, unchanged; latest release `desktop-v0.5.22` | Existing selective import record remains authoritative | Ortak backend/native builds above; no new upstream import |
| Hermes | `006b1beb00d9d25230571d14277aca3d70e5e11f` at 13:36:57 UTC; compare reports 70 commits ahead of prior `5ac75e91` | Release `v2026.8.31` still dereferences to pinned `29112bef`; only the bounded patches below reviewed | Worker/controller digest pair above, no running private executor |
| Honcho | `be54355545b64ddb10203829d323861f52423685`, unchanged, 14 ahead of selected source | Annotated `v3.1.1` still dereferences to `5d992bc65afcfbc05a5911ab4edbaa88ef64c690`; latest-release API returns 404 | Running extension `cc8b4a29…`; no rebuild |

Sources: [Buzz head](https://github.com/block/buzz/commit/f038cbbb0d4092a72ffd93f17916f84d2b39bb43),
[Buzz release](https://github.com/block/buzz/releases/tag/desktop-v0.5.22),
[Hermes bounded delta](https://github.com/NousResearch/hermes-agent/compare/5ac75e91e2012497db474835a58e0139e89047cd...006b1beb00d9d25230571d14277aca3d70e5e11f),
[Hermes release](https://github.com/NousResearch/hermes-agent/releases/tag/v2026.8.31),
[Honcho delta](https://github.com/plastic-labs/honcho/compare/5d992bc65afcfbc05a5911ab4edbaa88ef64c690...be54355545b64ddb10203829d323861f52423685).

The Hermes review read patches for `gateway/platforms/base.py` (deny delivery of
session/kanban SQLite stores and sidecars), `hermes_cli/env_loader.py` (process
profile boundary), `tools/approval.py` (deny rules before isolated-container
fast path), `tools/mcp_oauth.py` (avoid logging invalid token values),
`agent/credential_pool.py` (durable intentional cooldown reset), and
`tools/file_tools.py` (shared container-backend classification). Decision:
**defer import**, retain tested pins. The current Ortak worker disables all
tools, does not launch the gateway/MCP, and has one selected isolated profile;
these changes are relevant review inputs before those surfaces expand. This
is not a full review of all 70 commits or a claim that source pinning solves
every security issue. Honcho's 14-commit implementation delta remains deferred
until scoped memory, atomicity, provenance and migration gates pass.

## OAuth integration findings before implementation

The source inspection, before further work in this task, found that the Ortak
profile validator, worker and constructor accept only `openai`/`openrouter`
with `provider-token`. Merely selecting Codex OAuth cannot activate this
artifact. Existing `inspect` validates local files, not a real provider call.

The extracted pinned source is
`/private/tmp/ortak-hermes-source-29112bef/hermes-agent-29112bef099274229cadff79cdff7bf7b99c4b77`.
All 17 source-lock hashes matched. Its `agent_init.py` accepts explicitly
resolved credentials plus the fixed Codex base URL, selects `codex_responses`
for `openai-codex`, and adds Hermes originator/account headers through
`agent/codex_headers.py`. This can use the existing contained AIAgent without
launching a separate Codex app-server process.

Its reasoning vocabulary recognizes `gpt-5.6` specially and otherwise uses
the legacy Codex effort set; `ultra` clamps down. The current Ortak constructor
does not pass reasoning configuration. A new integration must preserve the
user's actual requested/effective model and effort, refuse unsupported choices
explicitly or supply a narrowly reviewed compatibility change, and test the
real constructor/transport. Changing a model string alone is insufficient.

OAuth enrollment must be a fresh explicitly selected private login. The normal
Hermes resolver can fall back to/import an ambient Codex CLI credential store;
the Ortak integration must prevent that path. Refresh tokens are single-use,
so renewal needs one durable owner and bounded cross-process locking before
atomic persistence. No existing host or old employee OAuth state was accessed.
Additional relevant source files must join the fixed source lock, and changed
worker/controller artifacts require new constructor, loop, containment and
selected-provider receipts before activation.
