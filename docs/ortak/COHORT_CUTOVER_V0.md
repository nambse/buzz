# Central routing cohort and bounded Office reconciliation

Central routing requires an explicit server-owned company selection. The relay's
`ORTAK_CENTRAL_ROUTING_ENABLED` deployment switch alone does not admit work. An
absent cohort or an `off` cohort is disabled. A cohort selects at most 64 live
stream channels and 64 employee IDs from its own company/community. Employee
status, active revision, signer identity, membership, permissions and existing
root-chain limits still apply independently.

The production operator entry point is `ortak-cohort` in `ortak-server`. It
requires `ORTAK_COHORT_ENABLED=true`, `ORTAK_DATABASE_URL` and
`ORTAK_COHORT_CONFIG_JSON`. Configuration contains public IDs and selections;
never put credential values in JSON. The command validates its entire bounded
configuration before reading the database selection. It installs shutdown
handlers before work and has a 30-second total deadline. Every invocation makes
one explicit operation and prints bounded JSON. It does not invoke a provider,
start a worker, or alter an employee manifest.

## Capture before scanning

Deploy the new atomic ingress hook on **every relay accepting events for the
selected community**, with its central-routing switch enabled, before beginning
capture. Drain any old ingress process or acceptance path that can bypass the
hook. The operator declaration below records this required ordering; the
database cannot prove a deployed relay artifact. Existing independent employee
subscribers must remain disabled for the selected cohort.

Example public configuration, with operator-selected UUIDs substituted:

```json
{
  "community_id": "<selected-community-uuid>",
  "action": {
    "kind": "capture",
    "relay_capture_hook_installed": true,
    "channel_ids": ["<selected-channel-uuid>"],
    "employee_ids": ["<selected-employee-id>"]
  }
}
```

Invoke the already built binary with those explicit environment selections. A
capture operation atomically replaces the selected lists, generates a new
`capture_id` and pauses inbox claims. Its result includes the company resolved
from the community, both selected lists and the capture identity. New accepted
events in selected channels commit their canonical signed event and inbox row
in the same transaction. Outside-channel events remain stored without inbox
insertion. There is no employee runtime subscription involved.

Capture starts a new generation; it is not the recovery command. After a lost
capture acknowledgement or interruption, inspect current status:

```json
{"community_id":"<selected-community-uuid>","action":{"kind":"status"}}
```

Use the returned `capture_id` to resume. Do not restart capture merely to obtain
its ID. A status result with `cohort: null` means routing is disabled.

## Reconcile one finite page

For each selected channel, issue:

```json
{
  "community_id": "<selected-community-uuid>",
  "action": {
    "kind": "reconcile",
    "capture_id": "<retained-capture-uuid>",
    "channel_id": "<selected-channel-uuid>",
    "limit": 256
  }
}
```

The first invocation pins the maximum canonical `(created_at, event_id)` key
visible in that channel, including previously accepted events with future signed
timestamps. It then scans at most `limit` rows; the allowed range is 1–256 and
the default is 256. Subsequent invocations preserve that original upper key and
resume its durable cursor. `scanned` counts canonical rows examined, `inserted`
counts newly added inbox rows, and `completed` is the committed completion
receipt. A completed retry returns the same progress. Each call performs one
page; the command contains no automatic repeat loop.

Cursor progress and idempotent inbox insertion share a transaction. A conflicting
existing inbox row or any failed invariant rolls the page back, preserving the
previous cursor for inspection and retry. New backdated events whose key precedes
the cursor are protected by the already deployed atomic ingress hook. There is
no cutoff based on a client timestamp, local wall clock or transaction-start
`received_at` time.

Historical scanning covers live channel text kinds 9 and 40002. Deleted events,
other channels and other kinds are excluded. Newly accepted gift wraps retain
the existing explicit unsupported-DM audit path; this command does not backfill
historical DM ciphertext or make it executable.

## Enable or revoke

After every selected channel returns `completed: true`, enable the exact capture:

```json
{"community_id":"<selected-community-uuid>","action":{"kind":"enable","capture_id":"<retained-capture-uuid>"}}
```

The database permits this transition only from the current capture with completed
receipts for every selected channel and a nonempty employee selection. Receipt
scope, upper key and start time are immutable. Progress is monotonic, bounded and
checked against canonical inbox facts. Completed receipts cannot be changed or
deleted; an empty or premature completion cannot be stamped by direct SQL.

Changing either selection invalidates the current capture identity and pauses
dispatch. Explicit disable is:

```json
{"community_id":"<selected-community-uuid>","action":{"kind":"disable"}}
```

Selection and lifecycle changes advance the existing Office authority generation.
Routing commit refreshes the employee intersection; canonical normalization checks
the current channel selection. Runtime admission and Office delivery revalidate
through that same normalizer under the existing authority fence. Removing an
employee prevents a queued or previously authorized run from starting and prevents
an already frozen Office output from being published. Existing inbox rows,
decisions, run records and capture receipts remain available for inspection.

An interruption does not imply rollback of an acknowledged database commit. Read
status and resume the retained capture/channel; never mint a replacement capture
to conceal an uncertain page result. Enable/disable and completed-page retries
are idempotent. No command claims that a failed capture or incomplete scan enabled
the cohort.

## Production seam validation

Disposable PostgreSQL tests cover default-off capture, future and late-backdated
events, duplicate reconciliation, durable restart, false receipt rejection,
page rollback, cohort removal, current target eligibility, runtime admission and
frozen Office delivery. Existing low-level inbox fixture helpers retain their
explicit bypass for library contract tests; production relay ingress uses
`insert_selected_accepted_event_on`.

The schema proposal is `COHORT_SCHEMA_59.sql`; migration59 and desired-schema
integration are maintained by the main schema owner. This implementation work
does not select or mutate any private deployment cohort. A deployment claim
requires the selected relay artifact, actual capture/scan/enable receipts and
the runtime evidence from that isolated stack.
