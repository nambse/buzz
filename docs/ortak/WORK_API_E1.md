# Manual Work API E1

Status (2026-09-05): manual E1 implemented and verified through actual
PostgreSQL, signed HTTP, headless desktop tests, and the actual live private-stack
manual Work workflow, including an idempotent replay. The native package is built
and its private app start and loopback relay TCP connection are verified. No native visual/OS interaction is claimed. Runtime dispatch,
artifact access, project sharing controls and realtime delivery remain later
integration gates.

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
network checks. Root TCP verification of the restarted app remains a separate
pending check; these receipts do not mark runtime dispatch, artifacts, or sharing
as implemented.

The actual live Work script then passed its first execution and its repeat
against the fresh private stack. The first execution exercised its negative
checks and advanced the work item from version 1 to 7. Replay retained version 7,
with one project, one work item and eight operation receipts. The tested scope
retained zero runs, outbox entries and routing decisions, and SQL confirmed Ada
remained draft. This is live evidence for manual persistence and idempotency,
with no employee execution claim. A subsequent database-only backup through
migration 0054 restored into a fresh verification database and matched all 103
tables; it does not establish a full-stack backup gate.


## Employee manual assignment queue

`GET /api/v1/employees/{employee_id}/work-items?limit=25&cursor=...` returns
`employee_id`, `work_items`, `next_cursor`, and `execution_available: false`.
Each entry contains the same safe Work summary as a project list plus
`assignment_role` (`owner`, `contributor`, or `reviewer`). The existing assignment
primary key allows one role per employee and work item, so entries are unique.
No project descriptions, history, runtime state or artifacts are expanded.

The fixed query includes every active assignment role in active projects and
excludes completed/cancelled work and released assignments. An inactive, paused
or disabled employee remains inspectable: this read-only queue describes saved
manual assignments and does not test readiness or initiate execution. The target
employee must exist in the current company and the configured employee cohort;
otherwise the response is404. Every returned item additionally requires the
current human's durable project grant, configured channel audience, live channel
membership and canonical source visibility. Inaccessible entries are omitted
before pagination; losing all project/channel access produces an empty page.

Pages contain at most25 entries, ordered by work creation time and UUID descending.
The cursor is bound to company/community, authenticated principal, global role,
configured channel and employee audiences, requested employee, and this fixed
query policy. It is not an authority token; every continuation reauthorizes its
rows. Reusing it with another context returns400. Selected project, work and
assignment rows are locked and rechecked, including the lookahead. A concurrent
change detected after selection refuses the page (404 for lost project access,
409 for changed eligibility), so the client must refresh from the first page.
The existing5-second operation,500ms lock,2-second SQL and256KiB response bounds
apply. This endpoint accepts no state filter, write body or dispatch option.

All five core queue PostgreSQL tests passed within the full19-case Work suite
(6.55s); both signed queue tests passed within the full12-case API suite (5.93s).
The four-case headless suite passed in17.0s and the assigned-work screenshot was
visually checked. The rebuilt private API also returned the expected empty Ada
queue with `execution_available:false`, rejected a non-cohort employee, and
passed all nine live checks. Ada remains draft and has no assignments.

## Community detach and final private checkpoint

Migration0055 permits project API bindings to detach only in an approved,
leased community deletion. A deferred guard rechecks the real executor at the
final purge commit. Company-owned projects, grants, items, history and operation
receipts are retained. The three new actual deletion regressions passed in2.45s;
the20 existing deletion PostgreSQL cases passed in7.19s. The first scratch
candidate exposed a PL/pgSQL variable ambiguity; the corrected final migration
was tested on a new database, without rewriting an applied checksum.

Full community removal may fail API admission with503 before project lookup;
binding absence in a still-serving Office scope is404. Both refuse access.
Neither retained grants nor replay receipts restore detached API authority.

The final55 schema matched actual pgschema plus twice-run reconciliation on a
fresh database and the production migrator on another fresh database. The
bounded probe receipt is recorded in `runtime/private-stack/SCHEMA_PARITY.md`.
Clippy for buzz-db/ortak-work/ortak-server all targets passed. The private
relay/API/admin were rebuilt; private migrations through55 applied, nine live
API checks passed, and the manual Work replay retained version7. A database-only
backup restored into a new verification database with matching103 tables,
migration checksums and schema. Native queue rebuild passed in43.77s after
clearing only regenerable task artifacts; its production frontend matrix also
passed. Native OS visual interaction remains unverified.


## Manual definition editing (E follow-up)

The definition endpoint is `POST /api/v1/work-items/{item_id}/definition`.
Central verification passed five domain tests, three actual PostgreSQL facade
tests, two signed HTTP tests and the backend build. SQL62 is applied to the
disposable verification databases; nine focused desktop Work tests also pass.
Private deployment and visual/runtime acceptance for this newer editing path
remain separate from the earlier E1 evidence above.

The request contains `operation_id`, `expected_version`, and a nested
`definition` with nullable `title` and `description`, every existing criterion
in its original order as `{id,text}`, and optional `additional_criteria` strings.
A null replacement preserves the canonical current value under the version
lock; an empty description clears it. The desktop sends null for unchanged
safe projections so redacted display text cannot overwrite canonical values
while editing another field. Changed text keeps the existing UTF-8 byte and
secret-like text validation. The API caps the complete criteria list at16 and
keeps the16KiB request/receipt fingerprint limit.

Only a current contributor/owner with existing human operator authority can
edit. The project must be active; the item must be proposed, ready, in_progress
or blocked, with every criterion and approval gate still pending. Review,
terminal and resolved-evidence items explain why definition editing is read-only.
Existing criterion identities/order and all recorded review evidence are retained;
criteria may be amended or appended, never removed. Empty edits are refused.

One accepted edit changes all supplied fields and children, advances one version,
appends one bounded `work.definition_edited` history event, and records its
operation receipt in one transaction. Replays reauthorize current project,
channel and canonical source access, then return current state without applying
the edit again. No runtime, dispatch, delivery or artifact action is initiated.
History records changed field flags, criterion IDs and a canonical previous
creation-definition hash. The first such hash preserves original promotion
retries after definition changes, including legacy company-service retries.
