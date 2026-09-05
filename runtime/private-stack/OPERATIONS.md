# Private stack operator handoff

This dated local stack uses `/private/tmp/ortak-private-20260905` and fresh
resources. Its current recipes demonstrate parts of a private development
installation. They do not satisfy the full install, upgrade, backup/restore,
legacy removal or deployed workflow gates of
[`REMAINING_WORK_V1.md` slice G](../../docs/ortak/REMAINING_WORK_V1.md).

## Read-only status

From the repository root:

```sh
python3 scripts/ortak/private_status.py --state-dir /private/tmp/ortak-private-20260905
```

The command verifies the exact private state marker and emits one bounded JSON
observation. Add `--no-network` for local files only. Optional
`--expected-owner <64-lowercase-hex-public-key>` and
`--expected-company <canonical-public-UUID>` compare the selected **public**
fixture identities against the local configuration; neither option accepts a
secret. Without these options, identity comparisons say `not_supplied`.

The command reads only the marker and selected public configuration/receipt
files: `api-config.json`, `memory/worker-memory.json`,
`memory/bootstrap.json`, `object-store/image.env`, and optional
`worker-config.json`. It checks file ownership, private permissions, bounded
size and symlink restrictions. It never opens the secret-bearing
`identities.json`, `runtime.env`, object-store credentials, provider profiles,
Docker socket, bridge token files, or any unrelated state directory.

HTTP probes are finite unauthenticated GETs to the literal loopback host. There
is a ten-second request budget, a two-second maximum socket wait, and a
twelve-second whole-command alarm. No proxy, redirect, authorization header or
response-body read occurs; response headers are bounded by `http.client`.
Statuses and fixed observation labels are emitted, never response bodies,
headers, credentials or environment values. A successful command exit means
observations were collected, not that every service or the MVP is ready.

| Surface | Selected origin/path | Meaning of a successful observation |
| --- | --- | --- |
| Relay | `127.0.0.1:8089/_liveness`, `/_readiness` | HTTP health returned 200; not a signed Office workflow proof |
| MinIO | `127.0.0.1:9008/minio/health/live`, `/ready` | HTTP health returned 200; not authenticated bucket conformance |
| Product API | `127.0.0.1:8787/api/v1/employees` | 401/403 requires authentication; no audience or DB data was read |
| Honcho | `127.0.0.1:8009/v3/ortak/protocol` | 401/403 requires authentication; not a memory witness or provider check |
| Hermes bridge | Optional `127.0.0.1:8650/v1/capabilities` | Auth fence only, after explicit local origin publication |

Port 8650 is the controller recipe's default, not an automatically selected
running service. Hermes says `not_configured` and is **not probed** unless the
protected `worker-config.json` selects company slug `ortak-private-20260905`
and exact bridge origin `http://127.0.0.1:8650`. This status command does not
create that file or enable an executor. Other origins/configuration errors are
reported and skipped. Matching documentation defaults do not publish a private
worker configuration or select a running service.

The optional `semantic` selection is opaque to this command. A valid or
malformed selection does not change the selected Hermes probe, trigger any
credential lookup or add a provider request. Semantic scoring is explicitly
`not_checked`; only the worker validates and uses that selection.

Local artifact presence is checked for the task-owned relay, API and worker
binaries in `/private/tmp/ortak-root-build-target/debug` and the separately
identified `Ortak Private.app` bundle. Presence is not a build provenance,
running-process, app identity or UI acceptance claim. Local MinIO image
selection is reported separately from the uninspected running image.

The memory journal is checked against the bootstrap's canonical intent,
receipt, native IDs and write provenance. Its roundtrip is explicitly
`historically_verified`; current native ownership and execution witnesses still
need the production adapter. Employee status, activation, routing, worker
execution, provider response, Office delivery, upgrade and backup/restore are
always `not_checked` here. Those require separate authenticated or integration
gates. A healthy listener cannot substitute for them.

## Existing install and start chain

