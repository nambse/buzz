# Private failure coordinates

The candidate records a failure's stage, closed exception class, recognized bridge
boundary code and at most eight file/line coordinates in the existing private
SQLite journal's `private_failure_diagnostics` table. The public `run.failed` event
keeps its closed error code. No API exposes the private table.

Only exact Python filenames in the selected Hermes source lock under `/opt/hermes`
and four fixed bridge modules are admitted. Unknown paths, exception class names,
messages, arguments, source text, locals, chained exceptions, prompts, output and
credentials are excluded. The traceback walk stops after 64 frames; each retained
coordinate has a bounded integer line. Persistence revalidates the closed schema
and caps each record at 2 KiB. A request failure also retains the first original
exception's closed coordinates (at most four frames) before upstream error
handling, plus an allowlisted HTTP status and classifier reason when available.
Later retries cannot replace that original cause; arbitrary error context is
never retained. One row per run bounds total storage by the existing
run journal ceiling.

Failure state, the public terminal event and private coordinates commit together.
A storage error rolls back all three and propagates to containment/recovery. A
late failure cannot replace cancellation or overwrite a terminal result. Inspect
only the selected run's row in the operator-owned journal; do not collect generic
Python tracebacks or container environment/output for diagnosis.

This is a source change requiring new pinned worker and controller artifacts.
The reviewed upstream revision remains `29112bef099274229cadff79cdff7bf7b99c4b77`.
The previous running image supplies no evidence for this diagnostic boundary.

The pinned Codex transport deliberately omits `max_output_tokens` on the selected
ChatGPT Codex backend. Therefore the candidate constructor's `max_tokens=2048`
alone does not establish the cause of the observed longer-response failures.
Likewise, `provider_failed` can represent a caught ordinary exception or a bridge
boundary error that the public code mapper does not separately expose. Read the
new coordinates before deciding which behavior needs correction.

Source regressions cover secret-free coordinates, stage binding at the production
request wrapper, preservation of unknown bridge errors as private boundary codes,
atomic rollback on diagnostic-storage failure, cancellation, exact source-file
allowlist and unchanged public projections. The image-only Codex check additionally
uses short and over-100-word final-answer fixtures through real pinned Hermes;
these replace provider I/O and are not live provider health evidence.

A live diagnostic exposed an integration bug at the pinned conversation loop's
credential-pool recovery call: Hermes unpacks `(recovered, has_retried_429)`.
The disabled worker override now returns `(False, has_retried_429)`, preserving
that contract without entering the pool, refreshing credentials or switching
identity. The image-only fixture raises an actual SDK authentication error inside
the real pinned loop and requires a classified original cause and a normal
incomplete terminal result, rather than a masking tuple-unpack TypeError. This
corrects the observed wrapper defect; it does not establish why the original
provider request failed.

The request fixture also drives the actual OpenAI SDK over HTTPX MockTransport
through the real pinned constructor, conversation loop and request client. A
synthetic HTTP200 SSE error must yield the SDK's base APIError (closed
`provider_api`), classifier `timeout`, no HTTP error status and exactly the
existing three requests. Only numeric timeouts are inspected: the keepalive factory starts with connect
15, read unbounded, write 15 and pool 10 seconds, while the actual SDK request
extensions carry 1800 seconds for all four phases. The pinned request builder
passes `_resolved_api_call_timeout()` to the Codex transport, which forwards that
scalar and overrides the base client policy. The fixture separately asserts both
observations. Its OS-header discovery is an explicit Linux metadata fixture to
avoid Python invoking `uname`; the network/process audit stays active. No timeout is raised or
changed by this source change; the run and container wall deadlines remain the
outer containment bounds. Distinct closed HTTPX timeout/protocol/I/O categories
avoid conflating a server SSE error with a local socket read timeout.
