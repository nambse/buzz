# Selected workspace files in G74 recovery

Root completed an actual populated G74 capture and offline physical restore on
2026-09-06: bundle `214fd4f027a34604aeb7469d9dfb9a60`, restore
`cea594c6416d42f7a3403aa7509d2c70`. The workspace component verified 16 physical
entries; the named-volume journal retained two workspace runs and two terminal
calls with zero pending/invalid rows. Same original source services resumed;
no restored executor or provider was activated. See
[Current private G74 recovery selection](CURRENT_PRIVATE_RECOVERY74_2026-09-06.md).
Earlier statements about source `MAIN_SCHEMA_VERSION=73` or a `None` selection
below describe the retained pre-rollout checkpoint, not today's active selection.

The source core now imports these supporting modules through an optional
`workspace_files` component. `WORKSPACE_SELECTION=None` preserves the G73
component shape; historical frozen G73 operators remain unchanged. A populated
live74 capture requires root's current74 owners, explicit filesystem
selection and actual coordinated barrier, as exercised by the completed operation.
C2's six retained database tables
alone do not prove filesystem recovery. No physical erasure or automatic
activation is claimed.

## Ownership and integration

`scripts/ortak/private_recovery_workspace_files.py` exposes:

```python
receipt = capture(selection, fresh_private_output_directory, observe)
proof = verify(output_directory, externally_pinned_manifest_sha256)
# restore_workspace_files.py, under the same bounded offline recovery caller:
restored = extract(output_directory, externally_pinned_manifest_sha256,
                   fresh_unused_offline_directory)
```

`recovery_workspace_io.py` owns descriptor traversal and retained nonblocking
flocks; `recovery_workspace_layout.py` validates the bounded retained layout.
The caller must run capture in a bounded helper process under its recovery
barrier. The internal 30-second checks and byte bounds do not replace a process
watchdog when regular-file I/O blocks. If the process is interrupted, no returned
success receipt exists; the caller must refuse the partial output.

Ops owns the database projection and shared capture/restore insertion.
The production projection returns layout and the existing full-row hash evidence
in **one read-only REPEATABLE READ transaction**. A later unrelated layout query
cannot claim the earlier snapshot. Current frozen G73 operations remain independent.
The implemented composition requires the following for a populated G74 capture:

1. Explicitly select the new roots and reader identity in a newly approved
   preparation; close enrollment and workspace input admission.
2. Stop the selected worker/controller and prove exact reader absence. An
   expired lease, watchdog deadline, restored row or process-list error is not
   proof. Retain the original executable path, SHA, UID and execution identity.
3. Obtain drained DB74 evidence plus layout from the same transaction, and
   prove the selected cold controller journal has no pending/resolved tool
   delivery. Preserve its `workspace_runs` and `tool_calls` rows through the
   existing cold SQLite backup path.
4. Call the helper while the barrier remains held, retain its two output files
   and returned manifest hash in the outer bundle, and revalidate closure before
   releasing the outer barrier. Freeze these source files in that preparation's
   operator-code inventory.
5. On offline restore, verify the externally pinned file manifest and archive
   alongside the same database and SQLite evidence, then call `extract` into a
   fresh unused private destination. Require its physical readback proof before
   claiming the outer offline foundation complete. The helper does not register
   grants, reset partial copies, rewrite old paths or start any service. Later
   activation still needs original-owner containment, same-key reconciliation
   and an explicitly selected new configuration.

## Selection and callback contract

The explicit selection has exactly `company_id`, `input_root`, `run_root`,
`reader_binary`, `reader_sha256` and `reader_uid`. Paths are absolute, canonical,
disjoint and explicitly selected. No environment, profile, OAuth directory,
home directory or ambient credential discovery occurs.

`observe()` is executable trusted barrier composition, **not** a JSON file
containing an operator's assertion that processes stopped. It must reject live
or ambiguous writers/readers, nonterminal parent runs, unsettled actions and
pending journal delivery before returning:

```text
database_evidence: existing {schema_version:74, company_id, tables}
workspace_layout:
  company_id
  bindings: [{revision, grant_bytes}]
  runs: [{run_id, revision, manifest_hash, store_ref|null, status}]
  readers: [{id, run_id, revision, executable|null, executable_hash|null,
             operating_uid|null, state, stop_proof,
             created_at, owner_deadline, stopped_at}]
closure_evidence:
  format: ortak-workspace-files-closure/v1
  barrier_id: exact held barrier UUID
  selection_sha256: canonical selection hash
  database_evidence_sha256: canonical database_evidence hash
  journal_sha256: exact selected cold journal observation hash
  process_observation_sha256: actual selected process/containment observation hash
  workspace_journal_pending: 0
  live_reader_count: 0
  live_writer_count: 0
```

The callback is called before and after copying. Its entire canonical result
must remain equal. The helper independently verifies terminal run states,
stopped-reader chronology and exact retained executable/SHA/UID, and matches
binding/file/use/reader keys against the database witness. Hash equality binds
evidence; it does not manufacture containment authority. Raw grants are parsed
locally and are never printed or placed in the public receipt. Original grants
remain in the DB archive and exact per-run `manifest.json` copies.

