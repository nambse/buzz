# Private full-stack recovery preparation

## Current checkpoint: actual G74 volume capture and offline restore passed

Root captured the populated schema74 stack at 04:37:36 UTC on 2026-09-06 and
completed the offline foundation restore at 04:39:45 UTC. Bundle
`214fd4f027a34604aeb7469d9dfb9a60` and restore
`cea594c6416d42f7a3403aa7509d2c70` verified 135 main/19 Honcho tables, the
current local-volume journal's physical bytes and coherent rows, and 16
workspace filesystem entries. The journal had 25 terminal runs and two
terminal workspace calls, with zero pending/invalid counters.

The same source services resumed successfully; restored execution remains
disabled. Eight exact stopped temporary containers were removed after evidence
capture, with volumes/bundles retained and no image build. This is same-host
offline storage recovery, without independent-host/daemon or restored-runtime
activation claims. Exact hashes, selected policy and owner/resume limits are in
[Current private G74 recovery selection](CURRENT_PRIVATE_RECOVERY74_2026-09-06.md).
All earlier checkpoints and commands below are historical, including their
then-pending status and superseded process IDs. They are not current authority.

## Historical checkpoint after G3 — 2026-09-06

G3 completed both database components and failed on MinIO's previously unreviewed
eight-byte `user.total_deletes` attribute. Sources resumed unchanged; current
owners are relay9858/API9869/management9878/worker9894/native9935, with the exact
schema69/backend/native6ff image selection. Read-only identity, health and
Work/fact persistence passed. Both MinIO counters are now preserved through the
reader/PAX/extractor/full-tree hash; actual permanent-DELETE, six installed-reader
guards and the full subsequent capture tail fixtures passed. See the
[complete G3 cause, receipts and limits](G3_VOLUME_READER_RECOVERY_2026-09-06.md).
G4 preparation5046b481… and registrycd6d2150… are ready for root review;
no G4 pause/capture or successful live full capture is claimed here. Earlier
G3-running and prior-owner checkpoints below are historical.

## Historical checkpoint — G3 prepared after verified G2 source resume (2026-09-06)

**Both G1 and G2 performed a real coordinated pause. Neither produced a successful
full capture, and no complete captured-bundle offline restore has run.** Failed
attempts and component archives remain retained; they are not restore authority.

| Checkpoint | Observed result |
| --- | --- |
| G1 pause | Passed with actual held quiescence/ownership checks. |
| G1 capture `d6b4737afd1145ff8f2917584230c883` | Failed on an exact Honcho CHECK catalog round-trip difference. |
| Isolated Honcho repair fixture | Passed on a fresh isolated 19-table target; target-only repair restored exact metadata, without weakening schema comparison or changing the source. Receipt: `/private/tmp/ortak-v0-evidence/g-honcho-check-roundtrip-f78e6335c8394df4b6e2092b1872ed2f/receipt.json`. |
| G2 preparation / owner operation | `dfeb71816ab1438cbe74dfbb8eb47a32` / `e3535f007c194ebe992a2c610884b73a`, with 20 frozen operator files. |
| G2 pause `0ce86c156a5c41858ef405a2383a386f` | Passed. |
| G2 capture `dbd527bf32db44b78e45b2de5a4074b1` | Failed with `FileExistsError` in the database `main-settings` phase: source and restored verification reused `recovery-obligations.sql`. |
| G2 component archives | Main backup `f9b307814edb40c395ad5b18433cd808` and Honcho backup `cbf9a241fc884c3aafe6dabcb1466913` verified 127 and 19 tables respectively. The actual Honcho target repair passed. These successes do not seal the failed full bundle. |

All source services resumed with the same selected containers, images,
schema69/backend69 and compiled native6ff. The read-only resume receipt is
`/private/tmp/ortak-private-20260905/recovery-operations/e3535f007c194ebe992a2c610884b73a/source-resume-f52c6403a004472db1ba31153332b0f0/validation-acd1600fe8fb4d10b6706c79a326ceb6/receipt.json`.
It records relay liveness/readiness 200/200, unauthenticated API401, and identical
counts plus complete row hashes for 16 scoped tables before/after resume.
Work `4419…` remains COMPLETED v10; Ada remains ACTIVE, epoch1, Sol/high.
Fact `853d…` remains withdrawn with zero text rows, one retained header/tombstone
and both publish/withdraw operations. This is scoped resume evidence, not a
successful whole-stack capture or an offline restore.

| Service | Observed post-G2 PID/session — not signal authorization |
| --- | --- |
| Relay | 3241 / 52012 |
| API | 3252 / 92895 |
| Management | 3263 / 42627 |
| Worker | 3277 / 67436 |
| Native6ff | 3290 / 22975 |

Root separately verified the real native UI after G1 resume: Work COMPLETED v10,
a 39-word artifact, two satisfied criteria, the required approval entry and live
Activity. That was operator navigation verification, not a personal review by
the user. The read-only G2 receipt explicitly does not claim a new signed API or
native UI acceptance action.

