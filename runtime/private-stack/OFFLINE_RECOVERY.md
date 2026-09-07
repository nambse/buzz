# Offline storage recovery

`restore_private_recovery.py` restores a sealed selected bundle into new offline
storage destinations. It never replaces a source, runs an application/controller,
mounts a Docker socket, publishes a host port, refreshes OAuth or sends a
provider/Office request. Every destination has a generated UUID and exact
ownership labels. It refuses existing volumes/databases and retains failed
artifacts. PostgreSQL proof containers stop gracefully after validation; their
containers, databases and volumes remain retained.

The actual captured-bundle command is:

```sh
python3 scripts/ortak/restore_private_recovery.py \
  --bundle /private/tmp/ortak-private-20260905/recovery-bundles/<capture-UUID>/manifest.json
```

No real full-stack capture exists yet. Run this only after root completes the
[held capture operation](FULL_STACK_RECOVERY.md). Fixture bundles use a separate
directory/marker and cannot enter that production CLI path.

## What is verified

All payload hashes, sizes, selected filenames and the sealed manifest are checked
before opening the recovery key or creating Docker resources. The key must be
the exact separate mode0600 `recovery-keys/<capture-UUID>.key`; no key/profile
discovery is implemented. AES-GCM associated data binds the operation, every
component and the frozen secret-file generation. The exact archive allowlist is
decrypted into a new private tree. Modes, ownership and lengths are verified;
secret contents and individual secret hashes never become log fields. Provider
health stays unvalidated even if backed-up credentials decrypt successfully.

Schema68/69 bundles additionally bind the exact probe and export primary keys
to complete-row hashes. Restoration compares that frozen witness even if time
has advanced and a retained withdrawal is now due. It never executes the job or
invents an erasure ACK. Schema69 requires the full Honcho D2a header/content/
tombstone/operation family, verified with scoped lifecycle and content-hash
checks. Restored activation remains closed until the original writers are
contained, same-key external state is reconciled, and retained expiry/withdrawal
work catches up through an explicitly approved activation operation.

