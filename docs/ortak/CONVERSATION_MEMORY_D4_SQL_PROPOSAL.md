# D4 conversation memory: additive persistence proposal

Status: root approved the storage/epoch fields below and allocated migration75
on2026-09-06 for reviewed conversation storage, scoped invalidation and legacy
exclusions. Assembly and desired-schema parity are in progress. Source/storage
fragments passed focused resolver and signed API checks on disposable PostgreSQL
over its74 ledger;75 has not been applied as a numbered migration or deployed.
Snapshot-v4 wire, publication/runtime selection and a later additive migration
remain unallocated. Existing immutable migrations1–74 stay intact.

This narrows [the D4 plan](CONVERSATION_MEMORY_D4_PLAN.md) to the persistence
boundary. The first audience is one existing project, its explicitly bound
Office channel and one stable Employee; thread is the default audience, channel
is an explicit wider choice. Existing project facts keep their meaning. Artifact
sources, DMs and inferred project/thread identities are outside this slice.

## Proposed rows and columns

All new UUIDs are non-nil; Employee IDs retain the existing EmployeeId type,
not an invented UUID restriction. Digests are 32-byte BYTEA in SQL and lowercase 64-character hexadecimal
on the wire. Event identities always include their exact TIMESTAMPTZ partition.

| Relation | Exact proposed fields | Persistent invariant |
| --- | --- | --- |
| `reviewed_memory_facts` | `audience_kind TEXT NOT NULL DEFAULT 'project'`, closed to `project`/`conversation` | Immutable with the existing approved content/source fields. No backfill from source kind: a legacy project fact sourced from a message is still a project fact. |
| New `conversation_memory_authorities` | `company_id UUID`, `community_id UUID`, `project_id UUID`, `channel_id UUID`, `epoch BIGINT NOT NULL DEFAULT 0`, `last_change_reason TEXT NOT NULL`, `created_at TIMESTAMPTZ`, `changed_at TIMESTAMPTZ`; PK `(company_id,project_id,channel_id)` | Durable identity, nonnegative monotonic epoch. Initial reason `registered`; subsequent closed reasons listed below. FK company, community tombstone and `(company_id,project_id)` only; no FK to purgeable channels or project_api_bindings. |
| New `reviewed_memory_conversation_audiences` | `company_id`, `community_id`, `fact_id`, `project_id`, `employee_id`, `channel_id`; `kind TEXT` (`channel`/`thread`); nullable `thread_root_event_id BYTEA` + `thread_root_event_created_at TIMESTAMPTZ`; required `source_event_id BYTEA` + `source_event_created_at TIMESTAMPTZ`; `audience_bytes BYTEA`, `audience_hash BYTEA`, `source_evidence_hash BYTEA`, `source_hash BYTEA`, `provenance_bytes BYTEA` | PK `(company_id,fact_id)`. Immutable/no DELETE/no TRUNCATE. Durable fact, project, Employee, community and authority-scope FKs; never an event/thread/channel FK. Both root fields null for channel, both present for thread. Source equals the parent fact's source_message_id; source_artifact_id must be null. |
| `reviewed_memory_targets` | `conversation_consumption_enabled BOOLEAN NOT NULL DEFAULT false`, `conversation_channel_id UUID NULL`, `conversation_consumption_epoch BIGINT NOT NULL DEFAULT 0` | Separate from 71 project `runtime_consumption_enabled`/`consumption_epoch`. Enabling requires the explicit current project/channel selection and existing owned binding/I/O witness. Channel may transition null→exact selected channel once; subsequently immutable, including while disabled. |
| `run_reviewed_memory_uses` | `conversation_audience_hash BYTEA NULL`, `conversation_authority_epoch BIGINT NULL`, `conversation_consumption_epoch BIGINT NULL` | All three absent for project facts, all present for conversation facts. Existing PK, unique fact/run, ordinal 0–7 and immutable/no-delete rules remain. A use must match the exact audience, target, frozen input and applicable run origin. |

The audience/provenance bytes have the existing pure-v1 ceilings 2048/4096 bytes.
Their parsed values must exactly equal the typed SQL columns, and their hashes
must equal the existing canonical encoding. JSONB text is not a substitute for
the canonical byte format. Use the explicit UTC six-microsecond encoding from
`crates/ortak-control/src/memory/conversation/wire.rs`; reject a byte/column/hash
mismatch rather than normalizing it. The provenance contains hashes and IDs,
not another copy of source text.

