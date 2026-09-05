# Private Ada memory bootstrap

`scripts/ortak/bootstrap_private_memory.py` provisions one fresh Honcho bundle
for the existing `ada-private` identity in the marked private stack. It uses
the extension's explicit create operation, verifies its immutable receipt and
current native resource IDs, and performs one canonical diagnostic
remember/recall roundtrip. It publishes a secret-free configuration fragment
for the production `WorkerMemory` composition. It never activates an employee,
starts a worker, enables routing, or reads a Hermes/provider credential.

## Preconditions and invocation

Use the canonical `/private/tmp/ortak-private-20260905` state initialized by
`init_private_stack.py`, with the fresh identity bundle created by
`private_native_services.py prepare`. The existing company UUID and employee
`ada-private` are checked; the helper does not create or change those identities.
The selected Honcho extension must already be running at
`http://127.0.0.1:8009`, with native authentication enabled and the additive
`/resources/inspect` protocol present. Build and select the reviewed extension
artifact from `runtime/honcho-adapter`; a protocol response alone does not prove
which image is running. Do not point this dated recipe at another service.

The operator supplies one fresh selected native Honcho admin token through an
explicit environment variable, for example `ORTAK_HONCHO_PRIVATE_TOKEN`. Its
value must be supplied by the private launcher/subprocess environment, never
as a command argument, printed shell assignment, or checked-in config. No
fallback to another variable, dotenv, proxy, keyring, profile, or provider token
exists. The helper sends that bearer token only to the literal loopback address.
The create operation requires the selected native admin authority.

Choose and retain one non-nil deployment UUID for this exact selected Honcho
deployment. With the selected token already present in the process environment:

```sh
python3 scripts/ortak/bootstrap_private_memory.py \
  --state-dir /private/tmp/ortak-private-20260905 \
  --deployment-id <selected-Honcho-deployment-UUID> \
  --token-env ORTAK_HONCHO_PRIVATE_TOKEN
```

The UUID placeholder must be replaced; it is an artifact/deployment selection,
not the company UUID or an existing production employee's identity. The helper
does not generate a new deployment selection each time it runs.

## Durable intent, receipts and recovery

The helper creates an owner-private `memory` directory and uses a nonblocking
exclusive file lock. Before its first HTTP request, it atomically persists:

- The exact company, employee, deployment, endpoint, token-variable name and
  binding; workspace `ortak_ada_<company UUID without hyphens>`, user peer
  `operator-private`, and employee peer `ada-private`.
- Original create key `ortak-memory:<company UUID>:ada-private:<deployment UUID>`.
- One random diagnostic run UUID and a canonical UTC provenance timestamp.

The journal `memory/bootstrap.json` then records the exact server-issued create
receipt, original request hash, immutable native workspace/peer IDs, and
canonical diagnostic write response in that order. Snapshots use a mode0600
temporary file, file fsync, atomic replace, and directory fsync. Existing private
files must be bounded regular files owned by the invoking user; symlinks and
group/world-readable files are refused. The original `identities.json` remains
unchanged.

Each invocation makes one attempt, with a 20-second whole-operation alarm,
5-second maximum per-socket wait, 16 KiB request cap and 64 KiB streamed response
cap. There is no automatic retry loop, redirect or proxy handling. An explicit
retry uses the exact same deployment/token-variable arguments:

- A lost create acknowledgement replays the durable original key. Once a create
  receipt is saved, retries use only read-only resource inspection to recover
  ownership; they never call create again.
- A lost write acknowledgement or interrupted recall replays the same
  diagnostic write key, content, scope and provenance. It cannot add another
  diagnostic record under a newly generated key.
- Missing, replaced or mismatched native resources fail before diagnostic I/O.
  They are never automatically recreated. The extension checks the original
  immutable receipt against current native IDs inside its read transaction.
- A completed retry verifies protocol and resource identity only. Its result
  says `previously_verified`; it does not claim a fresh roundtrip.
- If config publication is interrupted after the completed journal is durable,
  the next invocation publishes the same config without another memory write.

Do not remove the journal as a retry strategy, alter its receipt IDs, or change
its deployment key to bypass a refusal. A failure preserves durable state and
does not log credentials or response bodies.

## Worker composition

Successful completion writes `memory/worker-memory.json`, also mode0600. This
is the value of the outer private worker configuration's `memory` property:
the deployment selection, exact binding, original create key, stable diagnostic
run/time, and `validate_memory_io: true`. It contains an opaque credential
reference and the selected environment-variable **name**, never its value.
The helper does not modify or publish the outer worker configuration itself.

On startup, production `WorkerMemory` calls the Rust adapter's
`resume_created_resources` read-only recovery operation, checking the original
create request hash and current native resource IDs, then explicitly validates
the same diagnostic roundtrip. Its bounded refresh loop keeps execution
witnesses current; the adapter rechecks ownership and the witness at I/O.
The helper's historical receipt is not an activation or an execution witness.
Office membership, runtime, signer, memory, and control-plane activation gates
still apply independently.

## Validation

```sh
python3 -m unittest discover -s scripts/ortak -p test_bootstrap_private_memory.py -v
```

The 12 focused tests passed locally on 2026-09-05. They drive the production
bootstrap state machine with an independent protocol fixture and cover durable
intent before create, lost acknowledgements, read-only completed restart,
missing/replaced native IDs, altered saved receipt/arguments, canonical
provenance and nonempty recall, interrupted publication, identity/file
boundaries, streamed response bounds, redirects and expired deadlines. No
socket, real credential, provider, employee activation or service mutation was
used in those tests. A central live run of this helper remains a separate gate.