The source fix now separates source/restored main and Honcho log names. The
production `Capture.databases` plus real `Commands.run`/exclusive-create
subprocess fixture passed; reverting either label independently reproduces
`FileExistsError`. Focused Python tests: 128 passed, one explicit skip out of 129.
Receipt: `/private/tmp/ortak-v0-evidence/g-capture-exclusive-regression-aa472464328f4a5abd92e0bd614bb285/receipt.json`.
Seal file/parent fsync failures preserve the original manifest and a separate
`failure.json`; restore refuses that marker even if the captured hash is valid.

G3 read-only preparation `5e4af2cc2c7543c38a467231559e9ac8` and registration
`15cc30dc9d3147979876a83b4056acb4` passed with 20 frozen operator files, five changed
for capture, obligation logs, restore refusal and current inventory/native owner.
Owners digest: `6f4ca742ad69acc15f7dd060597d123aed8fc2783fab0aaadd184d46c358a695`.
The unchanged generic v3 helper is `/private/tmp/ortak-pause-for-g69-checks-v3.py`,
SHA256 `7661a94bcbccfdd0e640c77694ba48541cc95b449fcd2ce6bfd2fac781fafa13`.
The new operation's `ROOT_CAPTURE_RECIPE.md` retains its exact selected commands.
Root has started the G3 pause/capture attempt. Its outcome is pending; no G3
successful full capture or offline restore is claimed.
The generic v3 pause helper requires explicit `--owners` and `--owners-sha256`.
**The G2 registry is stale after resume and cannot be used for another pause:**
PIDs/sessions changed and the next operator code also changes. Revalidate the
selected source, create a fresh preparation/registry, freeze that exact code and
use its new digest. G3 supplies the newly prepared candidate; it does not itself
perform a pause. All old exact pause/capture commands below are historical,
not current runnable instructions. Root retains live pause/resume authority.

## Historical schema69 rollout selection — before G1/G2

Root completed the schema69 rollout. The G source selection now binds the
actual public receipts under
`/private/tmp/ortak-private-20260905/rollouts/schema69-605742d230054d619a9561a4444529c9`
and the immutable seven-binary directory
`/private/tmp/ortak-private-20260905/artifacts/backend69-d0d4a2e2671d4b9faf229440ad25d994`.
The root health receipt records relay/API health200 and unauthenticated API401.

| Service | Historical rollout owner | Receipt |
| --- | --- | --- |
| Relay | PID71727/session23923 | `buzz-relay-resumed.json` |
| API | PID71761/session68369 | `ortak-server-resumed.json` |
| Management | PID71791/session58813 | `ortak-management-resumed.json` |
| Worker | PID71826/session23277 | `ortak-worker-resumed.json` |
| Private native | PID72102/session16306, `6ff3a935892066429308ec720b3cd3b8c80031b2a53094316be0185f4dd77a21` | `native-resumed.json` |
| Hermes controller | `9335e4dfd7b6ff90f9d0e91e8089bf470723cf89f5a2ef3d99c972ecb98cdca0`, image `sha256:dbc9bcf93f7681110052da3a437ab2920906b0c171dfacc8bf07a35f51cec247` | `hermes-resumed.json` |
| Contained Hermes worker image | `sha256:8ee1899da85d40e26db381160f9fef50f4ba69a029699f77c7aced590b3a00f1` | `hermes-resumed.json` |
| Honcho D2a | `ad579c8e6cd7c556cb3155630dc7f1c8db79ccc030dd2d72e5d9160380bc35a4`, image `sha256:febea5609d74f51026ab5a98ac9ce7b8648989ac7f639893ef4f71846c65dc1b` | `honcho-d2a-2934fc7da116489dab977d5f88f32e88/receipt.json` |

Main PostgreSQL, Redis, MinIO and Honcho PostgreSQL IDs/volumes remain the exact
retained stores listed in the original inventory below. The four new Honcho
reviewed tables are deployed; the [Honcho rollout record](HONCHO_D2A_ROLLOUT_2026-09-06.md)
preserves both the first143 failure and the successful exact checkpoint resume.
Every prior native Honcho table count and complete logical row digest, plus
settings/sequences, matched after initialization. The helper executed G's
schema69 read-only drain query successfully against the actual database; new
probe/export tables were empty. The subsequent populated future-withdrawal G SQL fixture passed25 cases in
fresh disposable55432, with every positive schema69 guard active and synthetic
remote ACK identity. Its receipt is
`/private/tmp/ortak-v0-evidence/g-obligations69-0ac644a1fe1f4c99a0a48aa5997a8239/receipt.json`.
This is separate from signed API/provider acceptance and actual full capture.

That rollout controller's public configuration is under
`/private/tmp/ortak-hermes-models69-27af1db0a0e044b1a14eb0adc24757ae`.
Its exactly three public profiles select Astra/max, Sol/high and Astra/high.
All use employee `ada-private`, the same original explicit OAuth directory
`/private/tmp/ortak-hermes-v0-private-20260905/oauth/ada-private` and the same
opaque credential reference. Preparation validates each exact model/effort,
provider marker, immutable profile marker and binding file; unknown, missing,
duplicate or rebound profiles refuse. The two new public profile directories
and their read-only controller mounts are included explicitly. No credential
was copied or OAuth enrollment/refresh invoked for this source update.

