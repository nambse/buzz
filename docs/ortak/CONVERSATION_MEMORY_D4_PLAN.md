# Conversation-scoped reviewed memory — D4

Status: migration75 and reviewed conversation preview/approval/list/Stop source
verified,2026-09-06. Real migration-versus-desired parity passed across15 catalog
components with two identical reconciliation passes. Twelve canonical resolver/
epoch PostgreSQL tests and three signed API tests passed on the actual75 ledger.
Eleven focused desktop checks, TypeScript and Biome passed. These checks did not
deploy the feature; the live stack still serves74 and its pinned artifacts.

Root allocated76 for conversation publication and runtime v4, now being
implemented under the [runtime contract](CONVERSATION_MEMORY_D4_RUNTIME_CONTRACT.md).
75 and earlier migrations are immutable. Source now depends on76 while that
integration is incomplete;75's successful evidence is tied to its earlier
compiled binaries and retained parity receipt. No new container/image,
credential or live resource was created for this increment. The current rollout
receipt, not this plan, establishes which artifacts are running.

[Architecture §4.5 and §8](ARCHITECTURE_V0.md) require explicit scopes, provenance
and permission checks before dispatch and at use. [Remaining D](REMAINING_WORK_V1.md)
includes conversation and employee memory. D2c's actual project-memory
publication, Work recall and withdrawal are recorded in the
[continuation ledger](CONTINUATION_PROGRESS_2026-09-05.md). The historical status
paragraphs in the D2b/D2c design documents are not the latest deployment state.
The broader boundary remains [the memory/DM plan](MEMORY_AND_DM_NEXT_D_PLAN.md).

## First delivery and explicit exclusions

Add **new human-reviewed conversation facts** for one employee in an existing
project's bound stream channel. The reviewer chooses either that channel or one
canonical thread inside it. Thread is the default and channel-wide use requires
an explicit audience choice. Storage remains the real Honcho project namespace;
the narrower conversation audience is enforced centrally before every selected
read and use. This does not invent a project for employee-wide storage.

The first fact source is one currently visible, decided plaintext Office message
in that same channel. The human edits its fact text; no source body is copied
automatically. Artifact-sourced conversation facts, cross-channel sharing, DMs,
company truth and employee/relationship experience are separate follow-ups.
Existing project facts, exports, uses and snapshots retain their original meaning
and exact bytes. Nothing is reclassified or published during migration.

New conversation facts may be used by explicitly enabled human-origin Office
runs in that audience and by Work promoted from an authorized message in that
audience. Manual Work, independent children without a source, Work in another
thread and employee-origin delegation do not gain this context. Ordinary project
facts remain Work-only; adding Office conversation recall does not expose them
to Office. RunScratch behavior and central semantic-scoring input stay unchanged.

This delivers conversation memory, not all Remaining D. An owned non-project
employee store and an explicit human identity for relationship memory still need
their own approval/sharing, runtime, Honcho, UI and retention contracts. Automatic
extraction, embeddings, derivers and peer-global representations are optional;
they are not prerequisites for this reviewed, deterministic lexical slice.

## Canonical audience and provenance

The authenticated request supplies the project, employee, source message ID and
`audience.kind` (`thread` or `channel`). It never supplies a community, thread
root, root timestamp, author, target, credential, run or provider endpoint.
For a thread fact, the source message also anchors the intended thread.

The server resolves and freezes:

```text
audience format = ortak-reviewed-conversation-audience/1
company_id, community_id, project_id, employee_id
channel_id
kind = channel | thread
thread_root_event_id + thread_root_event_created_at  (both set only for thread)

provenance format = ortak-reviewed-conversation-provenance/1
audience + audience_hash
source_event_id + source_event_created_at + source_evidence_hash
source_hash
```

Read the source through exact `office_inbox` and canonical `events` agreement:
company/community, event ID and partition timestamp, author, kind9/40002,
channel, decided inbox and undeleted event. Require the current project binding
to name that same stream channel. Do not guess a project from its channel.

For thread identity, use the canonical `thread_metadata` parent/root event IDs
**and their partition timestamps**, with community/channel equality. Validate
the root as a current top-level event and validate the bounded parent chain
against the recorded root. Cap depth at32; refuse cycles, missing parents,
cross-channel edges, partial ID/timestamp pairs and inconsistent root metadata.
A source without metadata is top-level only when its canonical event has no
parent/reference claim requiring unresolved thread metadata. A complete
parentless row must agree with top-level identity. Missing or inconsistent
metadata is a closed refusal, not a switch to channel-wide memory.

