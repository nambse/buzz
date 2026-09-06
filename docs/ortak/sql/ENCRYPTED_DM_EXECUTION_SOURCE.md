# Protected execution source handoff

This is an inactive source candidate. Assemble `encrypted_dm_jobs.sql`, then
`encrypted_dm_admission.sql`, then `encrypted_dm_execution.sql`. No existing
numbered migration changes. Construction of `PgConfidentialExecution` or
`EncryptedExecution` installs no subscriber, worker, selected pair or key.

`EncryptedExecution::{dispatch_once,observe_once,seal_reply_once,publish_once}`
each handles at most one due operation. The dedicated leases retain exact run,
generation and attempt metadata. Dispatch has three attempts with 1s/5s backoff;
observation has a finite generation ceiling, three consecutive failure limit and
three reserved cancellation attempts. Reply copies each have three attempts and
5s backoff. A lease lasts at most 30s; external operations are bounded to 5s and
the original ten-minute execution deadline never renews. Failures either retain
retry/containment obligations atomically or propagate.

Current Office, pair, source, revision/lifecycle and immutable identity checks
precede selecting the wrapped key or opening any content. The caller retains the
Office/source transaction fence during the bounded protected effect; deferred
checks refuse an expired current operation at commit. A lost response is handled
by keyless lookup and exact frozen snapshot replay. Only a newly selected current
start derives snapshot/runtime-event keys; neither the Office key nor per-run
master enters the bridge transport. The selected policy must actually be empty.

Protected events are copied with their original envelope bytes and timestamp
before any opening. The initial bridge grammar is exactly run.started, optional
one final assistant.delta, delivery.intent, run.completed: three silent events or
four reply events. Strict canonical inner parsing binds full identity, sequence,
time and final intent. It rejects tools, workspace and other event kinds. Reply
text is sealed into one protected draft before signing. The purpose-selected
Office provider creates one NIP59 rumor and two immutable encrypted wrappers,
human delivery and employee history, frozen atomically with both outboxes.

The NIP42 publisher accepts an explicitly selected bare relay origin, validates
the exact frozen copy and intended recipient before connecting, and sends it
only after an acknowledged purpose-specific authentication challenge. It bounds
frames, bytes and time, performs no reconnect, and never rewrites a failed copy.
Every retry rechecks current authority. Each accepted copy keeps its own ACK;
loss of the other ACK retries only that same frozen event. An ACK is receipt
accounting, not renewed publication authority. Its atomic UPDATE requires the
exact still-pending copy, token and generation; it may settle after lease expiry
only if no takeover occurred. Non-ACK retries still require a live lease.

The four new tables are `confidential_execution_leases`,
`confidential_event_receipts`, `confidential_reply_bundles`, and
`confidential_reply_outbox`. Each keeps the universal community fence. Execution
states are observing, sealing, cancelling, complete, stopped and unconfirmed.
The latter three are retained terminal proof; complete may only move to
cancelling for an actual subsequent cancellation obligation. Stopped requires
an acknowledged keyless runtime cancellation. Unconfirmed always prevents a
drained recovery/deletion claim.

At a drained terminal boundary every confidential run has exactly one dedicated
dispatch and execution row. Failed/cancelled dispatch requires stopped execution
and the cancellation ACK even with zero dispatch attempts: cancel-by-start-key
installs a prestart tombstone. There is no inferred never-started omission.
Complete requires exact protected terminal receipts and, for a reply, the draft,
bundle and two copy obligations. Copy states pending, acked, failed and retired
distinguish unresolved delivery, actual acceptance, exhausted failure and an
authority-retired unsent obligation. No existing ACK or source receipt is erased.
The consumed decrypt job retains its original verified lease/claim tuple.

Ordinary preparation, event cursor lookup, reconciliation, cancellation claim,
and the ordinary worker run selector explicitly require payload_mode ordinary.
This prevents ordinary I/O before discovering the absence of plaintext context.
The protected cancellation lane owns its keyless settlement and mirrors the
existing human cancellation receipt. Ordinary format 1–5 behavior and existing
ordinary sinks remain unchanged; protected content still cannot enter them.

Prepared source regressions, to be executed only by the root integration gate:

- Runtime unit filter `encrypted::tests`: three strict inner/fold cases.
- Office unit filter `encrypted::key_provider::operations::tests`: two actual
  NIP59 selected-purpose/two-copy cases.
- Ignored PG filter `confidential::execution`: two cases using the existing
  actual admission fixture and explicit synthetic key leaf. The first includes
  bounded loopback HTTP lost-start recovery plus a real NIP42 WebSocket with one
  lost ACK, an independently accepted history copy, and byte-identical retry.
  A rolled-back legitimate short-lease case executes the exact production ACK
  statement: expired pending retry and mismatched takeover are refused, while
  the unchanged expired claim retains its known ACK without changing its deadline.
  The second revokes current membership, proves ordinary claims omit the run,
  and completes keyless stop with no usable unwrap/decrypt provider entry.

These tests require the three candidate SQL fragments on a disposable database
and the prior fixture's explicit synthetic key selection. Their source is not a
claim of passing execution. Root owns final migration, bridge installed-image
selection, daemon composition, recovery/deletion integration and live acceptance.
