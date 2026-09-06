# Reviewed project memory D2b

Status: first publication/cleanup slice implemented. Central validation passed
all three new Rust units and all nine actual PostgreSQL tests, including the
canonical purge/retained-receipt workflow. Root is integrating immutable69 and
desired-schema parity; selected live69/Honcho rollout remains pending.
D2a validation passed: 23 real Honcho PostgreSQL tests (including all nine reviewed
record tests), 11 local tests, two Rust socket contract suites and pinned runtime
and test images. The candidate is not deployed to the selected live Honcho.
Root owns migration integration, final image selection and live validation.

## First deliverable — D2b

An authorized reviewer explicitly requests publication of one existing D1 fact
for its named employee and project. One database transaction records the command,
retained export identity and two durable operations. The existing worker publishes
only the approved text using D2a, records a validated hash-only acknowledgement,
and removes that exact reviewed-store text after Stop using or expiry.

This slice does not change runtime inputs, snapshots, run-use records, semantic
routing or recall. Employee runs do not consume these publications. The later
D2c boundary below describes planned work, not delivered behavior.

Existing D1 records remain preview-only until that explicit action. The shipped
D1 UI said that saving a fact would neither publish it nor put it into runs;
deployment must not silently reinterpret those earlier approvals. Newly created
facts can later offer a clearly labelled combined save-and-publish action, with
both durable records in the same transaction.

The planned D2c runtime integration supports Work-origin runs only. `WorkRunOrigin` already
contains the authoritative project ID; ordinary Office runs do not. A channel
must not be guessed to identify a project. Conversation, relationship, general
employee and company-truth namespaces, DM decryption, central semantic routing,
automatic extraction and native Honcho embeddings/derivation remain separate.

## Existing seams that must change

- D1's `AuthorizedWork` owns human/project/source approval and Stop using. Its
  current full-text recall is a preview over the approval registry.
- `WorkerMemory` restores the full original creation receipt and maintains the
  existing explicit I/O witness, but its selected employee bindings currently
  have an empty project allowlist. Reviewed publication needs a finite explicit
  project allowlist plus current database authorization; either alone is
  insufficient. Ordinary health remains read-only and cannot issue a witness.
- `runtime_memory_writes` is an Office-delivered RunScratch queue. Its canonical
  output and delivery-ack guards are deliberately inappropriate for reviewed
  facts; it must remain unchanged for this purpose.
- `FrozenRunSnapshot` versions 1 and 2 accept only same-run RunScratch. A new
  snapshot version must represent reviewed records and their approval pins
  separately, without inventing a run that originally authored the human fact.
- Active-run reconciliation currently becomes due on Office/Work generation,
  lifecycle or admission expiry. It needs a durable due predicate for a used
  reviewed fact's withdrawal/expiry, even if no unrelated generation changes.

## Durable export contract

The existing worker recipe opts in per employee with `reviewed_projects`, a list
of exact project UUIDs (at most 16 per employee and 128 in the whole recipe).
An omitted/empty list advertises no targets. A nonempty list requires the full
original `creation_receipt`, even when the legacy recipe's global strict-receipt
flag is off. Ordinary health is insufficient: only the existing explicit owned
resource/actual-I/O validation can create an advertisement. Advertisements live
for at most 55 seconds and are refreshed at most once every 25 seconds. Their
revision/lifecycle and full current memory binding are rechecked in PostgreSQL;
the worker rechecks the configured original receipt and live adapter witness
before the actual remote operation. No existing recipe allowlist is changed by
deployment.

The signed API exposes `POST /api/v1/projects/{project}/reviewed-memory/{fact}/publish`
with `operation_id`, `expected_version: 1` and `confirmed: true`. Failed jobs can
be reopened through `POST .../{fact}/exports/{publish|withdraw}/retry`, supplying
`operation_id` and the current `retry_version`. The current fact inspection
returns `publication_available` and opaque publication/cleanup status; it never
returns the endpoint, native IDs or full resource receipt. No separate worker or
unbounded drain loop is introduced.

The publication API reuses current Operator plus project Owner/Reviewer gates,
current employee and source eligibility, exact company/project/employee scope,
and the fact's immutable content/expiry. It also resolves the selected full
Honcho creation receipt. The browser supplies an operation UUID and fact/version,
never an endpoint, credential, native resource ID or arbitrary publication body.

