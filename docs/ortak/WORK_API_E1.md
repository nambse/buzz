# Manual Work API E1

Status (2026-09-05): manual E1 implemented and verified through actual
PostgreSQL, signed HTTP, and headless desktop tests. The native package is built
and its private app start is verified; root TCP verification and the actual live
manual Work workflow remain pending. No native visual/OS interaction is claimed. Runtime
dispatch, artifact access, project sharing controls and realtime delivery remain
later integration gates.

## Authority

Every request uses the existing server-derived NIP-98 principal, company binding,
Office membership fence and replay guard. Project creation additionally requires
global `operator` and server configuration `can_create_projects: true`, which
defaults to false. Creation atomically binds a new project to one explicit Office
channel and installs its creator as its durable owner. Existing projects receive
no automatic API access, and global operator does not bypass project grants.

A project role and current membership in its configured channel are both
required, including for public channels. Viewer reads; contributor creates,
assigns and changes ordinary status; reviewer resolves criteria, approvals and
review outcomes; owner has all currently exposed manual operations. Global
reader remains read-only regardless of project role. Assigning an employee the
reviewer assignment role grants no human review permission.

Canonical source promotion requires a decided Office inbox row matching the
stored event's community, partition, author, kind and channel. The source channel
must equal the project's immutable binding. Deletion or loss of that source
audience removes its Work item's visibility. Assignment requires a currently
active employee in the caller's configured employee cohort and valid Office
membership for this project channel. Assignment and manual `in_progress` create
no runtime start or dispatch outbox.

## Requests

All mutation bodies include a non-nil `operation_id` UUID. A retry sends the same
body with a fresh NIP-98 event. Each successful operation commits a durable
principal-attributed receipt, the domain change and its history atomically.
Replays reauthorize the current resource and return its current safe projection;
they do not apply the mutation twice. Payload conflicts and stale versions return
a generic409 without other resource IDs. Actor, company, grants, runtime state
and credential material are never accepted from a body.

| Endpoint | Body or result |
| --- | --- |
| `GET /api/v1/projects` | `projects`, optional `next_cursor`, `can_create_projects`, and authorized named `create_channels: [{id,name}]`; limit1–25. |
| `POST /api/v1/projects` | `operation_id`, `channel_id`, nested `project: {slug,name,description}`. |
| `GET /api/v1/projects/{project_id}` | Safe project detail, current role, `can_contribute`, and `can_review`. |
| `GET /api/v1/projects/{project_id}/work-items` | `work_items`, optional `next_cursor`; limit1–25 and optional `state`. |
| `POST /api/v1/projects/{project_id}/work-items` | Manual work creation. |
| `POST /api/v1/projects/{project_id}/promotions` | Work creation with required `source_message_id`. |
| `GET /api/v1/work-items/{item_id}` | Safe work detail, manual status, criteria, approvals and bounded history. |
| `POST /api/v1/work-items/{item_id}/assignments` | `operation_id`, `expected_version`, `employee_id`, assignment `role`. |
| `POST /api/v1/work-items/{item_id}/transitions` | `operation_id`, `expected_version`, `target`, optional `reason`. Completion and review rejection require reviewer authority. |
| `POST /api/v1/work-items/{item_id}/criteria/{id}/satisfy` | `operation_id`, `expected_version`. |
| `POST /api/v1/work-items/{item_id}/approvals/{id}/resolve` | `operation_id`, `expected_version`, `decision` (`approve`/`reject`), optional `reason`. |

Work creation accepts `title`, optional `description`, `priority`, up to16
criteria and8 approval gate definitions, besides `operation_id`. Promotion also
requires its source ID; manual creation refuses it. The project ID comes only
from the path. Unknown body/query fields are rejected. Work routes accept at
most16KiB of signed body bytes; existing Activity routes retain4KiB.

## Projections and limits

Responses contain explicit product fields, never raw aggregates. Run/decision
IDs, attachments and labels, dependency targets, raw attachment history and
private employee-cohort identities are omitted. Text receives pattern redaction
and control-character removal. Human attribution exposes a valid public key;
other historical actor IDs are omitted unless explicitly in the employee cohort.
Work history reports typed action names and transitions with attribution, not
raw event payloads. Omitted and truncated history are explicitly flagged.

One work detail returns at most the foundation's500 history entries and has a
256KiB serialized response bound. Full history pagination is not implemented.
List queries authorize before LIMIT. Work cursors include the project ID and
cannot be reused with a different project's URL. All responses remain `no-store`.

Unknown, foreign and ungranted resources return404; insufficient action privilege
on a visible resource returns403. Malformed inputs return400/422, oversized bodies
413, state/version/idempotency conflicts409, and corrupt/unavailable state503.
No endpoint here enables model execution, grants artifact access, archives a
project or edits project sharing. Those actions remain absent until their full
authority and recovery contracts are integrated.


## Verified integration evidence

The 2026-09-05 candidate passed 14 Work core PostgreSQL cases in 4.50 seconds
and 10 signed API PostgreSQL cases in 5.53 seconds (including seven Work API
cases). These exercise actual persisted authority, operation receipts, manual
transitions and failure paths against disposable local databases.

The desktop has six focused Work unit tests passing, alongside the existing
signed-client and Activity tests. TypeScript, scoped Biome, and the isolated
mock-mode build pass. The full three-case headless Ortak smoke passed in
16.4 seconds; the final Work-only screenshot run passed in 2.9 seconds.
That Work flow verifies an uncertain write retried with the exact same body and
operation UUID after a tab switch, reviewer criteria/approval completion, saved
manual status, and removal of private content after authorization loss. Native
signing uses the existing mock bridge in these UI tests; fixture HTTP responses
are not evidence of a live product deployment.

The Projects & Work tab offers named-channel project creation, project and work
lists/details, manual work creation, status changes, review gates and assignments
from the visible employee directory page. It displays saved manual state and
never claims that assignment or `in_progress` starts employee execution. Writes
retry only on explicit action; a 409 refreshes current state and permissions.
Pending operations remain in memory across tab changes, not across a private
screen/origin or signing-identity reset. Source promotion is exposed in the API;
a conversation-promotion desktop control is still outside this UI slice.


The final headless fixture captures (`projects.png`, `work-review.png`, and
`work-completed.png`) were inspected after animation completion. All three have
distinct SHA-256 hashes; the 923×224 project view and 441×891/441×435 review and
completed panels have readable labels and complete, unclipped content. This is
mock-bridge visual evidence, not native OS interaction.

Deployment evidence recorded after those tests: the final native Rust package
build passed in 43.53 seconds with the production frontend/protected-mode matrix
also passing, and the private app was restarted. The fresh private database has
migrations 0053/0054 applied, and the refreshed API passes its seven original
network checks. Root TCP verification of the restarted app and an actual live
manual Work workflow are separate pending checks; these receipts do not mark
runtime dispatch, artifacts, or sharing as implemented.