The delivery-chain `runs.root_message_id` is not this thread root. Replies and
explicit delegation can create different routing roots within one conversation.
Work obtains its anchor only from its retained `source_message_id`, re-resolved
through current canonical storage. An attachment, title UUID or child/parent link
cannot supply it.

Use a versioned canonical UTF-8 JSON hash with fixed UUID/hex/timestamp forms
and cross-language vectors. `audience_hash` covers only the audience tuple above,
including company/project/employee and the thread partition when applicable.
It excludes the source message and evidence hash: two facts sourced from
different messages in the same audience share that audience identity. Source
identity and evidence belong to the separate provenance value. Preserve the
original evidence hash separately. For these new facts only, the opaque Honcho
`source_hash` becomes SHA256 of canonical
`{audience_hash, format:"ortak-reviewed-conversation-source/1", source_evidence_hash}`.
The human approval request hash covers edited text, expiry, source and resolved
audience. Thus existing D2a publication/withdrawal/selected-recall payloads can
bind the narrow audience without adding a new Honcho namespace or wire field.
Legacy project source hashes do not change.

### Pure v1 byte contract and current source boundary

The isolated implementation is in
[`memory/conversation`](../../crates/ortak-control/src/memory/conversation/mod.rs).
It exports `ConversationAudienceV1`, `ConversationEventIdentity`,
`ConversationProvenanceV1` and a strict 32-byte `ConversationMemoryDigest`.
Fields are private; channel and thread constructors are explicit and have no
default or project/DM fallback. No variant was added to the existing
`MemoryScope` or `MemoryRecord`, and no existing consumer invokes this module.

Canonical bytes are compact UTF-8 JSON with recursively lexicographic keys,
lowercase hyphenated non-nil UUIDs, lowercase 64-character event/hash hex and
UTC timestamps exactly `YYYY-MM-DDTHH:MM:SS.ffffffZ`. Years1970–9999 and lossless
PostgreSQL microsecond precision are supported; sub-microsecond data and leap
seconds are rejected rather than rounded. Channel root fields are both explicit
JSON null; thread root fields are both present. Source timestamps are not ordered
against the root because client timestamps do not establish ancestry. The same
event ID used as root and source must have the same exact partition timestamp.

The audience parser accepts at most2048 bytes and provenance at most4096 bytes,
checking bounds before JSON parsing. Unknown/duplicate fields, unsupported
versions, partial root pairs, noncanonical order/whitespace/UUID/time encodings
and mismatched derived audience/source hashes are refused. The retained
provenance includes the full audience, both hashes, exact source locator and the
original evidence hash. Its parser verifies consistency, **not** evidence
authenticity or source membership. The future resolver must verify that the
evidence digest covers the canonical source being frozen; a caller-supplied
digest is not authorization. Legacy69 `ortak_reviewed_export_source_hash` and
Honcho reviewed-project payload meaning remain unchanged.

Literal [Python/Rust vectors](../../crates/ortak-control/src/memory/conversation/test_vectors.json)
cover explicit channel and thread audiences, their source-binding preimages and
complete retained provenance. The eight tests bind constructors, canonical
parsers, every audience axis, source/audience separation, partition precision,
forged hashes, bounded malformed input, closed errors and legacy project record
serialization. Suggested root gate:
`cargo test -p ortak-control memory::conversation::tests --lib`.

The canonical resolver, authorized preview/approval and recovery routes are
integrated in source. Migration75 includes immutable storage and scoped epochs;
the current rollout still serves74. Conversation publication, runtime use,
withdrawal integration and native acceptance below remain undelivered.

## Authorization, expiry and non-revival

Approval uses the existing signed NIP-98 human, Operator plus project
Owner/Reviewer ceiling, selected employee/channel grants and current active
project/employee/Office identity. Both source and destination are the project's
same channel; the approver and employee must currently read it. The source/root
must remain readable. The fact can expire no later than90 days or a sooner
canonical channel deadline. Project membership does not authorize another
channel, private DM or hidden source.