The export retains the original deployment ID, complete non-secret binding and
creation-receipt identity, fact ID, content hash, canonical source hash, approval
operation and approving human. The source hash uses canonical signed-message
identity, or the immutable artifact identity and content hash; no raw evidence
is copied into job metadata. Use the fact ID as the remote record ID and derive
stable publish/withdraw keys from the retained export identity. Request
hashes cover the exact D2a request, including normalized immutable expiry.

Proposed additive relations:

| Relation | Retained facts and constraints |
| --- | --- |
| `reviewed_memory_targets` | Short worker advertisements after current actual I/O validation; explicit finite project allowlist, exact full owned creation receipt/binding, active employee revision/lifecycle and expiry. Retired identities remain available for cleanup. |
| `reviewed_memory_export_commands` | Immutable signed human publication/retry receipts; exact operation/payload identity, auth deadline and atomic effect. |
| `reviewed_memory_exports` | One immutable publication instruction per company/fact; project, employee, selected binding/creation receipt, hashes, approving action and auth evidence. Unique authenticated operation key and exact-payload replay. No content copy or credential. |
| `reviewed_memory_export_jobs` | Exactly two stable jobs per export: publish and withdraw. Immutable action/request identity; state, lease token/expiry, bounded attempt count, next-attempt timestamp and closed failure code. Withdrawal is scheduled at immutable expiry when publication is requested, so expiry never depends on scanning all facts. |
| `reviewed_memory_export_receipts` | One immutable, validated acknowledgement per export/action, with native scope/binding and request/content hashes, observed status and narrowly scoped erasure proof. No text or arbitrary remote response JSON. |

Foreign keys point to durable company/project/fact/run provenance, avoiding the
transient binding-FK purge blocker fixed in D1. Any community-owned rows require
the universal community write fence and deletion inventory integration. The
publication instruction, its jobs and its API receipt must commit atomically;
Stop using atomically brings the existing withdrawal job forward when an export
exists. Deferred
guards must reject a committed instruction or state transition without its
matching durable records. Scope/keys/receipt identity cannot be rewritten and
terminal acknowledgements cannot be replaced. Applied migration 66 is unchanged.
Every initial export requires both jobs and its authenticated command at commit.
Claims require a due unlocked job and a fresh bounded lease; acknowledgement
requires the exact live lease and immutable receipt in the same transaction.

The worker claims one due job with a short lease, rechecks its current authority
in a bounded transaction, commits, then calls Honcho outside the transaction.
Publication requires the fact still eligible, unrevoked and unexpired, and the
selected current binding to equal the retained binding. A lost acknowledgement
retries the same request and key. Withdrawal may reach Honcho before a delayed or
uncertain publication: its stable key installs an irreversible record tombstone.
A delayed publication cannot resurrect the text. Expiry uses this same withdrawal
identity; a third operation key would compete for that one removal identity.
A late acknowledgement cannot overwrite a
newer lease or mark a withdrawn/expired fact eligible again.

Removal is recovery: current project grants, source visibility and employee
Active state cannot be prerequisites for removing the already exported text.
It uses only the exact retained owned binding and removal identity. Changing a
binding does not retarget that job to a new workspace. Unavailable old credentials
or bindings leave a visible durable cleanup failure with an explicit same-job
retry action. No resource adoption, creation or whole-workspace deletion occurs.
Canonical community quiescence/purge must finish this external cleanup beforehand.
It refuses unacknowledged exported-text removal and pending publication leases,
including expired leases with an uncertain external outcome. The universal write
fence remains authoritative after quiescence; cleanup cannot continue behind it.
Cleanup success must not be implied by local purge. Root owns final lifecycle and
deletion integration.

Backup capture does not withdraw live facts. A future scheduled withdrawal may
remain as an exact retained recovery obligation while all writers are paused.
The first capture refuses pending leased operations, uncertain external results,
due publication/removal and failed cleanup. Acknowledged jobs retain their
historical lease as receipt provenance, so only pending jobs' lease fields count
as active leases. Backup creation never mints a cleanup acknowledgement. Restored
services remain inactive until original-writer containment and same-key
reconciliation/expiry catch-up are established.

Use the existing cancellation-first worker order, one job per pass, a finite
external deadline, capped backoff and terminal failure after 20 attempts. Every
failure persists or propagates. Manual retry reopens the same immutable job and
key through an authorized audited action; it never creates another export.

## Deferred D2c — recall, frozen input and current authority

