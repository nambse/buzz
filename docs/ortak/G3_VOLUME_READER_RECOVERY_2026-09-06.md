# G3 failure and actual G4 recovery

The current stack subsequently completed populated schema74/local-journal-volume
capture and offline restoration. See the
[G74 result and current recovery boundary](CURRENT_PRIVATE_RECOVERY74_2026-09-06.md).
This document retains G3/G4's schema69 evidence; its owner IDs and exact commands
remain historical and do not authorize a later operation.

## Historical checkpoint: G4 capture and offline restore passed

Root executed the reviewed G4 operation
`cd6d2150e4eb417a8858369824e88735`. The actual selected schema69/native6ff
stack capture completed with status `captured`:
`/private/tmp/ortak-private-20260905/recovery-bundles/233e81719c8644beaefba324e6683fc7/manifest.json`.
Both databases, Redis/MinIO volume archives, the SQLite journal, immutable
artifacts/images and the encrypted secret package were captured under the
reviewed barriers. This supersedes the earlier preparation-only status below;
G1/G2/G3 failures and their evidence remain retained.

Root resumed the same original services. The read-only validator returned
`passed`, with relay19787/session38441, API19799/77485,
management19810/13725, worker19824/57323 and native19857/60853.
Both PostgreSQL processes remained unchanged, health was200/200/401, and all16
scoped persistence hashes/counts matched. Receipt:
`/private/tmp/ortak-private-20260905/recovery-operations/cd6d2150e4eb417a8858369824e88735/source-resume-f7d2b6a99e124a18901a4f0f88f4ff6a/validation-5a03238d344141b4b82e9afd8f70bd8a/receipt.json`.
The validator made no signed mutation or provider request and did not assert a
new native UI acceptance.

Root then restored this exact bundle into fresh retained offline destinations.
Status `offline_foundation_verified`, with `runtime_activation:false`, is
recorded at
`/private/tmp/ortak-private-20260905/recovery-offline-restores/19271dbb0d0649b393b8733a71482941/manifest.json`.
Main/Honcho metadata, rows and settings, complete volume trees, journal rows,
artifacts, images and private configuration recovery passed. Restored execution
stays disabled; this is a same-host storage recovery rehearsal. Independent-host
disaster recovery is unverified. Redis AOF/expiry and MinIO application behavior
were not exercised against these actual restored source volumes; the separate
owned Redis/S3/permanent-DELETE fixture below proves that mechanism only.
Schema73, Honcho D2c, semantic scoring and C2 remain outside this G4 result.

## Historical G3 failure and G4 preparation

G3's actual pause passed. Bundle `c0f6b31cb3f845fea09be764e0fbee85`
failed during `volumes`, after both database backups and their complete
schema/row/settings/sequence checks had completed. Redis produced its complete
six-member archive. MinIO stopped after116 members with exit3 and empty stderr;
the former reader hid its refusal reason. The failed bundle, original20-file
operator closure, readers and all database targets remain retained.

Root confirmed reader/lease process containment and absence of the exact schema
lock owner, then resumed the same original services. Read-only validation passed:
relay9858/session4661, API9869/68486, management9878/24778, worker9894/90236,
native9935/55115. Both PostgreSQL services stayed up. The immutable backend69,
native6ff, Hermes9335/dbc9 with worker8ee, and Honchoad579/febea remain selected.
Receipt:
`/private/tmp/ortak-private-20260905/recovery-operations/15cc30dc9d3147979876a83b4056acb4/source-resume-7b828cb275814e6fb39002dea24e603a/validation-30ad9c7e87854ed6a48620532150a7af/receipt.json`.
Loaded hashes/inodes/start/cwd were checked twice, health was200/200/401,
and16 scoped table row hashes/counts plus the completed Work, active Ada,
withdrawn fact and Honcho no-text/tombstone receipts were unchanged. No provider
request, signed API mutation or new native UI action was performed by validation.

## Exact metadata cause and repair

Root authorized a bounded read-only metadata inspection of `.minio.sys` immediate
children, without reading file contents. The selected `format.json` is a regular
single-link0644 file owned by10001. It contains both `user.total_writes` and
`user.total_deletes`, each exactly8 bytes. The former reader only allowed writes.
Metadata receipt:
`/private/tmp/ortak-v0-evidence/g3-minio-metadata-b1126d2c663748f498ec49268288d199/receipt.json`.
The exact dbc9 helper used a read-only volume mount, networknone, no Docker socket;
its exit0/pid0 was verified. No live metadata value or file content was changed.