The native selector now uses its actual compiled-policy build receipt and the
new separate resume receipt; backend69's receipt no longer carries a native
hash or a directory field. Backend receipt validation binds the selected path,
current schema/status and all seven binary sizes/hashes. Current native6ff
contains the compiled legacy execution boundary; the later voice-note omission
is source-tested but has not been rebuilt into that artifact.

**Historical pre-G1 checkpoint: fresh G69 preparation and owner registration passed; no pause/full capture had run then.**
Preparation `e81d55418f4045dbbc2a33b50d5213df` and owner operation
`ea8c88f50f2e40e2beb5a016dd386a08` revalidated the current4 writers, native6ff,
Hermes/Honcho and all19 frozen operator files. The registry digest is
`31473a078943e858b41748617648cfba00dc959b19126955cb9c50603ff77bfe`.
The private operation's `ROOT_CAPTURE_RECIPE.md` retains the then-selected pause,
frozen capture, owned source resume and offline restoration commands; these are historical after G1/G2. Main127
tables/schema69 and Honcho19 native/extension tables were observed; these
row counts are not a snapshot or drain claim. All writers/native remain resumed.
The schema69 rollout's old `paused-drain.json` is historical; a new coordinated
pause is still required. At that checkpoint root reported native Work completed with active Sol/high.
Its chosen signed Stop and same-key Honcho withdrawal ACK now completed for the
published fact: version2, retained original hash receipts, no text, one tombstone
and both publish/withdraw operations. Evidence is under
`provisioning/native-reviewed-memory69/{published,withdrawn}.json`.

The new bounded root-only `scripts/ortak/pause_private_recovery.py` is prepared,
with13 focused production-dispatch tests passing and read-only verification of
all19 selected frozen imports. The private mode0500 copy is
`/private/tmp/ortak-pause-for-g69-v2.py`; its exact hash is recorded in this owner
operation's `pause-helper-v2-selection.json`. The executable pause replaces manual
attestation: exact loaded-owner rechecks precede each SIGTERM; schema69 semantic
drains precede successive ingress/writer/container stops; no source force-kill or
automatic resume is allowed. It leaves both PostgreSQL services up and publishes
`pause.json` only inside actual held executor/OAuth/schema leases. Failure retains
the exact effects and root resume/reconciliation commands. No live pause was
performed in preparing or testing this helper. The [runbook](../../runtime/private-stack/FULL_STACK_RECOVERY.md)
and private recipe have the exact root command and timeout/uncertainty contract.

Earlier local private Python validation:145 tests,144 passed and
one existing opt-in database test skipped. The source update does not advance
any restored execution/provider/Office activation gate. Earlier generation
observations below are historical unless explicitly retained above.

## Implementation milestone and retained earlier evidence

The executable [recovery runbook](../../runtime/private-stack/FULL_STACK_RECOVERY.md)
now covers preparation, frozen process/session launch authority, a live held
quiescence gate and full-bundle capture. Capture executes all components inside
Linux executor/OAuth locks plus the main PostgreSQL schema fence. It preserves
failed phase evidence, verifies complete logical database row hashes as well as
counts/settings/sequences, captures complete cold Redis/MinIO trees and SQLite
through its backup API, and packages only exact selected secrets with local
authenticated encryption. At that earlier milestone no source stop/resume action
was implemented; the selected root-only pause helper is the later addition above.

The capture/preparation G source tests pass. Actual isolated Linux lock/WAL/no-SHM and
named-volume reader fixtures also passed; the runbook links their retained
receipts. A new native+extension Honcho archive was restored into the generated
retained database `ortak_honcho_verify_2bf8697d1c8c4af2a25665212cf06699`; all
15 native/extension tables, complete logical row hashes, schema, owners and
pgvector 0.8.6 match. Main/Honcho settings and sequence SQL and the shared schema
lock were separately exercised against existing retained verification databases
without source writes. Linux source binds remain read-only when SHM is absent:
the lease checks a bounded cold main+WAL copy in private tmpfs, permitting normal
SQLite working metadata there with RW access and query-only SQL. Cold capture
uses the same working-copy approach before the real backup API. No `immutable=1`
shortcut is used.

The [offline storage implementation and rehearsal](../../runtime/private-stack/OFFLINE_RECOVERY.md)
also passed using the exact retained main/Honcho archives and synthetic secret,
volume, native and SQLite data. New networkless PostgreSQL containers preserved
all schema/count/logical bytes/settings/sequences, then stopped and remained
retained. Redis/MinIO volume byte/metadata restoration and authenticated separate
key decryption passed. A separate fresh network-none rehearsal also verified real
Redis multipart AOF replay/absolute expiry and MinIO object versions/delete marker,
body/metadata and its bounded write-counter xattr. All generated storage servers
stopped cleanly and remain retained. At that checkpoint the private Python suite had137 tests:136 passed and one
explicit database-dependent test skipped; the current count is recorded above.
The populated-credential database restore regression passed using the shared
strict target-only compatibility helper; source functions/migrations did not change.