Every new FK is restrictive, with no cascade deletion. Use an exact composite
authority key including community, or a deferred equality guard, so an audience
cannot reference a company/project/channel row belonging to another community.
The deferred audience guard also compares project, Employee and community with
the parent fact; matching only `(company_id,fact_id)` is insufficient.

For conversation uses, propose that the old non-null `consumption_epoch` column
is exactly 0 and is not consulted for conversation authority. The three new
columns carry the actual conversation pins. The v4 conversation wire therefore
has its own consumption epoch; a project toggle cannot accidentally retire or
enable conversation use. Root approved this sentinel for storage75; a later
runtime migration must enforce its v4 meaning before admitting any use.

Keep 66's existing 1024 retained fact cap per `(company,project,employee)` across
both kinds; do not multiply it by thread. Register at most128 retained authority
scopes/company and256/community, under the existing Office/project selection
locks and bounded community/company registration locks. Removed scopes still
count. The community limit prevents registration from creating an unbounded
future closure operation. Overflow or BIGINT epoch overflow refuses the
originating transaction; there is no reset/reuse.

Registration only creates the scope row when the current company/community,
project/channel binding and selected employee are valid. It grants no recall by
itself. The target still needs an owned creation receipt, exact current binding
and fresh I/O advertisement; ordinary health or possession of public metadata
is insufficient.

## One approval transaction and retained history

The new conversation endpoint explicitly inserts a conversation fact, audience
row and original `reviewed_memory_operations` promotion receipt in one
transaction. Its deferred constraints check all three directions and the
existing same-transaction receipt convention from 66. A project fact must have
no audience row. A conversation fact must have exactly one new matching row;
an audience cannot be attached to an older fact or supplied by a later retry.
The existing project endpoint writes `audience_kind='project'` explicitly.

After the Office/project locks, resolve current canonical evidence and compare
the submitted expected audience hash. Canonical bytes/parser success is not
authorization. Re-resolve at the final commit boundary; constrain the approval
deadline by the resolver's `valid_before`, channel deadline, current human/
employee authority and 90-day fact expiry ceiling. The SQL guard must branch by
fact kind: 66's message visibility helper alone does not prove a thread audience.

Same-key lookup compares the original request hash before re-resolving a source
that may now be gone. Its authenticated recovery projection can return retained
IDs/status with text and source withheld. It cannot create a new approval or
renew a use. Permanent Stop using remains the one version 1→2 transition, with
the same original fact, approval, export and withdrawal identities.

Separate **current predicates** from **retained consistency**. Current use needs
active bindings, ACLs, exact source/root, current epoch, target opt-in and live
deadlines. Historical consistency needs immutable tuple/hash/receipt equality,
not a still-active channel, project grant, target, Employee revision or fact.
Expiry, revocation, project purge or an old stopped Files reader must not make
retained snapshot/use bytes invalid to back up or inspect with recovery access.
Restoring access may permit a fresh run with fresh pins; it never updates an old
use's epoch or reauthorizes its delayed output.

## Epoch invalidation and locking

The authority epoch protects remove→restore between worker polls. Every relevant
authority mutation advances the affected existing scope rows in the same DB
transaction. Epochs are server-derived and cannot be caller-set, decremented,
deleted or truncated. `last_change_reason` is a closed diagnostic value, not a
complete audit log and not permission in its own right.

| Mutation / closed reason | Affected retained scopes | Deliberately unchanged |
| --- | --- | --- |
| Channel identity/type/visibility/archive/delete/TTL fields: `channel_changed` | Old and new registered project/channel scopes in the exact community | Unrelated channels; message/reply counter maintenance |
| Human/employee channel membership removal, role or identity replacement: `membership_changed` | Registered scopes for that channel; retain old scope matching before removal | Other channels; an unrelated new participant cannot create memory authority |
| Project archive or API channel binding removal/replacement: `project_changed` | That project's old/new registered scopes | Other projects; project title/description edits that do not alter authority |
| Relevant human project grant revocation/role change: `project_grant_changed` | That project's registered scopes | Ordinary Work progress, criteria or artifact updates |
| Referenced source/root/ancestor canonical event removal or identity/content mutation: `event_changed` | Registered scopes in the event's old/new channel | New unrelated events and ordinary new replies |
| Existing thread ancestry/channel/root identity or depth changes: `thread_changed` | Registered scopes in the old/new affected channel, conservatively covering referenced descendant ancestry | reply_count, descendant_count, last_reply_at; an ordinary unrelated insertion |
| Human identity deactivate/reclassify, Employee Office key/signer replacement: `identity_changed` | Scopes selected through current **or retained** actor/employee/channel mapping | Same Employee with a model-only revision and unchanged Office/memory identity |
| Company/community serving authority removal: `scope_closed` | The company's registered scopes, before the existing community write fence closes them | Other companies; no remote cleanup is attempted after quiescence |
| Fact Stop / conversation target opt-out | Existing fact version / separate target conversation epoch, respectively | Scope epoch; project consumption epoch; ordinary target advertisement renewal |
| Passage of time | Direct source/channel/fact/admission deadline checks | No periodic epoch increment or extension of a frozen deadline |

