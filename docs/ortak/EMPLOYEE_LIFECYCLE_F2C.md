# Employee disable and re-enable — F2c

The employee identity and its immutable revisions survive a lifecycle change.
An explicit disable permanently invalidates work admitted in the preceding
lifecycle epoch. Re-enable requires a new prepared selection, fresh health
probes and a new revision; it never resumes earlier work or delivery jobs.

## Durable barrier

Add `employees.lifecycle_epoch`, starting at zero, and immutable
`employee_lifecycle_epoch` pins on `routing_recipients`, `runs`, provisioning
operations, configuration drafts and management commands. Existing rows start
at zero. A new Office recipient pins the current epoch in the routing commit;
an Office run copies that recipient pin rather than the current employee value.
A Work run pins the current epoch inside its authorized execution transaction.

Disable atomically changes status to disabled, advances the epoch exactly once,
and stores an immutable barrier attributed to the signed management command.
The event retains the exact command lease token and expiry; its deferred commit
check requires that lease to remain live after the worker clears its active lease.
Only the employee transition trigger may create a lifecycle event. A raw event
insert cannot forge a barrier or its attribution. The revision and all prepared
external resources remain intact. A replay returns
the same command/barrier and cannot increment twice. A different request must
match the current revision and epoch. Direct disabled-to-active writes are
rejected unless the same transaction consumes the sealed re-enable operation.

Dispatch, immutable snapshot admission, active-run authority refresh, Office
output preparation/publication, Work artifact materialization and post-run
memory writes all require the original pin to equal the current epoch. Old
completed artifacts remain readable under current ordinary read authority;
this barrier forbids new external effects, not historical inspection.

The existing bounded runtime reconciliation scans epoch mismatches even if a
disable/re-enable cycle finishes between scans. It creates durable cancellation
requests and stops by the existing stable runtime start key. Cancellation never
requires permission to start. Pending, acknowledged and failed stops remain
visible; disabled does not claim every process has already stopped. Future
Office dispatches from old decisions remain permanently ineligible.

## Audited product operations

Reuse the F2 execution capability, exact employee scope, signed NIP98 admission,
immutable request key and server-owned prepared catalog. Extend commands with
`disable` and `reenable`, plus `expected_lifecycle_epoch`. The API only commits
the command and its audit receipt; it never takes the exclusive Office fence
while authentication holds a shared fence, and never probes an adapter inline.

Disable needs the current active/paused employee revision and epoch. It requires
no catalog, credentials or external health. The worker applies it in one short
Office-authorized transaction. Existing per-employee command serialization stays
explicit: a pending management operation must settle before another command is
admitted. Queued disable is shown as queued until the barrier commits.

A disabled employee may save a new immutable prepared draft pinned to its epoch
and revision. That draft admits only re-enable, using a fresh Update-mode saga
operation. A legacy disabled identity with no active revision uses the same
Update flow with an explicit null expected revision and exact epoch; it still
requires its first fresh activation and cannot use ordinary Adopt. The
repository sealed to the leased command alone may pass the
disabled identity reservation gate and issue activation authority. Existing
operator CLI and ordinary Update paths still refuse Disabled employees.
All ten steps and the final short-lived runtime, memory, signer and Office
health probes run through the production adapters. Successful activation creates
a new immutable revision and consumes the exact disabled epoch intent. Failed
or interrupted probes leave the employee disabled, with ordinary durable retry
and retained-resource compensation; they do not toggle status speculatively.

The new revision remains in the post-disable epoch. New decisions and runs can
use it, while every old pin remains invalid. Another disable advances the epoch
again, including invalidation of unfinished provisioning selections.

## Integration and proof

SQL65 is a proposal only; migration63 and migration64 remain unchanged. Root
integrates the append migration, desired schema, reconciliation and live parity.
The implementation touches management admission/executor/UI, sealed provisioning
activation, runtime/Office/memory authority and the two coordinated Work seams.

Production PostgreSQL regressions must prove: exact concurrent disable replay;
scope/current-role denial; no secret lookup on disable; immutable epoch/barrier;
old queued Office and Work refusal after re-enable; old active cancellation even
when the intermediate disabled status was missed; old terminal Office/Work/memory
output refusal; fresh epoch execution; ordinary CLI and direct-SQL re-enable
refusal; fresh sealed re-enable health and revision commit; failed health remains
disabled; revoked/replaced lease cannot activate; retry cannot adopt another epoch.
Desktop tests bind actual controls, exact expected revision/epoch request bodies,
pending versus stopped wording, failure recovery and authority revocation.

Actual deployment acceptance still requires disable/re-enable and a fresh run
with the real prepared resources; missing-credential fixtures cannot claim it.

## Migration regression

Before freezing migration65, root creates a fresh disposable database on 55432
and applies source migrations1–64, then applies these files in order:

1. `proposals/0065_legacy_disabled_fixture.sql`
2. `proposals/0065_employee_lifecycle.sql`
3. `proposals/0065_legacy_disabled_verify.sql`

The fixture contains an already disabled employee with an old completed run and
interrupted operation, an unactivated disabled identity, and an active control.
The verification requires disabled epoch1, active epoch0, retained old run/op
pins0, two anonymous migration barriers, preserved revision identity and refusal
of a direct re-enable. Use another fresh database whenever the proposal changes.
Do not register a provisional migration65 receipt in the migration ledger.

Use immutable migrations to upgrade an existing64 database. Applying pgschema
to such a database is unsupported and must be refused: its new transition-only
guards would reject backfill DML applied afterward. Fresh desired-schema
bootstraps contain no existing employees. Root owns the upgrade sequencing and
parity reconciliation, including replacing the older management guard/check
bodies.

After applying the reviewed proposal, the focused production lanes are:

```sh
cargo test -p ortak-server --test postgres_authenticated_routes lifecycle -- --include-ignored --test-threads=1
cargo test -p ortak-runtime --test postgres_run_supervision lifecycle -- --include-ignored --test-threads=1
```

They use the explicit disposable URL and real signed routes, repository leases,
activation saga, durable dispatch/output journals and cancellation worker. Shared
test adapters supply deterministic health only. Real resource activation and
native lifecycle acceptance remain separate deployment checks.

The central provisional SQL65 PostgreSQL gate passed all nine server lifecycle cases
(seven management and two Work execution cases) and all five runtime lifecycle
cases (four new epoch scenarios plus the existing lifecycle/binding regression).
The fresh 64 legacy-disabled fixture, backfill and post65 assertions also passed.
The desktop Ortak matrix passed 57 tests, including nine management controls/hooks,
and TypeScript typechecking passed. These results prove the source/provisional
schema behavior; they do not claim migration integration or private native
lifecycle acceptance.
