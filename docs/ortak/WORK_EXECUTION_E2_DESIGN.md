# Work execution E2 — bounded vertical slice

Status: source implementation, after the verified definition editor in SQL62.
Migration63 has been integrated into source; applied-database parity, the complete
PostgreSQL regression gate and actual isolated model execution remain pending.
This document does not authorize deployment or changes to private resources.

## Implementation and current evidence

The candidate now implements the start operation, shared supervisor Work origin,
terminal output reconciliation, current project-authorized Activity/SSE/cancel and
memory projections, and the existing desktop Work detail's execution/artifact UI.
The definition editor and human criterion/approval controls remain available under
their existing state and role rules. An unreconciled execution retains the cancel
controls and a visible explanation of why another execution cannot start.

Key production seams are:

- `ortak-work/src/postgres/authorized/execution.rs`: one transactional, versioned
  start receipt, definition snapshot, queued run and dispatch outbox.
- `ortak-runtime/src/postgres/work.rs`: current Work admission through the same
  supervisor, validated adapter bindings, containment and cancellation machinery.
- `ortak-work/src/postgres/authorized/output.rs`: terminal job lease and atomic
  immutable text artifact, attachment, result history and move to review.
- `ortak-server/src/work/execution.rs` and `store/visibility.rs`: signed start,
  scoped text reads and current project/source/employee visibility for run reads.
- `desktop/src/features/ortak/work/ExecutionPanel.tsx`: explicit assigned-employee
  start, existing Activity and cancellation controls, and bounded plain-text output.

The output reconciler has a 30-second pass deadline, at most eight normal claims,
15-second leases and a 20-attempt ceiling. Its expired-final-attempt sweep is capped
at 64 retained jobs. It performs no provider I/O. The runtime worker requests one
output job per pass. A terminal run remains an active Work execution until the
durable output reconciliation closes it; terminal status alone does not allow a
second start to race its artifact commit.

The focused desktop Work/client/RunPanel tests passed **31/31**. They exercise
current assignment selection, exact retry body/key preservation, literal text
display, artifact size limits, access revocation and reader cleanup errors through
the production components/client. The scoped bridge Python tests passed **23/23**,
including Work text preservation under silent Office intent and closed diagnostic
codes. These are source tests, not evidence of a deployed provider run.

Three new signed PostgreSQL suites under
`ortak-server/tests/authenticated_routes/work/execution.rs` are awaiting the central
gate. They cover transactional start rollback/concurrency, the shared supervisor
through fake adapter ports, one artifact/review commit, late SSE output, current
project revocation, Work mutation cancellation and refusal of truncated output.
Fake adapter ports prove composition without claiming real runtime/provider health.
The integrating task owns full schema parity and the eventual actual signed
conversation → Work → model run → artifact → review proof.

## Outcome and current seams

An authorized human promotes a visible conversation using the existing E1 API,
assigns one employee, and explicitly starts that assignment. The existing runtime
supervisor executes it, saves a verifiable text deliverable and moves the Work
item to review. Criteria and approval gates retain their existing human decisions;
the runtime cannot satisfy a criterion, approve a gate or complete the Work item.

Before E2, the database permitted a Work foreign key on runs, but the runtime,
admission, Activity and output paths only supported canonical Office decisions.
E2 supplies a separate sealed Work origin at each of those boundaries. Filling in
the foreign key alone still does not authorize execution or expose its output.

## One start operation

Add `POST /api/v1/work-items/{id}/executions` with `operation_id`,
`expected_version` and `employee_id`. Input, model, workspace, credentials, tool
policy, project, source event and employee revision are all server-derived.

Reuse the E1 human operator gate and current contributor/owner grant, actual
project-channel membership, canonical promoted source and configured audience.
The selected employee needs an active owner/contributor assignment, current
cohort/channel eligibility, active lifecycle and exact validated runtime/memory
bindings. A reviewer assignment is not execution or human-review authority.

