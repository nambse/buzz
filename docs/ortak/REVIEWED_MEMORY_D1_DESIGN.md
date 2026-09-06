# Reviewed project memory D1

Status: domain, signed API and desktop implementation are present. Five actual
PostgreSQL signed-API regressions passed against provisional SQL66; the integrating
task owns its remaining retained-provenance/purge check and final migration/parity.
No D1 live/provider write has run. Desktop current-authority, explicit approval
and exact retry regressions and TypeScript checking passed; native workflow
validation remains with the integrating task.

## Product boundary

A human reviewer edits a fact, confirms its exact project and employee audience,
chooses a permitted-use expiry, and saves it with canonical evidence. The source
is a decided Office message in the project's bound channel or a complete retained
text artifact from that project and employee. Raw model output is never promoted
automatically. The fact is reviewed project context, not a replacement for company
policy files or an assertion that Honcho derivation produced verified truth.

D1 stores these records in the Ortak database. It offers current-authority
inspection, a bounded full-text recall preview, and an audited **Stop using**
operation. It does not publish to Honcho, inject new context into employee runs,
enable peer-global representations, or claim remote erasure. This leaves full D
open while making the human review and scope boundary usable and testable.

Only `project_context` paired with exactly one employee is supported initially.
A conversation may supply evidence but does not become a company-wide memory
namespace. Employee-wide experience, human/employee relationship and company-truth
promotion require their own explicit approval/audience policies; the existing
unqualified relationship namespace is insufficient to promise per-human isolation.

## Authority and atomicity

The server derives the company from the authenticated deployment. Current human
membership, configured channels/employees, actual project-channel membership and
the durable project grant are rechecked under the existing Office and project
fences. Creating a fact requires global Operator plus project Owner/Reviewer,
an active project and a currently eligible employee. The edited text is at most
4 KiB, contains no disallowed control bytes, and must pass the existing secret
redaction policy without changes. Unknown request fields and absent affirmative
review are refused before persistence.

The first transaction validates a future expiry no later than 90 days, freezes
the canonical evidence and edited text, writes the fact and one operation receipt,
and commits them together. An operation key is scoped to company and human;
identical replay reauthorizes and returns the original fact identity, while a
different payload conflicts. An expired replay never creates another fact or
extends expiry. No provider or other remote I/O occurs inside the transaction.

Stopping use retains a reason, actor and timestamp, advances the fact version
once and writes the operation receipt atomically. Revocation and expiry cannot
be reversed. Replacing a fact is a new explicitly reviewed operation. Stop using
remains available to an authorized reviewer after project archive, employee
deactivation or evidence removal; these states must not hide the recovery control.

## Reads and permitted use

Inspection is a finite page of at most 25 rows with a durable keyset cursor. It
includes current active/expired/revoked state and provenance. If the source is no
longer visible, content is withheld and the reason is shown with the retained
Stop using control; possession of a fact ID cannot bypass project authority.

Recall preview accepts one project, one configured employee and a bounded text
query. Current active project/employee eligibility and source visibility are
joined before search limits. Only unrevoked, unexpired records qualify. Its result
is capped at eight facts and 8 KiB, with an explicit truncation flag. No automatic
retry loop runs in the browser; transient failures retain a manual retry action,
and authority loss clears retained content.

Expiry means the record stops being eligible at the declared time, even if no
background task runs. Neither expiry nor revocation removes the retained approval
record, database backup or prior outputs. The UI must say **Use until** and
**Stop using**, not **Deleted** or **Forgotten**. Any later runtime integration must
pin the included fact IDs/content hashes, cap the aggregate context, revalidate
withdrawal/expiry at admission and active-run refresh, and durably cancel stale
snapshots. A project-generation notification alone is not sufficient authority.

## SQL66 proposal boundary

The planned retained `reviewed_memory_facts` relation has company/project/employee
foreign keys, exactly one canonical source, immutable edited text/approval/expiry,
a version and monotonic revocation tuple. `reviewed_memory_operations` records
the authenticated action, request hash, fact/version, authentication evidence and
authority deadline. Database guards require the corresponding fact transition and
receipt to commit together, reject scope/source/expiry rewrites and retain evidence
on ordinary delete/truncate paths. Final migration, desired schema, parity and
community-deletion inventory belong to the integrating task.

Tests must bind the signed API and actual PostgreSQL transaction: unknown fields,
reviewer versus contributor, wrong company/project/employee/source, duplicate
operation concurrency, receipt-insert rollback, expiry without a sweeper,
revocation replay, source removal, and recovery controls after deactivation.

## D2: genuine Honcho forgetting needs a separate contract

The selected `ortak-honcho/1` routes currently provide resource ownership inspection,
remember and scoped recall. Remember persists full fact text in immutable replay
receipts and native messages, and can enqueue derivation and pending embeddings.
Deleting one native message does not establish that receipts, queued work,
representations or derived context have forgotten it. No current endpoint promises
record inspection, expiry or erasure, so D1 cannot truthfully expose those claims.

A minimal D2 route may support a new explicitly owned reviewed-fact session with
no derivation or embedding enqueue. It must remain on the same reviewed upstream
revision and use a new tested Ortak extension artifact. The contract needs:

- Exact company/employee/project ownership and immutable native resource identity;
  separate record IDs and durable write/withdrawal keys, never guessed native IDs.
- Transactional record text, source hash and idempotent receipt creation; mutation
  receipts retain hashes and outcomes without retaining erased text.
- One current exclusion/expiry predicate shared by list, recall and replay. A
  withdrawal racing a lost write acknowledgement must prevent recreation.
- Bounded physical removal of the owned text and every associated pending index,
  embedding or queue entry. Any running derivation would require an additional
  durable generation fence; refusing derivation in this slice avoids claiming an
  unverified undo capability.
- Tests with real selected Honcho tables, request retries, delayed acknowledgements,
  concurrent recall/withdrawal and restart. Existing automatic RunScratch records
  remain outside this new erasure capability unless separately proven compatible.

Honcho full-text behavior, embedding/derivation health, D1 reviewer authorization
and a future D2 erasure receipt are separate claims. None authorizes deletion of
the preserved external employee stack or implies that backups were erased.