The current reproducible pieces are linked rather than silently combined into
an installer that enables work:

1. [`README.md`](README.md): fresh marked PostgreSQL/Redis state and Compose
   startup; selected credentials remain outside Git. Native service `prepare`
   creates the fresh identity bundle once; `migrate` applies the normal schema.
2. [`MINIO_BUILD.md`](MINIO_BUILD.md): verified source build, immutable image
   selection publication, separate credentials, startup and authenticated
   bucket initialization. The recorded local image is
   `sha256:e1d7f7262c86498b45f869bcc7e3bbe7c11b3c026d9aad25f7759b053fd60a41`.
3. The native launcher starts relay/API in the foreground using a reconstructed
   private environment. Office creation and `bootstrap_private_control.py`
   establish the selected company, owner audience and draft Ada separately.
4. [`MEMORY_BOOTSTRAP.md`](MEMORY_BOOTSTRAP.md) and the
   [Honcho extension recipe](../honcho-adapter/README.md) cover explicit owned
   memory creation and read-only receipt recovery. Keep the exact final tested
   extension artifact/manifest with its central validation receipt.
5. [`CONTROLLER.md`](../hermes-bridge/CONTROLLER.md) covers the separately
   contained Hermes worker/controller. The reviewed containment identities are
   worker `sha256:623fae9e3b38c75bc3cb94f73bc3d1c303bc3ed6a77765eb51fc17b54cc90b18`
   and controller
   `sha256:ef9a9d2a7446d9e13cdbf94cf1a2152011b5a72050e450d500356f059852d7b1`.
   These fixture receipts do not mean this private stack has published an
   executor configuration or has a selected model credential.
6. `desktop/scripts/ortak-private-native.mjs` builds the distinct private app;
   the explicit native launch recipe uses its fresh test owner. It preserves
   existing desktop/keyring identity. Actual UI and reply workflow acceptance
   remain separate from successful native packaging.

The source recipes still rely on an integration owner to select immutable
artifacts, private mount ownership, exact Honcho service composition and the
explicit worker/controller configuration. There is no installed service
manager, process registry or single idempotent start/restart orchestrator yet.
Start commands must retain their private foreground terminal/process ownership
so they can be stopped deliberately. Do not infer a process identity from its
port or reuse an old binary because its filename matches.

## Stop and retained state

The native launcher replaces itself with a single foreground process. Stop that
owned process from its terminal, rather than using broad name-based kills.
Quiesce any separately enabled routing/worker first and confirm durable
cancellation plus contained child termination before stopping its controller.
The current setup has no unified command that proves these steps, so they must
not be represented as an automated safe shutdown.

After the selected writers are stopped, the **complete store overlay** is:

```sh
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin /usr/local/bin/docker \
  --host unix:///Users/nambse/.docker/run/docker.sock \
  compose --project-name ortak-private-20260905 \
  --env-file /private/tmp/ortak-private-20260905/compose.env \
  --env-file /private/tmp/ortak-private-20260905/object-store/image.env \
  -f runtime/private-stack/compose.yaml \
  -f runtime/private-stack/object-store.compose.yaml stop
```

This stops only the dated PostgreSQL/Redis/MinIO project and retains its volumes.
The reconstructed environment, exact local daemon socket and explicit project
name prevent ambient Docker/Compose settings from selecting another target.
It does not stop separately launched Honcho, bridge, API, relay or desktop
processes. The base Compose file alone does not cover the MinIO overlay. Do not
use `down -v`, global pruning, unrelated-container stops or old-resource
deletion as routine cleanup. Published private state and original memory/runtime
idempotency receipts must survive restart.

## Remaining slice G operational gates