**No successful full G capture or complete offline restoration has been recorded.**
The later failed G1/G2 capture attempts are recorded at the top.
The management executor is a required fourth writer beside relay/API/worker;
pending/running commands and Work outputs must drain. Historical E2/66 process
receipts cannot become current signal or capture authority. Root replaced the
old0f2 native with compiled-isolation6ff after the actual early policy probe;
root used an exact verified SIGKILL for old process45301 to avoid its inherited
shutdown reaper. The latest historical schema66
preparation `b8b5f99909c74d2f9a030912bae9dba4` and owner operation
`352a680fae154d90b9298cd31964573b` record all four writer sessions plus native
ingress and the 18-file frozen operator closure. They authorize no pause, and
become historical when any selected service or artifact changes. Gate/capture
must use that operation's frozen `operator-code`, not mutable repository code.
The old a5ed app remains excluded. Restored executor/provider/Office activation
is still closed; real full-bundle restoration follows the final coordinated
capture. The completed synthetic foundation rehearsal does not close that gate.

## Schema68/69 recovery source integration — 2026-09-06

This source slice was initially tested while schema66 was live and Hermes8ee/dbc
and Honcho D2a `febea…` were only built/tested. The later actual schema69 selection
is recorded above. Future schema versions still require explicit review.

`private_recovery_obligations.py` is now shared by preparation, inventory,
held-barrier drain, capture verification and offline restoration. Explicit
schema61–69 inputs select a fixed retained table set; missing/partial new tables
or a newer version require review. Schema68 requires
`provisioning_runtime_probes`; schema69 additionally requires
`reviewed_memory_targets`, `reviewed_memory_exports`,
`reviewed_memory_export_jobs`, `reviewed_memory_export_commands` and
`reviewed_memory_export_receipts`. The frozen operator closure is now19 files.
Old18-file registries and old preparation plans cannot silently gain this policy.

Every `provisioning_runtime_probes` row in `running` state blocks capture,
including an expired deadline or a terminal bridge response without the exact
contained acknowledgement. Preserve the original bridge and immutable probe
identity for recovery; a replacement profile is not evidence of containment.

For reviewed memory exports, root selected distinct backup and deletion rules.
A backup does not withdraw live facts. Future pending withdrawal/expiry jobs
may be retained only with the exact immutable keys, targets and job/receipt
state recorded. Capture refuses pending publication leases even if expired,
due pending publish/withdraw work, uncertain remote publication and failed
cleanup. An acknowledged job may retain historical lease fields; that does
not make it a current lease. A successful publication with a future pending
withdrawal is an explicit recovery obligation, not an active writer.

The executable gate also refuses a retried or uncertain future withdrawal
(`total_attempts>0` or a last error), as well as any pending publication.
Acknowledged jobs require an exact company/community/target/request-hash/
lease/attempt receipt match; withdrawal ACK requires erased text and a tombstone.
At most1024 scoped rows are inventoried in one read-only repeatable-read query.
Their exact public primary keys and SHA256 of every complete row are frozen;
the database archive and complete logical table digests preserve the actual
bindings and rows without projecting credential references or fact text into
the public witness. Both barrier checks must match, and source/verification
databases must retain identical witness rows. Offline restoration compares the
same evidence without executing newly due jobs; elapsed expiry cannot turn a
backup into a cleanup acknowledgement.

Honcho D2a requires all four tables together: `ortak_reviewed_records`,
`ortak_reviewed_record_content`, `ortak_reviewed_tombstones` and
`ortak_reviewed_operations`. They are installed with the extension's existing
`init_db` path, without a new upstream Alembic revision. Native plus all seven
extension tables are captured/restored under exact schema/count/row-hash/
settings checks. New read-only lifecycle checks verify content hashes, absence
of text after tombstone, scoped publication/erasure receipts and header identity.
Expiry only filters reads; mutation is an explicit job, so expired but not yet
erased retained text is not falsely reported as a tombstone.

Ten new local Python regressions passed through the production witness and
gate admission seams; the entire137-test private suite passed136 with one
existing opt-in skip. These are local source/fixture tests, not a new populated
PostgreSQL69 drain rehearsal. The later root rollout exercised that SQL against actual69 with empty new tables;
the later25-case populated G SQL fixture passed; final full capture remains open.

Offline restored services remain inactive until original-writer containment
and same-key reconciliation/expiry catch-up succeed. Backup or local purge
never creates an erasure acknowledgement. Destructive community quiescence
must instead require every exported fact's real withdrawal acknowledgement
with `erased_from_reviewed_store=true` before the universal write fence makes
further cleanup impossible. The [canonical DeletionStore guard](REVIEWED_MEMORY_DELETION_GATE_2026-09-05.md)
and its populated exporter/deletion regression passed in the integration owner's
retained disposable69 fixture (three unit and nine actual PostgreSQL tests).
The private schema69 rollout later passed; actual G capture remains open.

## Original read-only inventory and design

Prepared read-only on 2026-09-05 after the schema61 backend rollout, while root
performed the first real activation/Office integration. This is slice G design
and inventory evidence, not a completed full-stack backup or restore. No service
was stopped, mutable file snapshotted, credential copied, restore container
created or provider/Office request made by this preparation.

