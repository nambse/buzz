# Private MVP product API

Status: implemented API transport; runtime workers and desktop composition are
separate integration gates. This API alone does not enable central routing,
start Hermes, acknowledge a stop, or prove deployment/runtime readiness.

## Identity and audience

The `ortak-server` binary accepts NIP-98 `Authorization: Nostr <base64 event>`.
The retained `buzz-auth` verifier checks the signature, timestamp, exact method
and canonical URL (including the query string). POST additionally requires
exactly one signed SHA-256 payload tag over the transmitted bytes. Every request
uses the shared Redis replay guard, scoped by the resolved company. A Redis or
Postgres error fails closed. Signed authentication is never logged or stored;
the audit keeps only the public key and authentication event ID.

`ORTAK_API_CONFIG_JSON` is non-secret, server-owned deployment configuration:

```json
{
  "origin": "http://localhost:8787",
  "allowed_web_origins": ["http://localhost:5173", "tauri://localhost"],
  "community_id": "<isolated Office community UUID>",
  "humans": [{
    "public_key": "<human lowercase hex PUBLIC key>",
    "role": "operator",
    "channel_ids": ["<allowed cohort channel UUID>"],
    "employee_ids": ["test-employee"]
  }]
}
```

These placeholders are deliberately not runnable credentials. Supply actual
public identifiers through the process environment; never insert a private key,
OAuth material, or provider credential. `reader` can read; `operator` can also
request cancellation. At most32 human grants, each with at most64 channels and
64 employees, are accepted. Every principal must remain a live Office member
and must not be deactivated or recorded as an employee/legacy bot.

The configured origin selects one deployment/community; Host must match it and
forwarded host/protocol headers are ignored. The company is looked up afresh
through `office_company_bindings`, never supplied by a request. The company and
community must remain active. No authenticated request can select a company,
role, actor, grant, or broader audience.

After cryptographic verification and the shared replay check, middleware takes
B1b's shared Office authority transaction fence before reading live human facts.
It holds that fence through the handler's database work and response construction,
so a concurrent authority mutation must serialize before the read or fail with a
retryable serialization error. No runtime or Redis call occurs under the fence.

Run visibility requires both the configured employee/channel grant and live
private-channel membership. The inbox event partition, author, kind and channel
must agree with the canonical Office event under the company/community binding.
Deleted events/channels and non-stream, DM or Work-originated runs are not
exposed. Channel/archive history remains readable while its audience permits.
Employee directory grants are explicit and separate: a directory entry exposes
only ID, name, title, lifecycle and active revision, never its full manifest,
credential references, runtime profile or memory bindings.

This intentionally narrows full-v0 role/project policy to an explicit private
cohort. It is not a general company-member API or a substitute for the remaining
Work/Projects audience and administrative configuration workflow.

## Endpoints

All endpoints require fresh signed authentication. Responses are `no-store`.

| Method / path | Result |
| --- | --- |
| `GET /api/v1/employees?after=<employee_id>&limit=25` | Granted durable directory, ordered by ID, `next_after` and `has_more`. |
| `GET /api/v1/employees/{id}` | Directory entry and newest visible nonterminal run, plus whether others exist. Runtime health is explicitly `not_probed`; permission enforcement is `not_verified_by_api`. |
| `GET /api/v1/runs?employee_id=<id>&status=<status>&cursor=<cursor>&limit=25` | Existing Activity `RunListPage`, filtered before keyset paging. |
| `GET /api/v1/runs/{id}` | `{detail: RunDetail, cancellation: null | Cancellation, can_request_cancel: boolean, office_delivery: null | OfficeDelivery, memory: RunMemory}` from durable Activity, delivery and request records. |
| `GET /api/v1/runs/{id}/events?after_sequence=0&limit=100` | Existing Activity `RunEventPage`: ordered entries, exclusive next sequence, `has_more`, explicit gap signal. No raw payload option. |
| `GET /api/v1/runs/{id}/stream?after_sequence=0` | Signed short-lived SSE; current run detail plus durable event pages, followed by pushed updates. |
| `POST /api/v1/runs/{id}/cancel` with body `{}` | One auditable durable cancellation request. Returns202 with `status: pending` while the worker has not acknowledged a stop. |