| Gap | Concrete next implementation and proof |
| --- | --- |
| Reproducible install | One private artifact inventory must join source manifests, inspected image IDs, native build identity, schema versions, mount paths/UIDs and service selections; replay the recipes in a new isolated state and prove no old resource access. |
| Start/stop/restart | Add a registry of owned native/container processes and exact selected configuration, then exercise service restart and coordinated dispatch quiescence/cancellation. An absent bridge configuration must remain disabled. |
| Authenticated status | Read company/employee/run authority and current adapter witnesses through the selected signed API/production adapters. Keep this separate from the no-credential health command. |
| Backup | The [main PostgreSQL helper](DATABASE_BACKUP.md) has a verified database-only dump/restore receipt; full-stack backup still needs the following. Quiesce writers and settle contained runs before one recorded backup barrier; include control PostgreSQL/outboxes, native+extension Honcho PostgreSQL receipts, Redis replay/AOF state, MinIO data, durable bridge SQLite state/tombstones and private configuration/credential references. Secret material requires an encrypted private backup path, never Git or ordinary reports. |
| Restore | Restore into a **new isolated** stack, verify database/schema and immutable receipt/native-ID agreement, replay cursors and cancellation tombstones, verify object-store consistency, then revalidate identities and all activation gates. Do not blindly rerun previously admitted work. The database-only receipt below does not demonstrate this full-stack recovery gate. |
| Upgrade | Record the migration/source/artifact transition from the pinned baseline, exercise upgrade plus rollback/forward recovery against restored disposable state, and preserve retry journals. No current receipt demonstrates this yet. |

The [database backup helper](DATABASE_BACKUP.md) is implemented and verified for
the main PostgreSQL database only. The cross-store backup/restore rows remain
required work. A filesystem copy while SQLite/WAL, PostgreSQL, MinIO or worker
writers remain active is not a validated consistent backup. Legacy surface removal and the
single deployed full company workflow remain additional slice G prerequisites.

## Status validation receipt — 2026-09-05

Thirteen status tests and eight private control bootstrap tests passed in
0.032 seconds using disposable files and mocked database subprocesses. Status
tests exercise fixed endpoint
selection, absent/invalid Hermes configuration, no authentication/body reads,
redirect refusal, deadlines, private file guards, tampered bootstrap receipts,
public owner mismatch, malformed numeric UUIDs without losing the JSON report,
opaque valid/malformed semantic selections without credential reads or added
HTTP requests, a strictly boolean optional project-creation configuration flag,
and separation of local evidence from authority. Bootstrap tests exercise the
explicit capability upgrade, preservation of previously enabled capabilities,
protected backup and pending snapshots, interrupted atomic publication, and
refusal of malformed or changed audiences before database access. These tests
do not enable the capability on the selected private stack.

At 08:30 Istanbul the actual selected-state status invocation returned relay
and MinIO live/ready 200, product API and Honcho authentication-required 401,
and Hermes `not_configured` without probing it. API public config, local memory
bootstrap receipts and the immutable MinIO selection checked successfully;
the four task-owned artifacts were present. No expected public owner/company
arguments were supplied. The command loaded no credentials, used no DB/Docker
API or provider call, and changed no state. These observations expire as the
services/configuration change; run the command for a new observation.

## Database-only backup validation receipt — 2026-09-05

The actual 08:48:01 Istanbul invocation created a 502,264-byte custom archive
and restored it into a fresh, retained verification database in the inspected
private PostgreSQL container. All 100 public table counts, successful migration
checksums through 0052 and the selected schema catalog matched the exported
snapshot. The schema SHA256 is
`07139b29b4777e67f4f6188d514c82dbb8238306d9a190bd39cc57ba5a17e011`.
[The detailed receipt](DATABASE_BACKUP.md#actual-database-verification-receipt--2026-09-05)
records the private artifact location, archive checksum and verification database.

Eight unit cases plus an actual SQL column-order regression passed: **nine tests
in 0.602 seconds**. The earlier failed comparison artifacts and verification
database were retained; the original main database was never restored over or
removed. This is a main-database recovery observation only. The read-only status
command still reports backup/restore as `not_checked`, and the cross-store,
independent-host and activation gates above remain open.
