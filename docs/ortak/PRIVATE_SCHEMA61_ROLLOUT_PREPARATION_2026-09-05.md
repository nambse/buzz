# Private schema61/backend rollout preparation

The original 15:28 UTC / 18:28 Istanbul checkpoint below was preparation only.
The authorized rollout completed at 16:18 UTC / 19:18 Istanbul, 2026-09-05;
the selected private backend now runs schema61. Source now includes migration62
for a separate Work change, which this rollout did not deploy.

## Completed schema61 rollout

Root supplied the completed A–C build receipt
`/private/tmp/ortak-v0-evidence/backend-ac-artifacts-20260905.json` and authorized
the isolated backend rollout after the actual schema61 parity and deletion
gates passed. The receipt source SHA256 is
`96ab6aaf76ec72510bea577578f4b2298ddd0e35c9d369df6435bdf90ec20921`.
Its five binaries were copied with exclusive creation and no symlink following
to the new private directory
`/private/tmp/ortak-private-20260905/artifacts/backend-ac-20260905T160204Z-1c63001b83e84f72bc9a3485ee02020f`.
Every copied hash matched the source receipt during copy and again at completion.
Directory mode0700 and binary mode0500, all UID501; the mode0600 artifact
`receipt.json` records source paths/inodes, sizes and hashes. Later builds in
the shared target directory cannot replace these selected files.

| Copied binary | Bytes | SHA256 |
| --- | ---: | --- |
| `buzz-admin` | 32563872 | `cb5908860fd1d67e926a7526666cc94ca71ec55203f5b83de8cabb1edf50eae6` |
| `buzz-relay` | 126871984 | `5bdcd2862a9219325cb555f98abb67c9f0e2461058d86e679722b53063364858` |
| `ortak-cohort` | 13177728 | `1e40459b1c56585be9f1118a3958ee87a0f75cdb691c188d40de7e509ef68977` |
| `ortak-provision` | 26260352 | `3038157f8bc8b6b37c42ab5611090794b54cfbde6210eba616082ed95f75de62` |
| `ortak-server` | 27847952 | `d79505e400d525c283aa6bb54d8affbb74ad032f88a808aa2fb7ddcf53715c13` |

The pre-cutover source metadata still matched the earlier backup. Exact selected
API17461 and relay17426 identities were rediscovered and revalidated immediately
before SIGTERM; both exited without SIGKILL. With these writers stopped, another
fresh verified schema56 backup was retained. The unchanged native helper then
ran the copied `buzz-admin migrate` successfully and started only the copied
relay/API, with automatic migrations and central routing explicitly disabled.

| Running service | Ownership and listener witness |
| --- | --- |
| Relay | PID68255, UID501, session40350, started19:08:41 Istanbul; loaded copied `buzz-relay`, inode121484114; loopback3038/8089/9198 |
| API | PID68332, UID501, session85029, started19:08:52 Istanbul; loaded copied `ortak-server`, inode121484117; loopback8787 |

Both cwd values equal the exact private root. These foreground terminal sessions
remain running; their mode0600 logs are in the rollout directory below. PIDs and
loaded-file identities are observations, so any later signal must rediscover and
revalidate its target. The native a5ed app, all containers, old Cem/Zeynep resources,
Hermes OAuth/controller/worker and employee activation were untouched by this task.

Actual private schema checks found successful versions1–61 only, unchanged
checksums1–56, and exact source-file checksums for unchanged migrations57–61.
There are109 public tables; the six new tables were empty and the existing
employee remained draft. Only the migration table's row count changed at the
immediate post-migration checkpoint. Relay `/_liveness` and `/_readiness`
returned200; unauthenticated API `/api/v1/employees` returned401. The existing
exact private owner successfully read the one selected channel through
`buzz --format compact channels list`. An authenticated product API read remains
pending root's existing production-client validation; the relay read and401
probe are not claimed to cover it. The later16:47 UTC
[signed production-client acceptance](SIGNED_API_ACCEPTANCE_2026-09-05.md)
completed this API read gate, including real SSE abort/reconnect and exact
memory/Office receipt agreement. The original rollout receipt retains its
historical pending status.

