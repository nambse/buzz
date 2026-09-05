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
refresh removes action controls. Revocation clears private projections and
aborts pending network work; recovered access can reconcile the same uncertain
operation. Project sharing, archives, artifacts, source promotion and runtime
execution controls are outside this manual UI.

`work.test.mjs` binds the actual signed client, mutation/read hooks and forms.
The isolated `../smoke/work.spec.mjs` uses the production screen and native mock
signing bridge, proving tab-switch retry identity, review/completion and access
loss. It uses fixture HTTP responses, not live product services.
