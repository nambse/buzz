# Private runtime worker

The `ortak-worker` binary composes the durable Office inbox, deterministic
routing, pinned Hermes dispatch, ordered run events, cancellation, and signed
Office delivery. It is an explicit opt-in process, separate from the product
API. Compilation alone does not establish a working Hermes deployment.

## Startup configuration

Apply the repository migrations through 0052 before starting either process.
Use a fresh isolated database, Redis namespace, company, Office channel and
employee identities. Existing external test resources are not a default source
of configuration or credentials.

Set `ORTAK_WORKER_ENABLED=true`, `ORTAK_DATABASE_URL`,
`ORTAK_HERMES_BRIDGE_TOKEN`, and `ORTAK_WORKER_CONFIG_JSON` through the deployment
secret/configuration mechanism. The worker configuration contains public
identifiers and opaque secret references only:

```json
{
  "company_slug": "isolated-test-company",
  "bridge_origin": "http://127.0.0.1:8650",
  "poll_interval_ms": 1000,
  "batch_limit": 8,
  "office_signers": [{
    "company_id": "<company UUID>",
    "employee_id": "test-employee",
    "signer_ref": "credential://office/test-employee",
    "public_key": "<employee public key>",
    "secret_env": "ORTAK_TEST_EMPLOYEE_OFFICE_KEY"
  }],
  "office_relays": [{
    "company_id": "<same company UUID>",
    "community_id": "<isolated Office community UUID>",
    "origin": "https://<reviewed private Office origin>"
  }]
}
```

The referenced Office secret environment variable contains a fresh 64-hex private
key. It is loaded once, checked against the configured public key, and never
returned by the API or included in a manifest. Provision and verify the matching
Office binding through the control plane before activating the employee.
All signer and relay bindings must select this worker's company; one relay is
accepted. Both remote transports require HTTPS except on loopback, refuse
redirects and implicit proxies, and impose bounded timeouts and response sizes.

```sh
. ./bin/activate-hermit
cargo run --locked -p ortak-server --bin ortak-worker
```

The Hermes bridge must report durable cancellation by start key (`RunCancelStart`),
start lookup (`RunLookup`), and event replay (`RunEvents`) capabilities before
the worker can start. Missing any of these recovery capabilities refuses startup.
If recovery is available but an activation capability is missing, the worker
starts in recovery-only mode: it can drain cancellation and existing event streams,
but cannot route or dispatch new work. Missing or invalid Office delivery
configuration also pauses new work while preserving runtime recovery. Starting
new work requires the complete activation capability set, initialized Office
delivery, a currently validated selected memory adapter, and current company/community authority. Configuration and capabilities
are loaded at startup; restart the worker after correcting them.

The optional `memory` configuration must be the secret-free fragment produced
by the explicit private [memory bootstrap](../../runtime/private-stack/MEMORY_BOOTSTRAP.md).
It selects the immutable deployment UUID, exact binding, opaque token reference,
original resource creation key and persisted diagnostic run UUID/time. The
`validate_memory_io: true` field explicitly allows the worker to refresh that
same diagnostic write/read/replay evidence. Ordinary health checks never write.
Omitting or rejecting memory configuration leaves new work paused; existing
cancellation and event recovery remain available. Restart first inspects the
original receipt and native IDs without creating any missing resources. At most
one configured binding is refreshed per pass, with a six-second deadline and
bounded backoff. Memory I/O evidence does not prove model-provider health.

The worker does not create profiles, choose provider credentials, activate
employees, or enable independently subscribed Office gateways.

An optional `semantic` selection enables bounded relevance evidence for
untargeted human messages. Omission performs no credential lookup or provider
request and records `semantic_scorer_disabled`. Invalid selection records
unavailable semantics while preserving cancellation and deterministic routing.
The [semantic adapter contract](../../crates/ortak-routing-semantic/README.md)
defines its limits and exact protocol. Configuration has this shape, with every
value explicitly selected by the operator:

```json
{
  "semantic": {
    "deployment": {
      "deployment_id": "<selected deployment UUID>",
      "origin": "https://<selected provider origin>",
      "model": "<selected compatible model>",
      "response_model": "<exact reviewed response model snapshot>",
      "token_ref": "credential://<selected semantic credential reference>"
    },
    "token_env": "ORTAK_SELECTED_SEMANTIC_TOKEN"
  }
}
```

This is an unselected example. The current private stack has no semantic model
or provider credential configured. Neither capability checks nor local HTTP
fixtures prove a real model response.

## Recovery and visible delivery

Every pass first reconciles durable stop requests and changed Office authority.
This also runs after company suspension or removal of its Office binding.
Starting work requires current company/community authority. Each external call
gets its lease immediately before execution, and retries have finite budgets.
The worker pool bounds row locks to500ms and statements to5s. Each routing pass
attempts one inbox item; Office delivery and memory output also claim one item
immediately before use. A complete runtime stop/replay/acknowledgement attempt
has a35s deadline inside its60s lease. Partial event replay preserves the durable
cursor and leaves a retry record when that deadline expires.
Adapter failure leaves durable work for process supervision to retry; configure
the process manager with restart backoff.

Both SIGINT and SIGTERM are registered before startup I/O. The worker interrupts
an in-flight cycle promptly, preserving durable leases, start keys and cursors
for restart; this does not acknowledge remote execution as stopped. The API
stops accepting requests and gives existing requests at most15seconds to drain.
A drain timeout is an error and those requests may require retry. Coordinated
quiescence and contained runtime shutdown remain separate operational steps.

Completing a publishing run atomically creates an Office output job. The job
freezes the last assistant turn and canonical source-channel/thread tags before
enqueueing. Arbitrary runtime destination hints do not select the recipient.
Unsupported, empty, oversized or truncated output remains a visible delivery
failure. Delivery rechecks the source, employee identity and current Office
authority, freezes the signed event before HTTP, and reuses exactly those bytes
after a lost acknowledgement or restart. Fresh NIP98 request authentication is
signed independently for every retry.

Run completion and Office delivery are separate facts. The authorized run-detail
API returns `office_delivery` with `pending`, `delivered`, or `failed`, or null
when no output job exists. Clients continue polling a pending delivery after
the run has completed. A failed delivery preserves the run's terminal history
and the durable error record.

## Release evidence still required

Before enabling a selected live cohort, record the integrated source revision,
reviewed immutable Hermes image/source revision, schema version, and disposable
fixture identifiers. Exercise the actual PostgreSQL seams and browser screen,
then a real human message→Hermes→one Office reply. Demonstrate lost start
acknowledgement, worker/bridge restart, whole-process cancellation, dense cursor
replay, and byte-identical delivery retry. Do not substitute fake adapters or
capability declarations for this evidence.

## Memory and Activity

Before first start the supervisor freezes the entire admitted RunSpec plus its
bounded RunScratch recall/provenance once. Retries reuse those exact bytes and
still require fresh Office and memory authority. No cross-run employee/project
recall or promotion is enabled in this slice.

Only an acknowledged signed Office reply creates an automatic memory write job.
The worker revalidates canonical source and pinned/current bindings, then writes
the frozen published bytes with the original stable key. The API's scoped run
memory projection distinguishes unprepared context, prepared empty/nonempty
recall and pending/acknowledged/failed writes. Sensitive text is redacted;
credentials and runtime configuration are never part of this projection.