Root reported successful fresh Ada activation during this observation:
operation `3acfe3b7-14de-421a-b270-d7c5b396a702`, revision
`e36f3e77-4d63-48ae-afa5-88f62a1ba82c`. The earlier schema61 post-upgrade database
backup contains the pre-activation draft state. It is not an activated-stack
recovery point. Root is changing the central relay/worker composition; their
final ownership receipts must replace the earlier process observations before
any future stop.

## Selected scope and observed inventory

The only selected logical scope is company
`a4013353-a84d-49a1-8d2b-10a1caf896fe`, employee `ada-private`, main private root
`/private/tmp/ortak-private-20260905`, and new runtime root
`/private/tmp/ortak-hermes-v0-private-20260905`. Old Cem/Zeynep, the native a5ed
application, other host profiles/keyrings and unrelated databases are excluded.
Original verification archives/databases remain retained; they are not runtime
stores and must not be reset or bulk-dumped as a substitute for exact selection.

Docker observations used the explicit local socket
`unix:///Users/nambse/.docker/run/docker.sock`, exact selected container IDs and
only identity, mount, port, network and restart fields. No container environment,
command or whole configuration was printed. The backing Honcho database was
identified through the exact selected API network; root confirmed that this
dated `test`-named database is retained private state.

| Store/service | Observed container ID | Immutable image ID | Persistent data / endpoint |
| --- | --- | --- | --- |
| Main PostgreSQL | `01ad59c9f79fd50e47281ef85b829fb2a8d556f627a43b175e36fc8ecfde53c7` | `sha256:ef257d85f76e48da1c64832459b59fcaba1a4dac97bf5d7450c77753542eee94` | `ortak-private-20260905_postgres_data`; database `ortak`; loopback55433 |
| Redis | `90776da21e0a84d0f3e369e6dc82da0fe5c696afa407502ff772e0b16f48f6f9` | `sha256:ff02b58f971e7d7d156a1267e283fcbbeee91773b6aa36c49dac28ecfe28eadf` | `ortak-private-20260905_redis_data` at `/data`; loopback56382 |
| MinIO | `40163c68f2d617651e7e6460225634e1de73b78dc5b9f0311559095de41ac07a` | `sha256:e1d7f7262c86498b45f869bcc7e3bbe7c11b3c026d9aad25f7759b053fd60a41` | `ortak-private-20260905_minio_data` at `/data`; loopback9008 |
| Honcho PostgreSQL | `e5d4bd4ff4cabcc6f8e8d4712c3001e83fb8cd89291214dd840f4ea5edfe3d88` | `sha256:cf134a767f474095eeba57e0117be8e568e011a63f33fbf252f14c9b760f8e6f` | `ortak-honcho-test-data-20260905` at `/var/lib/postgresql/data`; no host port |
| Honcho API | `13cbff3d670de2030792ac515fe52b7506506ee227ac0df8fa7d54c7ed412182` | `sha256:cc8b4a29c0adda08978886e205ff5c5ff0a13923e4ed15e1626b24194d0c0c21` | `ortak-honcho-api-check-20260905`; no mounts; loopback8009 |
| Hermes controller | `9a74e06f97166f65b2ff65ec28c3bf2c47d678c1939f10d6a37ac72063bdf155` | `sha256:9877cb5636124534b06b5ed8718957af9fe0d8039846d64d85f00b6a90857194` | `ortak-v0-hermes-private-20260905`; exact runtime bind paths below; loopback8650 |
| Contained Hermes worker | Per-run ownership inventory required at barrier | `sha256:f18aa9349e1821ddc8853f33d4d3a73eddd7636a4cea9467d5811b760b5777e1` | Ephemeral children; durable registry belongs to controller |

All four store volumes use the local driver. Main PostgreSQL/Redis/MinIO volumes
have exact Compose project/volume labels. The Honcho volume has **no labels**;
its name alone is insufficient authority. Freeze the exact container, image,
mount tuple, network ID and root's explicit retained-store selection in the new
ownership registry before a future action. The executable preparation later
resolved the exact database under root's explicit narrow-setting authorization:
`ortak_honcho_adapter_test`, role `ortak_honcho`, host `honcho-test-db:5432`.
The saved selection and exact live API setting agree. A bounded repeatable-read
read-only catalog observation found15 public tables, all owned by that role,
with pgvector 0.8.6 and all three Ortak extension receipt tables. The URI/password
and other environment values were not emitted. No unrelated database was read.
Full role attributes/settings remain a capture preflight gate.

The selected Honcho network `ortak-honcho-test-20260905`
(`6bbcd7fc2eba5a6e55054e615beb7528d2914fe5aa6c46e3b0cca61ebad1995e`)
is internal and had only the API and database. The API also joins the selected
main store network. No separate deriver container appeared in that network;
this does not prove the API lifespan has no background writers. Its native
lifespan must be inspected in the deployed image before stopping/draining it.

The runtime control network observed in the original inventory
`ortak-v0-hermes-control-7ffa64600b564e1f88369ef1cc3a8270` and worker network
`ortak-v0-hermes-run-5214763bf281407fb8412121b4d26315` are **not internal**.
They support the explicitly selected OAuth/provider flow and must never be
reused by a recovery rehearsal. A momentarily empty worker network is not a
quiescence witness.

