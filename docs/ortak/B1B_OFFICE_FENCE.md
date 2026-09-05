# B1b Office authority fence

Status: database routing/admission fence verified; runtime reconciliation and
integrated production-seam verification remain the integration gate.
Date: 2026-09-05

This slice supplies migration 0048 and desired-schema parity for B1 §8. It does
not enable central routing, start an employee, or claim B1b is complete by itself.
The integration lane owns the Rust witness transport, normalization hash,
runtime admission and durable revocation worker.

## Protocol

`ortak_lock_office_authority(company_id UUID) -> BIGINT` is a VOLATILE function
used at the beginning of a short READ COMMITTED transaction, before locking
inbox, root, run or outbox rows. It takes:

1. A nonblocking shared transaction lock on the retained schema/destruction key
   `7094711454081051697` (`buzz_db::deletion::SCHEMA_DESTRUCTION_LOCK_KEY`).
2. A shared transaction lock on the company's Office authority key.
3. For a bound community, nonblocking shared transaction locks on its retained
   deletion key and its Office authority key, followed by a fresh lifecycle read.
4. A fresh SELECT of `office_authority_generations.generation`; no row means 0.

No row is inserted by a reader. Every SQL statement inside the VOLATILE helper
has a fresh READ COMMITTED snapshot after its lock grant. Non-active communities
fail closed. REPEATABLE READ and SERIALIZABLE are rejected, matching the retained
community deletion contract.

A snapshot records this generation **before any normalized input reads**. A
routing commit takes the same fence and compares the witness before persisting
the decision, reservations, chain counters and dispatch outbox. An unchanged
generation proves that the covered authoritative inputs have not changed since
the snapshot, including changes reverted to their original values. A mismatch
requires a fresh snapshot and decision, not reuse of the old eligible set.

Runtime authorization rereads canonical normalization under the same fence and
compares the persisted `office_input_hash`, which excludes policy/candidate
revision fields. It then reads the run's immutable pinned configuration. Thus a
new active revision with unchanged Office identity does not replace the old
run's pinned permissions. Preparation compares the newly captured generation,
then atomically stamps the admitted run. No transaction spans a runtime call.

## Mutation coverage and lock ordering

BEFORE-row triggers watch authoritative columns, not every UPDATE. All changed
old/new scopes are covered. Community writers take the community Office key
**exclusively using a try-lock before looking up the company mapping**, then
try-lock the company's Office key exclusively and advance its generation in the
same transaction. Binding insertion/deletion fences both the explicit company
and community, including a community with no mapping yet. A concurrent Office
insert cannot escape merely because its mapping lookup found no row.

UPDATE and DELETE can hold their target tuple before a BEFORE trigger executes.
Therefore mutation triggers never wait for an authority advisory lock: contention
raises SQLSTATE `40001` (`serialization_failure`) and the entire writer rolls
back. This prevents an inversion with a fenced reader that next needs the same
tuple. Community write-fence triggers run alphabetically before the Ortak
triggers, retaining existing deletion authorization. The reader's reverse-order
community/deletion acquisition also uses try-locks, so it cannot wait while
holding the company fence on a deletion executor that needs that fence.

Mutating callers must propagate this error or durably retry the whole transaction
with bounded backoff. A failed removal is not a successful revocation. This
private-MVP protocol intentionally favors a retryable failure over a deadlock;
lock/statement timeouts still apply to the reader and ordinary row locks.

| Relation | Authority watched |
|---|---|
| `channels` | identity, type, visibility, archive/deletion |
| `channel_members` | community/channel/key, role, removal |
| `relay_members` | identity/existence |
| `users` | key, automation markers, deactivation |
| `events` and all attached partitions | canonical identity, kind, text, tags, signature, channel, deletion; UPDATE/DELETE |
| `thread_metadata` | canonical event and persisted parent identity/timestamp |
| `communities` | lifecycle, deletion generation/timestamp |
| `office_company_bindings` | insertion/removal, including absence |
| `employee_office_bindings` | employee/key/signer, validity and verification, including historical-key insertion |
| `employees`, `employee_revisions`, `employee_aliases` | roster, lifecycle, active immutable manifest, routing names |
| `employee_runtime_bindings` | pinned runtime configuration and validation |
| `office_inbox` | canonical input facts; UPDATE/DELETE |
| `companies` | status and routing policy |
| `runs`, signed `office_publish` outbox | employee/message/root and frozen publish provenance |

New events do not advance the generation: a missing canonical message or parent
cannot have produced an authorized wake. Parentless thread metadata is equivalent
to its absence. New run rows have no publish provenance until a signed outbox row
exists. Cosmetic fields, unread/reply counters, inbox claims, outbox leases and
run lifecycle changes do not advance authority. Ordinary new messages therefore
do not revoke every active run.

Generation rows cannot be deleted, retargeted or decreased. A failed mutation
rolls back its generation increment too. Different companies do not share a table
lock. Hash collisions only introduce conservative contention, not an access gap.

## Time and durable reconciliation

