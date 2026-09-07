# Pinned Honcho extension for Ortak

This is a candidate `ortak-honcho/1` extension of Honcho 3.1.1 at
`5d992bc65afcfbc05a5911ab4edbaa88ef64c690`. It adds atomic memory-write receipts,
strict fresh resource ownership, and scoped recall with verified provenance.
No existing Honcho service or memory was accessed to build it. Deployment and
actual configured-provider activation evidence remain separate gates.

The upstream archive and reviewed file hashes are in `honcho-source-lock.json`.
`prepare_source.py` verifies the archive before extraction and applies only two
opt-in changes: native message creation may accept a prepared session and flush
without committing; the native queue-record builder may accept prepared session
and workspace objects. Native routes keep their original behavior. No session
get-or-create, cache write, background enqueue, or external provider call runs
inside the extension's write transaction. Pending embeddings are processed by
Honcho's reconciler; the native deriver consumes the atomically inserted queue.

## Build and test

Run these from this directory, using the already reviewed public source archive:

```sh
python3 prepare_source.py /private/tmp/ortak-honcho-5d992bc.tar.gz vendor
python3 build_image.py tests
```

`build_image.py` requires the SHA-256 digest in the source lock for the combined
Python/uv base. Python selection is explicit and `uv sync --frozen` retains the
upstream lock. The base's uv 0.11.6 differs from upstream's Docker uv 0.9.24; the
candidate must pass the tests before this becomes a tested build pin. Public
tiktoken assets are fetched during build, then cached in the image. No LLM call
or provider credential is needed for the tests. `vendor/` is generated and must
not be committed. The runtime image retains the upstream license.

This dated helper invokes only `/usr/local/bin/docker` against
`unix:///Users/nambse/.docker/run/docker.sock`, using `buildx --builder default`
and `--load`. It constructs a fresh owner-private home and Docker configuration;
ambient Docker contexts, builders, proxies, registry credentials and provider
variables are not inherited. It never prepares or fetches source on import.
The already prepared `vendor/uv.lock` and `vendor/pyproject.toml` must exist.
Public dependency downloads occur only during an explicitly invoked build.

The local CLI process group has a 20-minute deadline and an 8-MiB combined
output limit. Diagnostics stay in a mode-0600 `/private/tmp/ortak-honcho-build-*.log`
whose path is printed; ephemeral Docker configuration is removed after the
attempt. A failure stops and reaps the owned local CLI group. It does not claim
that a daemon-side build or its cache has been removed. Retain failure logs and
inspect only the explicitly selected local daemon when recovery is needed.

The build context now admits only Dockerfile inputs and prepared public source,
excluding Python bytecode, local test caches and environment/credential files.
`python3 -m unittest -v test_build_image` exercises the helper with mocked
subprocesses and real disposable output pipes; it invokes no Docker daemon.
These operational tooling/context changes have **not been rebuilt**. Previously
recorded image IDs and integration receipts describe the earlier built/tested
artifacts; they do not establish a new image identity for this source snapshot.

Create a **new disposable PostgreSQL database with pgvector**, named
`ortak_honcho_*`. Supply its explicit local URL; there is no default database or
fallback to `DATABASE_URL`:

```sh
docker run --rm \
  -e ORTAK_HONCHO_TEST_DATABASE_URL='<explicit disposable local PostgreSQL URL>' \
  ortak-honcho-adapter-test:3.1.1
```

The test runner applies native Honcho migrations, initializes only the extension
tables, and executes the real ASGI handlers against PostgreSQL. It randomizes
local JWT material and uses a nonfunctional test provider key. Tests cover:

- lost acknowledgement/reconnect, concurrent duplicate writes, conflicting
  payloads, and identical content under distinct write keys;
- queue-insert failure rolling back messages, pending embeddings, session,
  membership, and receipt; retry then succeeds once;
- create replay/concurrency, foreign-resource preservation, and failure during
  peer creation rolling back the workspace;
- nonempty full-text recall, workspace/session/scope isolation, byte bounds,
  and rejection of changed message content/provenance;
- native JWT scope checks, malformed provenance, body bounds, and injected
  provider failure with no database resource lock held during the call;
- ownership changes while a provider call is paused, including invalidation of
  pre-call SQLAlchemy ORM identities before the second authority check.

These tests do not establish a live provider's credentials or quality. An
explicitly configured full-text mode can establish `Recall`/`Remember` evidence
only through the actual adapter's isolated, binding-specific write/recall test.
That evidence does not validate embeddings, derivation, or an external provider;
those require their own checks before claiming that mode is healthy.
`GET /v3/ortak/protocol` reports only the wire protocol and upstream version,
not healthy capabilities. See `HTTP_SMOKE.md` and `VALIDATION.md`.