The verified schema61 native binaries and their source/hash receipt are in
`/private/tmp/ortak-private-20260905/artifacts/backend-ac-20260905T160204Z-1c63001b83e84f72bc9a3485ee02020f`.
The rollout recorded relay68255/session40350 and API68332/session85029. Later
preparation rediscovered relay74262, API68332 and worker76818 from their private
cwd, start time, loaded artifact path/inode and frozen hash receipts. At that checkpoint root owned
their sessions2121,85029 and22653, respectively. These are historical
observations, not permission to signal reused PIDs. See
[the rollout record](PRIVATE_SCHEMA61_ROLLOUT_PREPARATION_2026-09-05.md).

## Exact files and opaque identity selections

Under the main private root, preserve the marker, immutable backend artifacts,
selected API/worker/provisioning configuration, original provisioning operation
selection and all `memory/` intent/receipt fragments. Main PostgreSQL includes
Office events, signed delivery bytes, cursor/dedup state, active revisions,
cohorts, reservations, execution/Work history, retained Office identity snapshots
and inbox reconciliation receipts. `repos/` must be included if it contains
retained git data; `pack-cache/` is reproducible and can be excluded explicitly.
Do not recursively archive the whole private root with earlier backups inside it.

Selected memory deployment is `efd1ad6f-df29-4346-8a2d-f2c271ff4b72`, endpoint
`service://ortak-private-20260905/honcho`, origin `http://127.0.0.1:8009`, token
reference `secret://ortak-private-20260905/honcho-admin`, and resolver variable
name `ORTAK_HONCHO_PRIVATE_TOKEN`. The immutable creation receipt's request hash
is `0b3d7375c7a186f9518eb169d110ded86e138f4c42fcc13ec7bea54cfdefdd33`.
Native workspace/peer/session IDs and the full original create/write receipts
must survive byte-for-byte; Adopt activation does not confer deletion rights.

Under the selected runtime root, the controller mounts `state/` writable,
`profiles/` and `controller/` read-only, and `oauth/` writable. Relevant paths:

- `state/journal.sqlite` and any SQLite WAL/SHM companions present at quiescence;
  permanent run/tombstone/cursor and probe selection records must survive.
- `profiles/ada-private/ORTAK_DISPOSABLE_PROFILE.json`,
  `ORTAK_RUNTIME_BINDING.json`, `ORTAK_PROVIDER.json`: the three public OAuth
  profile files. Exact profile reference is `ortak-private-20260905-ada-oauth-v0`.
- `controller/config.json` and `controller/service-token` (secret).
- `oauth/ada-private/`: explicit fresh selected OAuth store; source defines
  `ORTAK_OAUTH_IDENTITY.json`, `oauth-state.json` and `oauth.lock`. Preparation
  inspects directory/file metadata only; no OAuth file is opened or hashed.

OAuth reference is `secret://ortak-private-20260905/ada-codex-oauth-v0`.
Public selection receipts are
`/private/tmp/ortak-v0-evidence/private-oauth-selection.json`,
`private-hermes-controller-selection.json`, and
`private-hermes-controller-start.json`. Exact tested image/OCI evidence is linked
from [continuation progress](CONTINUATION_PROGRESS_2026-09-05.md).

The following secret-bearing paths are metadata-only inventory for a later
explicit packaging operation: main `identities.json`, `runtime.env`,
`secrets/postgres-password`, `secrets/redis-password`, `secrets/redis.conf`,
`object-store/credentials.json`, `object-store/root-user`,
`object-store/root-password`, `honcho-tests/postgres-password`,
`honcho-tests/service.env`, and the controller/OAuth paths above. Exact future
worker resolver sources must be added from root's final launcher receipt; an
opaque reference is not recoverable secret material by itself. `test.env` is a
test launcher input, not automatically part of runtime recovery scope.

Observed private directories were mode0700 and ordinary host files mode0600,
UID501. Exact service-mounted password/config leaves use mode0444 inside those
private parents by the initializer's reviewed design. Runtime UID10001 is
presented through Docker Desktop bind mapping; validate permissions from the
restore container as well as the host. Do not bulk-chown an existing directory
or blindly normalize these service-readable leaves.

## Required owned-process registry

Implement one private durable operation record before touching services. Its
frozen source set must include deployment/operation UUID, main/runtime roots,
marker digests, exact daemon endpoint and daemon identity, current immutable
container IDs/image IDs/mounts/networks, service launch receipt, native PID/start
time/UID/cwd/loaded binary inode/hash, and owning terminal session. Record
configuration file paths and safe hashes for public config only; tokens must
not become a registry field or a public checksum oracle. Keep the manifest
separate from encrypted secret payloads.

Each later action rechecks this identity and records its outcome. A changed PID,
binary, image, mount, network or deployment generation refuses the action;
ports and process names never authorize it. Failed and partial operations retain
their journal. No wildcard stop, `down -v`, pruning, database reset, credential
discovery or automatic adoption is part of this mechanism. Never make a source
resume depend on successful cleanup of a failed restore rehearsal.

## Proposed consistent backup transaction