The first slice permits start from ready or manually in_progress, with resolved
dependencies and no existing review evidence. One active execution per item is a
database invariant. In one bounded transaction, lock Office authority, project,
item, run and outbox in that order; compare the version; freeze the definition and
assignment/revision pins; insert a queued run, execution record and dispatch
outbox; append one Work execution-requested event and advance its version once;
insert the existing operation receipt. Ready becomes in_progress; the UI separately
shows queued versus actually running. Never start a provider inside this transaction.

An identical operation replay reauthorizes current visibility and returns the
existing run. A different payload with the same key or a stale version conflicts.
Retrying a failed execution is an explicit new operation after the prior durable
run is terminal; it is not a hidden automatic fresh run.

## Shared runtime, distinct authority

Represent the sealed source as `OfficeDecision` or `WorkExecution` inside the
existing supervisor. Add `work_run_dispatch` to the typed outbox kind; use the
existing run ID on the lease to look up canonical execution rows and ignore JSON
payload hints. Work runs have null routing decision/message/root columns, even
when their item was promoted from a message. Do not manufacture another routing
decision for an already decided conversation or consume its old chain reservation.
Explicit human operation receipts and the one-active-execution constraint bound
this separate trusted Work-service dispatch origin.

Reuse validated revision configuration, prepare/correlation fences, immutable
RunSpec bytes, stable `ortak-run:{company}:{run}` start keys, lookup after lost
receipts, cancel-by-key tombstones, process containment, bounded pumps and retries.
Snapshot v2 must carry a tagged origin with item, project, execution version and
definition hash. Preserve exact v1 Office snapshot decoding and bytes. Setting
`RunContext.work_item_id` and supplying bounded canonical Work text needs no new
credential or profile fallback.

Current Work facts remain authority. Preparation, snapshot freeze and active-run
refresh recheck current project grant for the recorded requesting human, source
visibility, assignment, item execution version, employee identity and Office/cohort
eligibility. The global API operator configuration is the request-admission ceiling;
durable project grants and current Office membership govern the delegated execution.
If continuous global-config revocation is required, persist a config generation
before adding that promise; the worker does not currently own the API grant config.

Use existing project/item row locks as the Work authority mutex, plus a separate
per-project generation signal for active-run reconciliation. Do not reuse the
exclusive Office mutation fence for Work changes: API authentication holds the
shared Office fence on another connection throughout a request. Project grant
mutation already uses the correct parent FOR UPDATE NOWAIT fence. Generation
triggers cover project archive, grants, item definition/state, assignment and
dependency changes. Work admission records fresh generation/token and checks them
at commit; the generation is a reconciliation signal, never a substitute for
current row/role checks. Reconciliation drains cancellation first and remains
bounded and fair. Work/source/grant revocation enqueues `work_revoked` through the
existing durable cancellation machinery; stopping never needs start permission.

## Deliverable and review commit

The smallest useful artifact is the complete normalized final assistant turn:
nonempty, at most32KiB, at most4096 fragments/1MiB encoded fragment data, with no
truncation marker. These checks already exist in Office `canonical::final_text`;
extract the shared pure assembly without borrowing Office target authorization.
The currently selected Hermes bridge is narrower at8KiB and no tools, so this
slice honestly produces text deliverables, not fabricated workspace files.

Store immutable SHA-256-verified text bytes and provenance in PostgreSQL for this
bounded slice. The artifact address includes company, run and digest; content type
is fixed to text/plain UTF-8. General files, executable/renderable content and large
object-store payloads require a separate artifact-storage port and durable upload
journal. The inherited media store is community/Blossom-oriented and is not a
project-authorized artifact API. This bounded text choice is an explicit limitation,
not a claim that general artifact storage has been delivered.

Completion inserts a durable Work-output job in the same transaction as the
terminal run event. One bounded job transaction locks project, item then run,
verifies the exact current execution/version and canonical terminal event, saves
the artifact and Work link, moves in_progress to review, advances one Work version,
appends one result-ready event and completes the output job atomically. Repeated
terminal cursors/jobs cannot duplicate any of these effects. Human edits, cancellation
or lost authority prevent automatic review; the retained job reports a closed reason.
Failed/cancelled/empty/truncated output never presents a successful deliverable.

