# Validation receipts

All infrastructure below was newly created for this task. No existing external
Honcho service, memory, provider credential, or employee resource was accessed.

The preceding candidate's frozen source manifest was
`152775705731404beb62cdc3dac63d5a38451bf6158ce85d842c480a8f0f712d`.
It passed **11 actual PostgreSQL/ASGI integration tests in 6.46 seconds**,
including a fresh-interpreter lost-ack replay, on 2026-09-05. Root ran it centrally
against a fresh pgvector 17 database on private Docker network
`ortak-honcho-test-20260905`; the database had no host-published port.

Its runtime image was
`sha256:f3df03c1a5177f1c68e09ce26c12504d43dfb98e855ba5f416db40526f57958a`.
Native and extension initialization and the real API lifespan completed.
The actual socket HTTP smoke passed inside the new API container
`ortak-honcho-api-check-20260905`, with these public receipt identifiers:

- Workspace: `http_smoke_a5fe3e4621964e8e81fe37050610dbed`
- Session: `run_a5fe3e4621964e8e81fe37050610dbed`
- Record: `8Gt66Q0zBKk9Uje6PwNJ1`
- Recall mode: `full_text`
- External provider validated: `false`

A subsequent read-only review found that SQLAlchemy's `expire_on_commit=False`
could preserve first-phase workspace/session attributes across a provider call.
The current source explicitly detaches those ORM identities before the second
read. A twelfth production-seam regression pauses the provider, changes live
workspace ownership, then requires a 409 refusal. The correction passed **12 actual PostgreSQL/ASGI cases in 3.40 seconds**
on the rebuilt test image
`sha256:74ee44f2c0b2cb8e9e2327f7362302e31714d1ae1affcd3e89bc36d00e41f7ea`.
The rebuilt runtime image is
`sha256:5d6811ddb356c61d7f9e9fde77b44feeb846394c7391d38391b8af239ca58eda`.
Its real socket HTTP smoke also passed, creating fresh workspace
`http_smoke_01a4c64da263425c94c0f34f7caf44c5` and record
`e-qwTQtTSlZ6GhKXJMsTL`. Recall remained `full_text`; external provider
validation remains `false`. These receipts cover the ownership-mutation
regression and the final corrected service code.


## Ownership and witness fixes — candidate awaiting central validation

The next candidate adds the authenticated read-only resource inspection route.
It validates the frozen creation receipt against current locked native IDs and
returns no healthy result for replacements with identical names/metadata.
`tests/test_resource_inspect.py` checks exact receipt/hash/ID agreement, unchanged
native/extension table counts, missing resources without creation, foreign JWT
and company/binding denial, and a real PostgreSQL peer replacement preserving
metadata. No migration or old-resource mutation is required by the route.

The matching Rust adapter freezes that identity and checks it during health,
capability probes, validation and I/O. Witness expiry and refresh generation are
rechecked immediately before memory dispatch; a superseded validation cannot
restore capabilities. Added Rust HTTP regressions use delayed inspection and
changed native IDs; a production gate unit test exercises overlapping refreshes.
Python syntax checks and four Rust unit checks passed locally; all seven HTTP
fixture tests plus the live extension test compiled but remain central gates.
Read-only worker receipt resume is covered without any resource-create call. The preceding 12-PG/HTTP/live-adapter
receipts describe the earlier candidate and do not validate these new changes;
central builds and focused regressions must supply the new receipt.
