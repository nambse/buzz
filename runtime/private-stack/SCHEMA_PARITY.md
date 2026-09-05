# Disposable migration 56 schema parity

This check is separate from the private application database. It accepts only
an explicit `ORTAK_SCHEMA_PARITY_TEST_URL` with the literal host
`127.0.0.1`, port `55432`, a database name and selected test credentials. It
rejects port `55433`, port `5432`, alternate hosts, URL query parameters and
missing selection. It does not fall back to `DATABASE_URL` or inspect private
stack credentials. Supply the selected URL through the environment without
printing it or putting it in command arguments.

Prerequisites are Python >=3.11 with `psycopg2`, the already installed
`/Users/nambse/Library/Caches/hermit/pkg/pgschema-1.7.4/pgschema`, and an explicitly
selected compiled `buzz-db` test executable. The current root cache contains
`/private/tmp/ortak-root-build-target/debug/deps/buzz_db-685e65fc72ce9e7e`.
The final migration 55 executable was compiled and used on 2026-09-05. For the
current check, select a freshly rebuilt migration 56 executable; the receipt hashes
the actual executable. No build or installation occurs in this helper.

With an existing, canonical, owner-private mode 0700 receipt parent and the
selected environment already supplied, run from the repository root:

```sh
/Users/nambse/.pyenv/versions/3.12.8/bin/python3 scripts/ortak/check_schema_parity.py \
  --migration-test-binary /private/tmp/ortak-root-build-target/debug/deps/buzz_db-685e65fc72ce9e7e \
  --receipt-parent /private/tmp/ortak-schema-parity-receipts
```

The helper records fresh random database names before creation, applies a
protected snapshot of `schema/schema.sql` through the real pgschema binary with
an explicit plan database on the same disposable server, then executes the
reconciliation snapshot twice. The second database receives exactly migrations 1–56
through the exact existing production-migrator test. The test creates its own
disposable rows and exercises the deletion fence; it never receives the base
database as its target.

Comparison covers the three Work API tables, `office_company_bindings`,
`provisioning_operations` and `provisioning_operation_steps`: column definitions
keyed by name, ordered index/constraint shapes, sort/null options, trigger
enablement and deferrability, nine exact migration 53–56 guard function bodies,
and community fence attachments. It also requires the routing claim and
Work receipt triggers to remain enabled, row-level, AFTER INSERT, DEFERRABLE
INITIALLY DEFERRED, plus `project_api_binding_purge_at_commit` as enabled AFTER
ROW DELETE (`tgtype=9`), DEFERRABLE INITIALLY DEFERRED. Function comparison
includes `ortak_assert_project_binding_purge(uuid, boolean)`,
`ortak_guard_project_api_binding()` and
`ortak_project_binding_purge_at_commit()` with their exact bodies and catalog
signatures. The migration 56 activation guard must be enabled AFTER ROW INSERT
OR UPDATE (`tgtype=21`), DEFERRABLE INITIALLY DEFERRED on provisioning operations.
Both activation immutability guards must be BEFORE ROW UPDATE OR DELETE
(`tgtype=27`), and both truncate guards BEFORE STATEMENT TRUNCATE (`tgtype=34`),
on their exact tables. The three activation function bodies and signatures are
compared without normalization. The post-pgschema reconciliation script asserts
these live trigger shapes and function bindings on each run. A migration 55
binary or an unreviewed migration 57 database fails the exact version gate.
Catalog equality does not replace authenticated Work, activation or concurrency
behavior tests.

Each pgschema or migration-test command receives a reconstructed environment,
a 120-second deadline, a 4 MiB combined output limit, and an owned process group
that is stopped on completion or error. Commands and direct SQL share a
300-second budget. Each direct SQL call runs in its own spawned process with a
30-second wall-clock limit, capped by the remaining total budget, plus at most
one second for terminate/kill cleanup. Explicit libpq connection, statement and
lock deadlines remain enabled; an unresponsive server cannot defeat the parent
watchdog. No child output is printed. SQL results are read only after the worker
exits and are capped at 4 MiB; SQL error text is capped at 8,192 characters and
stays in a private result file. Public failures expose only a fixed error code.
The mode 0700 probe directory contains mode 0600 intent, source snapshots, bounded
logs, SQL results, compared catalogs and the final receipt. A failed command may have
committed some database state: both generated databases are retained on success
and failure, never retried in place or dropped by this helper. A fresh invocation
allocates a new pair; use the previous receipt when inspecting or later removing
the old pair explicitly.

Eleven local fixture tests passed in 1.365 seconds without PostgreSQL, Docker or
provider access. They include an actual spawned query worker that ignores
SIGTERM and hangs: the production watchdog returns within its deadline and
kills/reaps that worker. Mutation tests require all five activation guards on
the exact tables, with correct event/deferral flags and all three functions.
Actual migration 56 parity is recorded in the executed checkpoint below.
Catalog parity alone did not detect the initial migration 55 candidate's
PL/pgSQL variable ambiguity, which the real deletion tests caught.

## Historical final migration 55 receipt

The actual final55 probe passed at07:50 UTC on2026-09-05. Receipt:
`/private/tmp/ortak-private-20260905/logs/schema-parity-3a439dc5ddba492e9cc5e4661fda4c34/receipt.json`.
Both generated databases were retained. The desired schema hash is
`d1ae174288f7b855b7c8248eb23e05a47b59da8861af76aa7ee517a290b16545`;
the reconciliation hash is
`fc82471234b813203c8836f08ef05f361926bebe0a18db50cf17202b61a86614`.
All seven catalog components matched after two reconciliation passes.
The real approved-purge, stale-executor and commit-expiry regressions separately
passed against the corrected final55 migration; catalog equality alone did not
prove these behaviors.

## Executed migration56 checkpoint

The final56 helper passed actual pgschema/production-migrator parity at08:13 UTC
on2026-09-05. Receipt:
`/private/tmp/ortak-private-20260905/logs/schema-parity-b9c1c3fbe80944e6b827b41a7428bbbd/receipt.json`.
Both generated databases are retained; all seven catalog components match,
including the two provisioning tables, all three56 functions and the exact
deferred/immutable/truncate trigger shapes. Reconciliation ran twice.
Desired-schema SHA256:
`8acafb2213bc3bdf7406064cfa1b20ff342919722a9ff3a90896b24b17d360a0`;
reconciliation SHA256:
`06c93f40d652457ea35e48fd0c7235dd51d979cc1b2cf4f844a4c0a2d4b3b409`.
Separate actual provisioning PostgreSQL tests passed13 cases in4.56s, including
commit-time admission expiry rollback, stale baseline/fence refusal and immutable
success receipts. A subsequent focused Office-binding reuse case passed in0.31s,
bringing the distinct PostgreSQL cases to14. It proves a second real saga
activation keeps the original Office binding identity/provenance while refreshing
its exact admission observation. These tests use fixture adapter facts and do not activate a
real-provider employee or close the production provisioning composition gate.
