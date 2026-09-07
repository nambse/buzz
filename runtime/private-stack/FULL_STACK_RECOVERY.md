# Selected private full-stack recovery

## Current result — schema78, 2026-09-06

Full capture `c9705a580a8149668143f31079847123` and isolated offline restore
`bbd0fc8063d34abe81aa06730d5c6600` completed. Both databases, retained volumes,
protected bridge journal, native ciphertext store and workspace entries were
verified. Original services were then resumed without activating restored state.
See the [current continuation receipt](../../docs/ortak/CONTINUATION_PROGRESS_2026-09-05.md)
for exact manifests, owner generations and the restore-only operator selection.
This proves same-host physical recovery, not independent-host disaster recovery.

The owner subsequently replaced the shared ChatGPT enrollment. This captured
bundle therefore contains the previous account's encrypted snapshot and remains
historical. A new capture must freeze the current source and credential generation;
never restore the old connection over the new enrollment implicitly.

The sections below preserve earlier failures and operator interfaces. Their
PIDs, registries, image selections and pending statements are historical.

## Historical G4 preflight boundary

G3's databases completed, then its MinIO reader refused `user.total_deletes`.
Sources were resumed and verified; no successful full live capture exists yet.
The exact two8-byte counter contract, actual installed-image fixtures, capture
tail and current owner evidence are recorded in the
[G3 recovery checkpoint](../../docs/ortak/G3_VOLUME_READER_RECOVERY_2026-09-06.md).
All old pause commands and owners below are historical until a newly prepared
G4 registry/hash is selected. G4 remains schema69/native6ff. Unknown xattrs and
non-eight-byte counters refuse; reader failures retain fixed code/phase only.

## Historical checkpoint — G3 prepared; source resumed and full restore gate open

G1 and G2 pauses both passed on real selected services. G1 capture
`d6b4737afd1145ff8f2917584230c883` failed at the exact Honcho CHECK catalog
round-trip. A fresh isolated 19-table target-only repair fixture then passed
(`/private/tmp/ortak-v0-evidence/g-honcho-check-roundtrip-f78e6335c8394df4b6e2092b1872ed2f/receipt.json`).

G2 preparation `dfeb71816ab1438cbe74dfbb8eb47a32`, owner operation
`e3535f007c194ebe992a2c610884b73a` and pause
`0ce86c156a5c41858ef405a2383a386f` passed. Capture
`dbd527bf32db44b78e45b2de5a4074b1` failed with `FileExistsError` because the
`main-settings` source/restored verification reused `recovery-obligations.sql`.
Its main/Honcho archives `f9b307814edb40c395ad5b18433cd808` and
`cbf9a241fc884c3aafe6dabcb1466913` verified 127/19 tables, including the actual
Honcho repair. **Neither full bundle is sealed; neither failed attempt authorizes
an offline restore.** No complete captured-bundle offline restore has run.

Source resume is verified on the same containers/images, schema69/backend69 and
native6ff: relay3241/session52012, API3252/92895, management3263/42627,
worker3277/67436 and native3290/22975. These are dated observations, not signal
authority. The [scope document](../../docs/ortak/PRIVATE_FULL_STACK_RECOVERY_PLAN_2026-09-05.md)
links the read-only receipt: health200/200/401 and unchanged hashes/counts for
16 scoped tables, completed Work, active Ada and retained erased-fact receipts.

Separate source/restored main and Honcho log names now pass the production
exclusive-create subprocess fixture, with both label regressions falsified.
Focused Python tests passed 128 with one explicit skip. Seal failures preserve
the original manifest plus `failure.json`; restore rejects this marker even
when the captured hash matches. Receipt:
`/private/tmp/ortak-v0-evidence/g-capture-exclusive-regression-aa472464328f4a5abd92e0bd614bb285/receipt.json`.

G3 read-only preparation `5e4af2cc2c7543c38a467231559e9ac8` and owner operation
`15cc30dc9d3147979876a83b4056acb4` passed, freezing 20 operators with five updates.
Owners SHA256 is `6f4ca742ad69acc15f7dd060597d123aed8fc2783fab0aaadd184d46c358a695`.
Its `ROOT_CAPTURE_RECIPE.md` contains the selected commands. Generic pause helper
`/private/tmp/ortak-pause-for-g69-checks-v3.py` remains SHA256
`7661a94bcbccfdd0e640c77694ba48541cc95b449fcd2ce6bfd2fac781fafa13`.
Root has started the G3 pause/capture attempt; its outcome is pending. This is
no live full-capture success.
The G2 registry cannot run again after these owner/code changes. Generic v3 pause
requires both explicit registry path and exact registry digest. After a new
reviewed preparation/registration and frozen helper selection, its contract is:

```text
python3 <new-frozen-pause-helper> \
  --owners <new-operation>/owners.json \
  --owners-sha256 <new-registry-sha256> \
  --execute-root-pause --host-oauth-enrollment-fenced
```

This is a prospective interface, not a runnable command for either old registry.
Preserve the failed attempts; do not overwrite their files or reuse their PIDs,
operator closure, pause receipt or capture command. Root owns all live actions.

## Historical implementation and preparation evidence

The helpers implement a frozen preparation, process/session registry, held
quiescence checks and a complete capture state machine for
`/private/tmp/ortak-private-20260905`. **At this earlier checkpoint no live full-stack capture or offline
full-stack restore had run.** A storage-only offline fixture rehearsal has passed;
see [its runbook and retained evidence](OFFLINE_RECOVERY.md). Root retains all live
pause/resume authority. These fixtures establish narrower evidence.

Read the [scope and design](../../docs/ortak/PRIVATE_FULL_STACK_RECOVERY_PLAN_2026-09-05.md)
and [database backup runbook](DATABASE_BACKUP.md). Old Cem/Zeynep, the old native
application a5ed, unrelated profiles and existing verification databases remain
outside mutation authority. Ports, process names and old PIDs cannot prove
ownership. Never reuse an old approval for a new schema/image/process generation.

## Historical preparation and registration; next-operation procedure

The pre-G1 source selection bound the actual schema69 root rollout, Honcho D2a
`ad579…/febea…`, Hermes `9335…/dbc9…` with worker `8ee…`, four then-current writer
receipts, and native72102/session16306 with compiled-isolation6ff. The exact
receipts and hashes are in the scope document linked above. All services resumed;
the schema69 rollout's old `paused-drain.json` is historical. The authorized
fresh preparation `e81d55418f4045dbbc2a33b50d5213df` and owner registry
`ea8c88f50f2e40e2beb5a016dd386a08` now passed. The operation's private
`ROOT_CAPTURE_RECIPE.md` retains that historical selection's commands. At that
checkpoint no pause receipt or live capture had been created; G1/G2 supersede
that statement and those commands.

After updating the selection to new verified owner receipts and freezing the
reviewed operator code, the next-operation preparation interface is:

```sh
python3 scripts/ortak/prepare_private_recovery.py \
  --state-dir /private/tmp/ortak-private-20260905

python3 scripts/ortak/register_private_recovery.py \
  --preparation /private/tmp/ortak-private-20260905/recovery-preparations/<returned-UUID>/preparation.json
```

Preparation inspects exact resource identities, public configuration and
secret-file metadata. The selected Honcho setting is parsed internally into
public database/role/host fields; no complete environment or credential is
printed. Registration freezes mode0500 public launcher/helper code and changes
only the exact helper-directory literal. All four selected launchers have actual source-hash receipts. Their frozen
rebased resume recipes are prospective; unknown historical hashes are never attested.

The required native writer set is now exactly `buzz-relay`, `ortak-server`,
`ortak-worker` and `ortak-management`. All four need current PID/start/UID/cwd,
loaded binary inode/hash, selected artifact receipt and session evidence.
Consolidated seven-binary backend receipts bind both worker and management.
Older three-writer registries refuse capture. Management additionally requires
root's exact launcher, flat PID/session receipt and helper-import directory;
`MANAGEMENT_SELECTION=None` intentionally refuses registration until that new
reviewed selection exists. The management launcher's public code is frozen and
only its exact helper-directory literal is rebased; no secret value is copied.

To revalidate, pass `--verify-preparation <exact-path>` to preparation. A new
retained result is produced; the old plan cannot gain additional authority.
Live row counts may change during preparation. Changed ownership, schema,
images, mounts, public configuration or secret generation must refuse. Update
the source's exact selection from root's final rollout receipts first. New
runtime/backend generations require new preparation and registration.