For a Work run, first derive its current project, employee, revision, lifecycle,
source audience and assignment through the existing runtime authority boundary.
Select at most 32 eligible published fact IDs using current D1 source predicates,
unrevoked/unexpired status and the exact retained binding, before the limit. The
query is a bounded derivative of the Work input and passes normal redaction.

D2a's current `/recall` filters its own store but cannot accept Ortak's exact
current fact allowlist. Filtering its eight results afterwards can let withheld
records consume the whole result window. D2c therefore needs a distinct bounded
selected-recall endpoint (or a versioned request) that accepts at most 32 IDs and
applies them before search/ranking/limit. This is an additive owned-extension
change on the same upstream pin and requires a newly tested Honcho artifact.

The returned records must match the local allowlist, immutable hashes, approval
provenance, expiry and exact resource binding. A local registry result is not
substituted for a failed Honcho request. An empty permitted allowlist can skip the
remote call; selected but unavailable required context follows the existing
bounded memory failure path. The first version returns at most eight reviewed
records and 8 KiB; combined RunScratch plus reviewed context stays within the
existing eight-record/16-KiB total and encoded snapshot cap.

After external recall, the final freeze transaction re-derives current authority
and locks/rechecks the exact selected fact rows in UUID order. It inserts the
immutable uses with the durable snapshot winner, verifies their one-to-one hash
agreement, and bounds admission by the earliest used fact expiry. Lost-start
retries reuse those exact bytes; they do not recall a different fact set.

Use one established order: Office authority, project, Work item, sorted fact rows,
run, then dispatch outbox. Stop using takes Office/project/fact locks but does not
synchronously fan out run-row updates while holding a fact row. The immutable
uses and changed fact version provide the durable cancellation scan signal.

Current used-fact validation is mandatory at snapshot load/freeze, final start
admission, correlation/recovery, active admission refresh, and terminal artifact,
delivery and post-run memory materialization. It checks the fact is still
eligible and version/hash/binding match, plus all existing Work/Office/lifecycle
guards. A notification or cached generation is only a wake signal. Reconciliation
claims a bounded set of active runs with invalid/expired uses and creates existing
durable cancellations; expiry alone must make a row due. A queued cancellation
survives restart and stays effective even if unrelated grants later return.

Withdrawal racing the final remote start has the same unavoidable external-call
boundary as existing cancellation: the completed freeze is the admission point,
and a subsequent withdrawal durably cancels the admitted run. Already delivered
context cannot be retracted from the provider. Late output must still be refused.
The UI must distinguish 'use stopped', 'runtime stop pending' and 'reviewed-store
text removed'; none means that approval evidence, prior outputs or backups were
erased.

## Validation and delivery order

1. D2a validation is complete on fresh isolated resources; selected live rollout
   remains root-owned. Runtime image:
   `sha256:febea5609d74f51026ab5a98ac9ce7b8648989ac7f639893ef4f71846c65dc1b`.
   Tests image:
   `sha256:ed36fa1d772d7fa47b4727297489b25b845712396f926f2ac37548ef6b328f5b`.
2. D2b: explicit publication, atomic two-job/command/export persistence, hash-only
   acknowledgements, cleanup and truthful UI. Passed production regressions:
   duplicate command, rollback if a command/receipt cannot persist, lease fencing,
   durable backoff, source/current-target revocation, withdrawal before uncertain
   publication and expiry. No existing fact is automatically published.
   Central evidence: `reviewed69-test-4287bb317a904ab18216e29e3199616e/test-receipt.json`
   in the private evidence ledger. Desktop focused memory tests passed 10/10;
   TypeScript typechecking and scoped Biome checks passed. These proofs use
   disposable PostgreSQL and controlled adapter outcomes; D2a's two actual socket
   suites and real Honcho PostgreSQL/image tests separately validate transport and
   native tombstone behavior. No live publication or physical-erasure claim is
   made by the fixture evidence.
3. D2c: first add and validate selected recall with the allowlist applied before
   ranking/limit; then typed runtime context and at most eight immutable
   `run_reviewed_memory_uses` per run. Prove wrong audience rejection, stale remote
   response rejection, byte-identical lost-start retry, and withdrawal/expiry
   cancellation plus late-output fences across restart.
4. Root performs selected-stack publication and referenced reviewed-store text
   removal checks for D2b; actual Work consumption follows only after D2c. No
   erasure claim extends to RunScratch, prior provider inputs, backups, approval
   records or the preserved external stack.