Run-list limits clamp to1–25; event pages to1–100. Run cursors retain the existing
`RunListCursor` microsecond/UUID encoding. Employee pages clamp to1–25. Unknown
query/body fields are rejected. Activity bodies are capped at4KiB; manual Work
routes use16KiB as documented in `WORK_API_E1.md`. The auth header cap is16KiB,
request URL cap4KiB, and request timeout15seconds. Concurrency is half the pool
connection limit (maximum16), reserving one fence connection and one query
connection per request; the default8connection pool permits4active requests.
The router refuses a pool smaller than2connections.
The private binary uses the shared API/worker connector, which enforces500ms
PostgreSQL lock waits,5s statements,10s idle transactions and5s pool acquisition.
An expired authority transaction cannot commit a successful response. A held
run-row integration test verifies503, complete cancellation/audit rollback, all
eight reusable connections, authority release, and a successful fresh-signed
retry once the lock is released.

The private-MVP list implementation selects only authorized IDs with one bounded
SQL query, then reads each selected run through the real Activity query service.
This costs at most25 detail reads per list page. Replace the bounded fan-out
with a shared audience-aware Activity query before widening the deployment.

Clients reconnect with their last *rendered* sequence, append only larger
sequences, and never advance beyond `gap`. Empty pages retain the cursor and do
not imply completion; lifecycle state comes from the run detail.

### Live Activity

Migration0060 adds transactional PostgreSQL notifications. The signed streaming
GET uses the same NIP-98, Redis replay and configured company/audience boundary;
there is no query-string bearer token. `LISTEN` completes before backfill, so
concurrent commits are either in that read or wake another read. Notifications
contain company/run UUID hints only. The server never forwards notification
content or treats it as a cursor.

SSE `activity` data is `{detail: <GET run response>, page: <RunEventPage>}`; `id`
is the page's `next_after_sequence` when present. Each page contains at most25
events and the serialized frame is capped at4MiB. `heartbeat` data is `{}`;
`control` data has one code: `renew` asks for fresh signed authentication,
`retry` closes a failed connection, `revoked` clears private state, and `resync`
requires explicit timeline reload after a cursor gap. Every activity frame and
idle heartbeat rechecks current human/company/channel membership and configured
audience under the shared Office fence. Authority mutations notify immediately;
idle revalidation occurs every5seconds. Configured role/grant changes are
process configuration changes and require API restart.

A stream lives at most45seconds and uses one of four separate listener
connections. A one-frame queue bounds backpressure; cancellation or the absolute
deadline releases its listener even if the HTTP peer stops reading. Query
capacity is shared with ordinary HTTP handlers, so even a2connection query pool
can progress. No fence/query permit spans a notification wait or network send;
each stream read has an8second deadline,500ms lock wait and2second statement cap.

The desktop keeps the last dense cursor through up to five failed reconnect
attempts with bounded exponential backoff. An initial replay alone does not
reset a repeated-disconnect loop. A normal45second renewal signs a new request;
reload/remount replays persisted history from the start. The display retains
500events and writes no private content to browser storage. Disconnected,
reconnecting and paused states remain explicit, with manual reload available.

`OfficeDelivery` reports `pending`, `delivered` or `failed`, a bounded error code
and optional delivery time. Completion does not imply publication. Streams stay
open after terminal execution and push late cancellation, Office delivery,
context and memory receipt changes even when no new run event is appended.
Every reconnect also rereads these current durable details.

## Cancellation and audit

Migration0048 supplies the coordinated authority fence; migration0049 creates `ortak_api_audit` (immutable) and `run_cancel_requests`.
A cancellation transaction takes the shared Office fence before any row lock,
rechecks the live human, locks the company-scoped run, rechecks visibility, refuses terminal
runs, inserts the one request per `(company_id, run_id)`, and appends the
principal-attributed audit entry in the same transaction. If the audit insert
fails, the queue insertion rolls back. A newly signed duplicate returns the
same request ID; a reused NIP-98 event is rejected by the shared replay fence.

The stored fixed reason is `human_requested`. No arbitrary user text, runtime
reference, credentials or signed auth JSON enter the request/audit records.
Reader-role denial and out-of-audience/unknown run operations are durably audited
before returning. Authentication failure without a verified principal is not
attributed to a guessed actor.