Work completion is silent in Office for this first slice. The candidate bridge
derives this from the frozen Work context, retains nonempty assistant text in
`Journal.complete`, and uses a Work-specific instruction. This bridge change requires
the new pinned worker/controller artifact; an older deployed bridge is insufficient.
The Office scheduling trigger has an explicit conversational-origin predicate. No
model-supplied target can publish a Work result to an arbitrary channel.

Required pre-start memory validation and bounded run-scratch recall remain intact.
Do not widen recall to project/company memory in E2. Existing post-run writes require
an acknowledged Office output, so a Work artifact cannot masquerade as one. Initially
report post-artifact memory as unavailable/not requested. Adding it requires a real
artifact-source memory job with current artifact/Work admission and a stable digest
key; that is a separate, explicitly tested extension.

## Exact relational changes to integrate

1. `work_executions`: company + run primary key/FK; same-company project/item,
   assigned employee and pinned revision FKs; requesting human/op ID; requested
   version and execution-start version; bounded immutable definition bytes/hash;
   request time and monotonic terminal/reconciled state. Unique operation attribution,
   unique item/requested-version, and a partial unique company/item active slot.
   Pins never change; no ordinary deletes. A deferred cross-row guard verifies run
   employee/revision/item/origin and the operation receipt/Work history relationship.
2. `work_authority_generations`: company/project primary key, monotonic generation;
   project FK, no reset/delete/truncate. Add nullable Work admission generation/token
   to runs, paired and immutable outside fresh admission. Existing Office admission
   fields continue to protect Office/cohort facts. Project/item/child/grant triggers
   advance the new signal under their existing mutexes; deferred admission checks
   require the same project/current generation at commit.
3. `runtime_work_outputs`: company/run primary key/FK, pending/materialized/failed
   state, terminal source sequence, artifact ID, lease token/expiry, attempt count
   and maximum20, next-attempt time, closed error code and immutable completion
   receipt. Due index and no-delete/monotonic terminal guards mirror existing output
   jobs. A run-terminal trigger schedules it exactly once for Work origin.
4. `artifacts`: company/artifact ID primary key, company/project/item/run FKs,
   terminal event sequence FK, employee revision provenance, fixed media type,
   bounded content bytes,32-byte digest and exact size with SQL hash/length checks;
   unique company/run final-text output; immutable rows. Link it to Work through a
   typed artifact attachment (extend `work_attachments` kind/column/FK and its
   exclusive-reference CHECK/index), not an unverified URL string.
5. Extend outbox kind/CHECKs for `work_run_dispatch`: run and employee required,
   routing decision null, unique company/run ticket, run/execution employee identity
   checked at commit. Extend the adapter-specific dispatch claim to both origins.
   Add `work_revoked` to cancellation reason CHECK/type. Limit the existing Office
   output trigger to conversational runs. Work/API receipt action CHECK only needs
   extension if execution uses a distinct action rather than `mutate_work_item`.

No new conversation routing table, fake source event or credential table is needed.
Every new retained table/function/index/trigger needs desired-schema reconciliation,
live catalog parity and the explicit community deletion inventory review.

## Product and proof

Expose promotion from a visible Office message into a named authorized project;
show the existing/manual assignment and a separate Start execution action. Work
detail links run state, immutable text artifact, output failure/retry and review
controls. Cancel stays reachable when ordinary editing is frozen. Review completion
continues to require existing authorized human criterion/approval decisions.

Work Activity, SSE, cancellation, memory metadata and artifact reads must share a
new current project/source/employee visibility branch; the present Office gate cannot
be widened just by removing `work_item_id IS NULL`. Artifact GET is authenticated,
returns nosniff/plain text or attachment, and never returns storage credentials,
private paths or cross-project digest existence.

Proof: same-operation concurrency and storage-failure rollback; stale version and
released assignment; project/Office/cohort revocation during recall/start/late output;
lost start receipt and contained cancellation; exact v1 Office snapshot regression;
terminal replay/job crash recovery; digest mismatch and truncation; one artifact,
one review event, unchanged criteria/approvals; authorized and revoked artifact/SSE
reads; then one actual isolated model run through the signed UI/API flow. No provider
call or transaction may use a synthetic passed witness to satisfy these checks.
