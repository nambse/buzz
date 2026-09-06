# D4 runtime integration contract

Root integration decision, 2026-09-06. Migration75 passed real migration versus
desired-schema parity, twelve resolver/epoch PostgreSQL cases and three signed
conversation API cases. The live stack remains74. Additive migration76 is now
allocated to conversation publication, runtime v4 admission and current-use
guards. Never rewrite the applied75 or earlier migrations.

This document narrows the existing D4 plan for parallel implementation. It does
not claim deployed publication, recall or user acceptance.

## Immutable v4 wire

Preserve versions1–3 byte-for-byte. Add optional `conversation` to SnapshotWire,
omitted from all legacy writes. Version4 has `conversation` and no legacy
`reviewed`; its ordered records can include existing project records for a
promoted Work run. Office contains conversation records only. At least one
conversation record is required for4; otherwise retain scratch/legacy3 behavior.

`ReviewedConversationContext` contains `origin`, `records`, and `truncated`.
`ConversationMemoryOrigin` contains `requester_public_key` and `provenance`.
The latter is an exact canonical v1 provenance JSON **string**, derived with a
thread audience from the run's current human Office message or promoted Work's
retained source. It includes the exact source/root partitions and source digest.
It is historical provenance, never current authority. Constructors remain
crate-private; decoding validates canonical bytes and subsequent database checks
re-resolve it under current authority.

Each ordered record is a closed tagged union:

```text
{scope:"project", record:ReviewedMemoryRecord}
{scope:"conversation", record:ReviewedConversationRecord}
```

The project record/pin is unchanged. The conversation record has `pin`, `content`
and `provenance` (the fact's exact canonical v1 provenance JSON string).
`ReviewedConversationPin` explicitly contains all existing ReviewedMemoryPin
fields, plus `conversation_audience_hash`, `conversation_authority_epoch` and
`conversation_consumption_epoch`. Its legacy `consumption_epoch` is exactly0,
the already reviewed75 storage sentinel. Do not use flatten/unknown-field
fallbacks. The audience hash and source hash must agree with canonical
provenance; scope tuple must match origin and a thread root must match exactly.

Render project records as existing `reviewed_project_memory`; conversation
records as `reviewed_conversation_memory`, both `untrusted_data`. Keep the
current Hermes RunSpec/bridge wire. Each rendered string is at most8192 UTF-8
bytes. All reviewed records together are at most8 records/8192 content bytes;
reviewed plus scratch remains8 records/16384 content bytes. Reject duplicate
fact IDs across both variants. Use ordinals follow the ordered reviewed records,
independent of scratch prefix. Keep original snapshot bytes on decode/retry.

## Current origin and selection

Office selection comes only from configured employee/project/channel mappings,
with one project per employee/channel. A canonical human source is mandatory;
employee-origin delegation cannot gain conversation context. Work requires its
retained source and requesting human. Manual Work keeps its legacy project-only
behavior. Never derive a project from arbitrary model input or use the delivery
chain root as a canonical thread root.

Add a central PostgreSQL origin resolver taking company, run and explicit
project. It resolves the human and source from retained run/Work/Office rows,
then invokes75's canonical thread-source resolver. Return requester public key,
exact provenance bytes and current deadline. The runtime parses these bytes into
the sealed origin; no caller-supplied requester/root/evidence hash is accepted.
The SQL implementation must also preserve current central routing/recipient and
Work authority checks; an origin alone is not dispatch authority.

After audience and publication eligibility, inspect at most32 candidate facts,
ordered thread, channel, project, then stable fact ID. Select the final at most8
IDs/8192 content bytes centrally, also checking encoded record size. Send only
these final IDs to the existing Honcho selected recall. Reorder verified remote
results to that selection. Missing remote results remain missing; never use
local fact text as fallback. All publication ownership/I/O witness checks remain.

## Atomic persistence and later use

Reuse75's target fields and three nullable use pins. Lock Office, project,
optional Work, conversation authority, sorted facts, sorted targets, run,
outbox. No provider I/O occurs under these locks. Re-resolve the exact origin
and every fact source, check scope/target epochs and deadlines, and freeze the
snapshot with its immutable use rows. Same-run retries reuse the exact winner.

Migration76 extends current-use and snapshot/admission guards for both Office
and Work. Preserve72's whole-document escaped-NUL comparison and original byte
accounting. Close the current Work-only SQL join/admission/reconciliation gaps.
All output/delivery/post-ACK writes and visible memory projections must consult
current conversation use. Receipt-only correlation/cancellation remains possible
after revocation without renewing execution or output authority.

Conversation publication uses the existing exact owned target and retained
publish/withdraw keys with the audience-bound source hash. Source/permission
loss changes current use; it does not fabricate remote deletion. Explicit Stop
and expiry retain the existing durable withdrawal job and irreversible tombstone.
Recovery and deletion inventories must retain75's two tables and new use pins;
live rollout waits for exact76 parity, recovery inventory and bounded flow proof.
