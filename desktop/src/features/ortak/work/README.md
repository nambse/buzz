# Manual Work UI

The Projects & Work tab uses the signed E1 API. Project creation chooses a named
currently authorized Office channel. Contribution and review controls follow the
server's separate capability fields; assignment uses only active employees in
the current directory page. Every status is saved manual state. None of these
controls starts a runtime.

One outstanding write holds its exact serialized body and operation UUID. An
uncertain result exposes an explicit same-operation retry with fresh NIP-98
authentication. No mutation retries automatically. A 409 clears the stale attempt
and refreshes current authority/state before another action. The tab stays
mounted across Employees/Activity selection so its retry is retained. Changing
company/origin or signing identity remounts the private surface and clears its
in-memory data; this is not a cross-restart operation journal.

Reads are selection-fenced, abort on change, poll every 5 seconds, and stop after
five consecutive transport failures or immediately after 401/403/404. A failed
refresh pauses every write, including direct form submission and uncertain-write
retry. Same-scope refreshes and transient errors retain mounted forms and their
unsaved entries, with the last successful read explicitly labelled while stale.
Routine polling keeps the last successful authority while its request is pending,
so timer ticks do not disable a focused field; explicit refreshes and failed
reads pause writes immediately. Each mutation still checks current authority at
the server.
Origin/client, project, item, or pagination changes clear that state immediately.
Revocation clears private projections and
aborts pending network work; recovered access can reconcile the same uncertain
operation. Project sharing, archives, artifacts and runtime
execution controls are outside this manual UI.

## Office message promotion

A delivered canonical text message's More menu offers **Promote to Work** when
its relay has an explicit Ortak API binding. The dialog reads the current signed
routing projection and one bounded page of projects; it offers only active
projects on that channel for which the server reports contribution permission.
A message without a recorded decision remains unavailable for promotion. The
person supplies a work definition, criteria and optional required review; the
source reference is the message ID, with author/channel/decision provenance
resolved again by the existing server endpoint.

Every submit and explicit retry rereads source and selected project authority
before the signed promotion POST. An uncertain response retains its exact body
and operation UUID above the dialog content, including close/reopen while the
message view remains mounted. No write retries automatically. Scope/client
changes and 401/403/404 clear private drafts and requests; transient refresh
errors keep drafts mounted but pause writes. This is in-memory recovery, not an
application-restart journal. Removing the message view ends that local lifetime;
durable server promotion uniqueness still prevents a second Work item for the
same source message. Conflicts instruct the person to inspect existing Work.

The confirmed result opens the exact project/item in Projects & Work, using
validated UUID navigation and fresh authorized detail reads. Promotion itself
does not assign an employee, start a run, approve work or mark it completed.

`promotion.test.mjs` binds the production React hook/form and real signed client
to frozen-body retries, fresh authority, pagination, stale reads and private
scope changes. `promotionMenu.test.mjs` binds the production message action bar
and Radix keyboard/pointer menu-to-dialog focus handoff. These use synthetic HTTP
and native ports; a later root-built native interaction is a separate acceptance
gate.

`work.test.mjs` binds the actual signed client, mutation/read hooks and forms.
`refresh.test.mjs` binds the production Work screen to held reads, bounded failure
recovery, direct stale submissions, and every private scope/revocation reset.
The isolated `../smoke/work.spec.mjs` uses the production screen and native mock
signing bridge, proving tab-switch retry identity, review/completion and access
loss. It uses fixture HTTP responses, not live product services.

## Employee assignment queue

Employee cards open a read-only assigned-work panel. It fetches the authenticated
employee queue endpoint with a fixed 25-item limit and displays one page at a
time. All returned assignment roles are shown; inactive employees can inspect
outstanding manual work. Empty results are distinct from unavailable or revoked
access. Refresh starts from the first page, and polling failures stop after five
attempts. Employee/origin changes abort reads and reject late results; current
queue access is rechecked while polling. Closing returns focus to the employee
card. There are no start, assignment-edit, or task-navigation actions here, so
this panel cannot discard the separate Work tab's uncertain-operation retry.

`employeeWork.test.mjs` tests the actual signed fetch, read hook and panel.
`../smoke/employee-work.spec.mjs` exercises the directory entry point, pagination,
employee selection, access clearing and keyboard focus with fixture HTTP only.


## Definition editor

The detail view edits title, description and acceptance criteria in one save.
Existing criterion IDs/order are retained, and new pending criteria are appended.
Work under review, terminal work, resolved criteria/approvals, archived projects
and insufficient project roles show the reason editing is unavailable. Current
version changes reset an open draft; uncertain writes retain their exact pending
operation and explicit retry in the Work screen.

Unchanged fields send null to preserve canonical server text because displayed
text may be redacted. Changed fields use the existing UTF-8 byte bounds; the
manual editor caps the full criteria list at16. `work.test.mjs` covers the actual
screen/hook edit and retry, read-only reasons, byte bounds and safe-projection
preservation. This UI requires the definition API and integrated SQL62 before
release; it does not add runtime execution or artifacts.
