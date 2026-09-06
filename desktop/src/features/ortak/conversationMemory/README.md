# Conversation memory review

This UI uses the signed conversation-memory preview, approval, list and Stop
endpoints. A delivered plaintext Office message opens a separate More action;
the server remains responsible for canonical source, thread and current ACL
resolution. Project and employee choices come from the existing authorized
directories. Thread is the explicit default, and channel is a wider choice.

The server audience preview precedes an empty edited-text form. Content is never
copied from the message. Changing the selection or refreshing its observation
clears review. Async observations and callbacks are fenced by their current
client, source and selection. A current access refusal clears private derived
state; a source-only preview refusal leaves saved-fact recovery available.

One submitted operation retains identical serialized bytes and operation ID
across uncertain I/O and close/reopen of this mounted dialog. Retry is explicit
and rechecks current project role and employee access without requiring the old
source or new-admission expiry. Navigating away/unmounting is not durable browser
storage. The server's immutable operation receipt remains the replay authority.

Saved conversation facts have their own list, including a project-level recovery
panel that does not need a message. Source-hidden approvals withhold text and
audience details while keeping Stop using available under current reviewer and
employee permissions. Archived projects and inactive employees retain recovery.
Stopping use retains approval history and is not a physical-erasure claim.
Publication has its own explicit, initially unchecked confirmation. A fresh
fact observation retires that consent. The conversation export POST uses a
separate `{export}` receipt; approval and Stop continue to use `{fact,created}`.
The hook accepts only the exact approval, Stop, publish and publish/withdraw
retry routes. One retained path and serialized body blocks all other writes
until confirmation or a definite refusal; an uncertain retry reuses the same
operation ID and body. Export receipts validate fact identity, job metadata
and an advanced retry generation before releasing that request.

Saved facts display publication acknowledgement, cleanup and current runtime
eligibility from the server. There is no target or runtime opt-in control, and
publication never changes an operator setting. Failed withdrawal remains
retryable from project recovery after source loss, employee disable or project
archive, subject to the current reviewer/employee access ceiling. Stop using
does not claim remote erasure: only the saved withdrawal acknowledgement can
confirm reviewed-store text removal. Retries preserve the original remote job;
the UI sends its current retry version and respects the eight-cycle limit.

Prepared focused tests bind the actual React form/hooks and signed client, plus
the actual message menu/Radix focus handoff. They cover explicit review, exact
uncertain replay, scope races, access loss, source-hidden Stop recovery and
unsupported-message refusal. They do not claim native execution or PostgreSQL
authorization proof; the root integration lane runs these source tests and the
separate signed API/PG gate.

From `desktop`, the focused command is:

```sh
node --import ./test-loader.mjs --experimental-strip-types --test src/features/ortak/conversationMemory/conversation.test.mjs src/features/ortak/conversationMemory/menu.test.mjs src/features/ortak/conversationMemory/publication.test.mjs
```

The additive `publication.test.mjs` source exercises actual React controls,
both receipt branches of the mutation hook and the production signed client:
explicit consent, refresh invalidation, uncertain exact-byte retry, hidden-source
cleanup, retry-generation/foreign-receipt refusal, stale response fencing and
current access loss. Run it with the same loader. These new tests have not been
executed as part of the source-only implementation handoff.
