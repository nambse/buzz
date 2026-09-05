# B2 pinned permission transport

Date: 2026-09-05

Scope: the runtime-independent library handoff from an immutable employee
revision to `RuntimeAdapter::start_run`. This is the transport portion of
`REMAINING_WORK_V1.md` slice B.6.

## Authority and behavior

`PgControlPlane::authorize_dispatch` selects the revision named by the durable
WAKE recipient. `validate_pinned_revision` reads its manifest once, validates
the employee definition, and verifies the corresponding validated runtime
binding row. It returns a sealed `ValidatedRunConfiguration` containing that
manifest's binding and `PermissionPolicy` together.

`DispatchAuthority` privately retains this configuration. Every `RunSpec` it
constructs requires `permissions`, copied from the same pinned revision as
`revision_id` and `binding`. The latest active revision, dispatch lease payload,
runtime options, and client policy cannot supply or replace this policy.
Advancing the active revision after routing or between start and retry leaves
the dispatched policy unchanged. A retry retains the durable run id and stable
idempotency key.

No migration or policy snapshot column is needed: the existing immutable
`employee_revisions.manifest`, recipient revision reference, and run revision
reference already retain the policy's source of truth.

## Structural validation

`PermissionPolicy::validate` is shared by employee definition validation and
`RunSpec::validate`. It preserves the existing rules: workspace and network
lists have at most 64 entries; references are nonblank, control-free, and at
most 1,024 bytes; tool and approval identifiers use the closed domain enums
and cannot repeat. Tool and approval lists also have explicit 64-entry caps.
Empty policies remain structurally valid, and duplicate workspace/network
references remain accepted as before.

A malformed stored policy cannot produce a dispatch authority. The supervisor
records the existing bounded `ManifestUnreadable` refusal through the outbox
retry mechanism before creating a run or calling the runtime. Invalid policies
on a directly constructed `RunSpec` return `RuntimeError::InvalidSpec` with a
fixed message that does not echo policy values.

## Limits

This change transports and structurally validates policy only. Hermes tool
boundary enforcement does not exist in this slice. There is no real Hermes
adapter, tool authorization decision, permission event, human approval, or
pause/resume implementation added here. The fake runtime records received
specifications and validates their structure; it does not prove tool access
enforcement.

B1b mutable Office authority fencing, live employee activation, and composition
remain separate gates. Central routing is not enabled. The existing Coolify
Hermes/Buzz/Honcho test stack and external Cem/Zeynep resources are untouched;
verification uses only the fake runtime and an explicitly selected disposable
local PostgreSQL database.

## Verification

The focused suite extends the existing grouped fixtures:

- Forged lease permissions cannot change the full policy captured at
  `FakeRuntimeAdapter::start_run`.
- A new active employee revision cannot replace the routed revision's policy.
- Lost start acknowledgement followed by a revision advance and supervised
  retry produces identical captured `RunSpec` values and one runtime run.
- Control-containing network references and duplicate approval requirements
  in stored immutable revisions produce a durable refusal, zero run rows,
  and zero runtime start calls.
- Domain checks cover exact reference size/count boundaries, invalid values,
  and typed uniqueness; the run-spec check exercises the same validation and
  fixed error text.

`FakeRuntimeAdapter::start_specs` captures every start invocation before
validation and before the idempotent receipt lookup, so a refusal or retry
cannot be hidden by the fake's own validation or deduplication.

Verified on 2026-09-05 with the repository's Hermit environment active:

| Command | Result |
| --- | --- |
| `cargo fmt -p ortak-domain -p ortak-control -p ortak-runtime --check` | Passed |
| `cargo clippy -p ortak-domain -p ortak-control -p ortak-runtime --all-targets -- -D warnings` | Passed |
| `cargo test -p ortak-domain -p ortak-control -p ortak-runtime --lib --tests` | 70 passed; 24 PostgreSQL tests intentionally ignored in this invocation |
| `cargo test -p ortak-runtime --test postgres_run_supervision -- --ignored` | 7 passed against the disposable database explicitly selected by `ORTAK_TEST_DATABASE_URL` at `127.0.0.1:55432/ortak` |
| `git diff --check` | Passed |

The initial PostgreSQL run exposed a test expectation missing the existing
`dispatch refused: ` error prefix. The expectation was corrected; the full
seven-test runtime PostgreSQL suite then passed. No production behavior changed
in response to that test failure. Repository-wide `just ci`, other crates'
PostgreSQL suites, desktop checks, and a deployed Hermes run were not performed
for this bounded transport change.
