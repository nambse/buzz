# Explicit runtime connection checks for prepared activation

The production Hermes bridge's OAuth profile health requires a recent real probe
bound to the exact employee, runtime binding, image and OAuth/account generations.
Its 120-second witness is read-only. A prepared activation previously ran the
memory diagnostic but did not establish this runtime witness, so a later native
"Check and re-enable" failed after the operator's earlier probe expired.

The explicit provisioning runner now prepares the selected HermesProfile runtime
before memory preparation and the existing saga. Adopt/Update still acquire an
existing profile with Adopt ownership. Create/delete remain unsupported. Ordinary
profile inspection, credential presence, capability queries, API reads, dry runs
and environment-owned profiles do not run a model diagnostic.

The proposed `provisioning_runtime_probes` journal persists a random probe UUID
before external admission. An operation has at most 20 generations. A fixed
deadline allows at most 90 seconds for the probe; exact cancellation/containment
has a separate 15-second bound. Current company/Office, employee epoch and sealed
management authority are checked before I/O and again while waiting, including
while an HTTP request is pending. Final readiness is committed only under current
authority and before the persisted deadline. Existing management execution remains
outside HTTP handlers, bounded by its 170-second driver and 180-second lease.

Only one `running` probe may exist for a company/employee across all operations.
That state includes an unknown admission acknowledgment, a crashed controller and
pending cleanup. Reconnecting reuses its UUID; a terminal bridge status alone
never frees it. The exact bridge cancellation must acknowledge containment first.
The original canonical origin and credential environment **reference** are pinned
for cleanup after another operation selects a different bridge. Missing original
credentials retain the pending record and forbid a replacement. No credential
value, model response or OAuth file is stored or returned by this journal.

`succeeded` and `failed` rows have a retained `contained_at` receipt and cannot be
changed or deleted. Failure accounting can settle after revocation; it cannot be
turned into readiness. A fresh succeeded receipt may be reused only after another
current health read and an authority/deadline check after that I/O. An interrupted
or expired pending record remains recoverable through the same explicit operation.

Command reads show only probe generation/state; the scoped provisioning detail
also shows fixed timestamps and a closed failure code. This is saved state, not a
claim that a worker is alive. Pending/running/failed operations keep the existing
command retry affordance after lease reconciliation; the per-operation probe limit
disables unsupported further retries. A new prepared configuration can establish
a new operation. Read routes never inspect or start the bridge.

The table is retained company evidence, linked to durable operation/selection
rows and independent of transient Office bindings. G's drain gate must reject
every `running` record regardless of deadline or bridge terminal status. The
bridge child key is `ortak-run:{company_id}:{probe_id}`. A populated approved-purge
test must prove retained journal survival and revoked access; the community-only
table inventory must not silently classify this company-owned table as purgeable.

Central all-target compilation and the real HTTP/PostgreSQL lane passed on a
fresh disposable database initialized through immutable 67 plus reviewed 68.
The receipt is `/private/tmp/ortak-v0-evidence/profile68-test-build-7f00b880ae6547678543996b75bb0166/test-receipt.json`:

- Three real HTTP adapter regressions bind exact profile/UUID/company/auth fields,
  read-only ordinary health, Create refusal, strict terminal status and containment.
- Four production preparation tests use disposable PG plus actual HTTP transport
  to exercise durable-before-admission, process death/reconnect, failed containment,
  stale authority during polling, dry-run exclusion and missing old credentials.
  The populated canonical DeletionStore purge test preserves the exact journal,
  rejects cached reads/admission/readiness, and permits only failed containment
  accounting through the original issued child identity after purge.
- Five signed-management PG tests exercise concurrent admission, CLI exclusion,
  authority revocation versus cleanup, pre-saga retry visibility, and lease expiry
  at the admission commit. A final-generation fresh success remains reusable;
  its expiry closes both the displayed retry affordance and signed admission.
- The full Ortak desktop test matrix passes 73/73 and TypeScript checking passes.

All three HTTP and nine PostgreSQL cases passed. SQL 68's final immutable
integration and schema-parity gates remain root-owned; existing schema-67
artifacts must remain separate until that integration completes.