For thread metadata, migration 48's trigger currently names event/partition and
parent identity. The additive migration must also fence `channel_id`,
`root_event_id`, `root_event_created_at`, `depth`. Current channel trigger fields
also include the later 73 participant/TTL additions; replacing it must preserve
those. An INSERT that changes the interpretation of a previously referenced
parentless event must invalidate. It cannot inherit the old unconditional
parentless-INSERT skip. Conversely a new unrelated reply must not advance D4
epochs. Resolve this using the exact canonical event/partition and existing
reference evidence under the Office mutation fence, not event time ordering.
Existing ancestry identity UPDATE/DELETE may conservatively fence its registered
channel scopes; this avoids walking every retained fact on the writer path.

Bound updates to at most512 scope rows covering old/new communities, sorted by
company/project/channel. Select at most513 and refuse overflow before writing.
Required lookups are authority `(community_id,channel_id,company_id,project_id)`,
audience source/root exact event+partition and `(company_id,employee_id)`, plus
existing project-grant and channel-member indexes. Actor/employee lookups must
retain old mappings on removal; a lookup only through live membership cannot
prove that a removed actor had no affected scope. No provider I/O, scan over all
runs, or synchronous per-run cancellation is allowed in these triggers.

Admission order is Office → project → optional Work → sorted conversation
authority → sorted facts → sorted targets → run → outbox. Office/event/channel
writers use the existing Office exclusive mutation fence and its nonblocking
reverse-order refusal. **Project-grant writers are a distinct seam:**
`0054_ortak_work_api_access.sql::ortak_project_access_guard` intentionally uses
the project `FOR UPDATE NOWAIT` fence without acquiring Office exclusive,
because signed API authentication retains shared Office authority on another
connection. Advance the scoped epoch under that same project fence, followed
by sorted scope rows. Do not introduce an Office-exclusive self-conflict there.

## Exact wire and predicate split

Reuse pure `ConversationAudienceV1` / `ConversationProvenanceV1` unmodified.
Rawls's resolver supplies `ConversationObservation.audience()`, `.provenance()`,
`.observed_at()`, `.valid_before()`; it does not supply epoch authority. Its
source-evidence digest covers compact lexical JSON fields `author_pubkey`,
`channel_id`, `community_id`, `company_id`, `content`, `event_created_at`,
`event_id`, `format='ortak-reviewed-conversation-evidence/1'`, `kind`, `sig`,
`tags`, preserving canonical content/tag order. SQL persistence must reproduce
that exact identity from current rows or compare a bound resolver result under
the same final fence, never substitute the message ID-only legacy hash.

Keep the old `ortak_reviewed_runtime_eligible(company,fact,target,epoch)` a
**project-only** predicate. Similarly keep the old project publication selection
closed to project facts. Proposed separate internal predicates:

- `ortak_reviewed_conversation_export_eligible(company,fact,target)` validates
  the new audience/source, current approver/Employee/Office/project authority
  and common original owned binding/publication conditions.
- `ortak_reviewed_conversation_runtime_eligible(company,fact,target,run,
  authority_epoch,conversation_epoch)` additionally resolves the exact admitted
  human Office origin or promoted Work source, pinned audience and runtime
  opt-in. A run ID is essential: a channel and epoch alone do not prove that
  this thread's facts belong in the run.
- An internal dispatch-by-stored-kind publication predicate may be used only
  by the new explicit fact-aware publish/worker paths. It cannot widen the old
  project listing, preview or recall endpoint.

