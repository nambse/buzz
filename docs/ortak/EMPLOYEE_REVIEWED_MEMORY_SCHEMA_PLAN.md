# Employee-owned reviewed memory: storage candidate

Status: source-only SQL design, 2026-09-06. The parent reports six pure identity
tests passed. This [candidate](sql/employee_reviewed_memory_candidate.sql) has
not been executed or assigned a migration number. It changes neither the
deployed schema nor the current recovery inventory. Approval, publication and
target registration intentionally refuse through three closed integration
ports. There is no runtime, API, native UI, provider operation or current-access
claim in this delivery.

The [pure contract](EMPLOYEE_REVIEWED_MEMORY_CONTRACT.md) remains the identity
authority. [Architecture §4.5](ARCHITECTURE_V0.md) requires employee experience
and explicit human/employee relationships; neither is a project in disguise.
The candidate depends on the immutable chain through 76, including source75's
canonical JSON encoder and existing Office/community locks. It adds no value to
legacy `MemoryScope` or `reviewed_memory_facts.audience_kind`, and no joins from
legacy project/conversation selectors to these new tables.

## Approved first policy

A current non-agent human may share edited memory from **their own** exact
decided plaintext Office message. Both human and employee must currently belong
to its source channel and the explicitly selected destination channel, in the
same bound community. Relationship additionally names that human and requires
the authenticated approver to be that human. There is no project placeholder,
company-admin override, inferred private-retention permission, imported peer
summary, automatic extraction or company-wide sharing.

The signed edited approval is the separate destination-sharing action. Source
membership alone does not grant it. The UI must preview source/destination and
kind, require edited text plus explicit review, and never prefill the source
body. Changing destination or relationship human requires a new preview and
approval. Initial Stop/retry-withdraw recovery belongs to the original approver
under the same explicit employee ceiling, even after source loss or employee
disable. Recovery does not reveal formerly readable content.

## Storage and immutable bytes

| New retained table | Purpose |
| --- | --- |
| `employee_memory_channel_authorities` | Source/destination channel epochs keyed by company, community and durable employee; 128 retained keys/company, 256/community. |
| `employee_reviewed_memory_facts` | Edited experience/relationship text, explicit destination, original source partition, pure canonical audience/provenance, approval and one-way Stop. |
| `employee_reviewed_memory_operations` | Immutable authenticated approve/Stop operation, exact submitted bytes/hash and resulting version. |
| `employee_reviewed_memory_targets` | Exact owned employee namespace, immutable old binding/creation receipt, destination opt-in, current revision/lifecycle witness and target epoch. |
| `employee_reviewed_memory_exports` | One immutable target selection and content/source/sharing hashes per fact. |
| `employee_reviewed_memory_export_jobs` | Exactly two stable publish/withdraw operations, finite attempts/backoff and lease ownership. |
| `employee_reviewed_memory_export_commands` | Immutable explicit publication/retry instruction and resulting retry version. |
| `employee_reviewed_memory_export_receipts` | Exact remote result tied to request, original binding, attempt and live lease. |

No transient source event, channel or membership FK cascades into history.
Every table has a community fence, no DELETE and no TRUNCATE. Fact content and
identity are immutable; version 1→2 records Stop only. Operations, exports,
commands and remote receipts additionally reject UPDATE. Historical parse/read
does not re-run current source eligibility. Restore must preserve these exact
bytes and later install/verify the normal guards; it cannot manufacture a new
approval by replaying expired history through a current mutation API.

The pure audience is canonical compact JSON, ≤2 KiB; provenance ≤4 KiB; edited
content ≤4 KiB UTF-8. The source ID, original PostgreSQL timestamp partition,
channel, author and evidence hash are columns and bound by the pure source hash.
The approval binds edited-content hash, actual approver, operation/approval ID
and absolute expiry; sharing hash covers the full provenance bytes. SQL rebuilds
the declared pure wire and compares exact UTF-8, never merely JSONB equality.
Expiry is finite, after approval, ≤90 days and no later than the current source/
destination/Office identity deadline. Six-digit UTC timestamp formatting is
shared with the pure wire; old expired values remain structurally valid.

Approve submitted fields are operation ID, employee, kind/human, selected source
ID+partition, destination, expected audience hash, edited content, expiry and
`reviewed=true`. No client source hash/root is accepted. Canonical command bytes
have a separate `ortak-reviewed-employee-command/1` domain and 32 KiB escaped
budget. Stop submits fact ID and expected version. Actor + operation ID is the
immutable key: authenticate and apply the recovery ceiling, then compare the
submitted fingerprint and return the original receipt **before** fresh source
resolution/expiry checks. A changed submission conflicts. Fresh approval/Stop
must atomically write the fact effect and matching receipt, both checked at
commit. Same-transaction approve-and-Stop is deliberately refused; each action
has its own receipt.

## Three closed ports and next implementation dependencies

`ortak_employee_memory_observation(company, employee, actor, source_id,
source_created_at, destination_channel, memory_kind, relationship_human)` returns
zero rows in this candidate. Its future implementation must return exactly one
current observation or none: community, source channel/author/evidence hash,
current employee revision/lifecycle, database observation time and earliest
valid-before deadline. It must use the exact canonical event joined to decided
Office inbox under the same company/community binding, not source75's real
project-dependent facade with a fabricated ID. Source kind 9/40002 only;
encrypted 1059, groups, unresolved ancestry, changed/hidden source and ambiguous
partition refuse. Canonical private 1:1 DM support must use the existing direct
participant resolver; the port stays closed for unsupported audiences.

