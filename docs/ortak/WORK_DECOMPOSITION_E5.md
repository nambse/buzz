# Independent child work

Architecture v0 §7 and Remaining Work E require parent/child decomposition.
E5 adds explicit creation of a new manual child plus authorized structural
navigation. This source is awaiting its SQL70 and production PostgreSQL gates;
it is not a claim that the deployed product already supports decomposition.

`POST /api/v1/work-items/{parent}/children` uses existing NIP-98 authorization
and the current Office, channel and project contributor/owner grant. Its body is:

```json
{
  "operation_id": "a fresh UUID retained for retries",
  "expected_version": 7,
  "child": {
    "title": "Independent task",
    "description": "Explicit child context",
    "priority": "normal",
    "criteria": ["Human accepts this result"],
    "approvals": [{"gate": "review", "required": true}]
  }
}
```

The actor, project and child ID are derived by the server. The child has no
inherited source message, attachments, assignment, runtime context, dependency,
review decision or approval. It starts Proposed at version 1. Its independent
human acceptance never satisfies the parent, and neither item's completion or
cancellation cascades. Existing explicit dependencies express blocking work.

The company-owned `work_decomposition` relation is immutable. It permits one
parent per child, at most 32 direct children and depth 8 (root depth 0). Under
Office shared authority, operation identity and project-exclusive locks, the
transaction reserves an unused child ID before creating it, advances the parent
once, and records both histories and one signed operation receipt. Deferred
child/receipt foreign keys and a final consistency guard reject missing rows,
torn history or existing-item attachment. Update, deletion and truncate are
refused. A failed commit leaves no child or parent change. Lost acknowledgment
reuses the same operation and returns the same currently authorized child.

`GET /api/v1/work-items/{item}/decomposition` returns `work_item_id`,
`work_version`, an optional visible `parent` and at most 32 visible `children`.
Each endpoint's canonical source remains independently authorized. Hidden
endpoints are omitted, including their IDs, titles, ancestry and counts. Generic
history omits structural relation events just as it omits dependency targets.
An independently defined child remains readable under its own grant when its
parent source is no longer visible. These links do not broaden runtime or memory
context.

The desktop panel offers the independent form and current parent/child
navigation. Writes share the existing exact-body operation recovery hook.
Read failures clear linked content; five bounded retries end with explicit
recovery, and authority failures stop retries and clear the Work surface.
Completed/archived or read-only views retain navigation without creation.

Validation at the coherent source checkpoint:

- Six new actual client/hook/component tests passed; the current complete Ortak
  desktop matrix passed 85/85 and TypeScript passed.
- The initial domain/core/API and positive PostgreSQL test snapshot compiled in
  the central all-target lane; two domain tests are written.
- Six production PostgreSQL cases are written: atomic concurrent replay and
  independent human acceptance; hidden-parent/current-role/company/channel
  authority; concurrent parent CAS plus depth/count/terminal/archive limits;
  receipt rollback and direct existing-child/mutation refusal; held/live parent
  execution revocation without child dispatch; and populated canonical community
  purge with retained evidence and current read/admission refusal.
- The final test snapshot still needs central compilation and those SQL70
  PostgreSQL cases must pass before SQL70 becomes immutable. The current SQL
  proposal SHA-256 is `57c74fb44b7242a06183256759b3415e8555889503ea422c60de6d54760936af`.

Root owns final SQL numbering, migration/desired-schema/reconciler integration,
disposable PostgreSQL execution, schema parity and native/deployment acceptance.