The recovery source now understands the explicit schema68 probe journal and all
five schema69 reviewed-memory export tables. The selected live generation is69. A69 preparation also requires all four Honcho
D2a reviewed tables; an older extension cannot qualify. The operator closure is
now19 files, including `private_recovery_obligations.py`; old18-file registries
require a fresh preparation/registration. Three exact controller profiles share
one selected original OAuth store/ref. All nine public marker/binding/provider
files, both new profile-directory mounts and the selected config are frozen;
missing/duplicate/unknown model variants refuse. No other auth directory is
adopted. Full schema and row hashes remain
authoritative, with at most1024 scoped probe/export primary keys and complete-row
hashes retained in the capture witness.

Capture rejects every uncontained probe, pending publish, leased/attempted or
uncertain export, due withdrawal, failed job or mismatched immutable ACK. A
future pending withdrawal is allowed only with no attempts/error/lease and its
publication already acknowledged against the exact retained target. Do not
withdraw live facts merely for a backup. The future job and all receipt bytes
remain recovery obligations; they are not completed by capture.

## Historical v2 pause invocation and retained capture contract

The reviewed root-only [`pause_private_recovery.py`](../../scripts/ortak/pause_private_recovery.py)
was selected for the exact historical operation `ea8c88f50f2e40e2beb5a016dd386a08`.
Its separate mode0500 private copy is `/private/tmp/ortak-pause-for-g69-v2.py`;
`pause-helper-v2-selection.json` beside `owners.json` binds source/copy hashes.
It was source-tested and its19 frozen imports were validated read-only. It has
**not** been run against live services at that pre-G1 checkpoint. Later G1/G2
pauses passed; this v2 selection is not current pause authority. Root's signed native Stop and exact
Honcho withdrawal ACK are separately recorded under
`provisioning/native-reviewed-memory69/{published,withdrawn}.json`; those receipts
do not replace the helper's fresh production drain checks.

Historical v2 command, retained for evidence only; do not run it for current owners:

```text
/Users/nambse/.pyenv/versions/3.12.8/bin/python3 /private/tmp/ortak-pause-for-g69-v2.py \
  --execute-root-pause --host-oauth-enrollment-fenced
```