This sequence requires implementation and review after the first soak. A shared
timestamp alone does not synchronize independent stores. The proposed consistency
boundary is an interval during which every selected cross-store writer is stopped.

1. Persist `planned` with an exact allowlist and fresh mode0700 backup directory,
   size/deadline budgets, frozen source artifacts and intended source-resume
   configuration. Resolve the exact Honcho database and all active native/child
   process owners. Preflight private capacity and a local encrypted destination;
   do not acquire an external encryption key or upload anything automatically.
2. Stop new Office/API ingress and new dispatch admission without revoking the
   employee or changing durable Work/cohort intent. Drain existing admitted work
   while the original delivery/reconciliation path can still finish. The current
   worker SIGTERM only drops its local future and explicitly leaves remote work
   unchanged. It is **not** a drain. A small explicit maintenance/admission fence
   plus bounded drain witness is required before claiming automated quiescence.
3. At the drain deadline, retain pending outboxes and use the existing durable
   cancel/reconcile path for still-admitted runs. Confirm every selected contained
   child has stopped using ownership/image checks, even terminal or failed runs.
   Reconcile acknowledged Office event IDs and runtime cursors; retain unresolved
   acknowledgements with their original signed bytes/start keys. Never invent a
   new event/run ID or mark an unknown external result successful.
4. Stop the selected worker, relay/API writers, Honcho API and any owned native
   deriver/reconciler, then the controller after children settle. Stop any explicit
   OAuth enrollment writer and hold the exact OAuth/executor locks. Inspect
   bounded database client metadata without SQL text or environment to refuse
   remaining application writers. Unknown writers or an uncertain refresh refuse
   the barrier. Native a5ed is not stopped; shutting the selected ingress keeps
   it from changing this stack during the interval.