Rows alone cannot fence the clock. The witness includes the earliest future
`valid_from`/`valid_until` boundary from Office bindings. Both the witness cutoff
and normalization use `clock_timestamp()`, so a transaction waiting for its
company fence cannot continue authorizing from its earlier transaction `now()`.
A boundary crossed after witness capture must still invalidate that witness.

Migration 0048 adds:

- `routing_decisions.office_authority_generation`,
  `office_authority_valid_before`, and `office_input_hash`.
- `runs.office_admission_generation`, `office_admission_valid_before`, and
  `office_admission_token`.

Deferred constraint triggers recheck generation and `clock_timestamp()` when a
witnessed decision or newly stamped/changed run admission is about to commit.
Expiry while waiting for a root/run row therefore aborts the transaction, even if
an earlier application check succeeded. Every explicit admission/re-admission
writes a fresh UUID token, even if its generation and deadline are identical to
the previous attempt; changing that token forces the deferred check. A witnessed
run must carry a token. Run lifecycle-only updates retain the token and do not check
an expired admission witness: cancellation must remain possible after revocation.
Historical NULL witnesses are preserved and must not authorize runtime admission.

The generation row is a bounded, coalescing durable reconciliation signal, not
an event log. Post-admission mutations advance it transactionally without locking
or updating any run. A worker must compare every active run's admitted generation
and deadline with current authority, rerun canonical authorization, and either
advance its admission witness or persist/retry cancellation by stable runtime
idempotency key. A changed active revision with the same Office identity can pass
that revalidation while retaining the original pinned configuration. Lost start
acknowledgements require discovery/cancellation of the same external run; they
must not be abandoned or started under a new identity.

**That worker is not provided by this SQL slice.** Generation storage alone is
not proof of post-admission revocation. Live routing remains gated on its
integration and restart/reconnect tests.

## Schema operations

Row mutations and scoped deletion are the supported online paths. TRUNCATE is
rejected on the authority tables and existing event partitions. Row triggers
clone onto attached partitions. The desired-schema reconciliation script removes
copied parent-trigger names before ATTACH, restores per-partition TRUNCATE guards,
and verifies the live event row-fence catalog.

The retained `crates/buzz-db/src/store/partition.rs` only creates future partitions;
it has no partition retirement path. New partition creation must run the catalog
reconciliation before serving. Migrations take the exclusive schema/destruction
lock and cannot overlap a fenced routing/admission transaction. Administrative
DROP/DETACH/trigger disabling can bypass row triggers and are **offline deployment
operations**: quiesce routing, make the schema change, advance affected company
generations, verify the guard catalog, then resume. The shared schema lock alone
does not invalidate a snapshot taken before an administrative DDL change. No
online partition retirement or generic arbitrary SQL API is authorized by this
change.

## Verification

The dependency-free script `scripts/ortak/test_office_authority.py` exercises the
production SQL functions/triggers using independent `psql` backends. It accepts
only an explicit `ORTAK_TEST_DATABASE_URL` pointing to localhost port 55432,
ignores `DATABASE_URL`, creates fresh generated fixtures, and never creates or
drops a database. Query/process waits and captured session output are bounded.

Executed successfully on 2026-09-05 against two isolated databases:

- `ortak_b1b_fence_desired`: full `schema/schema.sql`, followed by
  `scripts/reconcile-schema-after-pgschema.sql` (direct SQL bootstrap; this is not
  a claim that the pgschema CLI itself was exercised).
- `ortak_b1b_fence_migrations`: all checked-in migrations 0001–0048, each in its
  own transaction through psql (SQLx wrapper/checksum behavior is tested by the
  integration lane).

```sh
ORTAK_TEST_DATABASE_URL=postgres://ortak:ortak@127.0.0.1:55432/ortak_b1b_fence_desired \
  python3 scripts/ortak/test_office_authority.py
ORTAK_TEST_DATABASE_URL=postgres://ortak:ortak@127.0.0.1:55432/ortak_b1b_fence_migrations \
  python3 scripts/ortak/test_office_authority.py
```

Both runs passed 17 mutation paths plus concurrent mutation after a reader's
fence, absent employee-key insertion, cross-company progress, stale-decision
commit rejection, actual writer-first advisory waiting with a fresh generation,
absent community mapping insertion, retained deletion lock ordering, cosmetic
write exclusions, deferred routing/admission/re-admission expiry, cancellation after expiry,
generation rollback/decrease protection, parent/child TRUNCATE denial, schema
lock exclusion and isolation-level rejection. These tests bind the SQL seam;
they do not replace the integrated router/runtime tests or a real employee run.


Review follow-up: the same-witness re-admission regression was added after finding
that a blocked existing-run retry could rewrite identical generation/deadline
values and skip the original deferred UPDATE guard. The admission token makes
that explicit attempt observable to the trigger without blocking status-only
cancellation. Both schema-path databases passed the expanded suite after the
same token-column/constraint/function amendment; SQLx integration owns the fresh
full migration checksum test for the final 0048 text.