Office use additionally requires an eligible human-origin canonical message,
current central cohort/recipient, and a current project grant for that requesting
human. Read-only project access is sufficient for recall; it does not grant Work
mutation. Work use keeps its current contributing requester, assignment,
project/Work generation and source gates. A narrower thread fact requires exact
root ID and partition equality. A channel fact requires the exact channel.

Publication still needs the current full retained memory binding, owned creation
receipt and actual I/O witness; read-only health does not grant it. An ACKed fact
may survive a model-only revision with the same employee and memory identity.
Every new run independently pins the current revision/lifecycle.

Current predicates alone miss remove→restore between worker polls. Add a small
retained `conversation_memory_authorities` row per selected
company/project/channel, with a monotonic epoch. Conversation uses pin that epoch.
Relevant authority removal/identity change advances it in the same database
transaction; restoring permission never decreases it. New runs may use a still
approved fact after permission returns, but old uses cannot revive. Do not pin
the entire company Office generation: an unrelated reply must not cancel every
conversation-memory run.

The epoch invalidation matrix is explicit:

- Bound channel archive/delete/type/visibility/TTL/identity changes, removal or
  replacement of its current human/employee membership: advance that channel's
  selected project scopes.
- Project archive, bound-channel removal/replacement or human project-grant
  revocation/change: advance that project's scope.
- Source/root event removal or canonical identity mutation, and thread metadata
  mutation changing an already referenced anchor/root: advance affected scopes.
  New unrelated events, ordinary new replies and counter-only changes do not.
- Human identity deactivation/reclassification and employee Office identity
  replacement: advance affected scopes through their retained current/history
  mappings. Employee disable also retains the existing lifecycle-epoch fence.
- Fact Stop using and target consumption opt-out retain their existing permanent
  fact-version/target-epoch semantics; ordinary advertisement renewal is not an
  authority change. Time-only expiry is checked directly and bounds admission.

Indexes must make affected-scope lookup bounded. Permit at most128 retained
selected channel/project authority rows per company in this initial slice;
refuse new scope registration at the cap. Trigger updates touch that finite
set, not every fact or run. No remote I/O or synchronous run fan-out occurs.

Extend the existing Office mutation trigger's `thread_metadata` field inventory
to include channel, root ID/root timestamp and depth: migration48 currently
fences parent identity, not all fields this new resolver would depend on.
Do not alter the safe skip for an ordinary unrelated insert; an insert which
reclassifies an already referenced formerly parentless event must fence it.
The scoped epoch updates run under the same existing Office mutation fence.

## Minimal persistence and migration dependency

Additive migration75 contains the storage and authority prerequisites below.
Runtime v4 admission and publication will use a later additive migration; they
are not enabled by75. Never rewrite66/69/71/72/73/74. Storage scope:

| Change | Required invariants |
| --- | --- |
| `reviewed_memory_facts.audience_kind` | Closed `project`/`conversation`, default `project` for old rows; immutable after insert. The old public approval endpoint creates project facts only. |
| New `reviewed_memory_conversation_audiences` | One row per conversation fact; tuple and hashes above, immutable/no delete/no truncate. FK only to durable company/community/project/employee/fact evidence, never purgeable channel/event projections. Fact plus audience plus original approval receipt commit together; neither an orphan nor attaching an audience to an older fact is permitted. |
| New `conversation_memory_authorities` | Durable company/project/channel identity, monotonic nonnegative epoch and closed last-change reason; no identity replacement/delete/truncate or epoch rollback. Registration requires the exact current project/channel selection. |
| `reviewed_memory_targets` extension | Explicit conversation-consumption flag, pinned channel and separate consumption epoch. Default off; immutable channel once selected. Opt-out advances only conversation epoch, so existing project-only Work use is not accidentally opted in or retired. |
| `run_reviewed_memory_uses` extension | Nullable conversation audience hash, scoped-authority epoch and conversation-consumption epoch for old compatibility; all-or-none for conversation rows. At most8 total project+conversation rows/run with unique ordinal/fact. Deferred agreement with fact kind, audience, exact snapshot and current run anchor. Existing rows remain immutable. |

Fact insertion's deferred guard requires the matching audience row and approval
receipt created in the same transaction. Audience insertion requires the new
conversation fact in that same transaction. Old project APIs/SQL eligibility
explicitly exclude conversation rows before LIMIT; no implicit fallback through
`ortak_reviewed_runtime_eligible` is possible. Refactor common eligibility only
behind separately named project and conversation predicates. Preserve all
existing exported hashes, lease, receipt-at-commit and source checks.

