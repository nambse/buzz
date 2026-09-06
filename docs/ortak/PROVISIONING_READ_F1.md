# Provisioning progress — F1

This is a read-only view of the real production saga's persisted operations and
ten ordered steps. It is a partial delivery of Remaining Work F. Preparation,
create/adopt/update execution, retry, compensation, disable and re-enable remain
operator actions; this API does not run them or satisfy the full dashboard gate.

## Authorization and transport

A deployment operator may explicitly add `can_manage_employees: true` to an
existing `operator` human grant in the product API configuration. The default is
false; a `reader` grant with the flag is invalid. Existing cancellation privilege
does not imply provisioning access. Employee IDs remain an explicit allowlist;
current company/community and live non-automated Office identity are verified by
the same signed NIP-98 middleware and authority transaction as other product
reads. Config changes require API restart.

`GET /api/v1/employees` includes `can_view_provisioning`. The Employees panel
renders its provisioning entry only when that server capability is true.
Every provisioning endpoint checks management authority again. Browser state
cannot grant it. Access denial is audited using the existing `read_employee`
action and `denied`/`not_found` outcomes; foreign or ungranted records return404.

| Endpoint | Projection |
| --- | --- |
| `GET /api/v1/employees/{id}/provisioning?limit=25&cursor=...` | `employee_id`, up to25 operations newest first, `next_cursor`, `has_more`, `read_only:true`. The opaque exclusive cursor binds creation time and operation UUID. |
| `GET /api/v1/employees/{id}/provisioning/{operation_id}` | `operation`, exactly10 ordered `steps`, `read_only:true`. Header and steps share one read-only PostgreSQL snapshot. |

Operation summaries contain only identity, mode, dry-run flag, status, current
step, committed revision ID and timestamps. Steps expose only typed name/state,
attempt count, adopted-resource protection and start/finish timestamps. Queries
never select or decode manifests, adapter receipt JSON, idempotency keys or raw
error messages. They perform no credential lookup, network probe or runner call.
Malformed private configuration cannot turn an authorized progress read into an
external action. No mutation method is offered on these routes.

## Desktop behavior

The panel labels all progress as **last saved state**. A saved `running` row is
not proof that its process still runs; an old successful health step is not a
current health probe. A dry run is distinct from activation. Failed steps and
adopted-resource protection remain visible without exposing internal errors.

The panel reads one page every 5 seconds while open; this is bounded polling,
not a realtime execution transport. Failures stop after five attempts with
exponential backoff, and Refresh progress remains available. Scope changes abort
and fence old results; authorization failure clears displayed operation data.
Closing the steps restores keyboard focus after the current list is loaded.

## Validation and next boundary

Signed production-router PostgreSQL tests use only explicit disposable 55432.
They cover default-denied management, reader refusal, foreign/ungranted/missing
employees, current deactivation, durable denial audits, exact step projection,
private-data canaries, exclusive paging and refusal of POST execution. Desktop
tests bind the production client, hook, panel and Employees capability gate,
including stale-result rejection, retry exhaustion and focus restoration.

The separate [F2a/F2b management slice](PROVISIONING_MANAGEMENT_F2.md) adds
actor-attributed command admission and an opt-in executor around exact prepared
resource choices. This F1 projection remains read-only. Create/delete and lifecycle
buttons remain unavailable until their real preparation and sealed lifecycle
ports exist. A 15 second API handler never calls the 180 second provisioning
runner under its Office authority transaction.

Focused commands (activate Hermit first):

```sh
ORTAK_TEST_DATABASE_URL=postgres://ortak:ortak@127.0.0.1:55432/ortak_f1_disposable \
  cargo test -p ortak-server --test postgres_authenticated_routes provisioning -- --include-ignored
cd desktop
node --import ./test-loader.mjs --experimental-strip-types --test src/features/ortak/ProvisioningPanel.test.mjs
pnpm typecheck
```

The `postgres_` integration-bin prefix is intentional: the repository CI lane
discovers PostgreSQL cases under that name. Source modules remain in the
`tests/authenticated_routes/` directory.
