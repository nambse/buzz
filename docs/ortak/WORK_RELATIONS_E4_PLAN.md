# Remaining Work relation editing: bounded implementation plan

This is a design assessment, not a delivered capability or migration.
Architecture v0 §7 explicitly includes parent/child decomposition and dependency
links; its data model names parent/dependency data on work items. Remaining Work E
and the usable-v0 continuation contract explicitly require dependency editing and
decomposition. These requirements survive the historical milestone inventory.

## E4: complete the existing dependency relation

Current production seams:

- `WorkItem::add_dependency`, `WorkRepository::add_dependency` and
  `postgres/commands.rs::add_dependency_on` already enforce same-project edges,
  no self-edge, bounded outgoing count, and cycle checks under the project update
  lock. Loading the entire project graph is currently unbounded; the new editing
  path must place a finite limit on graph work and fail explicitly at that limit.
- Migration 0047 retains every edge and rejects all UPDATE/DELETE. Neither a
  domain removal command nor an authorized HTTP operation exists.
- `work/projection.rs` omits all dependency targets and dependency history.
  Consequently the desktop cannot inspect, add or remove dependencies even when
  a core caller has created one.

Proposed additive storage contract: retain the edge identity and original creation
provenance; permit only guarded active/released state transitions. Removal records
a bounded reason and history; re-adding a released edge reuses its identity and
performs a fresh cycle check. Never hard-delete evidence. The SQL proposal must
replace the immutable UPDATE guard with a narrow transition guard, preserve
DELETE/TRUNCATE rejection, and extend the existing dependency child-authority and
Work generation triggers to UPDATE. Applied migrations remain immutable.

Every active-edge consumer must agree: aggregate loading, cycle detection,
domain blockers and runtime Work source admission. Merely hiding released edges
in the UI leaves phantom runtime blockers. Existing run version/generation pins
must invalidate held and active execution after removal or re-addition.

Add a signed, bounded relation-read projection and explicit add/remove commands
with operation ID, expected item version and reason for removal. The authenticated
human needs current Office/channel/project Contributor or Owner authority. Add
requires both endpoints' current source visibility. Removal must remain possible
for an existing hidden target without returning its title, source or raw history;
an opaque retained edge identity is sufficient to remove the blocker. Replays
reauthorize current source-item scope and do not reproduce an old mutation.

Graph mutations acquire Office shared authority, then the project EXCLUSIVE lock,
then item locks. Do not first call the ordinary `item_on()` helper, which takes a
project SHARE lock: two graph requests upgrading SHARE to EXCLUSIVE can deadlock.
The target must share the already locked project. One command advances the source
item once and appends one history/receipt in the same commit. Reads paginate
current targets without loading their histories or leaking hidden source fields.

Required PG seams: opposite-edge concurrent cycle attempts, remove versus held
prepare/live execution, remove/re-add replay and storage failure, graph limit,
current scope revocation, hidden-target removal recovery, terminal/archive refusal,
and dependency-clear start after removal. UI tests exercise the actual signed
mutation hook, target selection, hidden-target recovery and uncertain exact retry.

## E5: minimal explicit parent/child decomposition

No parent/child aggregate field, SQL relation, command, or API/UI exists. Existing
comments about "children" generally mean criteria/approvals/assignments and do not
implement decomposition. The contract specifies linkage, not automatic assignment,
context inheritance, completion roll-up, cascading cancellation or reparenting.

A bounded initial implementation can support creating one new child under a
currently visible mutable parent, plus authorized parent/child navigation:

- Same company/project, one immutable parent per child, no self-link, a finite
  depth and direct-child cap. Keep a separate durable relation keyed by child so
  existing `NewWorkItem` callers and canonical source-message uniqueness survive.
- One signed create-child command freezes the parent/version plus the child's
  independent human-authored definition. Under project EXCLUSIVE and parent
  UPDATE locks, create the child at version 1, link it, advance the parent once,
  and append both relevant histories plus a receipt in one transaction. Lost
  acknowledgment returns the same child; no duplicate decomposition.