Worker contract (integration owns the consumer): claim pending rows under a
lease with bounded attempts/backoff; select the run by its scoped ID; invoke
supervised cancellation outside a DB transaction; acknowledge only after a
durable terminal result; clear the lease and set `acknowledged_at` atomically
with `status=acknowledged`. Persist bounded terminal failure as `failed` after
retry exhaustion. Pending rows survive worker/API restart. `cancel_reason` on
the run is never used as an unacknowledged stop request. This API does not modify
run lifecycle state or append a fabricated cancellation event.

Stable API errors are401 authentication required/replayed,403 forbidden,
404 not found (same for unknown and cross-company/audience IDs),409 terminal run
or terminal cancellation failure,413 oversized body, and503 unavailable/unreadable
state. Malformed transport fields use Axum's400/422 rejection. Internal SQL,
authentication diagnostics, private IDs from another tenant and runtime errors
are not returned. A failed cancellation has no automatic unbounded reopen.
Retry/approval/provisioning APIs remain absent until their workers and authority
contracts exist.

## Local launch and verification

After applying the additive migrations using the deployment's controlled
migration workflow, set `ORTAK_DATABASE_URL`, `ORTAK_REDIS_URL`, and
`ORTAK_API_CONFIG_JSON` through the process environment. Launch:

```sh
. ./bin/activate-hermit
cargo run -p ortak-server --bin ortak-server
```

The binary binds only loopback (default `127.0.0.1:8787`; `ORTAK_API_BIND` can choose
another loopback address). It never auto-migrates, activates employees, subscribes
to Office, or starts runtime workers. Serve the UI through an explicitly reviewed
private proxy/desktop configuration. `allowed_web_origins` permits at most8exact
browser/Tauri origins; OPTIONS preflight runs outside auth, GET/POST may carry
Authorization and Content-Type, and cookies/wildcards are never permitted.
An empty list disables cross-origin browser access. There is no public bind.
Database and Redis connection diagnostics are fixed strings, not credential URLs.

Route tests send actual fresh Schnorr-signed HTTP requests through the production
router. The PostgreSQL test requires explicit `ORTAK_TEST_DATABASE_URL` on the
disposable port55432; there is no generic `DATABASE_URL` fallback. Each fixture
owns a unique company/community. Use a separate disposable database while other
migration lanes are active:

```sh
cargo test -p ortak-server
ORTAK_TEST_DATABASE_URL=postgres://ortak:ortak@127.0.0.1:55432/ortak_api_20260905 \
  cargo test -p ortak-server --test postgres_authenticated_routes -- --ignored
```

Runtime cancellation, durable Hermes event reconstruction, signed reply delivery,
full-stack reconnect/restart and an actual UI workflow remain integration smoke
gates; passing these API tests does not establish them.

## Scoped memory projection

`GET /api/v1/runs/{id}` includes `memory` only after the same canonical
company/channel/employee audience check as the run. This read performs no Honcho
request and never validates, creates or writes memory. It describes durable
admitted context and post-delivery jobs, so a healthy service cannot fabricate a
successful write or an empty recall.

`memory.scope` is `run_scratch` and `run_id` is the selected run. `recall.status`
is `not_prepared` when no snapshot exists, or `prepared` with a bounded `records`
array, `truncated` and `prepared_at`. Every record exposes redacted `content`,
`source`, `recorded_at` and an opaque record reference. The API verifies the
snapshot hash and original company, employee, revision, decision, message, root,
channel and run pins before showing it. Changed pins or corrupt data fail with
503; they do not become an authoritative empty result. Runtime configuration,
credential references, input prompts and memory binding options are excluded.

`write` is null when no post-delivery memory job exists. Otherwise it contains
`status` (`pending`, `acknowledged`, `failed`), finite attempt/retry metadata,
redacted published content and its signed Office source, and the durable
receipt/acknowledgement time when present. The original frozen source facts and
revision/channel pins must still match the current visible run. This prevents a
retargeted run from exposing an older conversation's notes. Memory status and receipts push through Activity after Office delivery,
including after run completion. No employee-global or project memory is exposed.
