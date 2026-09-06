# Selected reviewed Work context — D2c

Status: bounded production source and proposal71 are in review. Central Rust
compilation and all seven runtime PG regressions passed. A corrected populated
purge fixture and new Honcho image validation remain pending. This is not deployed.
Applied69 remains unchanged; root owns migration71 integration after70, schema
parity, image pins, rollout and live acceptance.

## Delivery boundary

An explicitly configured Work run may recall approved facts for its own employee
and canonical project. The existing D1 approval and D2b explicit publication must
already have a validated acknowledgement. Local approval text is never used as a
substitute for a failed Honcho request. Plain Office inputs, DM content, central
semantic routing, employee/company-wide memory, embeddings and extraction remain
outside this slice. Existing RunScratch recall/write behavior remains separate.

The worker recipe adds `reviewed_runtime_projects`, default empty, per employee.
It must be a subset of that employee's existing explicit `reviewed_projects`.
The full original owned receipt and actual I/O witness remain mandatory. Ordinary
health, recipe deployment and publication do not themselves enable runtime use.
The publication UI names potential selected Work use, while saving a D1 fact
alone remains preview-only.

The same retained target gains a default-false consumption flag and a derived
consumption epoch. Removing opt-in advances the epoch. Re-enabling that exact
target permits new runs but cannot revive runs that used its earlier epoch.
Normal unchanged worker advertisement refresh does not advance the epoch.
Already acknowledged approved memory survives a model-only revision when the
employee and exact current memory binding, source permissions and opt-in remain
valid. Run revision and employee lifecycle epoch are still pinned independently.

## Selection and fixed input

Only the server-derived `WorkRunOrigin.project_id` selects a project. At most 16
unique bounded alphanumeric terms from the human title and description form a
literal OR query. Runtime instructions, JSON field names and UUIDs do not become
search terms. This is deterministic lexical retrieval, not semantic scoring.
The canonical database filters active approvals, source audience, publication
acknowledgement, target/current binding and opt-in before selecting at most 32
fact IDs. No remote operation occurs in this short authority transaction.

The additive owned `/recall-selected` endpoint requires 1–32 unique, nonzero IDs
and applies them before its search/result limit. It returns at most eight records
and 8 KiB. The Rust transport checks the returned ID subset and existing native
scope, receipt, hash, expiry and provenance rules. The runtime checks the exact
locally selected approval pins again. Custom redaction that would change approved
text refuses the context rather than attaching an unchanged approval digest to
different bytes. Empty authorized selection skips the remote call.

Snapshot version3 adds a typed reviewed context distinct from RunScratch. Each
record contains its real human approval, source/content/binding hashes, target
and consumption epoch, fact/version and expiry. It never fabricates an author
run. Version1 Office and version2 Work snapshots keep their original exact bytes.
Combined context remains at most eight records and 16 KiB, and the existing
encoded snapshot ceiling remains in force. Reviewed records take priority when
the combined budget requires trimming scratch records.

The final freeze uses Office → project → Work → sorted facts → sorted targets →
run → outbox locks. It re-derives authority and stores snapshot plus one immutable
`run_reviewed_memory_uses` row per reviewed record in the same transaction.
Deferred guards bind their exact attribution and text to the original approved
fact and rendered RunSpec, refusing orphan uses, extra context or altered bytes.
Lost-start recovery loads the same snapshot bytes and does not recall again.

## Revocation, expiry and output

Current used-fact validation runs at snapshot load/freeze, Work derivation,
admission renewal/recovery and artifact materialization. Fact and target locks
prevent concurrent withdrawal or opt-out from committing behind a held admission
transaction. Deferred admission/artifact checks also enforce current expiry at
commit. Admission lifetime is bounded by both used-fact expiry and the current
short target witness.

The bounded existing reconciliation pass becomes due when any retained use is
invalid or expired. It schedules the existing durable runtime cancellation,
which survives restart. No unbounded fan-out occurs inside Stop using. A completed
provider response cannot materialize an artifact or open human review once its
context is no longer authorized. Criteria/approvals remain human-controlled.

Context admitted before a later withdrawal may already have reached the provider;
those bytes cannot be retracted. The memory view retains the opaque use receipt
and withholds currently impermissible text. Project Stop using controls remain
available. Reviewed-store removal still requires D2b's exact native withdrawal
acknowledgement and says nothing about prior inputs, artifacts, approvals or
backups.

## Retention and validation

The only new relation is `run_reviewed_memory_uses`, containing no content or
credentials, at most eight rows per run. It uses durable foreign keys, immutable
rows, no delete/truncate and the universal community write fence. Ops has been
notified of required retained/deletion and G inventory integration. Current G
manifests intentionally refuse unknown schema71 until explicitly upgraded.