- The child starts Proposed with its own criteria/approval gates and no inherited
  assignment or runtime. Child completion never satisfies the parent's human
  criteria, and parent state changes never silently mutate children. An explicit
  dependency is the mechanism for expressing a blocking relationship.
- Do not silently copy parent description, canonical Office content, artifacts,
  or runtime context into the child. Each work item remains independently source
  authorized; expose a link only while both endpoints are currently visible.
  Inheriting parent context would require recursive source-visibility fencing
  across Work reads, queues, execution, artifacts and reviewed memory and is a
  larger contract than structural decomposition.
- A newly created child cannot form an ancestry cycle. SQL still needs same-scope
  FKs, immutable link guards and a parent authority fence; populated retained
  community purge must preserve these company-owned relations.

Required tests: exact concurrent create-child replay produces one child/link,
receipt failure rolls back both items, stale parent version, depth/count caps,
scope/source/role revocation before commit and on reads, terminal/archive refusal,
retained purge, and independent human acceptance. The UI should start with an
explicit child form and bounded linked list, without claiming roll-up or automatic
delegation.

SQL numbering and final desired-schema/reconciler/deletion inventory integration
remain centrally owned. Dependency and decomposition storage changes should be
reviewed as additive proposals before a final immutable migration is created.

### E5 implementation boundary after E4

E4 is implemented and validated separately in `WORK_DEPENDENCIES_E4.md`. E5 is
reserved as proposal 70; 68 belongs to runtime preparation and 69 to reviewed
memory publication. No parent/child capability is delivered by this plan alone.

The proposed endpoints are signed `POST /api/v1/work-items/{id}/children` and
`GET /api/v1/work-items/{id}/decomposition`. Creation carries `operation_id`,
`expected_version`, title, description, priority, criteria and approval gates.
The project, actor and child ID are server-derived; the body cannot select a
source message or copy the parent's provenance. The result returns the current
parent and child through the existing explicit Work projections. Replay checks
both endpoints' current visibility and returns the same retained child.

A new `work_decomposition` relation has company/project/parent/child identities,
parent resulting version, child depth, creator and operation provenance. The
child key is unique and immutable, with a same-project parent FK and deferred
same-project child FK. Its BEFORE INSERT guard locks the active project
exclusively, then the mutable parent, checks direct-child count below 32 and
depth at most 8, and rejects any already-existing child. Reserving the link
before allocating the fresh child row proves this action creates new work; it
cannot become a hidden attach/reparent endpoint. A narrow internal creation
helper accepts the already allocated server ID. Deferred guards require child
version 1/Proposed with independent creation history, the parent's one version
advance and child-created history, and the same signed operation receipt. Any
failure rolls back the reservation, both histories and both item changes.

The receipt can use the existing `create_work_item` action and point to the new
child at version 1; its request hash includes the parent and expected version.
The immutable relation connects that receipt to the parent's version. No new
general-purpose HTTP action or authority grant is necessary. Parent mutation
already advances the Work authority generation, so held/live parent execution
must revalidate; the new child has no assignment and cannot dispatch itself.

Reads return at most one currently visible parent and 32 currently visible
children under the existing Office/project/source fences. Hidden endpoints are
omitted without disclosing IDs, titles, ancestor paths or counts. Relation
history is omitted from the generic item DTO, as dependency history already is.
An item's independent content remains readable through its own authorization
even if its structural parent becomes hidden. Decomposition does not broaden
runtime context, reviewed memory recall, artifacts or worker queues.

Implementation ownership is new domain/work/server decomposition modules, narrow
creation/export/route wiring, and a desktop decomposition panel/hook. Reviewed
memory publication modules and workers remain independent. Production PG tests
must bind signed creation/replay, concurrent parent CAS, direct existing-child
rejection, injected receipt rollback, finite depth/count limits, visibility and
role revocation, independent human acceptance, held parent execution refusal,
and populated canonical community purge with retained relations.