`ortak_reviewed_export_source_hash` may dispatch by the stored immutable kind:
the project branch stays byte-for-byte equivalent to 69; conversation returns
only its validated retained provenance source_hash. Preserve remote fact ID,
`reviewed:publish:<fact>` / `reviewed:withdraw:<fact>` keys, original request
hashes and receipt/lease-at-commit checks. Honcho's namespace/API remains the
same project selected-record protocol; it does not become channel authority.

Proposed v4 snapshot adds a typed `conversation_origin` containing exact
project/channel, human trigger event+partition, canonical thread root pair,
source kind `office`/`work`, and the Work source identity when applicable.
`reviewed.records` remains one combined ordered budget of ≤8; each v4 record has
closed `audience_kind`. Project pins retain their existing meaning. Conversation
pins carry the immutable audience hash and the two new epochs, with the common
fact/target/version/content/source/binding/approval/expiry pins. Canonical
provenance is part of the rendered `reviewed_conversation_memory` untrusted
record; order must exactly match retained ordinal. Office admits conversation
records only; a source-anchored Work run may combine matching thread, channel
and legacy project records. Source-less/manual Work and unanchored descendants
do not gain conversation scope. `runs.root_message_id` is never a thread root.

Snapshot v4 must still be unused when allocated. Existing snapshots 1–3 retain
exact bytes and decode rules, including 72's `ortak_snapshot_scratch_jsonb`
escaped-NUL comparison and 74's existing RunSpec workspace fields. New v4
comparison must not parse unrelated legacy JSON through a plain JSONB cast.

## Existing production seams requiring explicit changes

These are additive replacements/wiring sites, not instructions to edit the
immutable migration files cited as their definitions.

| Definition / production location | Required exclusion or current-use change |
| --- | --- |
|66 `ortak_reviewed_fact_guard`, `ortak_reviewed_fact_receipt_at_commit`, `ortak_reviewed_memory_operation_at_commit` | Immutable kind, exact audience+approval atomicity and current conversation source/approver checks; preserve project insert and one retained revocation. |
| `ortak-work/src/postgres/authorized/facts.rs` and `facts/reads.rs::{projection,fact_on,reviewed_facts,recall_reviewed_facts}` | Old creation and public project reads explicitly require project kind **before LIMIT**. New conversation recovery projection shares safe receipt fields without letting old fact_on expose conversation text. |
|69 `ortak_reviewed_export_eligible`, `ortak_reviewed_export_source_hash`, `ortak_reviewed_export_at_commit`; `authorized/reviewed_exports.rs` | Old predicate project-only; explicit kind-aware publication branch, original binding/source/command/two-job equality unchanged. Preserve cleanup despite current source loss. |
| `ortak-work/src/reviewed_exports/{targets,jobs}.rs`; `ortak-server/src/worker_memory/reviewed.rs` | Advertise separate channel/flag/epoch from default-empty explicit selection. Publisher dispatches by fact kind; a legacy worker cannot advertise or select conversation implicitly. Existing 25s/55s cadence and 60s DB cap remain. |
|71 `ortak_reviewed_target_guard` | Server-derived true→false conversation epoch increment; project flag/epoch unaffected. Channel null→selected once must be an explicit authorized registration, never general mutable target identity. |
| `ortak-runtime/src/reviewed_memory/selection.rs::select` | Current query is Work-only and LIMIT 32. Keep its legacy project branch excluded before full-text/LIMIT; add sealed-origin conversation selection and combine ≤32 eligible IDs before one remote recall. |
|71 `ortak_run_reviewed_memory_current` | Enumerate **every retained use**, then validate its applicable origin. Its current inner join to work_executions would silently omit Office uses. Missing run/fact/target/required Work-or-Office origin must make a populated use invalid, not disappear from NOT EXISTS. |
|71 `ortak_lock_run_reviewed_memory`; `ortak-runtime/src/postgres/reviewed_memory.rs::{validate_candidate,persist}` | Acquire scoped epoch rows before fact/target locks, branch by kind, persist new pins atomically, compare exact existing receipt on retry. Do not just remove the current Work-origin check. |
|72 replacement of `ortak_reviewed_snapshot_consistent`;71 `ortak_reviewed_run_admission` | v1–3 unchanged, exact v4 record/use count and origin/pin/rendered-byte agreement. Admission must also cover Office runtime reference/generation changes, not only changes to work_admission_token. Preserve contained lost-start-ACK metadata recovery. |
| `ortak-runtime/src/postgres/memory_context.rs::{load,freeze}`, `postgres/authority.rs`, `postgres/work.rs`, `reconciliation.rs::reconcile_office_runs` | Recheck conversation pins for both origins. Remove the Work-only condition around reviewed-current reconciliation; retain bounded 64-run pass, durable cancellation and deadline checks. No recall again on lost-start retry. |
| `ortak-runtime/src/office_output`, `office_delivery.rs`, `memory_output.rs`; `ortak-control/src/postgres` memory-write preparation; `ortak-work/src/postgres/authorized/output.rs` | Require current conversation-use authority at Office delivery, post-ACK scratch write and Work artifact/review boundaries. Terminal runtime status alone cannot authorize output after an epoch change. |
|74 `ortak_run_workspace_current`, `ortak_workspace_use_at_commit`, `ortak_workspace_run_admission`; `ortak-runtime/src/postgres/workspace_tools` | No conversation-specific workspace columns or alternative file authority. Existing project-bound Work/manifest/lease/reader-stop checks remain conjunctive with D4 checks. Receipt-only settlement and confirmed containment remain available after authority loss; they cannot renew execution. |