Prepared production regressions include selected-before-limit Honcho HTTP/PG
tests, exact Rust socket allowlist rejection, typed snapshot/hash/scope/budget
units, config opt-in refusal, and actual Work PG tests for artifact/review,
withdrawal plus late output, held target lock plus epoch re-enable, stale recall
at freeze, byte-identical start recovery and expiry without remote cleanup.
The seven runtime PG regressions passed central execution on the final71
disposable schema, including actual sealed model revision and forged-snapshot
refusal. The full reviewed-export suite passed15/16; the remaining populated
purge fixture discarded its simulated remote publication header before cleanup.
That fixture now retains the same remote adapter, with rerun pending. SQL guards
were unchanged. The new Honcho installed-image and Rust selected socket gates
remain pending at this checkpoint. Focused
desktop memory/run tests passed17/17; TypeScript typechecking, scoped Biome and
PostgreSQL test discovery across730 Rust files passed. Provider behavior and deployed-image acceptance require
root's later exact artifact checks; no live claim is made here.

### Installed Honcho image gate

Freeze `runtime/honcho-adapter` into a new private evidence directory before
building both `tests` and `runtime` targets. Use the locked upstream archive
(`7a7453159892790359d7643f9608a348cc328f0c40b25ceee4e4b6da64f3d0fb`)
and `prepare_source.py`/`build_image.py`; record source hashes and inspect the
two resulting image IDs. Select a fresh Docker config with the Desktop buildx
plugin path, never an ambient registry login. The existing bounded build helper
limits each build to 1,200 seconds and its log to 8 MiB.

For the following command, `SOURCE` is that frozen source directory,
`TEST_IMAGE` is its exact tests image ID, and `DOCKER_CONFIG`/`BUILD_HOME` are
fresh private directories prepared by the build helper. Only the three local
test/helper files are mounted: the production schema must import from the image
under `/app`, not from a mounted source tree.

```sh
env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin LANG=C LC_ALL=C \
  HOME="$BUILD_HOME" DOCKER_CONFIG="$DOCKER_CONFIG" \
  /usr/local/bin/docker --host unix:///Users/nambse/.docker/run/docker.sock \
  run --rm --init --network none --read-only \
  --tmpfs /tmp:rw,nosuid,nodev,size=32m --cpus 1 --memory 512m --pids-limit 64 \
  --mount "type=bind,src=$SOURCE/test_build_image.py,dst=/checks/test_build_image.py,readonly" \
  --mount "type=bind,src=$SOURCE/test_reviewed_contract.py,dst=/checks/test_reviewed_contract.py,readonly" \
  --mount "type=bind,src=$SOURCE/build_image.py,dst=/checks/build_image.py,readonly" \
  -e PYTHONPATH=/app:/checks --entrypoint /app/.venv/bin/python "$TEST_IMAGE" \
  -m unittest discover -s /checks -p 'test_*.py' -v
```

Expected local count is12: seven build-helper tests and five production-schema
tests. For actual storage, create a new internal network, volume and PG container
from image `sha256:cf134a767f474095eeba57e0117be8e568e011a63f33fbf252f14c9b760f8e6f`.
Use network alias `honcho-test-db`, a fresh `ortak_honcho_*` database and explicit
synthetic password; publish no host port. Run the tests image by exact ID on that
network with `ORTAK_HONCHO_TEST_DATABASE_URL` set to the synthetic connection.
Its default `/app/ortak_run_tests.py` initializes the native schema and discovers
all25 PG tests, including11 reviewed tests. Each child is bounded to120 seconds;
the outer capture should remain bounded to300 seconds/8 MiB and clean up only
its uniquely named test container.

The two new PG tests are in `tests/test_reviewed_selected.py`. They prove that
withheld earlier-sorting matches cannot crowd out permitted IDs, selected scope
and withdrawal fail closed, and the existing eight-record/8 KiB limit still
applies. Separately run `/app/.venv/bin/python -m ortak_honcho.init_db` from the
runtime image against that same fresh database with synthetic authentication,
`EMBED_MESSAGES=false` and metrics/telemetry/cache disabled. Compare installed
extension files in both images, tests in the tests image, and the patched native
message/enqueue files against the frozen source hashes. Stop the fresh PG and
record `State.Running=false`; retain its bounded evidence. Never reuse prior
evidence scripts' hard-coded resource names or select a live Honcho database.

The Rust actual-socket filter `http_contract_reviewed` now selects three tests,
including `http_contract_reviewed_selected_pins_exact_ids_and_rejects_foreign_results`.
These image/socket gates are required before publishing a selected runtime
project opt-in; build success alone does not prove storage or live recall.
