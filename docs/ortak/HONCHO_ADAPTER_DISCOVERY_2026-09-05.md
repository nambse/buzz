# Honcho adapter discovery, 2026-09-05

Status: source-reviewed candidate and proposed adapter contract; no deployment,
adoption, provider call, or remote memory access performed by this discovery.

**Native Honcho 3.1.1 cannot yet satisfy Ortak's retry-safe `Remember` contract.**
Health and resource inspection are implementable, and session-scoped message
search provides a concrete recall route. Activation must remain refused until
real recall and idempotent writes are implemented and exercised. The local
[memory port](../../crates/ortak-control/src/memory.rs) requires `HealthProbe`,
`ResourceInspect`, `Recall`, and `Remember`; its write key explicitly forbids
duplicate records on retry. The
[provisioning gate](../../crates/ortak-control/src/provisioning.rs) must keep
those requirements. A mock or a successful `/health` response supplies no
evidence for the last two capabilities.

## Candidate pin and fresh stack

Use upstream release **v3.1.1**, commit
`5d992bc65afcfbc05a5911ab4edbaa88ef64c690`, as the compatibility candidate.
The downloaded source archive has SHA-256
`7a7453159892790359d7643f9608a348cc328f0c40b25ceee4e4b6da64f3d0fb`.
This matches the version string in our
[existing read-only discovery](RUNTIME_DISCOVERY_2026-09-05.md), not proof of
the existing deployment's source or image identity.
[Official release](https://github.com/plastic-labs/honcho/releases/tag/v3.1.1).

Build this source and its frozen `uv.lock`; record the resulting image digest
and resolve base/database/cache image digests before calling a deployment
reproducible. Upstream uses Python 3.13, separate API and deriver processes,
PostgreSQL with pgvector, and Redis in its example stack. Create isolated
volumes, service names, and ports; do not reuse the external test resources or
copy the example's database trust setting into an exposed deployment.
[Dockerfile](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/Dockerfile),
[Compose example](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/docker-compose.yml.example#L12-L118).

The pinned configuration requires an LLM provider. Its documented default is
`LLM_OPENAI_API_KEY`, with `gpt-5.4-mini` for generation and
`text-embedding-3-small` for embeddings. An alternative requires explicitly
configured feature models/base URLs, tool-calling support, and a working
embedding provider. Provision fresh database credentials and an auth JWT
secret; enable authentication. Ortak manifests retain opaque endpoint and
credential references only. Existing employee OAuth files establish none of
these requirements and must not be reused implicitly.
[Pinned environment contract](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/.env.template#L60-L91).

## Exact resource operations

All `/v3` paths below are relative to the allowlisted resolved endpoint.
Bound response sizes, pagination, request time, and retries; reject redirects
to another origin. Inspect identifiers and resource metadata without reading
memory contents during adoption.

| Operation | Request | Meaning and constraint |
| --- | --- | --- |
| Liveness/schema | `GET /health`; `GET /openapi.json` | Health returns static `{"status":"ok"}`; it does not check providers, DB, or derivation. Compare the schema with the pinned adapter contract. |
| Find workspace | `POST /v3/workspaces/list?page=1&size=100`, body `{}` | Read-only, paginated; requires admin auth when auth is enabled. Match the exact configured ID. Exhaust a bounded page budget or return an explicit incomplete-inspection error. |
| Find peers | `POST /v3/workspaces/{workspace}/peers/list?page=1&size=100`, body `{}` | Read-only, workspace-authorized; match both configured regular peer IDs. Missing resources fail adoption. |
| Explicit create | `POST /v3/workspaces`, body `{"id":"..."}`; `POST /v3/workspaces/{workspace}/peers`, body `{"id":"..."}` | Get-or-create APIs: **201 means created; 200 means reused**, not newly owned. Never use them for inspection/adoption. |

Sources: [health and route mounting](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/main.py#L195-L212),
[workspace routes](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/routers/workspaces.py#L36-L92),
[peer routes](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/routers/peers.py#L81-L159).

Creation needs stricter handling than the native status code. Existing
workspace fields are preserved, but peer creation **updates supplied metadata
and configuration on an existing peer before returning 200**. Therefore,
sending ownership metadata and rejecting a later 200 can already have
overwritten someone else's resource. A lost 201 acknowledgement also becomes
200 on retry, losing creation attribution. Use operation-derived fresh names
and a durable provisioning journal, and add a server-side create-only receipt
operation for reliable retries: existing foreign ownership returns 409 without
mutation; the same operation returns its original created-resource receipt.
Compensation may delete only resources that receipt proves Ortak created.
[Workspace CRUD](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/crud/workspace.py#L87-L158),
[peer overwrite path](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/crud/peer.py#L304-L323).

## Scoped recall and provenance

Use `POST /v3/workspaces/{workspace}/sessions/{session}/search` with
`{"query":"...","limit":32}` for the first adapter. The handler forces
workspace and session filters from the path. Its result is raw messages,
including IDs and metadata; this gives record-level provenance that a generated
chat answer alone cannot supply. Search accepts a limit of 1–100 and, with
message embeddings enabled, calls the embedding provider before querying.
Provider/auth/DB errors must propagate, never become an empty successful recall.
[Session search](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/routers/sessions.py#L1075-L1106),
[request/response schema](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/schemas/api.py#L363-L383),
[search limits](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/schemas/api.py#L707-L717),
[search implementation](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/utils/search.py#L374-L458).

Proposed mapping: a company-owned workspace, with server-derived session IDs
for company truth, each project, each employee's experience, each
employee/operator relationship, and each run's scratch context. Resolve these
from an authorized company/employee binding; a caller-provided `MemoryScope`
is not an authorization grant. Canonical company files remain authoritative.
Respect Ortak's record/byte budget and an HTTP body ceiling; cap one search at
100 records and conservatively mark a full limit or byte cut as truncated.

Store a versioned provenance envelope with company, employee, optional run,
scope, source, recorded time, write key, and canonical payload hash. Recalled
metadata must match an authorized durable write receipt; reject missing or
conflicting provenance instead of inventing it for legacy memories. Protect
adapter-owned content and provenance from mutation through alternate Honcho
write routes, or detect mismatches against the durable receipt on every read.

## Native write gap and smallest proposed extension

The native endpoint is
`POST /v3/workspaces/{workspace}/sessions/{session}/messages` with a `messages`
array. `MessageCreate` accepts content, peer ID, metadata, configuration, and
time, but no client message ID or idempotency key. CRUD serializes session
sequence allocation, then generates a fresh nanoid and inserts a new message
for every item on every call. Metadata has no uniqueness constraint. A local
outbox, metadata lookup, or content hash alone cannot resolve a committed
write whose HTTP acknowledgement was lost.
[Write schema](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/schemas/api.py#L328-L383),
[write transaction](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/crud/message.py#L418-L499),
[message constraints](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/models.py#L206-L269).

Add one pinned, narrow ingestion extension with a unique receipt keyed by
`(workspace, session, idempotency_key)` and a canonical batch hash. In one
Honcho DB transaction, reserve the receipt, insert all messages and their
provenance, and persist the returned message IDs plus any required pending
derivation work. Equal key/hash returns the original IDs without inserting
messages or scheduling duplicate work; unequal hash returns 409. Refactor the
existing internal commit so receipt and messages cannot tear. Do not expire
receipts while an Ortak write can still be replayed. Ortak keeps its own
bounded durable retry journal, including an explicit terminal failure state.

The native route schedules derivation through a background task after message
commit. The extension must make required scheduling durable in the same
transaction, rather than interpreting an HTTP response as completed derivation.
Embedding rows already have pending state; reuse upstream processing where
possible. This is a proposal, not a claim that a patched server exists.
[Route scheduling](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/routers/messages.py#L95-L169),
[pending embeddings and commit](https://github.com/plastic-labs/honcho/blob/5d992bc65afcfbc05a5911ab4edbaa88ef64c690/src/crud/message.py#L467-L499).

Advertise `Recall` and `Remember` only after disposable-stack production-path
tests prove nonempty write/recall with exact provenance, company/scope isolation,
concurrent duplicate and lost-ack replay, restart recovery, conflicting-payload
rejection, unchanged create collisions, and propagated provider failures. The
activation smoke test must exercise actual configured embeddings/derivation as
applicable, not just liveness or a fake adapter. No current external memory or
credentials need to be read to implement or test this isolated candidate.
