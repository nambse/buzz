# Disposable migration 55 schema parity

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
The final migration55 executable was compiled and used on2026-09-05. An operator
must select the appropriate fresh executable; the receipt hashes
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
reconciliation snapshot twice. The second database receives exactly migrations 1–55
through the exact existing production-migrator test. The test creates its own
disposable rows and exercises the deletion fence; it never receives the base
database as its target.

Comparison covers the three Work API tables and `office_company_bindings`,
column definitions keyed by name, ordered index/constraint shapes, sort/null
options, trigger enablement and deferrability, exact migration 53–55 guard function
bodies, and community fence attachments. It also requires the routing claim and
Work receipt triggers to remain enabled, row-level, AFTER INSERT, DEFERRABLE
INITIALLY DEFERRED, plus `project_api_binding_purge_at_commit` as enabled AFTER
ROW DELETE (`tgtype=9`), DEFERRABLE INITIALLY DEFERRED. Function comparison
includes `ortak_assert_project_binding_purge(uuid, boolean)`,
`ortak_guard_project_api_binding()` and
`ortak_project_binding_purge_at_commit()` with their exact bodies and catalog
signatures. A migration 54 binary or an unreviewed migration 56 database fails
the exact version gate. Catalog equality does not replace authenticated Work or
concurrency behavior tests.

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

Ten local fixture tests passed in 1.363 seconds without PostgreSQL, Docker or
provider access. They include an actual spawned query worker that ignores
SIGTERM and hangs: the production watchdog returns within its deadline and
kills/reaps that worker. The initial migration 55 candidate passed actual central
pgschema parity; the corrected migration 55 and this watchdog refinement require
a fresh central verified receipt. Catalog parity alone did not detect the
initial candidate's PL/pgSQL variable ambiguity, which the real deletion tests
caught.

## Final executed migration55 receipt

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
