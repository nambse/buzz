# Reviewed-memory erasure before community deletion

## Proposal71 retained runtime use evidence — source ready

`run_reviewed_memory_uses` is now in the canonical exact scoped and retained
inventories, with no entry in the purge order. Its permanent community FK,
immutable/no-delete/no-truncate policy and universal fence remain mandatory.
The source unit regression rejects a pre71 approved manifest and a manifest
that omits this new table's fence. This gives no new external cleanup authority:
the same69 exact withdrawal ACK and pending-publication lease guard still runs
before community quiescence. The integration owner is compiling/testing the
populated71 publication→Work use→Stop/withdraw ACK→canonical purge fixture;
that new result is pending, distinct from the earlier69 pass below.

## Retained69 integration evidence

Status updated 2026-09-06: canonical Rust guard and inventory implemented and
the production populated exporter/deletion PostgreSQL regression passed in the
retained disposable database `ortak_reviewed69_eddd88280fe844c483bcdff749ae3039`.
The integration owner's receipt is
`/private/tmp/ortak-v0-evidence/reviewed69-test-4287bb317a904ab18216e29e3199616e/test-receipt.json`:
three unit tests and nine actual PostgreSQL tests passed, with no provider calls.
Its migration ledger was68 plus the exact SQL69 proposal
`744b7d3f5e5df7512dfc487ee020b513fb280947905dc9f4ab0a835bd79c3be3`.
Immutable migration69 integration and the later private rollout passed separately.
No live community deletion or cleanup was performed for this source change.

The universal community write fence applies to all five SQL69 export tables.
Once `begin_quiescing` commits, the exporter cannot persist further withdrawal
progress. Cleanup therefore must finish while the community is still active.
Local erasure of Office rows is not proof that the separate reviewed store has
erased a published fact.

`DeletionStore::begin_quiescing` now acquires the shared schema lock before any
request-row lock, validates the live catalog and exact approved inventory, then
takes the exclusive community deletion lock. Under that lock it requires each
scoped export to have an acknowledged withdrawal job and matching immutable
receipt: company/community/fact/action, canonical idempotency key, request hash,
target binding hash, lease token, total attempts, terminal remote status,
tombstone and `erased_from_reviewed_store=true`. Pending publication leases also
block the transition, even after their deadlines, because expiration alone does
not establish external containment. Historical lease fields on acknowledged
jobs do not count as active leases. The query requires READ COMMITTED so its
snapshot follows lock grant.

An unresolved export returns
`DbError::ReviewedMemoryExportsNotDrained { community_id,
unacknowledged_exports, leased_publications }`. The transaction rolls back
before archive/state changes, leaving cleanup possible. The existing executor
records this as a durable retryable drain condition. No automatic cleanup,
withdrawal scheduling or acknowledgement is manufactured by deletion. The same
read-only proof is checked again in `fence` and `purge_postgres`.

The five tables `reviewed_memory_targets`, `reviewed_memory_exports`,
`reviewed_memory_export_jobs`, `reviewed_memory_export_commands` and
`reviewed_memory_export_receipts` are in the exact expected and retained
inventory. They are absent from the purge list. Their community foreign keys
retain permanent tombstone provenance; their universal fences remain required.
An old approved catalog cannot acquire this new retention/destruction scope.
Source targeting SQL69 intentionally refuses deletion against an older catalog.

The production exporter fixture in the server integration suite exercised
live/future-withdrawal refusal with active community preserved,
unknown publication lease refusal, explicit Stop using plus real scheduler
withdrawal ACK, then canonical quiesce/fence/purge with all five tables unchanged,
unrelated-community isolation and post-fence writes rejected. Rust formatting,
scoped diff checks, the integration owner's all-target compile and the actual
PostgreSQL regression passed. This does not authorize deletion of a live community.

This deletion rule differs from [backup recovery](PRIVATE_FULL_STACK_RECOVERY_PLAN_2026-09-05.md):
a backup can retain a future scheduled withdrawal obligation under a declared
offline recovery plan. A destructive community fence cannot strand that same
obligation by making its final cleanup unwritable.
