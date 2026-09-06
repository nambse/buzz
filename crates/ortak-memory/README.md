# Honcho memory adapter

`HonchoMemoryAdapter` implements the existing control-plane `MemoryAdapter`
against the reviewed Honcho 3.1.1 extension `ortak-honcho/1`. It uses actual
HTTP operations; no SDK get-or-create, peer-global representations, dialectic
chat, deletion, or invented provenance is available.

Construct it with a server-resolved `CompanyScope`, `HonchoMemoryConfig`, and
`ResolvedHonchoToken`. Config fixes a deployment UUID, reviewed protocol/version,
HTTPS or loopback origin, opaque endpoint/token references, and at most 64
explicit employee/full-memory-binding tuples. Unknown bindings, duplicate
workspaces, unknown binding options, different credential references and
non-loopback plaintext origins fail before HTTP. No request can supply a
company, URL, credential, peer selection or runtime endpoint. The adapter and
resolved token have no Debug/serialization implementation; sensitive bearer
headers are never copied into errors. Resolve secrets from an explicitly named
fresh environment variable or an authorized secret resolver, never Git.

Each employee also has an explicit set of allowed project UUIDs and an opt-in
company-truth scope. Employee experience, relationship and run scratch remain
separate deterministic sessions. The control-plane caller must authorize the
particular run and reviewed company facts before constructing a memory request;
model-produced project/run identifiers are not authority. Session names are
`ortak_` plus the full SHA-256 of sorted compact JSON containing protocol,
company, employee and exact typed scope. Workspace and peer names remain the
full configured binding; the extension pins their native identities atomically.

## Activation sequence

1. `ensure_resources(Create)` calls only the extension's atomic create route.
   Store its idempotency key durably. The complete returned resource tuple and
   created ownership must match; the new read-only ownership endpoint must then
   verify the create-request hash and frozen native IDs before an in-process
   creation receipt is held.
   Existing foreign resources or differing retries fail; no overwrite fallback
   exists. A replay uses the same original key and exact binding.
2. Explicitly call `validate_memory_roundtrip` with a fresh diagnostic run UUID
   and stable timestamp, persisted before the call. It requires the verified
   create receipt and live owned workspace/peer metadata, writes one diagnostic
   fact into that run's scratch session, and requires an exact nonempty scoped
   recall of the canonical written record. This is memory I/O validation: a
   legitimate full-text Honcho deployment may make zero external provider calls.
3. Only that binding gains Recall/Remember and healthy activation status, for
   the configured monotonic lifetime (maximum 15 minutes). Another employee,
   binding, deployment instance, expired witness or process restart does not
   inherit it. Health and capability probing are read-only and independently
   check current resources through the same immutable identity receipt used by
   the extension. Every read/write rechecks its witness immediately after the
   awaited ownership inspection and before HTTP dispatch. A newer validation
   generation invalidates an older in-flight admission or validation result.
4. After creation, export `created_resources_receipt` with the original create
   request and durably persist its `HonchoCreatedResourcesReceipt`. On activation
   or worker restart, call `recover_created_resources` with that same receipt,
   then the same journaled diagnostic roundtrip (or a new persisted diagnostic
   run). Recovery uses only read-only inspection and fails for
   absent/replaced/mismatched resources; it never creates anything.
   Deserialized `MemoryRoundtripReceipt` objects cannot restore execution rights.
   Retry orchestration belongs to the caller's durable journal; no hidden retry
   loop or destructive cleanup runs in this library.

Ordinary Adopt mode calls only native workspace and peer list endpoints with
`{}` bodies. It reports adopted outcomes and can inspect resource existence;
it never creates sessions, writes probe facts or obtains Recall/Remember.
An explicitly prepared extension-owned bundle can be recovered separately with
its original `HonchoCreatedResourcesReceipt`. The receipt preserves company,
deployment, complete binding, employee, create key/hash, original created outcome
and frozen native IDs. The caller must authorize its selection. Recovery checks
all of those fields against the configured selection and the actual extension's
current ownership inspection; a deserialized or self-asserted receipt alone
cannot grant access. The adapter also rejects replacing a retained identity.

When the configuration's acquisition mode is Adopt, recovery and all subsequent
`ensure_resources(Adopt)` calls return **Adopted** outcomes. Original extension
ownership is preserved separately and never becomes saga-created ownership.
Recovery itself grants no I/O witness. Only a subsequent explicit
`validate_memory_roundtrip` can permit bounded scoped I/O; health, probing and
resource inspection never call that method. Activation and the worker must use
the exact same durable receipt, original diagnostic request and full binding;
neither may rewrite the other operation's acquisition history. Existing unowned
workspaces remain inspect-only. ResourceDelete always returns Unsupported,
including for resources created by Ortak, so Adopt compensation cannot delete
the prepared resources.

## Bounds and validation

- Eight concurrent operations; 30-second overall deadline including limiter and
  pagination waits. Each HTTP request is limited to the configured 1–15 seconds.
  Redirects and ambient proxies are disabled.
- At most ten native list pages of 100, with consistent pagination metadata;
  exhaustion is an explicit error rather than a false missing-resource result.
