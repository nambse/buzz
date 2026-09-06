# Remaining memory scopes and trusted DM — next bounded D slices

Status: source review and implementation plan only. No schema, credential,
deployment, model invocation or existing-resource mutation is authorized by this
document. Migration numbers and exact artifacts remain with the integrating task.
The current selected project-memory boundary is documented in
[D2c](REVIEWED_MEMORY_D2C_DESIGN.md).

## Required behavior and permissible staging

[Architecture §1, §2 invariants3/5 and §4.2](ARCHITECTURE_V0.md) include DMs and
require deterministic recipients derived by the authenticated server. The v0
origin matrix permits employee-to-employee wakes only through trusted routes;
it does not permit a runtime to independently subscribe or decrypt its Office
feed. [Implementation Plan milestone2](IMPLEMENTATION_PLAN_V0.md) requires
canonical DM membership, and milestone5 requires bounded, attributable memory.
[Remaining D](REMAINING_WORK_V1.md#d-semantic-routing-and-broader-memory) explicitly
includes conversation/employee scopes and trusted participant resolution and
decryption. These remain open after project-memory delivery.

The architecture does not mandate NIP-17 compatibility or an encryption protocol.
[Buzz Baseline's reuse matrix](BUZZ_BASELINE.md#reuse-rewrite-and-removal-matrix)
permits adapting its DM store and signed transport without preserving Buzz API
compatibility. Therefore a first supported DM route may use the existing
server-readable private DM channels. Keeping encrypted kind1059 explicitly
unsupported during this slice is safe staging; it does **not** complete the
decryption item in Remaining D. The existing B gate explicitly tests unsupported
DM silence; it is an interim safety gate, not permission to mark all D complete.

Optional features can stay deferred: automatic fact extraction, embeddings,
Honcho peer-global representations, semantic memory retrieval, arbitrary group-DM
encryption, historical-key discovery, and automatic widening to company truth.
Reviewed facts and deterministic lexical recall meet the first memory contract.
Canonical company files remain authoritative; learned context must not replace
them. Human/employee relationship memory requires an explicit human identifier;
the current unqualified `MemoryScope::Relationship` is insufficient.

## Next executable slice: existing private one-to-one DM channels

Support only a canonical private channel with exactly one current human and one
selected employee. This is the smallest DM slice: no key acquisition, decryption,
new crypto dependency or ciphertext migration. The retained desktop
`send_channel_message` already sends signed ordinary message events; it does not
implement an encrypted gift-wrap conversation. UI copy must not describe this
path as end-to-end encrypted.

The authority comes from `channels` plus the immutable DM participant fingerprint
and current `channel_members`, all under the existing Office authority fence.
Require the sender and recipient to be those participants, a live private DM,
current company membership, current verified employee Office identity/revision
and lifecycle, and an explicitly selected DM/channel cohort. Never trust a
client participant list, an outer `p` tag, alias text or model JSON as membership.
The first slice refuses larger groups rather than selecting a partial group.

Exact source changes would be:

- `crates/ortak-office/src/normalizer/dm.rs` and narrow `normalizer/mod.rs` /
  `normalizer/postgres.rs` wiring: bounded canonical participant resolution and
  a sealed `ConversationContext::Direct`, while preserving the kind1059 early
  refusal before selecting ciphertext.
- `crates/ortak-control/src/postgres/cohort.rs` and `postgres/inbox.rs`: explicit
  DM-channel opt-in and the same accepted-event/inbox transaction. The capture,
  reconcile and enable receipt must cover that selected channel; no broadening
  of existing stream cohorts and no replay of already decided historical input.
- `crates/ortak-office/src/identity/postgres.rs`: explicitly selected employee DM
  membership eligibility. Existing identity provisioning currently admits only
  stream/forum channels; changing the normalizer alone would leave the route
  unusable.
- `crates/ortak-runtime/src/postgres/*` and Office delivery authorization: reuse
  the current normalizer on admission/refresh/output, preserving lifecycle and
  participant-removal fencing and durable cancellation. The pure router already
  prioritizes `ConversationContext::Direct`; no semantic call is needed.
- `crates/ortak-server/src/store/visibility.rs`, `routing_read.rs` and the
  corresponding Activity/run reader: grant DM visibility to current participants
  only. Global Operator is not a DM content-reader override. Do not widen Work's
  stream-project binding endpoints as a side effect.

Reuse `buzz-db/src/store/dm.rs` as persistence, not as sufficient authority: its
hash/list helpers are not a current membership or employee-identity proof. The
existing generation triggers cover channel/member changes; add a trigger only
if the production race tests show an uncovered authority mutation. A new
migration, if needed, is additive and allocated by the integrating task.

Acceptance: native human→employee DM→one run→one DM reply; no scorer call and no
unrelated channel wake. Replaying the accepted event produces one decision/run.
A nonparticipant, cross-company key, archived DM, removed member, stale cohort
or disabled/re-enabled employee cannot read or admit old work. Held admission
blocks a conflicting membership removal; removal then cancels the old run and
rejects late output. Native Activity and DM recovery controls remain usable
without disclosing content to an operator outside the participants. Encrypted
1059 remains an explicit visible refusal with zero runtime/semantic/memory I/O.

## Conversation memory: narrow the approved audience first

The next memory slice should add an explicit conversation audience to **new**
reviewed facts in an existing project. Physical Honcho storage can remain the
real source project, so its current reviewed-store erasure contract is reused.
The logical audience is narrower: company, employee, channel and an optional
canonical thread root. This is project-backed conversation memory, not arbitrary
DM or employee-wide storage. No existing project fact is silently reclassified.

Derive a thread root from `thread_metadata.root_event_id` and its partition
timestamp, or the canonical top-level source event. The routing delivery-chain
root is a different identity and must not be used as the conversation key.
The approving human and employee must both currently read the evidence and
destination channel; the existing project Owner/Reviewer + Operator gate remains
the approval ceiling. Explicit selected-channel/project configuration supplies
the mapping; never infer a project from a channel that has several bindings.

An additive fact scope discriminator and immutable conversation-audience record
must commit with the fact and its original operation receipt. The approval hash
pins the exact audience and edited text. Retain only durable identities and
hashes in new scope/use receipts, not foreign keys to purgeable channel/event
projections. Existing project-only selection must explicitly reject the new
scope until a scoped reader is deployed. A conversation fact cannot fall through
the old `ortak_reviewed_runtime_eligible` predicate as ordinary project context.

Implementation belongs in new audience modules next to
`ortak-work/src/postgres/authorized/facts*`, existing `reviewed_exports` jobs,
`ortak-server/src/work/facts.rs`, and a conversation memory panel composed from
the current reviewed-memory UI. Runtime changes belong in
`ortak-runtime/src/reviewed_memory/*`, `postgres/reviewed_memory.rs`,
`memory_context/*` and `ortak-server/src/worker_memory/selected.rs`. Add a new
snapshot version/audience pin while preserving old snapshot bytes and use rows.
Office consumption needs its canonical channel/thread authorization; Work may
consume a conversation fact only when its retained source belongs to that exact
conversation. Plain project membership alone is insufficient.

Keep current limits: edited fact4 KiB, expiry at most90 days, preview25 records,
selected IDs32, remote output8 records/8 KiB, combined input8 records/16 KiB.
Source deletion, member removal, project/archive, expiry, Stop using, target
opt-out and lifecycle changes must all stop current use and fence late results.
Cleanup keeps the exact retained target even after admission is lost. Stop using
remains available when text is withheld. Native acceptance must show recall in
a second run in the same thread, no recall in a sibling thread/project/employee,
exact retry bytes, source-loss withholding, and a real remote withdrawal receipt.

## Employee experience: explicit sharing, not a permission bypass

Employee experience is a separate later slice. The current generic native port
allows `EmployeeExperience`/`Relationship` once a binding is selected, but that
is only a transport scope check. It is not human approval, source visibility,
safe sharing, expiry or erasure authority; do not connect it directly to runs.

The minimal useful employee scope is one durable employee plus a finite,
explicitly reviewed destination audience. Start with selected channels/projects,
not every future employee invocation. The human must possess source-review and
destination-sharing authority. A private source cannot become employee-global
merely because its employee could read it. Store an immutable sharing receipt
binding edited text, source, human, employee and destination IDs. If authority
to approve wider disclosure cannot be proved, refuse that promotion. A distinct
human/employee relationship scope must also pin the human public identity.

This requires a genuine owned non-project namespace in the Honcho extension;
do not invent a project UUID or reinterpret a legacy session as an employee
store. Generalize the reviewed record lifecycle with a versioned scope key while
preserving old project receipts and tombstones. Reuse publication/withdrawal job
semantics and hash-only remote receipts, and keep no embedding/deriver enqueue.
The actual extension schema/image change needs fresh installed PG/HTTP tests.
Local API and employee memory UI must show both the employee and approved
destination audience. Runtime selection needs source AND destination permission,
the current memory binding, and an immutable scope/sharing pin. Model changes
alone remain independent of identity.

## Encrypted DM boundary, after the explicit crypto contract

The retained `nostr`0.44.7 dependency supplies NIP-44 and a NIP-59 unwrap helper;
the repository's present gift-wrap tests use synthetic ciphertext and prove
relay acceptance/recipient filtering, not decryption. The unwrap helper verifies
the seal and checks rumor-author equality, but Ortak must additionally verify
the canonical outer event, exact recipient, expected seal/rumor kinds, canonical
rumor ID, bounded tags/content and trusted conversation membership. Do not treat
the helper's successful return as complete routing authorization.

Create a separate `OfficeEnvelopeDecryptor` capability behind new
`ortak-office/src/dm/{crypto,authorization,postgres}.rs` modules. It receives a
server-issued bounded request for one selected employee's exact opaque key
reference/version. Signing health does not grant decryption. No ambient keyring,
human desktop identity, profile scan, old-key search, gateway subscription or
runtime-side Office decryption is allowed. Reuse the retained cryptographic
primitives and validation patterns, not Buzz's removed agent orchestration.

Begin with one human and one current employee. Persist ciphertext + inbox
atomically before ACK. Resolve/decrypt outside the database transaction, then
recheck source, participant set, key version, lifecycle and inbox claim under
the central authority fence before committing one decision. Deduplicate by the
verified inner rumor identity as well as the outer delivery: different wrappers
for the same rumor must not create new runs or reset chain budgets. An employee
delegation still needs retained prior-output provenance for its chain root.

Persist hash-only verification/ownership evidence and the original ciphertext;
design the confidential durable RunSpec boundary before enabling this path.
Plaintext must not appear in generic Office projections, search, semantic scorer
inputs, ordinary error logs, or operator-only Activity. Any durable decrypted
input needs participant-gated reads and an explicitly selected at-rest protection
policy/key reference; do not promise end-to-end secrecy from the employee's
authorized server-side runtime. Read-only inspect/status must not decrypt as a
health side effect. Failed verification or unavailable credentials leaves a
bounded durable refusal/retry state and cannot wake anyone.

Acceptance requires real encrypted native send/receive and exact pinned crypto
tests for invalid signatures, wrong recipient/key/rumor author, duplicate inner
IDs with different wrappers, tampering, truncated ciphertext, key rotation,
membership revocation during decryption, disconnect/retry, bounded output and
no plaintext in excluded stores. Only then replace the encrypted-DM unsupported
decision. This gate is separate from opening server-readable private DM channels.

## Retention claims common to every new scope

Stopping permission is immediate even if cleanup is unavailable. Remote removal
is claimed only after exact owned-store text deletion and a matching retained
tombstone/receipt; retries cannot resurrect it. D1 approvals, source Office
content, artifacts, frozen inputs, provider-held context and backups have their
own retention boundaries. Expiry or a reviewed-store ACK does not erase those
copies. Every new durable table must join canonical deletion and offline backup
inventories before deployment; backup capture itself must not manufacture an
erasure acknowledgement or withdraw all active facts.
