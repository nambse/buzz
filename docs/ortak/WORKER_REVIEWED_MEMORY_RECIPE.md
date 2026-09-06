# Explicit reviewed-memory worker selection

This documents the schema76 source configuration. It does not select or modify
a live worker. The selected binary, migrated schema, memory adapter and current
ownership evidence must pass their deployment gates together.

Start with the full employee entry exported by the
[prepared memory bootstrap](../../runtime/private-stack/MEMORY_BOOTSTRAP.md#export-the-shared-activation-and-worker-receipt).
Keep its employee ID, exact memory binding, complete original `creation_receipt`,
creation key and diagnostic run/time. These fields are not replaced by the
selection fragment below. The outer recipe still requires explicit
`validate_memory_io: true`; credential fields remain opaque references and
environment-variable names, never secret values.

## Three independent selections

Each entry in the outer worker configuration's `memory.employees` array can
contain these fields. All three default to empty when omitted.

| Field | Explicit meaning |
| --- | --- |
| `reviewed_projects` | Project allowlist for owned reviewed-record publication and reads. At most16 projects per employee and128 total in the recipe. A nonempty list requires the complete original creation receipt. This alone does not enable runtime consumption. |
| `reviewed_runtime_projects` | Subset of `reviewed_projects` enabling the existing project-fact context for that employee's Work runs. It does not enable conversation facts or Office recall. |
| `reviewed_conversations` | Explicit `{project_id, channel_id}` mappings enabling conversation-fact selection for canonical human Office input and eligible Work promoted from that channel. Each project must also be in `reviewed_projects`; membership in `reviewed_runtime_projects` is independent. |

This is a partial employee-entry example using synthetic UUIDs. It enables
conversation selection while leaving project-fact runtime selection empty:

```json
{
  "reviewed_projects": ["11111111-1111-4111-8111-111111111111"],
  "reviewed_runtime_projects": [],
  "reviewed_conversations": [
    {
      "project_id": "11111111-1111-4111-8111-111111111111",
      "channel_id": "22222222-2222-4222-8222-222222222222"
    }
  ]
}
```

There are at most16 conversation mappings per employee and128 across the whole
recipe. UUIDs must be nonzero. A project and a channel may each appear only once
within one employee's mappings: there is exactly one selected project for that
employee/channel. The same channel cannot select two project namespaces. The
current database project/channel binding must agree. Unknown fields, including
a caller-supplied thread root, are rejected; the source resolver derives thread
identity from canonical message ancestry.

Upgrading the binary or retaining an old recipe does not opt anyone in. Adding
an explicit mapping requests conversation consumption for that selection;
after current owned-resource and actual-I/O validation, the worker advertises
its separate conversation flag, channel and epoch. The existing advertisement
lives at most55seconds and is refreshed at most every25seconds. Configuration
and advertisement must both agree at selected recall. An old advertisement or
an ordinary health result is not an execution witness.

## Publication and model changes

Recipe selection never approves or publishes a fact. An authorized human must
approve the edited text for its channel/thread audience and separately request
publication. Runtime selection requires the exact owned publication
acknowledgement, current fact/source permissions, canonical requester, matching
audience, active scope/target epochs and the current adapter ownership/I/O
witness. A local fact is never substituted when selected Honcho recall fails.

Office uses conversation facts only. Promoted Work may include matching
conversation facts and, only with its separate `reviewed_runtime_projects`
entry, project facts. Manual Work retains the project-only path. Selection is
central; employee runtimes do not subscribe independently or choose a project
from their input.

A model-only employee revision can preserve previously acknowledged memory
when the same employee, Office identity, exact current memory binding and
lifecycle remain valid. Keep the original memory creation receipt and native
resource identity; changing a model is not authority to create, copy or relabel
memory resources. The current source, opt-in, epochs and actual selected
ownership witness still apply, and each run keeps its immutable runtime
revision and snapshot.

Removing a conversation mapping retires its consumption epoch. Explicitly
re-enabling it can authorize a new run; it cannot revive an old frozen use.
Opt-out or source/ACL loss does not claim remote deletion. Human Stop using and
expiry use the retained publication's exact withdrawal identity and durable
cleanup receipt. Missing current source does not erase recovery metadata.

The configuration contract is implemented by
[`MemoryConfig::validate`](../../crates/ortak-server/src/worker_memory.rs),
[target advertisement](../../crates/ortak-server/src/worker_memory/reviewed.rs)
and the [selected recall boundary](../../crates/ortak-server/src/worker_memory/selected.rs).
The [D4 runtime contract](CONVERSATION_MEMORY_D4_RUNTIME_CONTRACT.md) defines
the v4 pins, budgets and final-use checks.