- Request bodies at most 1152 KiB; response bodies at most 2 MiB, checked both
  from Content-Length and while streaming. All upstream error bodies are dropped.
- Recall queries at most 4096 bytes, 100 records and 128 KiB returned content;
  writes at most 64 facts of 16 KiB, with bounded printable idempotency keys and
  provenance source. NUL, employee/run/scope mismatch, duplicates and returned
  budget excess fail closed.
- Write acknowledgements must match protocol, workspace, deterministic session,
  canonical request hash, ordered record IDs, exact content, scope, provenance,
  and each metadata envelope. Timestamps normalize to Pydantic's UTC microsecond
  spelling before hashing. Both 201 creation and 200 replay preserve one receipt.
  Recall validates scope/provenance/bounds again after the extension's canonical
  receipt checks. Authentication, transport and malformed responses propagate
  sanitized typed errors; no failure becomes empty success.

## Focused checks

Default tests do not bind sockets:

```sh
cargo test -p ortak-memory
```

The HTTP fixture tests exercise the real reqwest implementation and production
adapter methods, including auth, fixed company/payloads, scope/receipt tampering,
empty roundtrip rejection, restart/expiry, read-only adoption, redirects and body
bounds. Run in a socket-capable environment:

```sh
cargo test -p ortak-memory http_contract -- --ignored --test-threads=1
```

A separate live extension check creates only fresh UUID-named `ortak_rust_*`
resources. Supply an explicit disposable extension URL and its fresh admin token
through non-Git environment configuration. It never accesses existing workspace
names and performs no deletion:

```sh
cargo test -p ortak-memory live_extension_create -- --ignored --test-threads=1
```

That command requires `ORTAK_HONCHO_TEST_URL` and `ORTAK_HONCHO_TEST_TOKEN`. It
checks real create/write replay, scoped recall, empty other-project scope,
read-only adoption, frozen receipt recovery, adopted ownership after explicit
I/O validation, and revalidation after a new adapter instance. It
establishes neither provider credential health nor external model quality.

The worker/provisioning composition, durable validation journal, Memory UI,
retention and provider-specific health remain integration work. Do not claim
Milestone 5 complete from this adapter or a static protocol response alone.


The additive read-only route is
`POST /v3/ortak/workspaces/{workspace}/resources/inspect`. It accepts only
`{company_id, employee_id, user_peer, employee_peer}` and returns the exact
owned bundle, original creation request hash, and frozen native IDs after
checking current native rows. The client pins this identity against its first
successful create replay and rejects replacements even if their public names
and metadata are identical. Health and capabilities use the same check; native
list metadata alone cannot renew activation. Existing deployments lacking this
route fail closed until the matching reviewed extension artifact is selected.
No create/get-or-create endpoint is used by health or capability inspection.

Additional regressions cover delayed ownership responses crossing witness expiry
for writes, recalls, health and capabilities; successful-looking responses with
changed native IDs; and stale validation generations restoring a witness after a
newer failed refresh. Their central socket/PG execution receipts must accompany
the rebuilt candidate; previous successful runs predate these fixes.


`resume_created_resources(&MemoryResourceRequest)` remains available for the
older Create-only caller. It reconstructs an in-process identity from the
extension's durable create receipt by the original request hash, grants no I/O
witness, and requires explicit roundtrip validation. New activation and worker
composition should use `recover_created_resources` instead, preserving original
native IDs in their own durable journal as well as in the extension's receipt.

The `http_contract::recovery` tests exercise the production recovery, adoption,
health, capabilities and actual I/O methods. They cover unchanged Adopt outcomes,
absence of implicit witnesses/writes, restart and identical diagnostic retries,
replacement native IDs across restart, foreign company/deployment/employee/full
binding, altered create keys/hashes/outcomes, plausible forged receipt data checked
against the real HTTP inspection seam, and expiry during an awaited ownership
check. These use synthetic local HTTP responses, not real Honcho or provider
evidence. Run the separate live extension check for a newly selected artifact.

## Explicit reviewed project record family

The additive `publish_reviewed_project`, `inspect_reviewed_project`,
`recall_reviewed_project`, and `remove_reviewed_project` methods target only
`reviewed-project/1`. This is a dedicated extension store inside the selected
Honcho database, structurally outside native messages, embeddings, derivation
queues and peer representations. Current configured project/employee bindings,
frozen native resource identity and a current I/O witness are all required.
Ordinary adoption or health inspection cannot grant these rights.

The caller must authorize the human approval and persist its stable record UUID
and operation key. Publication receipts contain hashes and exact provenance;
withdrawal or expiry proves absence only from the referenced current extension
text store. It does not erase native resources, legacy RunScratch, original Office
or Work evidence, approval registry records or backups. Runtime admission of
reviewed facts is separate integration work and must recheck revocation/expiry.

`reviewed_operations_never_turn_read_only_adoption_into_io_authority` is a default
no-infrastructure regression. `http_contract_reviewed` exercises the real HTTP
client's identity/digest/shape/budget and delayed-ownership/witness boundary. The
separate extension PostgreSQL suite proves actual storage lifecycle atomicity;
HTTP fixture responses alone do not establish physical erasure.
