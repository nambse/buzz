# Schema74 workspace database recovery and deletion boundary

The actual populated G74 capture and offline foundation restore passed on
2026-09-06: 135 main tables and 19 Honcho tables, with retained workspace rows
bound to physically restored files and the current named-volume journal.
Bundle `214fd4f027a34604aeb7469d9dfb9a60` and restore
`cea594c6416d42f7a3403aa7509d2c70` preserve that evidence. Original source services
resumed; restored execution remains disabled. See
[Current private G74 recovery selection](CURRENT_PRIVATE_RECOVERY74_2026-09-06.md).
Earlier statements about source `MAIN_SCHEMA_VERSION=73` or a `None` selection
below describe the retained pre-rollout checkpoint, not today's active selection.

## Historical populated filesystem composition checkpoint — 2026-09-06

The source operator now includes an optional `workspace_files` component.
`MAIN_SCHEMA_VERSION=73` and `WORKSPACE_SELECTION=None` still select the existing
G73 behavior; all previously frozen G4/G73 operators and archives are untouched.
This is readiness for a later explicitly selected74 operation, not a live74
capture or an instruction to reuse the previous process registry.

`observe_workspace_layout` returns canonical grant bytes, retained run mapping,
reader identity and the complete row-hash witness from the same bounded
REPEATABLE READ READ ONLY transaction. A live `HeldBarrierWitness` rechecks the
original stopped writers, exact Linux lease container generation, current
PostgreSQL schema lease, cold SQLite tool journal and exact host reader absence
before and after copying. A serialized witness cannot call this seam. Each
observation gets fresh exclusive diagnostics paths. The entire callback result
must remain identical. Populated workspace rows require an explicit selected
input root, separate run root and reader path/hash/UID; a full bundle refuses
workspace rows from another company that this single selection cannot cover.

The cold journal rejects pending/resolved calls, retained transient result text,
unknown states or missing parents. Terminal rows remain hashed and preserved.
Every file-copy, journal-read, verification and physical extraction child has a
60-second bound plus exact group kill/reap. Before admitting paths it records
PID, UID, OS start identity and operator hash in a fresh operation output.
Failure to confirm containment leaves a retained `containment_unconfirmed`
receipt requiring root reconciliation. Sealed source bundles receive no such
new writes.

Offline preflight verifies the file manifest against the captured database
witness and explicit preparation selection. The outer foundation cannot report
success until `restore_workspace_component` physically extracts the reader,
inputs and run copies into a fresh unused private directory, reads back exact
bytes/modes/UID/GID/mtime and durably records `workspace_files_restored_offline`.
Original absolute paths are never restored in place. Runtime activation and
physical erasure remain false.

The new frozen operator closure has26 files: the previous21 plus
`private_recovery_workspace_capture.py`, `private_recovery_workspace_files.py`,
`recovery_workspace_layout.py`, `recovery_workspace_io.py` and
`restore_workspace_files.py`. A new74 source selection still needs the actual
post-rollout owners, controller image/config receipts, public helper/config
hashes, eight-binary artifact set and explicit workspace roots. Semantic and
additional employees are not implicitly admitted. Current source deployment
receipt checks remain bound to73 until root supplies the74 handoff.

Validation includes103 focused recovery tests,20 preparation tests,10 legacy
restore compatibility tests and8 physical extraction tests. The exact populated
readonly55432 fixture also captured two bindings, two runs and three stopped
reader histories into a real file archive and physically restored it under a
real PostgreSQL shared schema lease. Its synthetic files match production-seeded
canonical grants; the selected frozen reader binary is copied but never run.
Docker/application containment in that fixture is an explicit adapter, so this
is scoped database/filesystem composition evidence, not a full production pause
or independent disaster recovery. The capture and restore remain valid after
the fixture's original paths are moved aside. See the final source/receipt pins
in `G74_WORKSPACE_COMPOSITION_READINESS.json` under the private evidence directory.

A separate long-path process scanner probe correctly detected a copied system
`sleep` fixture, but macOS left that one process in `UEs` after SIGKILL and the
bounded reap failed. That experiment is a retained containment failure, not a
passed process lifecycle check. No live stack process was involved and no more
copied-system-binary probes were launched. Subsequent production Python child
fixtures passed normal containment; a falsifiable negative test retains the
unconfirmed state rather than claiming success.

## Earlier database-only checkpoint

This source slice reviews the exact six-table contract from proposal74 SHA
`1dc560c062aeb4f7e3076c9ce21357674166b99c6536639aae938a00e4bb9f99`.
Root owns immutable migration74, desired-schema parity, builds and rollout.
`MAIN_SCHEMA_VERSION` remains73. No live pause, capture, filesystem enrollment,
provider call or migration is part of this change.