| Verified backup | Cutover, writers stopped | Post-upgrade |
| --- | --- | --- |
| Directory under private `backups/` | `20260905T160654Z_7465312a840943fdab1a4a07676132b6` | `20260905T161716Z_f2d4e29921d248dda62d7d0081b246ce` |
| Archive bytes | 537977 | 587638 |
| Archive SHA256 | `c28f372dc76df9eb14ecc1ecc999c11385afea75f6ab473ebe5a81bd4b012cee` | `b78de69ea35dc6ac3679c9522d5ca208c4d75bd141786045733da6242a8cdd2d` |
| Fresh retained restore database | `ortak_verify_a429b2578e1f4242a65eedfa98c18083` | `ortak_verify_68b33ab2d7744aba93f42af5297d4e86` |
| Schema / public table count | 56 / 103 | 61 / 109 |

Both unchanged-helper manifests report `verified` with exact source/restored
metadata equality. The post-upgrade schema SHA256 is
`edea3088cd16e5473dec7f6f3dd65070841821e71a4d565ad50328d139a6b85f`.
All new archives, metadata and restore databases are retained; no existing
database was reset, replaced or dropped. Root held membership/activation writes
until this post-upgrade backup completed, then resumed its separate integration.

The complete owner-private rollout receipt is
`/private/tmp/ortak-private-20260905/rollouts/schema61-20260905T160452Z-e1c130f0eab34603906d7f2abe0048d5/receipt.json`.
Adjacent receipts record intent, verified stop identities, migration observation,
running process ownership, authenticated relay read and backup evidence.

## Historical preparation checkpoint

The following sections record the original 15:28 UTC observation, before the
rollout above. At that time no service stop/start, private migration, employee
activation or OAuth operation had been performed and the database was schema56.

## New verified database backup

The existing `scripts/ortak/backup_private_database.py` was used unchanged.
Root explicitly authorized its fresh retained verification database in the same
selected PostgreSQL container. The source `ortak` database remained read-only;
no existing database was reset, replaced or removed.

- Manifest: `/private/tmp/ortak-private-20260905/backups/20260905T152813Z_801d13f0fa074f87879d1c3d01ca0ad4/manifest.json`
- Archive: `database.dump` in that private directory, 537,977 bytes.
- Archive SHA256: `5555ac87ab267bfd9f1f04e6e24b6c2b541b1e153844cf82b6796491bbb8b890`.
- Retained restore database: `ortak_verify_d35e4382bcc745ca94c2919fa24d6f26`.
- Exact comparison passed for 103 public table counts, successful migration
  checksums 1–56, employee states and selected schema catalog.
- Schema SHA256: `8c78de1551cd2bba299b7919cdf3e2cccff4749f4113231c46f0050a8c9c42d8`.
- One company/draft employee, one project/manual Work item; zero employee
  revisions, runs, routing decisions, outbox rows and provisioning operations.
- Directory mode0700 UID501; every helper output is a nonsymlink regular
  mode0600 file owned by UID501. The private archive must remain local/private.
- Helper SHA256: `e1d2f19b2645051e8f0125723d472ef365ab203dcb72c5de13c9de19a48115b9`.

The helper verified immutable source container
`01ad59c9f79fd50e47281ef85b829fb2a8d556f627a43b175e36fc8ecfde53c7`,
Compose project/service ownership and named volume
`ortak-private-20260905_postgres_data`. It used the pinned PostgreSQL17.6 image
below, an exported read-only repeatable-read snapshot and the shared schema
destruction lock. The dump and source counts used the same snapshot. Restore
used a newly generated name and a single transaction. All earlier archives and
verification databases remain retained. This is database-only recovery evidence;
the scope and limits in [DATABASE_BACKUP.md](../../runtime/private-stack/DATABASE_BACKUP.md)
still apply.

## Current process and container identities

The exact private root marker passed; root mode0700 UID501. PIDs were rediscovered
and matched against private-root cwd, executable path, start time and listeners.
They are observations, not durable authority for a later signal.