Both new tables need universal community write fences, exact/retained deletion
inventory and G all-row witnesses. The existing full-row G proof for altered
tables must include their new columns. Canonical purge keeps these records and
severs current authority only after existing exact remote cleanup gates pass.
Root integrates the desired schema, reconciler functions/triggers/checks/indexes
and actual parity before any feature-enabled writer starts.

## Runtime and worker composition

The [worker recipe example](WORKER_REVIEWED_MEMORY_RECIPE.md) documents the
implemented selection fields and their independent publication/runtime meaning.

Add default-empty `reviewed_conversations` per-employee worker selection, bounded
to16 mappings/employee and128 total. Each entry explicitly names project and
channel, must be in `reviewed_projects`, and must agree with the current project
binding. At most one selected project per employee/channel is allowed; ambiguity
refuses configuration before credential lookup. Existing
`reviewed_runtime_projects` keeps its project-only meaning. Publication alone
does not enable either kind of consumption.

The worker advertises the new flag/channel/epoch only after the same owned
receipt and actual I/O witness validation. Use the existing55-second target
lifetime and25-second refresh bound. The local selection and canonical target
must both match before recall; a stale advertisement alone cannot authorize it.

Resolve a sealed `ConversationMemoryOrigin` under current Office authority from
the Office event, or the promoted Work source plus its known project. It carries
no model-selected audience. Office search terms are at most16 unique bounded
alphanumeric terms from the human body; Work retains its title/description rule.
Neither runtime instructions nor structured IDs become search terms.

Inspect at most32 eligible candidate facts after the current audience filters.
Office includes conversation facts only. Eligible promoted Work may include
its existing project facts plus matching conversation facts; prefer the narrower
thread, then channel, then project, with stable fact-ID ties. Centrally choose
the final at most8 IDs and at most8 KiB of approved content in that order before
the remote request. The existing Honcho implementation sorts selected UUIDs
before its eight-record/8 KiB limit; sending all32 candidates would lose this
priority. A single existing `/recall-selected` request uses that real project
and those exact final IDs. Reorder validated results to the central selection;
missing remote results remain missing. Empty selection skips I/O. Validate
every returned record's audience-bound source hash,
content, approval, expiry and retained binding. Never substitute local registry
text for a missing remote result. The response remains at most8 records/8 KiB;
all reviewed plus scratch context remains8 records/16 KiB.

Introduce snapshot version4 if still unused at implementation time (current
`SnapshotWire` accepts1/2/3). Add a distinct typed conversation context and origin;
keep project pins and versions1–3 byte-identical. The rendered record is marked
`reviewed_conversation_memory` and `untrusted_data`, with immutable audience and
provenance. The combined use ordinals must exactly match the rendered reviewed
record order. Preserve migration72's escaped-NUL-compatible validation of legacy
scratch JSON and all C2 workspace RunSpec fields.

The existing Hermes bridge receives rendered RunSpec context, not the central
snapshot version. Keep that wire unchanged. Validate every rendered UTF-8
string against its existing8 KiB limit before admission:4 KiB content plus
escaped audience/provenance can otherwise overflow it. This bound is separate
from the aggregate content limits above.

Use Office → project → optional Work → conversation authority → sorted facts →
sorted targets → run → outbox lock order. All API/advertisement/epoch writers
must obey or use the existing fail-and-retry mutation fence. No provider call
holds these locks. Final freeze re-derives the origin, locks the scope/facts/
targets, rechecks all pins and commits the unchanged snapshot plus immutable
uses atomically. Deferred guards repeat current checks at commit, including the
earliest fact, target, identity and channel deadline.

Revalidate at load, new start/admission renewal, running reconciliation, Work
artifact/review materialization, Office completion/delivery admission and the
Office post-ACK memory-write boundary. Lost-start retries reuse exact snapshot
bytes and never recall anew. Receipt-only stop/correlation recovery must remain
possible after source loss; it cannot start a run, deliver output or gain fresh
authority. Preserve the lost-start-ACK cleanup lessons from74.