| Retained table | Exact witness key after company_id |
| --- | --- |
| workspace_bindings | id |
| workspace_files | workspace_id, id |
| run_workspace_uses | run_id |
| workspace_tool_actions | run_id, call_id |
| workspace_tool_receipts | run_id, call_id |
| workspace_reader_executions | id |

All six remain in the canonical community inventory, immutable-evidence
retention list and universal write fence. None enters purge order. An older
approved inventory cannot acquire this new scope. `begin_quiescing` holds the
schema shared lock and community exclusive lock, uses READ COMMITTED, and
refuses unresolved readers, pending/result_ready or leased actions, and any
nonterminal parent run referenced by either a use or a preparation reader.
Refusal rolls back before the community fence: recovery can still settle the
owning runtime and readers. A deadline is never a stop proof. Purge thereafter
preserves complete rows, including original grant and result BYTEA values.

The G witness reads a bounded repeatable-read snapshot. It hashes complete
rows, checks all company/community/parent identities, exact canonical grant
bytes and file roster, result bytes and digest, original read execution lease,
and the successful stopped preparation that admitted a run use. A result
receipt's original attempt may be lower than the current retry attempt; its
lease need not match an action whose later ACK cleared the lease. A stopped
failed preparation without an admitted run use is valid retained history.
Current revision availability, later revocation and offline expiry do not
renew or invalidate terminal historical evidence. Restore compares exact full
row hashes and retains a closed activation gate.

Database74 support does not establish a complete populated workspace capture.
Before root advances the selected inventory, an initial empty workspace scope
must be verified. Before any populated capture, root must explicitly select
the immutable input and separate company/run-copy roots, canonical manifest
mapping, exact reader executable/hash/UID, and controller journal containing
`workspace_runs`/`workspace_tool_calls`. Close enrollment and input admission,
stop every writer and reader, settle pending/resolved journal results, and
preserve same-key ACK recovery. Future stopped readers cannot be inferred from
expired leases, empty process lists from another namespace, or a missing PID
alone. Restore cannot activate a reader/executor before original containment,
root/file identity verification and journal reconciliation. Semantic8651 and
additional employees require separate explicit inventories; they are not
admitted by this database revision.

The future operator closure gains only `private_recovery_workspaces.py`; all
historical frozen20 G4 operators remain immutable. Schema69–73 contract shapes
stay unchanged, partial74 inventories and unknown newer versions refuse.

Validation at this source checkpoint:126 recovery/restore Python tests pass. They bind
production query/observe/restore and exact counter-set refusal; they are not
actual PostgreSQL or filesystem recovery evidence. Root compiled the new
canonical purge test:

```text
work::workspace::retention::workspace_canonical_purge_requires_stop_and_preserves_all_six_tables
```

The prepared executable `scripts/ortak/rehearse_private_recovery_schema74.py`
accepts exact frozen bootstrap/server binary paths and SHA256s, creates only
two new retained `ortak_g_obligations_<uuid>` databases on disposable55432,
runs that signed workflow with controlled adapters, then tests bounded
rollback-only row faults and real pg_dump/pg_restore. It checks full catalog,
row bytes, settings, sequences and all six witnesses. The source-positive
fixture keeps production guards enabled; only explicit negative faults use
transaction-local replica mode and roll back. It never accesses live55433 or
workspace input files.

Actual PostgreSQL execution passed19 checks over135 tables. The final receipt is
`/private/tmp/ortak-v0-evidence/g-schema74-68b91b14beac4d6cbd4d6201a43735fc/receipt.json`.
It binds root's immutable74 bootstrap SHA
`ffecacf6be030725854350881f14d9c1604c003f437ce75ce0becb4ac6baad36`
and signed server-test SHA
`f16bf5870adbb621cf100b7e53b110378a153fdd64bb75682d44e746cbfefdfe`.
The preserved company has two bindings, two files, one use, one action, one
result receipt and three stopped reader executions, including failed prepare
without use. Another company stays active and unchanged. Both new databases
and archive are retained. Twelve scoped negative faults refuse capture;
expired/revoked terminal history remains valid. Real archive restoration
preserves full metadata, all row bytes, settings, sequences and both companies'
exact evidence while leaving activation closed.

The earlier passed attempt
`/private/tmp/ortak-v0-evidence/g-schema74-80b3ba76bcb6471c87274b2ea336cf0f/receipt.json`
is retained. The final attempt repeated the same gates solely to include the
new `workspace_catalog.py` transitive import in the before/after source hash
receipt. It did not broaden live authority. This remains database recovery
evidence, not workspace filesystem capture, provider execution or full-stack
failover proof.
