# Prepared employee management — F2a/F2b

This slice connects Employees to the real prepared-resource runner through
audited, durable command admission. It supports Adopt, Update, bounded Retry and
DB-only adopted compensation. It does not create external resources. The
additive F2c lifecycle implementation and its separate migration/acceptance gates
are documented in [EMPLOYEE_LIFECYCLE_F2C.md](EMPLOYEE_LIFECYCLE_F2C.md).
Remaining Work F also requires the separate fresh resource preparation flow.

## Deployment and ownership

The append-only catalog, drafts, commands and audit require the root-integrated
management migration. The proposal is
`proposals/0064_employee_management.sql`; do not apply it from an API request.
The API process owns current execution policy. At startup it atomically projects
the validated `ApiConfig` human grants into PostgreSQL. Existing F1
`can_manage_employees` grants only read access. Command admission additionally
requires `can_execute_provisioning:true` and `role:"operator"`, explicit employee
IDs and current Office membership. Both capabilities default to false.

Worker processes consume current DB policy. They do not import their own older
copy of API grants. Every command retains the actor, signed auth-event ID,
original grant snapshot and fingerprint, exact request fingerprint and immutable
prepared selection. Current policy replacement or live membership revocation
fences later writes and activation; an old grant snapshot is attribution, not
continuing authority.

The new `ortak-management` executable is default-off. It requires
`ORTAK_MANAGEMENT_ENABLED=true` and the same explicit isolated
`ORTAK_DATABASE_URL` used by the product control plane. Its two actions are:

* `ORTAK_MANAGEMENT_ACTION=import_catalog`: read the operator-selected
  `ORTAK_PREPARED_CATALOG_JSON` and import it atomically. This performs only
  structural validation and DB persistence. It never reads credential values
  or contacts adapters.
* `ORTAK_MANAGEMENT_ACTION=work`: use `ORTAK_MANAGEMENT_COMMUNITY_ID` to resolve
  the company and process one due command at a time. The process uses the same
  registered signal/shutdown seam as the existing private workers.

Catalog JSON contains `community_id` and up to 64 `entries`. Each entry has a
fresh immutable UUID `id`, a public `label`, and `configuration`: the complete
existing `ProvisioningConfig` for that one prepared employee. It contains only
opaque credential references and environment-variable selections, never OAuth
or signer values. The typed manifest, including defaults, is canonicalized at
import. Reusing an ID with changed contents fails. Omitted choices are retired;
already admitted commands retain their original configuration independently.

The actual Hermes profile, model/thinking tuple, selected credential references,
Honcho ownership receipt, Office identity/membership and worker allowlists must
already be prepared. Hermes currently matches the complete registered binding;
editing just a model string does not prepare a new profile. Offer distinct exact
catalog entries for the supported model/thinking choices. Import success does
not claim runtime health. Only the actual activation probes can establish that.

## Product contract

| Route | Effect |
|---|---|
| `GET /api/v1/employee-preparations` | Current authorized public catalog projection; explicit `create_supported:false`; F2c exposes lifecycle capability only with its server implementation. |
| `POST /api/v1/employees/{id}/configuration-drafts` | Freeze `draft_id`, `catalog_id`, `expected_revision_id` and the selected complete configuration. A draft has no external side effects. |
| `POST /api/v1/employees/{id}/management-commands` | Admit `idempotency_key`, `action`, `draft_id` or `operation_id`, and `expected_revision_id`. Return 202 only after command and actor audit commit. |
| `GET /api/v1/employees/{id}/management-commands` | At most 25 recent commands, durable status/attempt count, safe error code and the linked F1 operation. |

All routes use current company/employee authority and signed NIP98 requests.
POST bodies remain bounded to 4 KiB and reject unknown fields. They never accept
raw manifests, origins, paths, environment names, native receipts or secret
values. Catalog/draft/command responses explicitly project public fields; they
never serialize the stored configuration or raw adapter errors.

The first successful command admission may have no operation ID yet. The API
does not call `begin_operation`, reserve an Employee, construct adapters, or run
the 180 second CLI inline. These would also conflict with the API's outer Office
read fence. The executor starts outside that request transaction.

Lost acknowledgement replays the same request key and exact contents. Different
contents return 409. One pending/running command per employee prevents overlapping
admissions. An Update preserves the current active revision until fresh checks
and final activation commit a new immutable revision. A stale expected revision
refuses admission or subsequent execution. Employee identity remains independent
of runtime/model changes.

## Executor and recovery

A worker leases one command for 180 seconds and bounds the entire attempt to 170
seconds. At most three interrupted attempts run automatically, with 5/10 second
backoff. A failed saga step is a persisted failure requiring an explicit retry;
the saga's own three-attempt step budget is never reset. Pending/running command
rows and their operation selection survive process exit and SIGINT/SIGTERM.

The existing per-employee session lock excludes overlapping operator runners.
A repository instance sealed to the command and lease checks current policy and
Office authority before adapter construction, before steps, in every provisioning
write transaction and at deferred commit. No SQL transaction spans adapter I/O.
Replaced/expired leases cannot write. A normal CLI repository cannot replay a
managed operation without its delegated execution authority.

The runner's existing durable configuration fingerprint and frozen Office
profile bytes still govern replay. A crash after creating the operation but
before acknowledging the command recovers that exact operation by its immutable
key. A committed activation may be reconciled as complete even if the original
actor's access has since been revoked; reconciliation performs no new external
action.

Retry uses the original managed operation's retained full selection, even after
catalog retirement. Earlier CLI operations that retained only a fingerprint do
not automatically gain a fabricated retry selection. Compensation calls the
existing `compensate_adopted` repository-only helper; missing credentials or a
retired catalog cannot force an environment lookup. Activated operations,
created resources and ambiguous ownership receipts fail closed. Adopted resources
are retained, never deleted.

The UI explicitly distinguishes saved drafts, queued commands and persisted
activation results. Reads poll every five seconds; this is not a realtime stream.
Five failed reads stop automatic retry. Refresh and Close remain available.
Interrupted mutation retries preserve their exact body/key. Current authority
errors clear private state and fence overlapping late reads and mutations.

## Required follow-up and validation

F2c adds a durable lifecycle barrier shared by Office and Work execution. Its
separate SQL65 proposal and tests are described in the linked lifecycle contract.
Re-enable consumes a repository-issued intent bound to the disabled epoch,
expected revision, current actor and fresh health probes. Ordinary Update and
the CLI continue refusing Disabled employees. A compiled control does not imply
that SQL65 or its lifecycle behavior has been deployed.

Focused tests use the actual signed router, management executor, prepared runner,
repository guards and a disposable PostgreSQL database at 55432. They exercise
lost/concurrent acknowledgements, immutable selections, no-inline-run behavior,
Update CAS, current role/channel revocation, lease replacement and deferred
expiry, adopted compensation without credentials, and owned-worker SIGTERM
followed by replay. Positive provider-backed activation remains a deployment
acceptance gate, not a claim made by the missing-credential fixtures.

```sh
cargo test -p ortak-server --test postgres_authenticated_routes management -- --include-ignored --test-threads=1
cd desktop
node --import ./test-loader.mjs --experimental-strip-types --test src/features/ortak/ManagementPanel.test.mjs
pnpm typecheck
```

Use only the explicitly selected disposable DB URL for these PostgreSQL tests.
Root owns central builds, migration integration and live deployment acceptance.