For an API/deriver deployment, build `python3 build_image.py runtime`, record the
resulting immutable image digest, and configure fresh DB credentials,
`AUTH_USE_AUTH=true`, `AUTH_JWT_SECRET`, provider configuration, and cache settings.
The API entrypoint runs native migrations and explicit extension-table setup.
Run the deriver from the same image with entrypoint
`/app/.venv/bin/python -m src.deriver`; it does not run the API entrypoint.
Never start it before database initialization completes. No deletion/retention
endpoint is supplied by this increment, and receipts have no TTL or cascading FK.

## Wire contract

All routes require enabled native Honcho authentication. Fresh creation requires
an admin JWT. Remember and recall reuse native workspace/session write-scope
checks; peer-global credentials are not elevated. Ortak resolves endpoint,
company, employee, session ID, and scope from its authorized binding before
calling. Payload fields never select arbitrary runtime-authored peers or Honcho
configuration.

`POST /v3/ortak/resources/create`:

```json
{
  "idempotency_key": "provisioning-step-key",
  "company_id": "00000000-0000-0000-0000-000000000001",
  "employee_id": "employee-one",
  "workspace_id": "fresh-unique-workspace",
  "user_peer": "operator",
  "employee_peer": "employee"
}
```

Creates one fresh workspace and two distinct peers in one transaction. Returns
201 and `{protocol, workspace_id, user_peer, employee_peer, ownership:"created"}`.
The same operation/body returns 200 with the original receipt; changed body,
existing workspace, foreign ownership, or replaced native identity returns 409.
The first increment deliberately uses a separate fresh workspace per bundle.
Native list-only adoption remains separate; this extension refuses writes to
unowned adopted resources. Do not advertise `ResourceDelete`.

`POST /v3/ortak/workspaces/{workspace}/resources/inspect` (additive to protocol 1):

```json
{
  "company_id": "00000000-0000-0000-0000-000000000001",
  "employee_id": "employee-one",
  "user_peer": "operator",
  "employee_peer": "employee"
}
```

Uses native workspace JWT authorization and returns HTTP 200 with the original
create receipt fields plus `company_id`, `employee_id`, `request_hash` and
`native_ids: {workspace: "<native-id>", peers: {operator: "<native-id>",
employee: "<native-id>"}}`. The response is available only after the receipt's
immutable native IDs match the currently locked workspace and peer rows, their
owner metadata and valid peer identities. Missing ownership, replaced IDs,
foreign company/employee or a changed peer tuple returns 409. Inspection does
not create, adopt, repair, update or delete resources; it never opens a session
or adds messages, queues, embeddings or receipts. The Rust adapter pins the
create request hash and complete native identity tuple, then uses this route
before health/capability claims and I/O. Native list metadata is insufficient.
Existing endpoints and receipt tables retain their protocol-1 shapes. A new
adapter against an older image fails closed on this missing route; deploy the
reviewed matching adapter/extension artifacts together.

`POST /v3/ortak/workspaces/{workspace}/sessions/{session}/remember`:

```json
{
  "idempotency_key": "durable-write-key",
  "company_id": "00000000-0000-0000-0000-000000000001",
  "employee_id": "employee-one",
  "scope": {"scope": "employee_experience"},
  "facts": [{
    "content": "A canonical bounded fact.",
    "provenance": {
      "employee_id": "employee-one",
      "run_id": null,
      "source": "office_message",
      "recorded_at": "2026-09-05T00:00:00Z"
    }
  }]
}
```

Returns 201 for a new batch, 200 for an equal replay, and 409 for a differing
payload under the same workspace/session/key. The response contains `protocol`,
`workspace_id`, `session_id`, `request_hash`, stable `record_refs`, and frozen
`records`. Hashing uses normalized validated JSON with sorted keys and compact
UTF-8 encoding; the client does not supply a trusted hash. Facts are limited to
1–64, each 16 KiB of nonempty UTF-8 without NUL. The session is created only by
remember, permanently tied to the exact company/employee/scope context, and may
contain only the bundle's two active peers. Replay never adds messages or queue
items. Native-resource replacement or ownership changes refuse replay rather
than authorizing writes into a replacement.

`POST /v3/ortak/workspaces/{workspace}/sessions/{session}/recall` takes
`{company_id, employee_id, scope, query, max_records, max_bytes}`. It returns
Ortak's `{records, truncated}` shape. Limits are 100 records, 128 KiB content,
and a 4 KiB query. Native search is forced to the exact workspace and session.
Its optional embedding call runs outside a held transaction. A second identity
and receipt check rejects changed or injected results. Missing owned sessions
return an empty result without creating anything. Every result carries the exact
stored provenance; no legacy provenance is invented. A different requested scope
for an existing session is a conflict. This does not use peer representations or
Honcho's global dialectic chat as project-local context.

Database lock waits are 1 second, statements 5 seconds, idle transactions 10
seconds, each operation 10 seconds, and request bodies 1152 KiB with a 10-second
read deadline. Retryable database/time failures are 503; input errors are 422;
wire-body excess is 413. The caller must retain its durable write journal and
bounded retry policy. No transaction spans an LLM call, and no acknowledgement
claims that asynchronous derivation has already completed.