## Permission loss and irreversible cleanup are separate

Root resolved this policy for the initial additive slice: permission, source or
identity loss immediately denies current reads/use/output and advances the
scoped epoch. It does **not** request remote withdrawal. A still-approved,
unexpired fact may be used by a fresh run when current permission returns;
the older use remains invalid because its epoch cannot change. Automatically
tombstoning the fact on permission loss would defeat that intended behavior.

Only explicit Stop using or expiry triggers the existing 69 withdrawal job/key
rules. `ortak_reviewed_export_job_guard` advances a future withdraw job for a
same-transaction fact revocation; `reviewed_exports/jobs.rs::prepare` accepts
removal when the fact is revoked or expired. Preserve those semantics. No
cleanup epoch/reason fields, new remote key, automatic re-publication or
fabricated human revocation belong in this slice.

This is the precise initial-slice interpretation of the broader D4 plan's
permission-loss cleanup language. Canonical purge still requires its real
withdrawal acknowledgements: the operator must explicitly revoke and settle
remaining exports before quiescence. An inaccessible source must not remove
that authorized Stop/recovery affordance; it also cannot itself create an
erasure receipt. Historical remote text and backups are not claimed erased.

## Deletion, G and bounded acceptance prerequisites

Both new tables belong in `buzz-db/src/store/deletion.rs` exact and retained
inventories, the universal community write-fence inventory and G's explicit
schema/table classification. Old approved manifests must refuse the expanded
inventory. Canonical pre-quiesce export cleanup in `deletion/reviewed_exports.rs`
continues to require the exact withdraw ACK, including future scheduled jobs,
before the community fence closes. Do not add a post-purge cleanup bypass or
delete the new retained scopes/audiences. Existing 74 unresolved reader/action
guards and all six workspace retained tables remain intact.

`scripts/ortak/private_recovery_obligations.py` must witness both new tables'
exact keys/full rows and every added column in facts, targets and uses.
Historical withdrawn/expired uses remain valid
evidence; active/unresolved runs, due jobs and failed/uncertain cleanup still
block capture. Future unattempted withdrawal is retained recovery work, not an
instruction to withdraw live facts for a backup. Frozen G74 operators and
archives remain historical; no source version bump or new capture is part of
this proposal.

Before enabling D4, root's focused acceptance should prove: old project rows,
exports and v1–3 bytes are unchanged; populated Office and promoted-Work v4 uses
cannot disappear through joins; unrelated replies preserve epoch while
remove→restore retires old pins; final expiry/revocation rejects output without
blocking contained cleanup; canonical purge/restore retains exact new rows.
Desired schema plus reconciler must reproduce actual function/check/trigger/
index catalogs and new community fences. These are required observations for
the new seam, not a new general test matrix or a claim of implementation.

Root approved the two retained table shapes, one-time target channel selection,
independent epoch fields, legacy epoch sentinel and scoped mutation/locking
rules for storage75. The permission-loss/Stop/expiry distinction is final.
Exact v4 origin/pin wire and runtime/export admission remain the next boundary;
storage75 and its approval API do not authorize conversation runtime use.
