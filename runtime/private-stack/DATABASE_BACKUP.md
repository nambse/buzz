# Private PostgreSQL backup verification

This helper backs up **only the main PostgreSQL database** of the fresh dated
private stack. It does not establish full-stack disaster recovery or complete
remaining-work slice G. MinIO objects, Honcho's separate database, Redis state,
Hermes/bridge journals and profiles, application configuration, private keys,
credentials, global PostgreSQL roles/tablespaces, and an independent-host restore
still need their own backup and recovery procedures.

Run from the repository root after the marked private stack has been initialized,
migrations through 0052 have succeeded, and the private company has been
bootstrapped. Keep schema migrations/DDL paused for this short operation.
Ordinary row writes may continue; source verification counts and the dump use
the same exported PostgreSQL snapshot.

```sh
python3 scripts/ortak/backup_private_database.py \
  --state-dir /private/tmp/ortak-private-20260905
```

The helper has no destination-container, source-database, restore-file, cleanup,
or overwrite option. It connects through the fixed local Docker Unix socket
`/Users/nambse/.docker/run/docker.sock`, ignoring ambient Docker contexts,
`DOCKER_HOST`, PostgreSQL, credential and proxy variables. Before any database
command, it verifies the exact `ortak-private-20260905-postgres-1` container,
Compose project/service labels, repository-pinned PostgreSQL 17.6 image, and
named volume's project ownership, local driver and mount point. Subsequent
commands address that immutable container ID. Database clients use the
container's local PostgreSQL socket and explicit `ortak` user/database; no
password is passed in arguments or printed.

Each invocation creates a new mode0700 directory below
`/private/tmp/ortak-private-20260905/backups/`. Its dump, intent, final manifest,
SQL inputs and bounded diagnostic files are mode0600. **The archive contains
private table data and may contain secrets.** Keep the entire directory private;
do not commit or attach it to reports. Encryption and off-device retention are
not provided by this local verification helper.

The operation:

1. Journals the source and a fresh `ortak_verify_<32 lowercase hex>` database
   name before issuing creation. Existing names are never adopted or replaced.
2. Holds a read-only repeatable-read transaction and the coordinated schema
   destruction shared fence. It exports a snapshot, reads source counts and
   schema evidence from that snapshot, then creates a custom-format `pg_dump`
   using `--snapshot`.
3. Creates the new verification database from `template0` in this same fresh
   container. It invokes `pg_restore --exit-on-error --single-transaction`
   against that generated name. It never uses `--clean`, `--create`, `dropdb`,
   or restoration to the original database.
4. Compares every public ordinary/partitioned table's row count, successful
   SQLx migration versions/checksums, private-company count, employee lifecycle
   counts, server version and the selected catalog SHA256. The catalog digest
   covers relation/column definitions, constraints, indexes, user triggers,
   functions, view definitions, sequence definitions, policies and extensions.
   Live-column ordinals retain semantic order while ignoring physical holes from
   dropped columns, which PostgreSQL compacts when dumping/restoring. Component
   hashes and sanitized restored metadata identify differing fields on failure.
   It does not compare every row's bytes, sequence current values, database-level
   settings or every PostgreSQL catalog property. The custom archive has its own
   SHA256 and byte count.
5. Records `verified` only after exact comparison. The verification database
   remains available for inspection, with no worker or service pointed at it.

The archive is capped at 256 MiB, each diagnostic stream at 64 KiB, metadata at
1 MiB and table inventory at 2,048 entries. The whole command has a 120-second
deadline and container-side command deadlines; locks wait at most two seconds.
Disk capacity is checked against the source's physical size plus archive budget.
A command error, warning, contention, timeout, size limit or comparison mismatch
refuses verification. Any completed archive and a private failed manifest remain
available. A partially created verification database is retained, never removed
automatically. An interrupted container-side snapshot/client also has a finite
timeout; no previous database or external test stack is cleaned up on failure.

A fresh successful invocation is a new recovery observation, not permission to
activate employees, start the worker or claim provider health. Record the
manifest's public checksum/version/count evidence separately from the archive.
The default unit suite below does not execute Docker backup/restore; the separate
actual verification receipt follows it.

```sh
python3 scripts/ortak/test_backup_private_database.py
```

The focused tests exercise the helper's production command bounds and state
machine: private outputs, snapshot handoff, fresh restore destination, retained
failure artifacts, byte/time limits, exited-parent process-group cleanup,
diagnostic privacy, remote-environment rejection, invalid counts, and mismatched
container/volume ownership.

