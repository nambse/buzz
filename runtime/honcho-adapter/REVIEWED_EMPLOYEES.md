# Reviewed employee protocol source candidate

The isolated `reviewed-employee/1` implementation follows
[`EMPLOYEE_REVIEWED_MEMORY_PROTOCOL.md`](../../docs/ortak/EMPLOYEE_REVIEWED_MEMORY_PROTOCOL.md).
This checkpoint is source-only: no test, database initialization, image build or
deployment has been performed by this lane. Root owns those integration gates.
Existing project/conversation and session protocol modules remain unchanged.

The seven POST endpoints reuse native workspace JWT checks and original owned
resource identities. Namespace inspection is read-only. Publication and cleanup
use the two exact stable record keys, an independently checked typed commitment,
and a full canonical request fingerprint. Mutation ACKs never return reviewed
text. Selected reads are limited to the supplied IDs and destination/human;
they establish no current source permission or runtime grant.

Each request is limited to 32 KiB and the complete response to 64 KiB before
commit, under the existing 10-second transaction budget. A namespace retains at
most 1024 distinct reviewed record IDs and 128 diagnostic IDs, counting cleanup
that precedes a write. Validation failures use a fixed error without echoing
reviewed text or provenance. No provider, embedding, queue or periodic probe is
called by the new family.

Explicit initialization adds exactly these seven tables:

| Table | Key after workspace + employee | Retention |
|---|---|---|
| `ortak_employee_reviewed_records` | record ID | Immutable header, approval/source claims and hashes |
| `ortak_employee_reviewed_content` | record ID | Exact edited text; deletion requires matching cleanup |
| `ortak_employee_reviewed_tombstones` | record ID | Immutable cleanup, including before publication |
| `ortak_employee_reviewed_operations` | stable key; unique record + action | Immutable canonical body and typed request hashes |
| `ortak_employee_diagnostics` | operation ID | Immutable revision/lifecycle and synthetic write hashes |
| `ortak_employee_diagnostic_content` | operation ID | Synthetic challenge only until matching cleanup |
| `ortak_employee_diagnostic_tombstones` | operation ID | Immutable cleanup, including before diagnostic write |

All seven refuse TRUNCATE. The five evidence tables refuse UPDATE and DELETE.
Content tables refuse UPDATE and resurrection after tombstone. Deferred guards
require atomic header/receipt/content or tombstone/receipt/absence, with exact
common pins. Native resource rows have no cascading relationship to this
retained evidence. Current recovery/deletion inventories must explicitly review
these tables before selecting a deployed image that includes them; this source
does not update any live recovery selection.

A finite diagnostic has one explicit operation ID. Only its read endpoint can
return the synthetic challenge before cleanup. Cleanup-before-write and delayed
write both preserve absence; a later read reports `erased=true` and no challenge.
The remote service does not mint readiness witnesses. The Rust adapter requires
an actual exact readback and confirmed cleanup before a short-lived witness can
admit initial target registration. Ordinary use does not trigger more diagnostics.
Process restart or completed cleanup requires a new explicit diagnostic operation
to obtain a new witness; historical hashes cannot replace that readback.

Root's focused gate uses the existing disposable Honcho PostgreSQL harness and
these new production-handler tests:

```text
tests/test_employee_reviewed_records.py
tests/test_employee_reviewed_authority.py
tests/test_employee_reviewed_diagnostics.py
tests/test_employee_reviewed_atomic.py
```

The tests cover actual concurrent replay and cleanup, connection-pool restart,
same-key changed deployment/payload, current native identity replacement,
workspace JWT refusal, canonical provenance rehash attacks, exact selected read
scope/order/budgets, expiry without fabricated erasure, deferred guard failures,
injected transactional failure and finite quota admission. The quota fixture
uses bounded synthetic SQL with the production guards enabled; it is not signed
application authorization evidence. Existing project/conversation tests remain
the compatibility gate. A new installed artifact and actual adapter protocol I/O
remain separate from these source and disposable-database claims.