5. Persist `quiesced` only after the registry, store scope, durable run/cursor
   status and source generation all agree. Keep DDL/upgrade activity paused and
   retain the coordinated schema fence. Dump exact main and Honcho databases
   separately with exported read-only snapshots. No application writes occur
   between them, making their cross-store state stable. Include the selected
   role/tablespace/database-setting inventory and explicit secret handling for
   any role material; preserve extensions and exact pgvector image/version.
   Per-database dumps omit cluster-wide roles/tablespaces, as documented by
   [PostgreSQL](https://www.postgresql.org/docs/17/backup-dump.html).
6. Gracefully stop selected Redis and MinIO and prove they exited without forced
   termination before cold archive access. Archive the complete Redis `/data`
   set, including multi-part AOF manifest/base/increments, and the complete MinIO
   volume, including `.minio.sys`, object metadata and versions. Copy via a new
   bounded helper with read-only source volume mounts and the inspected immutable
   image/toolchain; never copy Docker VM paths from the macOS host. Redis7.4 uses
   AOF `everysec`; a live naive AOF copy is not this recovery contract. Redis
   documents the multi-file/rewrite constraint in
   [its persistence guide](https://redis.io/docs/latest/operate/oss_and_stack/management/persistence/).
7. With controller and children stopped, use SQLite's backup API to a fresh file
   and check integrity, rather than copying only a potentially stale main file.
   Preserve the original cold SQLite/WAL set separately as private recovery
   evidence when present. The [SQLite backup API](https://www.sqlite.org/backup.html)
   provides a coherent database snapshot; it does not synchronize other stores.
   Preserve all public profiles/config, memory/provisioning receipts and optional
   retained repos through an explicit file allowlist. Never use `Journal()` merely
   to inspect a source backup: its constructor creates schema and enables WAL.
8. Only in the later explicitly authorized operation, package the exact secret
   allowlist and fresh OAuth state directly into an authenticated encrypted local
   archive. Keep any generated recovery key in a separate owner-private location
   outside the bundle; never print it. Record encrypted artifact sizes/digests,
   not token values/hashes. Until decrypt-and-restore is actually exercised, mark
   secret recovery unverified. Local encryption alone is not off-device recovery.
9. Fsync payloads and final manifest, compare source barrier metadata a second
   time, and seal the bundle only if every component belongs to the same
   quiescence operation. Partial bundles remain `failed`/`incomplete`. Resume
   source stores and services in dependency order from the frozen launch recipes;
   revalidate identity and current adapter gates before explicitly releasing
   ingress/admission. Recovery/testing of the copy must not run on this daemon.

The first implementation should refuse rather than silently repair an unclean
Redis archive, MinIO/object mismatch, stale OAuth rotation phase or unmatched
Honcho receipt. Missing bytes cannot be repaired by reprovisioning resources.

## Fresh isolated restore rehearsal

Use a fresh restore UUID, roots, volumes, names and **separate Docker daemon/VM**
with no production socket, production networks, source volumes or host profiles.
Retain logical company/employee/native IDs and original receipts. Rewriting them
would prevent validation of the backed-up identity contract. Load verified
immutable artifacts without pulling floating tags. Bind network endpoints only
inside the new isolated environment; record mechanical endpoint/path remapping
in a separate restore configuration, leaving original receipt bytes unchanged.

This daemon boundary is essential: `docker_executor.container_name` derives
names from company/run start keys, and `owned_keys(company)` plus constructor
recovery can cancel matching containers on its daemon. A new network or cloned
directory does not protect production when logical identities are retained.
If a separate daemon is unavailable, complete data-only validation with no
controller socket/executor; mark execution recovery not exercised. Do not start
an executor-enabled cloned controller on the source daemon.

Restore storage first. Create fresh databases from `template0`, recreate only
the selected roles/settings, and restore with fail-on-error/single-transaction
behavior. Preserve exact extension/native tables together. Restore Redis/MinIO
to empty new volumes before starting their pinned processes; prove ownership and
expected tree digests first. Read/check SQLite from the destination, then retain
the verified original copy before any recovery code makes journal transitions.

Start with internal-only networks and no provider egress, production Office
origin, signer publication, semantic scoring, deriver, worker, schedules or new
provisioning. Store/server startup can itself mutate state: inspect the deployed
Honcho lifespan and disable or defer background processing before its first boot.
Initial database/object/journal validation requires no API startup. Controller
foundation mode can expose retained run/cursor state without execution opt-in;
keep OAuth unmounted during this stage. A health read must not refresh or probe.

Required comparison receipts:

- Main database: migrations/checksums/schema/fences, every scoped table count,
  immutable retained evidence bytes, operation/start/dedup IDs, signed outbox
  bytes, cursor/reservation relationships, sequence values and relevant settings.
- Honcho: native and extension schema/version, native IDs, original resource
  receipts, session ownership, message/provenance/write receipts, and pending
  embedding/derivation queues. No create/remember/derive occurs to repair a gap.
- MinIO: complete private tree integrity plus authenticated isolated bucket and
  object/version/metadata comparison; ETag alone is not a universal content hash.
  Match retained object references from the restored relay database. Cold volume
  recovery with the pinned image remains a proof to execute, not a claimed
  portable MinIO export format.
- Redis: pinned parser/startup accepts the complete AOF without truncation or
  repair. Compare non-expiring logical data and absolute expiry evidence where
  needed. Expiring replay/presence keys can legitimately expire during downtime;
  raw post-start key counts are not an exact-data invariant.
- Hermes: SQLite integrity, original run/start keys and cancellation tombstones,
  dense event cursors, terminality and probe selection metadata; restored health
  is expired/unvalidated, even if an OAuth file exists. Verify public bindings
  match the same opaque refs and memory/provisioning receipts.
- Isolation: zero provider requests, zero production Office requests, zero new
  provisioning/semantic work and zero production daemon/volume access. Exercise
  no-blind-rerun recovery using fixture runs in separate disposable rehearsal
  state, never by resubmitting backed-up real prompts or delivery intents.

The later secret restore test may decrypt only into the new private destination,
verify allowed paths/owners and envelope integrity offline, then leave real
OAuth/signers inaccessible to running rehearsal components. A copied refresh
token must not be refreshed while the source enrollment remains in use. Actual
failover is a separate single-owner cutover: fence the source, choose current
explicit credentials or fresh login, invalidate old readiness witnesses and
revalidate runtime/memory/Office/signer gates before accepting new work. The
rehearsal never promotes itself or publishes a pending signed event.

## Implementation handoff and completion gate

The [executable preparation helper](../../runtime/private-stack/FULL_STACK_RECOVERY.md)
now freezes and revalidates selected container/native/file/schema authority and
an exact bounded capture/offline-restoration dependency plan. Fifteen focused
Python tests and actual read-only prepare/revalidate invocations passed. The
retained preparation is
`/private/tmp/ortak-private-20260905/recovery-preparations/24f52b64c1b948c3b6198bf735948de9/preparation.json`;
revalidation is
`/private/tmp/ortak-private-20260905/recovery-preparations/5790b1c4a85f44748ba0750fc4c60ae5/preparation.json`.
Both explicitly report no quiescence, snapshot or restore. An earlier attempt
refused unstable Docker mount-array order; its evidence remains retained and a
production-seam regression now verifies canonical order without dropping fields.

Remaining deliverables are terminal-session/launch-recipe binding, a
maintenance/drain witness, fixed-scope backup state machine, secret-envelope
handling, and a fresh isolated restore verifier. Extend safe primitives from the existing
[database helper](../../runtime/private-stack/DATABASE_BACKUP.md); do not weaken
that helper's current exact-target guard to make it a general arbitrary restore
command. The new Honcho/database destination inputs need independent canonical
allowlist and ownership checks.

Falsifiable tests must cover changed process/image/mount identity, old approved
inventory gaining a resource, interrupted drain and refresh, live SQLite/WAL,
AOF rewrite/truncation, missing MinIO metadata, cross-store generation mismatch,
Honcho native-ID replacement, expired witness, partial secret envelope, unsafe
archive paths/links, source-daemon reuse and a second restore attempt refusing
occupied destinations. Failed artifacts remain retained. Capture/drain/archive
tests remain future work; the preparation tests do not prove those modes.

Slice G remains open until one post-soak consistent bundle and actual fresh
restore pass the comparisons above, the source resumes safely, and the verified
artifacts plus secret recovery are demonstrated on an independent host/daemon.
The deployed Honcho background-writer inventory, current owning sessions and
coordinated maintenance/drain implementation remain preflight requirements.
