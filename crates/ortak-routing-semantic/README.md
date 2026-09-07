# Explicit semantic routing evidence

This adapter implements the documented Chat Completions request with strict
JSON Schema output, explicit model selection and refusal/incomplete-response
handling. Protocol references, retrieved2026-09-05:
[Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create),
[Structured Outputs](https://developers.openai.com/api/docs/guides/structured-outputs).

It supplies evidence to the central router. It has no database, runtime,
dispatch, memory, tool, Office subscription or employee activation capability.
Only the control service constructs its sealed company/message/policy/revision
input; current authority and unique reservations are rechecked at commit.
Employee-origin and deterministic inputs never call the scorer.

The worker remains disabled for semantics unless an operator selects all of:
one deployment UUID, fixed HTTPS or literal-loopback origin, opaque credential
reference and environment variable, model, and exact expected response model.
The origin has no path/query/user information; `/v1/chat/completions` is fixed.
There is no default model, endpoint fallback, ambient proxy or credential scan.
Select a reviewed snapshot supporting this strict response schema; the adapter
does not discover model availability or silently change it. Invalid selection
produces unavailable semantics while cancellation/recovery remain enabled.

Only bounded, redacted human text and candidate identity/name/title/biography/
responsibilities/domains leave the process. Company, message and revision pins,
raw memory, tools, credentials and private conversation summaries stay local.
Limits are16KiB per text field,32 candidates,64KiB input text/encoded request/
response,4096 completion tokens, two immediate HTTP slots, no queued backlog,
no retries and a five-second request limit. Oversize candidate sets are refused,
never truncated to an arbitrary subset.

The control service separately owns one five-second total scoring budget across
revalidation attempts and caps it by the held inbox claim. Expired, cancelled
or late results cannot commit wakes. Malformed output, refusal, foreign/missing/
duplicate candidates, nonfinite/out-of-range scores, unknown evidence labels,
unexpected model/role/tool calls and duplicate JSON control fields fail closed.
Only the fixed evidence taxonomy is persisted; provider prose and arbitrary
provider metadata are omitted.

Successful evidence can be cached for five minutes in256 entries. Identity
includes company, message, canonical input hash, exact candidate revisions,
full policy/version, redacted wire hash, deployment/origin/credential reference,
model snapshots and compiled prompt/scorer/schema/redactor versions. Cached
hits record zero additional HTTP response bytes and provider tokens. Counters
and eligibility are reapplied by the routing transaction, not authorized by a
cache hit. Three consecutive provider/protocol failures open a30-second circuit;
one later probe may recover it. Dropping a request records its failed attempt,
including a cancelled probe, without detached cache work.

Focused tests use the actual sealed input service and HTTP transport against
owned loopback fixtures, including redaction, strict parsing, redirect/size
refusal, cache invalidation, concurrency, cancellation and circuit recovery.
Control PostgreSQL tests prove durable timeout silence, revision refresh and
stale-claim refusal. These fixtures make no real provider call. The dated
private stack has no selected semantic credential/model and semantics remain
disabled; this is not a deployed semantic quality or full slice D acceptance
receipt.
