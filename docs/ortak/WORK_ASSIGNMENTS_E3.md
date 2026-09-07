# Work assignment release and reassignment (E3)

Architecture §7 and Remaining Work E require assignment release/reassignment.
The domain and migration 0047 already retained released assignments, but the
manual API exposed only assignment creation/reactivation. An active assignment
could neither be released nor have its role corrected through the product.

This slice adds two signed NIP-98 POST operations:

| Path under `/api/v1/work-items/{item}/assignments/{employee}` | Body |
| --- | --- |
| `/release` | `operation_id`, `expected_version`, `reason` |
| `/reassign` | `operation_id`, `expected_version`, `replacement_employee_id`, `role`, `reason` |

Bodies reject unknown fields. Reasons are nonempty, bounded to 1,024 bytes and
refuse secret-like text. Roles remain `owner`, `contributor`, or `reviewer`;
employee assignment roles do not grant human approval permission.

The authenticated human needs current Office/channel authority, the server's
operator grant, and project Contributor or Owner permission. The old employee
must belong to the configured audience and hold an active assignment. Releasing
that assignment remains possible when the employee is inactive or has left the
channel. Reassignment additionally requires a currently Active, channel-authorized
replacement in the configured audience. A same-employee reassignment changes its
role; an unchanged role or an already-active different target is a conflict.

One action takes the existing Office shared fence, project share lock and item
update lock, compares the observed version, updates retained assignment rows,
increments the work version once, appends one history event, and records one
immutable API receipt in the same transaction. Receipt-storage failure rolls all
changes back. Exact retries recheck current authority and return current saved
state without another transition. Reassignment replay still requires replacement
eligibility. Released rows are reused by a later ordinary assignment, never deleted.

Existing Work generation signals invalidate old dispatch/admission witnesses.
The runtime's bounded reconciliation stops active runs through its durable
`work_revoked` cancellation journal. Held preparation and late terminal output
cannot use the old work version even if the same employee still has an active
assignment. Assignment changes preserve work status, acceptance criteria,
approval decisions and existing artifacts. They never mark work complete.
Terminal items and archived projects remain immutable.

The Work detail panel exposes these commands only for the current contributing
role and mutable item. Release remains available without a populated employee
directory page. Replacement choices come from the current Active directory page;
the server rechecks actual eligibility. Interrupted confirmations use the shared
Work mutation hook's explicit retry with identical body and operation ID.

No new SQL migration is required: existing assignment status/released timestamp,
history event format, atomic API receipt and generation triggers provide the
storage contract. Migrations 0063–0066 remain unchanged by E3.

Regression seams:

- Domain `work::assignment::tests`: atomic role/replacement/release, retained
  review, duplicate/invalid/terminal/overflow refusal without partial mutation.
- Server `work::assignments`: signed inactive release recovery, current role,
  exact replay, replacement eligibility, storage-failure rollback and concurrent
  idempotent reassignment.
- Server `work::execution::assignments`: held preparation refusal, active runtime
  cancellation, and late terminal artifact refusal without changing human gates.
- Desktop `work/assignments.test.mjs`: actual controls, one versioned command,
  eligible choices, authority/terminal/disabled states and explicit exact retry.

The complete Ortak desktop test matrix passes (68 tests), as does TypeScript
checking. The central compiled snapshot passed all five production PostgreSQL
regressions in 2.72 seconds and both domain tests. Its immutable-schema-66 lane
also reran all six reviewed-memory tests, including populated approved purge.
Root evidence: `schema66-e3-test-binaries-4afa8a8b6b13491a94e5e6da03196027/pg-receipt.json`.
This is source/PG evidence; private backend/native rollout acceptance is tracked
separately by the deployment owner.

Remaining E editing gaps: dependency removal and parent/child decomposition are
still unavailable. Dependency addition exists in the core but has no complete
manual API/UI editor; parent/child work linkage has no domain/storage model yet.
Neither capability is implied by this assignment panel.
