# Reviewed employee memory UI

Source-only integration of the signed review and publication routes. No live
grant, migration, runtime namespace, publication target or recall opt-in is added.
The existing signer generates a fresh NIP-98 event for every exact-body retry.

Own-authored plaintext Office messages expose a separate review action. Native
channel and Employee directories provide selection hints; the server preview
checks the source partition, author, current memberships and deployment ceilings.
The source body never enters the draft. Relationship sharing explicitly names
the signed human as its participant. Preview identity, edited text and expiry
must all be reviewed before approval. A changed selector or observation clears
the form/review and stale asynchronous responses cannot restore it.

Employee cards independently expose saved approvals and Stop recovery without a
source message, destination selector, active Employee or approval capability.
Hidden source/content remains hidden. The server's original-approver projection
is the authority; records are paginated in pages of at most 16.

Uncertain commands retain their operation ID and serialized bytes in memory
across dialog close/reopen. Retry uses remaining employee access even if the
approval capability or source was lost. Account/origin/source changes and
authorization denial clear private state; no plaintext is stored in browser
storage. The server receipt remains the durable recovery record if the component
or app exits. Stop records revocation, not physical erasure.

Saved approvals expose separate publication consent and bounded metadata reads.
The server selects the current owned target; the client submits only the exact
fact path, operation ID and expected version. A successful command queues work;
only a confirmed receipt displays publication or removal acknowledgment.
Neither means a run used the record. Destination opt-in and current authority
remain server checks; actual selected use is visible through existing Activity.
Stopped, expired or hidden-source facts cannot start publication. Original-approver
metadata and failed-removal retries remain reachable without approval capability
or source text. All mutations share one pending request, including across
close/reopen; no competing publication and Stop commands are issued.

Focused sources (root executes; no execution claimed here):

```sh
cd desktop
node --import ./test-loader.mjs --experimental-strip-types --test src/features/ortak/employeeMemory/employeeMemory.test.mjs src/features/ortak/employeeMemory/menu.test.mjs src/features/ortak/employeeMemory/export.test.mjs
```

These exercise the production React forms/hooks, signed client, bounds, stale
reads, source-hidden recovery, exact uncertain replay, cursor handling and actual
message-menu visibility. HTTP/native responses are controlled fixtures; they do
not claim a deployed capability, PostgreSQL authority or native acceptance.
