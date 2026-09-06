# Signed private API and Activity acceptance

Completed at16:47:09 UTC on2026-09-05 against the actual schema61 API,
PID68332/session85029, on `http://127.0.0.1:8787`. The existing exact private
human owner signed requests with installed `nostr-tools`2.23.12 `finalizeEvent`;
every signature was verified locally by that library. No handwritten
cryptography, provider request, API mutation or native UI interaction was used.

The test imported production `createOrtakClient`, `consumeActivityStream` and
`appendActivity` from the current checkout using pinned Node24.15.0 and the
existing TS import loader. The imported paths use no loader mock modules. Source
hashes were recorded and unchanged across the successful run. The private owner
was selected only through the marked fresh stack's owner-private identity file;
no key, auth header or response body was printed in diagnostics.

## Actual observations

Seven signed GETs returned200 with fresh nonces: employees, runs, exact run
detail, full events, first SSE subscription, SSE reconnect after cursor4 and
HTTP events after cursor4. Employee/run pages respected25-row bounds, HTTP
events the100-row bound, and SSE the25-entry/4MiB-frame bounds. These are actual
small-response observations, not a replacement for oversized-response unit tests.

Employee `ada-private` was active at revision
`e36f3e77-4d63-48ae-afa5-88f62a1ba82c`. Run
`ee847bb4-364e-4aec-b2f7-5af3daaadb9a` was completed, with reply intent,
no cancellation and `can_request_cancel:false`. Its input message was
`e731e888ece2a17665bcd86e256868dfbf0d8953ed2c791f2212b9c8acd1afaf`.

Office delivery was `delivered` at16:32:44.649788 UTC. Memory used the same
`run_scratch` scope/run ID, with prepared recall containing zero records and
no truncation. This is valid empty recall, not evidence of nonempty recall.
The durable memory write was `acknowledged`, attempt1, written1, at
16:32:45.066910 UTC. Its source was the exact Office reply
`b423b20bfcb5333ca73e184ccf29b7af8492e195f21fe7e7e70897fab21b3d92`.
The retained Honcho receipt reference is recorded privately.

The API assistant delta and memory content exactly equaled the canonical
33-byte Office answer supplied by root's signed-event DB verification:
“17 ile 25’in toplamı 42’dir.” This task verified the API projection and receipt
link; root's separate integrated run established the actual provider and signed
Office publication.

| Sequence | Public Activity event type | Projection |
| ---: | --- | --- |
| 0 | `run_queued` | lifecycle |
| 1 | `run_started` | lifecycle |
| 2 | `assistant_delta` | assistant output |
| 3 | `delivery_intent` | delivery intent |
| 4 | `run_completed` | lifecycle |

No event was redacted or truncated; the page had no gap or further page. The
production cursor reducer accepted the dense sequence and matched detail's
high-water mark4. The first real SSE connection received this same page and was
explicitly aborted after56ms. A new signed connection with `after_sequence=4`
returned an empty activity page retaining cursor4; it was aborted after26ms.
No duplicates were added, and the corresponding HTTP tail was also empty.
Both aborts reached the production parser's cleanup path. This tests completed
history replay/reconnect, not a concurrent revocation or new live delta.

## Retained private evidence

Successful receipt:
`/private/tmp/ortak-v0-evidence/signed-api-acceptance-817b24760fab43dfb95e59de3e6548fe/receipt.json`.
Its intent, sanitized detail, helper and diagnostics remain owner-private.
Production client SHA256:
`23f09870eb9040eb861e4cbd03e1dbc16e77b3943c3a97db26257cf46e6f38d9`.
SSE parser SHA256:
`782c4521bfd98f026704bfb7f02f0c66b48c49646a7077f7f40da7f5ebb5e4b4`.
Cursor reducer SHA256:
`c58b1c695efd68bd34f885ae92de9fb825610e81558c4e8ed387aee65aa9b67b`.

Three earlier failed helper attempts are retained. Two mistakenly expected the
equation spelling rather than the actual Turkish answer; the third expected
bridge wire event names with dots rather than the public projection's snake-case
names. Their HTTP requests succeeded. The final comparisons use the independently
confirmed canonical Office text and actual API contract; no product source was
changed to make these checks pass.

The same private directory contains `signed-api-operator.mjs`, SHA256
`01257ec8ecf49b00177aee9fd9119cb8a71de7dbbddf0e05225935cc711d5716`.
It requires explicit `--action read-run|cancel-run --run <canonical UUID>`, fixes
the company/employee/origin/owner selection, and uses the production client.
Its read mode passed two further real signed GETs, with `post_sent:false`:
`operator-944aa8bd-0030-42b0-8ce8-48070c033181/receipt.json`.

Cancellation mode was **not invoked** by this task. It first reads the current
audience/run, persists cancellation intent, checks current cancellability and
submits at most one signed empty-object POST. An existing cancellation receipt
is returned without another POST. Lost acknowledgements remain uncertain in the
private failure record; they do not cause an implicit retry or remote-stop claim.
Root must verify durable cancellation and actual containment separately when it
explicitly exercises that action against a newly selected active run.

This closes the authenticated product API read gap from the schema61 rollout.
It does not claim native UI acceptance, full cancellation/revocation behavior,
provider readiness beyond root's separate run, or full-stack recovery.