| Service | Current evidence |
| --- | --- |
| Relay | PID17426, UID501, started11:34:31 Istanbul; `/private/tmp/ortak-root-build-target/debug/buzz-relay`; listens127.0.0.1:3038/8089/9198 |
| API | PID17461, UID501, started11:34:32; `/private/tmp/ortak-root-build-target/debug/ortak-server`; listens127.0.0.1:8787 |
| Existing native | PID18023, started11:36:07; exact old a5ed `Ortak Private.app` bundle; left untouched |
| PostgreSQL | `ortak-private-20260905-postgres-1`, running/healthy, loopback55433; image `sha256:ef257d85f76e48da1c64832459b59fcaba1a4dac97bf5d7450c77753542eee94` |
| Redis | `ortak-private-20260905-redis-1`, running/healthy, loopback56382; image `sha256:ff02b58f971e7d7d156a1267e283fcbbeee91773b6aa36c49dac28ecfe28eadf` |
| MinIO | `ortak-private-20260905-minio-1`, running, loopback9008; image `sha256:e1d7f7262c86498b45f869bcc7e3bbe7c11b3c026d9aad25f7759b053fd60a41` |
| Honcho | `ortak-honcho-api-check-20260905`, running, loopback8009; image `sha256:cc8b4a29c0adda08978886e205ff5c5ff0a13923e4ed15e1626b24194d0c0c21` |
| Hermes8650 | No listener at this checkpoint. No connection was attempted; root's active OAuth login/session was not inspected or touched. |

Relay `/_liveness` and `/_readiness` returned200; unauthenticated API
`/api/v1/employees` returned401. MinIO `/minio/health/ready` and Honcho `/health`
returned200. These are liveness/auth observations, not authenticated workflow or
current employee/provider readiness.

Current backend file hashes (still pre-upgrade):

| File | Bytes | SHA256 |
| --- | ---: | --- |
| `buzz-relay` | 146672952 | `07248188450b135d58c464d82b05d4a0ced9afbaac5f755606c87f370fe3b17a` |
| `ortak-server` | 28624064 | `8ee9f28468edc70c17ebc5bf5176eac4e2d8a9a1bfe216fd69491e287222422e` |
| `buzz-admin` | 37513240 | `0f31b0fb97818139f51a096e543d89058812a40d9d8865b920734a0c8f56e185` |

Relay/API loaded text inodes121295006/121295807 matched those current files.
This does not attest a future file replacement. Preserve the old selected
backend files and their receipt before overwriting that build output directory.

## Historical proposed next steps

These proposed commands are retained as preparation history, not current restart
instructions. The actual rollout selected the immutable private artifact directory
recorded above, rather than the earlier shared build directory shown below.

First complete source61 migration/desired-state parity and real disposable
deletion/retention tests, then build and record new backend/admin artifacts.
The currently observed binaries above cannot be treated as schema61 artifacts.
No build or deployment command below was run by this preparation task.

Recheck exact service identity and stop only the selected foreground relay/API
through their owning sessions, following [OPERATIONS.md](../../runtime/private-stack/OPERATIONS.md).
The old native app, Cem/Zeynep resources and OAuth login are outside this backend
stop operation. Keep selected writers/DDL paused during migration. A fresh
backup is now available; if writes resume or the rollout is delayed, obtain a
new snapshot at the actual cutover.

After the build receipt proves that the selected `buzz-admin` at this path embeds
the unchanged migrations1–60 plus new61, the exact existing migration command is:

```sh
python3 scripts/ortak/private_native_services.py \
  --state-dir /private/tmp/ortak-private-20260905 \
  --binary-dir /private/tmp/ortak-root-build-target/debug migrate
```

The helper reconstructs the private database/signer environment without printing
secrets. It does not need or access Hermes OAuth state. Start the newly verified
backend artifacts in separate owned foreground sessions:

```sh
python3 scripts/ortak/private_native_services.py \
  --state-dir /private/tmp/ortak-private-20260905 \
  --binary-dir /private/tmp/ortak-root-build-target/debug relay
```

```sh
python3 scripts/ortak/private_native_services.py \
  --state-dir /private/tmp/ortak-private-20260905 \
  --binary-dir /private/tmp/ortak-root-build-target/debug api
```

The helper explicitly sets `BUZZ_AUTO_MIGRATE=false` and central routing off.
Worker provisioning, configured OAuth readiness and cohort capture/activation
remain separately controlled integration steps; these launch commands do not
enable them. After verified schema61/backend health and authenticated API
acceptance, run the same backup helper again and retain the post-upgrade receipt:

```sh
python3 scripts/ortak/backup_private_database.py \
  --state-dir /private/tmp/ortak-private-20260905
```

Do not roll the original database backward or restore over it as an automatic
binary rollback. The verified archive is a recovery input for an explicitly
selected fresh database; the existing source and verification databases remain
preserved until a concrete recovery operation is chosen.