The two names and eight-byte encoding match MinIO's
[storage counter implementation](https://github.com/minio/minio/blob/master/cmd/xl-storage.go).
MinIO treats a delete marker as a write; a permanent version deletion updates the
[delete counter](https://github.com/minio/minio/blob/master/cmd/xl-storage-disk-id-check.go).
Those public sources were observed for explanation; no floating source or image
was imported. The selected installed image remains the tested authority.

The source reader, PAX decoder, extractor and complete-tree digest now preserve
exactly these two8-byte attributes. Unknown names and every other width refuse.
Attribute values remain part of the full tree hash. Reader failure diagnostics
contain only fixed code/phase/kind fields; paths, values and arbitrary exception
messages cannot enter the public manifest. Existing fresh-exclusive outputs,
link refusal, deadlines, byte limits and failure-marker restore fencing remain.

## Actual isolated gates

* `recovery-service-fixtures/dbc628755a7d4de0b9dacfaf6fb07434/manifest.json`
  beneath the selected private state: real MinIO PUT/versioning, a temporary
  version's permanent DELETE, a retained delete marker, both8-byte counters,
  exact dbc9 cold reader, fresh volume restore/full-tree equality, then S3
  version/body/metadata verification. Redis AOF/counter/hash/absolute-expiry
  semantics passed in a separate fresh volume. All fixture services stopped
  and remained retained; no live source volume or credential was used.
* `/private/tmp/ortak-v0-evidence/g-volume-reader-guards-54a8bb75e2234cc596c2d8a9e021122d/receipt.json`:
  six actual installed dbc9 reader cases on one newly generated volume. Both
  exact counters pass; an unknown name and7/9-byte write/delete counters refuse
  with fixed diagnostics. Every reader's image, exit and pid0 were checked.
* `/private/tmp/ortak-v0-evidence/g-capture-tail-2fd5a5215bbb4700bd5bb42667bfccf8/receipt.json`:
  production `capture` sequencing and actual `Capture.volumes`, `journal`,
  `public_artifacts`, `images`, `secrets` and sealing passed. It used those cold
  fixture volumes, committed SQLite WAL without SHM plus diagnostic rows,
  an inert seven-entry synthetic app and repositories/resume/operator files,
  actual8ee+dbc OCI export (24 descriptor blobs,503601605 verified bytes),
  and14 invalid synthetic secret leaves plus one settings member encrypted
  with AES-GCM and a separate local0600 key. Database acquisition, live barrier
  admission, native identity and source configuration/credentials were explicit
  synthetic boundaries. The fixture never qualified as a live full capture.

The tail's first post-validation assertion confused an OCI manifest ID with a
config digest. The completed production capture and export were retained, a
failure receipt was added, and corrected index→manifest→config/layer validation
passed against the same export without repeating it. Fixture limits remained
the production component limits: cold reader120s/100000 entries, whole capture900s,
256MiB Redis,2GiB MinIO,64MiB SQLite,512MiB native trees,8GiB image exports,
and32MiB configuration/secret package. The independent OCI verifier is bounded
to60s,256 blobs and8GiB; no restored executor or provider was started.

G4 read-only preparation `5046b48146e74fe580feed1187a39293` and registry
`cd6d2150e4eb417a8858369824e88735` passed for these owners and operators.
The owners digest is
`cef637038684046e41e8aae8a3a7cd116b4f4b4973088e1295f122c41ff1674d`.
All20 frozen files were verified by the unchanged separately pinned root pause
helper; five changed operator hashes and the exact root commands are retained
in `pause-helper-selection.json` and `ROOT_CAPTURE_RECIPE.md` in that operation.
The final focused Python gate passed130 tests with one existing opt-in database
test skipped (131 total). Preparation observed25GiB free disk. No G4 live
pause/capture was performed by this subtask; execution remains with root.

At that preparation checkpoint, G1/G2/G3 were historical failed captures and
the actual G4 capture/offline restore had not yet run. Their later success is
recorded in the current checkpoint above. Schema73, Honcho D2c, semantic scoring
and C2 additions remain outside G4's exact69/6ff scope.