PostgreSQL's documentation explains [custom-format dumps and synchronized
snapshots](https://www.postgresql.org/docs/17/app-pgdump.html),
[single-transaction restoration](https://www.postgresql.org/docs/17/app-pgrestore.html),
and [snapshot export lifetime](https://www.postgresql.org/docs/17/functions-admin.html#FUNCTIONS-SNAPSHOT-SYNCHRONIZATION).

An additional actual SQL regression runs only when an explicit retained
verification database is selected. It creates three fresh probe tables inside a
transaction, compares the exact production column query, then rolls the DDL back:
a dropped-column hole must compare equal to its compact equivalent, while
reordered live columns must differ. It refuses the original database name.

```sh
ORTAK_BACKUP_SQL_TEST_DATABASE=ortak_verify_<32_lowercase_hex_from_manifest> \
  python3 scripts/ortak/test_backup_private_database.py
```

## Actual database verification receipt — 2026-09-05

The later10:14 Istanbul invocation also completed with `status: verified` after
the0054 upgrade and retained manual Work workflow. Directory:
`/private/tmp/ortak-private-20260905/backups/20260905T071413Z_9605df7a4ddc4795a342e62090b381fd`.
Its523,975-byte archive has SHA256
`1cb330c41326efbaac1179661e542061bf9917134a7997034feb940def0bf265`.
The retained restore is `ortak_verify_89a61d38d0e84f69a648682933a20de1`.
All103 table counts and migrations through0054 matched, including one project,
one completed item, seven history rows and eight operation receipts. The schema
SHA256 is `c545e95082c57a2cc77df3574c9e69af1cadf1be45c091ee7f7f9d93d8f4f5cc`.
No runs or routing dispatches exist; the sole employee remains draft. This
supersedes the older schema/count observation below without removing its backup.

The invocation beginning at 08:48:01 Istanbul completed with `status: verified`
and `database_only: true`. Its private directory is
`/private/tmp/ortak-private-20260905/backups/20260905T054801Z_2d3955c2f811470b919efd32654d4555`.
The custom archive is **502,264 bytes**, with SHA256
`146ed286d7e71dd82c0ee82b313b0037b2827112511cf6e7cfe0300cab43fa47`.
The retained verification database is
`ortak_verify_265600fcd735460c8c14ccdb0fd55ae4` in the same inspected fresh
PostgreSQL 17.6 container.

All **100 public table counts**, all successful migration checksums through
**0052**, and the selected schema catalog matched the exported source snapshot.
The schema SHA256 is
`07139b29b4777e67f4f6188d514c82dbb8238306d9a190bd39cc57ba5a17e011`.
The actual SQL dropped-column ordinal regression ran against this retained
verification database and rolled its probe DDL back. Together with the eight
unit cases, **nine tests passed in 0.602 seconds**.

An earlier verification attempt correctly refused a catalog mismatch: physical
column-number holes survived in the source but were compacted by `pg_dump`.
The comparison now preserves live column order using contiguous ordinals; the
SQL regression also proves reordered columns still differ. That earlier failed
archive, manifest and verification database remain retained. Neither attempt
restored over, dropped or changed the original `ortak` database or any older
external stack. The successful database receipt does not cover the other stores,
secret recovery, independent-host recovery, service restart or employee activation.

## Migration55 private checkpoint

The final55 private database backup restored successfully into the new retained
`ortak_verify_bab3b49077284126a479dc84b19c79d7` verification database. Receipt:
`/private/tmp/ortak-private-20260905/backups/20260905T075300Z_efb5da81275f4a688644bce108605676/manifest.json`.
The archive has528329 bytes and SHA256
`8390c1772538316acea047cb3e42f9114e62c8f4274e478b2618e77cb8fcbf51`.
All103 table counts, migration checksums through55 and the schema matched.
The preserved scope contains one project, one completed manual item, eight
operation receipts, seven history rows and zero runs/dispatches/decisions.
This is database-only evidence; previous archives and verification databases
remain retained.

## Migration56 private checkpoint

The11:35 Istanbul backup restored into the new retained verification database
`ortak_verify_7a359a24f12a4a8795768df594c74f84`. Receipt:
`/private/tmp/ortak-private-20260905/backups/20260905T083500Z_952d0c34d48f462ba1d3268d872a5438/manifest.json`.
The537,977-byte archive has SHA256
`e737171d4fa1177edba41c26d03b98a0dc48ec0a23952550e1ca2948ee6b9154`.
All103 table counts, successful migration checksums1–56 and the selected schema
catalog match. Schema SHA256:
`8c78de1551cd2bba299b7919cdf3e2cccff4749f4113231c46f0050a8c9c42d8`.
The snapshot preserves one company/draft employee, one project/completed manual
item, eight operation receipts and seven work history rows. Assignments,
employee revisions, runs, dispatch outbox and routing decisions remain empty.
This is database-only recovery evidence; every earlier backup remains retained.