The existing bounded reconciliation pass detects invalid fact/scope/target epochs
or deadlines and durably schedules cancellation. An old run cannot become usable
after revoke→restore, process restart or changed model. A late remote result
cannot attach an artifact, open REVIEW, publish to Office or produce an automatic
scratch write after its conversation authority is lost.

## API and native flow

Keep existing project endpoints backward-compatible. Add a separate signed
`POST /api/v1/projects/{project}/conversation-memory`:

```json
{
  "operation_id": "<retained UUID>",
  "employee_id": "<selected employee>",
  "source_message_id": "<canonical message hex>",
  "audience": {"kind": "thread"},
  "expected_audience_hash": "<hash from the current authorized preview>",
  "content": "<human-edited fact>",
  "expires_at": "<explicit UTC expiry>",
  "reviewed": true
}
```

The server displays the resolved channel/thread audience before confirmation via
`POST .../conversation-memory/preview`, accepting the employee, source message ID
and audience kind only (no durable write or Honcho call). Submission includes the
preview's expected audience hash; a changed canonical resolution
returns conflict rather than approving a different audience. This hash is a CAS
check, not authority. Same-key retries compare the original stored input hash
before recomputing a now-missing source; return the retained currently authorized
receipt with content withheld when needed, never a newly resolved fact. Compute
the immutable input fingerprint from the exact submitted fields, including that
expected hash; on first admission separately prove that it equals the freshly
resolved canonical audience. This lets receipt lookup precede source resolution
without treating the browser's hash as authority.

Add conversation list/preview reads with optional exact anchor and25-row keyset
pagination. A project recovery list may return opaque fact IDs, status and
cleanup controls while withholding text/source/root when current visibility is
lost. Existing project preview queries exclude conversation facts. Reuse the
existing fact-ID publish, Stop using and export retry actions after adding the
appropriate conversation publication checks. Missing source is not a condition
for removing an already published record.

The native message action and promoted Work view offer “Remember for this
conversation”, explicit employee and expiry, editable text, resolved thread/
channel label, and separate save/publish actions. Never prefill raw message text
as approved text. Project-wide recovery remains available when the source item
or thread disappears. Use one label owner, keyboard/focus recovery, existing
exact-body operation UUID retention, and scope-generation fencing across close,
reopen, project change, failed reads and revoked authority. Coordinate shared
message menu/client/Work files with the promotion owner before implementation.

Activity/run provenance displays the audience and approval when currently
visible, otherwise withheld metadata. Office visibility remains participant/
channel-authorized; an Operator is not a private-content override. Status copy
distinguishes local approval, published, use stopped, runtime stop pending and
reviewed-store text removed.

The schema76 source now exposes each v4 record's audience kind and its canonical
conversation audience while current. The run panel preserves the original order,
labels project versus thread/channel scope, and displays the approval ID and
visible channel/thread reference. Loss of current use clears the audience along
with reviewed, scratch and derived write text; opaque approval/use and write
receipts remain. Legacy1–3 projection fields are unchanged. The focused v4 React
case passed; updated PostgreSQL projection assertions and native rollout remain
pending. This read projection adds no namespace, publication action or migration.

## Withdrawal and retention

Use existing D2b export identity and its two stable publish/withdraw keys. The
audience-bound source hash is immutable, so withdrawal-before-publish still
installs the same irreversible tombstone and delayed publication cannot resurrect
text. Stop using advances the fact once and atomically makes its existing removal
job due. Expiry schedules the same withdrawal, not a third operation.

Permission loss stops current use immediately, even if remote cleanup has not
happened. Cleanup uses only the exact retained original owned binding; it does
not need current source/project/employee admission. Missing credentials leave a
durable failure and explicit same-job retry. Before community quiescence, the
canonical retained-cleanup gate remains mandatory; no bypass behind the universal
write fence is introduced.

The remote ACK proves deletion of that referenced owned reviewed-store text and
retention of its tombstone. It does not erase original Office content, approval
text, frozen inputs, artifacts, provider context or backups. G capture preserves
active facts and future removal obligations; it does not withdraw everything or
manufacture an erasure ACK. Restore remains inactive until original-owner
containment and same-key expiry/cleanup reconciliation.

## File ownership and implementation order