Limits are 32 bindings, 64 run/revision pairs, 128 reader records and 1 MiB of
callback data; each grant keeps C2's 8 files, 16 KiB/file and 64 KiB total limits.
One physical run with multiple historical revisions refuses until explicitly
reviewed; it is never relabeled or reset. This first helper selects one retained
reader binary, at most 256 MiB. Different retained reader identities require a
new explicit selection contract rather than dropping their history.

## Filesystem and archive guarantees

Every ancestor is opened with no-follow directory descriptors. Ancestors must
be root/current-UID owned and not group/world writable, except root-owned sticky
temporary parents. Selected roots are current-UID 0700, with exact company
markers at 0400. Every regular file is current-UID, single-link and within its
exact mode and size ceiling. Selected root contents and revision/run children
must exactly match the retained layout; unknown names refuse before file reads.

Input revisions preserve selected UUID filenames and exact UTF-8/hash/size.
Admitted uses require a complete sealed 0500 run copy, exact canonical manifest,
0400 files and the stable empty 0600 `.lock`. Locks are acquired nonblocking and
held through final observation and sealing. Failed preparations may retain no
copy, only the lock, a complete unadmitted copy, or an exact `.preparing` subset.
Partial-file bytes must be a prefix of the original selected file/manifest;
their modes and bytes are preserved. No cleanup, chmod, repair or resurrection
occurs during capture.

The archive contains only `reader`, `inputs` and `runs` entries. The bound JSON
manifest retains exact modes, owners, nanosecond mtimes and SHA-256s. Ancestor
links, descriptors, directory listings and file identity/ctime/size/mode are
checked again after I/O and after the final callback. The archive digest is fixed
while writing; its original descriptor, parent link, owner/mode/nlink and byte
digest must still match after the callback. Mutated output never becomes a new
capture baseline. Output is O_EXCL and fsynced.
A failed seal retains a separate failure marker; verification rejects the marker
even when a complete-looking manifest exists. Verification requires the outer
bundle's exact manifest hash and checks every tar member and byte; links,
duplicates, traversal, unexpected metadata and changed archive bytes refuse.

`restore_workspace_files.py` repeats the pinned archive check, then creates only
the selected archive entries using no-follow parent descriptors and exclusive
file creation. It does not reopen original source paths; they may be unavailable
at restore time. The destination must be empty, current-UID 0700 and disjoint
from the bundle and every original selected path. This initial contract preserves
the source UID, so a different-UID restore host refuses rather than silently
adopting ownership. Allowed retained group metadata, file modes and nanosecond
mtimes are applied only to newly created descriptors. Every restored file and
directory is then read back and compared to the externally pinned manifest,
including exact names, reader/input/run/partial bytes, hashes, modes and owners.
No executable is launched. A failed tree keeps a failure marker and cannot be
retried in place. The outer caller must durably retain the returned readback proof.

## Validation

```sh
python3 -m unittest discover -s scripts/ortak -p test_private_recovery_workspace_files.py -v
python3 -m unittest discover -s scripts/ortak -p test_restore_workspace_files.py -v
```

The 19 capture/verify and 8 physical restore tests pass on fresh synthetic local files. They cover exact
reader/input/run/lock bytes, failed preparation prefixes, actual flock contention,
link/path/mode/size refusal, crossed company and incomplete projections,
expired-lease versus stop proof, live journal/process refusal, changing closure,
same-size writes, parent exchanges, archive mutation during and after capture,
descriptor owner checks and failed directory fsync.
Restore regressions remove original source paths, preserve a failed preparation
prefix and its retained lock, inject destination symlinks/parent swaps, alter
copied bytes/modes/hardlinks before production readback, and interrupt writes.
No live workspace, OAuth material, provider, database or service is accessed by
these tests. The same-snapshot DB projection and held-barrier integration must
pass their own scoped gates before any selected populated G74 capture.

## Historical composed fixture evidence

The final scoped readonly55432 and physical filesystem receipt is
`/private/tmp/ortak-v0-evidence/g-workspace-files74-11304a7619714bd5a58c1efc57da5bd1/receipt.json`.
Two bindings, two runs and three stopped reader histories were projected under
a real PostgreSQL shared schema lease; exact production-seeded grant bytes bound
the synthetic inputs and copies. The watched helper archived a pinned real reader
binary without executing it, and the outer restore component physically extracted
and read back the archive. Original fixture roots were then moved aside and archive
verification still passed. Docker/application containment was a fixture adapter;
this does not claim a populated live74 pause/capture or independent disaster recovery.
That source checkpoint's operator closure contained26 modules. The later actual
volume capture used the separately frozen 28-file closure. The composition readiness receipt
records141 focused tests and separate source reviews, including the last durable
child PID/start and containment-failure receipt change.