Proposed evidence preimage for that resolver, for root to bind to independent
SQL/Rust vectors before opening it:
`{author_public_key,channel_id,community_id,company_id,content,event_created_at,
event_id,format:"ortak-reviewed-employee-evidence/1",kind,sig,tags}`.
Keys are recursively lexicographic, binary fields lowercase hex, timestamp
microseconds, and content/tags/signature remain the exact stored source. Source
content ≤65,536 bytes, tags JSON ≤16,384 bytes and encoded evidence ≤524,288 bytes
follow source75's finite limits. This evidence is computed privately; the fact
stores only its digest. Do not copy the conversation evidence format or source
hash into the employee format.

`ortak_employee_memory_command_current(company,employee,actor,action)` checks
current bound company/community, non-agent member and employee data. It does not
authenticate SQL-credential holders. The private Principal-only server facade
performs genuine NIP-98 verification and explicit deployment capability/ceilings,
with original-approver receipt recovery before fresh source/expiry admission.
Caller booleans, bare event IDs, SQL GUCs and historical receipts confer no grant.
The base candidate installs a refusing current-data placeholder; concatenate the
authority fragment after it. Source-only signed tests are prepared, not executed.

`ortak_employee_memory_target_authorized(...)` returns false. A new reviewed
remote protocol must prove the owned namespace
`{company_id,employee_id,format:"ortak-reviewed-employee-namespace/1"}` and its
creation receipt plus actual I/O witness. `reviewed-employee/1` is a proposed
capability, not one the current Honcho adapter advertises. Binding hash is
domain-separated by namespace hash and protocol. Existing project receipts,
generic experience/relationship calls, health alone, or forged receipt JSON
cannot pass. Root must define/test the exact owned creation result and immutable
request/result wire before replacing this gate. Target advertisement lives at
most 60 seconds, binds the current memory/revision/lifecycle, and never silently
rebinds an old export to new credentials or native IDs.

## Locks, revocation and cleanup

Snapshots and digests are not locks. Every new approval/publication transaction
uses READ COMMITTED: acquire the shared Office fence in a separate statement,
resolve current permissions, register/lock sorted source+destination scope rows,
then lock fact, target and finally jobs. Recheck the canonical source and clock
at commit; no network I/O under the transaction. Mutation hooks use exclusive
Office try-locks before discovering bounded retained scopes and sorted NOWAIT
scope locks; a busy fence is a retryable failure, not an unlocked fallback.
This also protects a concurrently registering first fact. Source/event/thread,
membership, bot classification, Office/memory identity, lifecycle and community
changes advance epochs. Unrelated first replies/plain user metadata do not
invalidate an output merely because it was inserted. Model-only revision changes
with identical Office+memory identities and lifecycle keep employee identity.
Time-only expiry must be rechecked explicitly; it does not manufacture an epoch
mutation. Target disable/enable, lifecycle changes and lapsed witness renewal
advance target epoch. Stop advances the fact version irreversibly.

Removal/restoration cannot revive a frozen old use: a later runtime integration
must pin fact version plus exact content/audience/source/sharing hashes, current
source and destination epochs, target epoch/binding/owned namespace and actual
run revision/lifecycle. It must resolve the actual destination and human from
central Office/promoted Work provenance. Relationship must match that actual
human; experience still requires authorized destination access. No session/model
inference or worker-local Office subscription is allowed. Recheck at selection,
remote recall, freeze, admission, output and post-ACK memory writes. There is no
employee runtime record/snapshot version or no-browse promise in this candidate;
those remain explicit typed/SQL/adapter integration work.

Publication creates both stable keys atomically:
`employee-reviewed:{publish|withdraw}:{company}:{fact}`. The scheduled withdrawal
initially falls due at expiry. Stop makes it due in the same transaction.
A future fair bounded worker scan also calls the one-fact cleanup scheduler on
source/destination/target loss, without an unbounded source-trigger fanout. This
worker and its scan cursor/coverage remain an integration dependency. Current
use denies immediately; remote erasure is only claimed after its exact ACK.

Before external publication, the worker must reacquire current permission and
its exact claim; lease possession alone does not grant sharing. Cleanup and ACK
recovery instead use the immutable old target, even after disable or permission
loss. Missing credentials become durable `target_unavailable`, never success.
Each claim has a unique ≤60-second lease; attempts are capped at 20 per explicit
retry, 8 retries/180 total, with ≤301-second finite retry delay. The original
stable request hash remains unchanged across retries. The target protocol must
permit withdrawal **before** an uncertain publish, permanently tombstone the
same identity and refuse late resurrection. Two remote operations are enough;
expiry must not get a third competing tombstone key. Exact duplicate ACK replay
is receipt-only and must not cause fresh I/O or model delivery.

Before migration/deployment, root must integrate all eight retained tables into
canonical deletion and recovery inventories. Deletion must refuse unresolved
publish leases (expired means uncertain), failed/pending cleanup and any future
runtime use before closing the community fence. Backup preserves future pending
withdrawals as retained obligations while all writers are paused; restore stays
inactive until original-writer containment and same-key reconciliation/expiry
catch-up. Local byte retention and a reviewed-store tombstone do not claim
physical erasure of backups, generic Honcho representations or other stores.

Root's later gates: exact pure↔SQL vectors, cross-company/partition/human/source
refusal, same-key replay after source loss, changed draft conflict, atomic
approval/Stop, held-lock membership/identity/expiry races, non-resurrection,
model-only identity stability, forged target/lease/receipt refusal,
withdraw-before-publish and crash retry, and populated deletion/physical restore.
Then the native edited approval → explicit destination publication → permitted
reuse/other-human omission → Stop/owned withdrawal workflow. None was executed
for this candidate.