| Boundary | Planned source/tests |
| --- | --- |
| Canonical conversation identity | New `ortak-control/src/postgres/conversation_memory.rs` plus bounded resolver/authority helpers. Shared by Office and Work; no duplicate client/tag authority. |
| Human approval/read/recovery | New `ortak-work/src/postgres/authorized/facts/conversation.rs` and audience types; narrow facts/read/receipt and reviewed-export wiring. |
| Runtime pins/freeze/current use | New `ortak-runtime/src/reviewed_memory/conversation.rs` and `memory_context/conversation.rs`; narrow selection, snapshot, PostgreSQL authority/output/reconciliation changes. |
| Worker and signed API | `ortak-server/src/worker_memory` conversation selection/advertisement; new `work/conversation_memory.rs` handlers and DTOs. Existing bounded export worker remains the only publisher. |
| Native | New scoped conversation-memory dialog/hook; reuse reviewed status/recovery components. Coordinate message action, client types and Work selection with Rawls; no edits during his promotion slice. |
| SQL/retention | One later-number proposal; root owns immutable/desired/parity, Ops owns exact deletion/G integration. No Honcho schema/image change is expected for the unchanged project transport; new exact contract tests still required. |

Implement storage/API with consumption default-off first, including the legacy
project-query exclusion. Then add runtime/current-use and output fences before
enabling worker advertisement. Finally expose publication/use wording and run
the native acceptance. No intermediate deployment may allow conversation facts
through an old project-only reader or writer.

## Falsifiable acceptance matrix

| Production seam | Required positive and negative evidence |
| --- | --- |
| Signed approval and SQL commit | Same-key concurrent/restart replay produces one fact+audience+receipt; different text/audience/expiry conflicts. Missing audience, late attachment to old fact, forged hash, torn receipt, changed preview and wrong company fail. |
| Canonical resolver | Root and deep reply produce the same exact thread tuple; another root in the channel differs. Fake routing root, cross-partition/root/channel, missing parent, cycle, excessive depth and source deletion refuse. |
| Legacy compatibility | Existing project APIs/SQL exclude new conversation facts before limits. Exact old snapshots1–3, escaped-NUL scratch, C2 workspace fields and old source/export hashes still validate without byte rewriting. |
| Real selected transport | Honest server returns only authorized IDs; forbidden earlier-sorting matches cannot crowd out permitted records. Foreign fact/audience/source hash, wrong project/binding and unavailable Honcho refuse without local substitution. |
| Both origins | Same-thread human Office and promoted Work consume the remote text; manual Work, source-less child, sibling thread/project/employee and employee-origin dispatch do not. Channel-wide fact works only after that explicit broader choice. |
| Races and non-revival | Held freeze/admission blocks conflicting scope/target removal. Revoke→restore between polls advances scope epoch; old queued/running/terminal output stays refused and a fresh authorized run may proceed. Unrelated reply/counter changes do not advance that epoch. |
| Time and recovery | Expiry with no notification becomes due; remote recall finishing after revocation cannot freeze. Lost start/stop ACK after authority loss permits containment/receipt recovery only; exact snapshot is reused, never recalled again. |
| Output and privacy | Actual Office delivery and post-ACK write, Work artifact/review and Activity projections enforce current conversation use. Hidden text/root IDs are withheld; the permitted stop/cleanup recovery remains reachable. |
| Remote removal | Publish ACK→Stop using→exact real Honcho withdrawal; lost ACK retry and withdrawal-before-delayed-publication retain one irreversible tombstone. Old binding is used after disable/revision/permission loss. |
| Retention/G | Populated new tables plus uses survive canonical purge byte-for-byte after real matching cleanup receipts. Unknown/missing fences, due/uncertain jobs and old manifest inventory refuse; fresh dump/restore and current schema parity pass. |
| Native company flow | Approve edited fact from a visible thread, inspect audience, publish, ask a fresh same-thread message and promoted Work to use an otherwise unknown synthetic fact, inspect real run attribution, then stop use and verify a later run omits it. Exercise sibling-thread and other-employee negatives, reload/uncertain submit and source-loss recovery. |

PG tests must invoke actual authorized APIs, current resolver, supervisor,
materializers and deletion store; no successful lease/health/use receipt is
hand-inserted to bypass the seam. Controlled adapters test race boundaries;
actual Honcho transport and native/provider evidence remain separately named.
Root alone runs Cargo, disposable PG/image gates and the selected live flow.