The helper revalidates actual loaded inode/hash/start/UID/cwd before each one-PID
SIGTERM. It checks schema69 run/cancellation/output/management and probe/export
drain before stopping native, after native, after API/relay, and after management/
worker. It then stops only the selected controller, Honcho API, Redis and MinIO.
Both PostgreSQL services remain running. Docker receives manual stop with
`--signal SIGTERM --timeout -1`; the local CLI wait is45 seconds and the daemon
does not schedule a force kill. This matches [Docker's documented stop timeout](https://docs.docker.com/reference/cli/docker/container/stop/).
Native waits are30 seconds; a process-wide SIGALRM enforces the entire900-second
budget, including nested frozen SQL helpers. The earlier private helper remains
retained and is superseded by this v2 copy. The102 recovery-related Python tests pass.
Unclean exit, changed identity, pending work, or timeout refuses and preserves
each intent/result plus exact frozen resume commands. An unacknowledged Docker
stop may still be pending in the daemon and must be reconciled before resume.
No source service is resumed automatically and no source SIGKILL is issued.

Only after fresh stopped-owner/client/cold-store checks and while actual Linux
executor/OAuth and PostgreSQL schema locks hold, the helper publishes the new
mode0600 `pause.json` beside `owners.json` with exactly these fields:

```json
{
  "format": "ortak-private-recovery-pause/1",
  "owners_sha256": "<exact registry_sha256 from owners.json>",
  "host_oauth_enrollment_fenced": true,
  "root_coordinated_pause": true,
  "resume_under_root_control": true
}
```

The pause helper's locks are released before returning. Root proceeds only on
`paused_verified`; a failed attempt may retain a physically true pause receipt,
but is not authorization to capture or ignore an unacknowledged lease. Fresh
capture identity/drain checks and locks remain mandatory. Use the exact frozen
operator copy, whose path is itself checked. The following is the old operation's
historical command, not a command for the resumed source:

```text
/Users/nambse/.pyenv/versions/3.12.8/bin/python3 /private/tmp/ortak-private-20260905/recovery-operations/ea8c88f50f2e40e2beb5a016dd386a08/operator-code/capture_private_recovery.py \
  --owners /private/tmp/ortak-private-20260905/recovery-operations/ea8c88f50f2e40e2beb5a016dd386a08/owners.json \
  --pause-receipt /private/tmp/ortak-private-20260905/recovery-operations/ea8c88f50f2e40e2beb5a016dd386a08/pause.json
```

The optional check returns `observed_then_released`. Its saved result is never
reusable capture authority. Capture acquires its own live leases and keeps all
components inside them. Root resumes the original source services from the
frozen recipes and records new ownership/health evidence even if capture fails.
A restore rehearsal must not become a prerequisite for source resume. A new
selection requires newly reviewed preparation, registration and pause code;
this exact root helper does not discover or adopt a replacement owner.

## Held capture contract

The gate verifies stopped exact application IDs/images/mounts/start generations,
fresh absence of private native writers, no running contained workers, no pending
run/cancel/outbox/Office/memory/Work output or pending/running management command,
and no unknown database client. Management stopping is necessary even if the
current queue is empty; old ownership lists cannot omit this writer. Host OAuth
enrollment stays root-fenced; host/Linux flock interoperability is not assumed.
A pinned Linux helper holds the existing executor/OAuth locks with read-only
source binds, no network/socket/application entrypoint and a finite 900-second
lease. The main PostgreSQL shared advisory schema fence spans the whole capture;
only its exact backend PID/start is excluded from database client drain checks.

SQLite counters and the final cold backup use a bounded private working copy of
the cold main+WAL pair. Only that copy opens RW with query-only SQL, permitting
normal WAL/SHM creation while source binds/files stay read-only. No `immutable=1`
shortcut or `Journal()` constructor is used. The final artifact uses SQLite's
real backup API. Every table, including `private_failure_diagnostics`, survives;
backup integrity is checked. Cold WAL/SHM companions are retained when present.

Capture records durable intents/completions for databases, cold volumes,
SQLite, public/native artifacts and selected images, then the secret envelope.
Both database helpers restore only into fresh retained verification databases.
Native Honcho and Ortak extension tables share one snapshot/dump. Complete
logical row hashes are compared for every table in each source/restore, including
retained receipts; equal counts alone cannot pass. Selected role attributes,
database settings and sequence values are compared too. Password catalogs are
never queried; potentially sensitive settings enter only the encrypted envelope.

Cold Redis/MinIO readers include the complete Linux named-volume tree, empty
directories and metadata. Links, devices, nonempty xattrs and unsupported xattr
inspection refuse. Docker Desktop host bind mounts return xattr ENOTSUP and
cannot stand in for the actual named-volume seam. Reader containers are pinned,
networkless, read-only and retained with only the selected read-only source mount.

Public files use the frozen allowlist. Native artifact directories, the frozen
resume closure and retained `repos/` are included; older backups, reproducible
pack caches and unrelated private roots are excluded. `images.tar` exports only
the unique exact IDs in `preparation.plan.images`, once per capture, with an 8GiB
combined cap. It never pulls a tag, exports a live container or adds earlier OCI
exports as duplicate inputs.

Only the last phase opens the exact secret/OAuth allowlist. An at most 32MiB
in-memory archive is encrypted with AES-256-GCM and a fresh key/nonce.
Authenticated associated data binds operation, secret-file generation and every
other component's hashes. No plaintext secret archive is written. The mode0600
key is separately fsynced under `recovery-keys/`, outside the bundle. Values and
token hashes never enter logs. The authenticated in-memory round trip does not
prove offline or off-device recovery.

The whole interval is bounded to 900 seconds. SQL/lock, row/file/count, archive,
diagnostic and capacity limits fail closed. Partial outputs remain retained with
a failed manifest and fixed failure class. Sealing requires all components,
final generation checks and both lease release acknowledgements. The sealed
manifest still records `full_restore_executed=false`,
`independent_host_verified=false` and `automatic_activation=false`.

## Completed evidence

These paths are relative to the selected main private root:

- Main schema61 database-only verification:
  `backups/20260905T161716Z_f2d4e29921d248dda62d7d0081b246ce/manifest.json`.
- Native+extension Honcho verification:
  `honcho-backups/20260905T172845Z_14551996b8ad42dcbe65faf47b00a8f4/manifest.json`;
  retained target `ortak_honcho_verify_2bf8697d1c8c4af2a25665212cf06699`.
  All 15 tables, owners, pgvector 0.8.6, schema/counts and logical row hashes match.
- E2 preparation `recovery-preparations/29710bfee8a2432182758bfdebb75ead/preparation.json`
  and registry `recovery-operations/c1411da5b71d4da1b9c17230b7a9588a/owners.json`.
  These bind E2 controller e14f / image 090758 and worker 6260. They are historical
  after the later rollouts and G1/G2 resumes and never authorize signaling an old PID.
- Actual Linux lock contention/release and cold WAL/no-SHM fixture:
  `recovery-linux-fixtures/cf1b5b9dba6845f4ad630d5c7f6d97e5/manifest.json`.
- Actual retained-database schema lock/settings/sequence SQL and isolated
  named-volume tree/mode/limit/link checks:
  `recovery-foundation-fixtures/4909ae26bea04cc6ab07bc6bc984ca6e/manifest.json`.
- Real new offline Redis AOF/expiry and MinIO versions/delete-marker/metadata:
  `recovery-service-fixtures/e01814fa91ae4d6c83c0e992c8c2240d/manifest.json`.
  The MinIO write-counter xattr is preserved by the cold archive and restore
  helper; unknown xattrs still refuse. All generated fixture servers are stopped.

Earlier refused fixtures remain retained. Those isolated fixtures made no provider
request or production volume mount. At that checkpoint source coverage included four-writer registration,
management/Work drain, original function configuration after populated database
restore, bounded xattrs and application-semantic fixtures. The earlier broader private
lane recorded 120 tests: 119 pass and one explicit database test skips by default:

```sh
python3 -m unittest discover -s scripts/ortak -p 'test_*private*.py'
```

## Remaining offline restoration

The [offline foundation restore](OFFLINE_RECOVERY.md) is implemented and its
owned fixture rehearsal passed. A real captured-bundle restore still requires
new empty destinations, verified images, storage-only processes on a fresh
internal network, no source mounts/networks and no Docker socket in any restored
controller. Preserve IDs/receipt bytes. Compare database sequences/fences, Redis
expiry-aware state, MinIO metadata/versions, SQLite cursors/tombstones and offline
secret decryption. Do not start application/provider/Office/schedule/deriver/
executor entrypoints. Actual execution failover requires a separate daemon/host
and coordinated single ownership; it remains closed until independently
rehearsed. No automatic restore promotion or automatic source resume is performed.
Root-owned source resume has now run after both failed capture attempts.

## Populated schema69 obligation rehearsal

`rehearse_private_recovery_obligations.py --execute-disposable-fixture` requires
`ORTAK_RECOVERY_TEST_URL` selecting only127.0.0.1:55432/postgres and creates one
fresh `ortak_g_obligations_<UUID>` database. It cannot reset/drop an existing DB
or select live55433. The frozen69 bootstrap binary runs first; the synthetic
SQL seed commits through every real schema69 constraint/trigger, including
atomic fact/publication/job creation and reciprocal same-key claim/ACK guards.
It is SQL admission evidence, not signed API/provider/remote-memory acceptance.

The retained successful receipt is
`/private/tmp/ortak-v0-evidence/g-obligations69-0ac644a1fe1f4c99a0a48aa5997a8239/receipt.json`.
All25 cases passed in the corresponding fresh55432 database. Each of two
companies has one target/export/command/publish ACK and two jobs. G accepts
the pristine future withdrawal and historical ACK lease; due/leased/expired-
lease/attempted/uncertain/failed jobs, missing pairs and incorrect immutable
ACK/target/company/community/key identity refuse. An expired uncontained probe
also refuses. Cross-company isolation and inert offline due-row/full-hash
comparison passed. Retirement of the short advertisement preserves the exact
already-published recovery obligation.

Adversarial faults use only an outer rollback transaction in that fresh DB,
with a transaction-local replica flag; the positive seed never disables guards.
The unchanged production witness ends in ROLLBACK, and both companies' baseline
witnesses plus the full trigger catalog digest were unchanged afterward.
Earlier failed attempts remain retained:0a958… skipped the ignored bootstrap,
c464… found a pre69 embedded migration mismatch, and c919… found a future71
column dependency. No migration ledger was manufactured or rewritten.
The broader private Python suite passes144 of145 tests, with one existing
explicit opt-in skip. No G live pause/capture followed from this rehearsal.