Each PostgreSQL target uses its captured immutable image, a fresh labeled volume,
network `none`, no published ports and a Unix-socket-only verifier. Only its own
restored PostgreSQL password file is mounted; OAuth/employee/Office material is
never mounted into a storage process. Source database names and logical resource
IDs are retained inside these isolated containers. Template0 databases are
created once and `pg_restore --exit-on-error --single-transaction` runs without
clean/create/drop flags. Main restoration uses the shared
[populated credential-array compatibility guard](DATABASE_BACKUP.md#populated-credential-array-restore-compatibility):
pre-data/data/post-data sections, a strictly allowlisted temporary target function
search path, then exact original function configuration/catalog restoration.
Schema, migration checksums, table counts, complete
logical row hashes, role attributes, database settings and sequence values must
equal the captured evidence. Locale/provider or settings outside the implemented
exact contract fail closed. `createdb` locale/owner/template options follow
[PostgreSQL's CLI contract](https://www.postgresql.org/docs/17/app-createdb.html);
a nondefault connection limit uses the supported
[ALTER DATABASE option](https://www.postgresql.org/docs/17/sql-alterdatabase.html).

Redis/MinIO archives are streamed into new empty labeled Linux volumes by a
pinned networkless helper. Path traversal, aliases, duplicate names, links,
devices, unsupported metadata and size/count excess refuse. The restored tree is
read again and compared by a digest covering every path, empty directory, mode,
UID/GID, timestamp and file byte. The reviewed MinIO `user.total_writes` xattr is
preserved as canonical base64 in an explicit PAX field, capped at 256 bytes and
included in the tree digest. Other xattrs refuse. Each newly restored file and
directory's metadata is fsynced. This stage does not start Redis or MinIO;
their separate synthetic application-semantic fixture follows below.

SQLite is copied as an immutable artifact and inspected through a disposable
working copy. The working copy opens RW with `PRAGMA query_only=ON` so SQLite can
create absent WAL/SHM metadata even when the backup has a WAL-mode header but no
sidecars. The original artifact never changes. Every table's count/logical bytes,
schema, dense cursor rules, foreign keys, tombstones and
`private_failure_diagnostics` size/count bounds are checked. Cold WAL/SHM evidence
is retained separately. No `Journal()` constructor or `immutable=1` shortcut is
used. Public configs/native artifacts are restored as inert files and never
executed. Existing image artifacts are verified without another image export or
duplicate image-archive copy; missing imported images refuse.

## Per-component destination groups

For a selected present native ciphertext store, restoration preserves the exact
source UID/GID as well as modes, timestamps and encrypted bytes. The native
extractor applies mode and timestamp but relies on the destination filesystem's
group inheritance; it does not remap ownership. A preflight now validates the
pinned native archive manifest and requires the fresh output directory's GID to
match every archived root/file GID before decryption or PostgreSQL/volume target
creation. An absent native store has no group requirement. The metadata-only
`native-confidential-destination.json` records the comparison; mismatches refuse
with `offline_native_destination_group_mismatch`. Final extraction retains all
exact metadata and byte checks.

The workspace component may have a different group. Its hash-pinned manifest is
also checked before decryption or database creation. `workspace-destination-plan.json`
selects only the fresh output or the fixed, canonical, current-UID/0700 `STATE`
directory. For a uniform archive group the selected parent must have that exact
GID; mixed groups must all be inherited or available to the current actor. An
unavailable group or filesystem boundary refuses before expensive restoration.

If `STATE` supplies the workspace group, this macOS operator creates one fresh
empty 0700 staging directory there, then moves that same inode into the fresh
output using descriptor-relative `renameatx_np(RENAME_EXCL)`. The move cannot
overwrite even an empty destination. The intent, staged inode, completion or
failure are retained under `workspace-destination-*.json`; a failed staged path
is not removed. The watched workspace extractor still checks every original
UID/GID, mode, timestamp and byte. Native ciphertext uses its own output group.

No automatic `chgrp`, recursive ownership repair or metadata normalization is
performed. An operator-approved parent group correction must preserve existing
children and original app data, record before/after ownership and identity, and
be followed by a fresh restore UUID. Changing a common parent does not prove all
components can inherit their distinct archived groups; the per-component
preflight remains mandatory.

## Owned fixture rehearsal completed

The explicit fixture command builds synthetic secret/OAuth/volume/native/journal
data around the two exact earlier retained verified database archives:

```sh
python3 scripts/ortak/rehearse_private_recovery.py --execute-owned-fixture
```

The successful rehearsal receipt is:
`/private/tmp/ortak-private-20260905/recovery-offline-restores/701aa0b7241943d6afd24ae7a9164511/manifest.json`.
Its digest is
`19c41a0fd431c4046cfa848c9236878b10701d08e3842bc009e116946b06929d`.
The fixture bundle is
`/private/tmp/ortak-private-20260905/recovery-fixture-bundles/ff08003827184af6997285be1cda36d2/manifest.json`.
Its separate fixture key remains under `recovery-fixture-keys/`; no real OAuth
or source credential file was read by this rehearsal.

- Main: schema61, 109 tables, complete row/schema/count/settings/sequence checks;
  retained stopped container `d4b81d66eddcca40d8523b2a1aed85378db935f7a461ca153cb70dcd1f393a75`.
- Honcho: 15 native+extension tables and pgvector 0.8.6 with the same comparisons;
  retained stopped container `a4a5da8d7f2d704421980313c63ae8a6f25051a7933f445bd02f693f723eefae`.
- Fresh Redis/MinIO volumes: one synthetic 23-byte file and three directories
  each; complete byte/metadata tree comparisons pass. These are archive
  restoration fixtures, not real AOF or MinIO objects.
- SQLite: one run, one event, one private diagnostic and one retained tombstone;
  all logical bytes and cursor/integrity checks match. Synthetic OAuth and
  database passwords decrypt with the exact separate fixture key.

Earlier failed attempts remain retained. They exposed Docker Desktop's exact
`/host_mnt` alias, a deprecated Docker stop flag, an unsupported createdb flag and
the SQLite missing-WAL working-metadata case. The source fixes refuse arbitrary
mount remapping and retain strict post-stop identity/exit checks. Only generated
offline fixture PostgreSQL containers were stopped; no source service was
paused or restarted.

The current broader private Python lane has 120 tests: 119 pass and one explicit
retained-database test is skipped by default. The offline module contributes 11
tests for authenticated binding, key handling, archive bounds/escape/overwrite,
actual diagnostic/tombstone bytes, WAL-without-sidecars, supported database CLI
flags, current-owner-only retained stop and bounded xattr preservation/refusal.

## Redis and MinIO semantic fixtures

```sh
python3 scripts/ortak/rehearse_private_recovery_services.py --execute-owned-fixtures
```

This explicit command creates only fresh UUID-named volumes and storage servers.
The seed server runs with network `none`, no host ports, read-only root, bounded
memory/PIDs and no Docker socket. After writing public fixture data it stops
gracefully, the production cold-volume reader archives its volume, and the
production bounded extractor restores a different new volume. Only this new
restored storage server starts for application checks. Its startup can update
its own storage metadata; cold byte/tree equality is established before startup.
Both seed and restored servers stop afterward and remain retained. Source
volumes, services, credentials, OAuth, provider and Office are never accessed.

Redis uses real multipart AOF with `appendfsync always` and
`aof-load-truncated no`. The fixture checks persistent data, a counter replayed
exactly twice, hash metadata, a key expired during downtime and a surviving key
with the same absolute millisecond expiration. Expiry must never be restarted
as a fresh TTL. This exercises Redis's
[AOF persistence model](https://redis.io/docs/latest/operate/oss_and_stack/management/persistence/)
without attempting AOF repair or losing the base/increment manifest.

MinIO uses only fresh synthetic credential files. Installed curl's
[AWS SigV4 signer](https://curl.se/docs/manpage.html#--aws-sigv4) receives its
credential config through stdin, inside a helper sharing only the new MinIO
server's network-none namespace. There is no custom signer or SDK installation.
The fixture enables versioning, writes two object versions and creates a delete
marker. Read-only checks on the restored server require the same version IDs,
delete marker, exact body digests/lengths and custom metadata; latest GET must
remain deleted. No credential appears in argv, logs, reports or inherited env.

Actual semantic receipt:
`/private/tmp/ortak-private-20260905/recovery-service-fixtures/e01814fa91ae4d6c83c0e992c8c2240d/manifest.json`.
Sealed manifest SHA256:
`f9241645ef133f0b27fd0513b608cd059f4471571f83d6f0e99fb33fbb9973f5`.

- Redis: three AOF files, two directories, 652 data bytes; restored tree SHA
  `e4ea4243e6101e784a47f57eef03c14280eaa3a2fbe9af4a1c4085bb25100c07`.
  Four surviving keys, counter2 and original absolute expiry verified.
- MinIO: eight files, eighteen directories, 14605 data bytes; restored tree SHA
  `74cc3fe251ea9a9b313bc01ccf56edeccc19a76e72cc6a9fd0a95012ef96a236`.
  Two versions, one delete marker, body/metadata and the observed write-counter
  xattr preserved by the same reader/extractor used for real capture.
- All four generated storage servers are stopped with clean exit and retained.
  Seven focused tests bind isolation, exact owner checks, cold-source refusal,
  AOF expiry/counter semantics and the real curl/S3 verification paths.

The earlier fixture `3ec062dde66f4ae899f96ad4d0febac5` remains retained. Redis
passed; MinIO's cold reader refused its observed xattr. That failure led to the
narrow metadata-preserving change above. No source or failed target was repaired.

Real current-stack capture and semantic checks on its restored data,
independent-host restoration and actual single-owner failover remain
open. `offline_foundation_verified` must never be interpreted as provider/runtime
activation readiness or a completed disaster-recovery acceptance.
