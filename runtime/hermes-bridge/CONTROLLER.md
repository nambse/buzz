# Private controller packaging and checks

For fresh ChatGPT/Codex OAuth enrollment, exact model/effort selection, separate
refresh ownership and explicit real health probes, follow [OAUTH.md](OAUTH.md).
Its three-file OAuth profile extends the API-key format described below.

The worker image and controller image are separate artifacts. The controller
has Python, the fixed SQLite library, bridge code, and Docker's static client.
Only the controller receives the daemon socket and service bearer token. The
selected worker image receives one disposable employee profile, one company
journal directory, and the bounded RunSpec on stdin.

The Dockerfiles use the selected BuildKit built-in frontend. Record the actual
BuildKit version with each build receipt (this continuation observed v0.26.2);
there is no floating `docker/dockerfile:1` frontend download. This is the
[built-in frontend supported by Docker](https://docs.docker.com/build/buildkit/frontend/).

Build the worker first with `Dockerfile` and retain its real constructor/guard
smoke and immutable image identity. Docker Desktop's buildx driver attempts
remote resolution for local `FROM repository@digest` and `FROM sha256:<id>`.
The controller therefore requires a named **OCI-layout** context `worker_image`;
it does not need a registry push or a mutable local tag as its base.

Export the reviewed worker build to a new OCI-layout directory while also
storing the Docker image, using `--provenance=false` and OCI media types. This
avoids a Docker attestation index that differs from the simultaneous OCI export:

```sh
docker buildx build --provenance=false \
  --build-context "hermes_source=$ORTAK_HERMES_SOURCE" \
  --output type=image,name=ortak-hermes-candidate:29112bef,oci-mediatypes=true \
  --output "type=oci,dest=$ORTAK_WORKER_OCI_DIRECTORY,tar=false" \
  -f runtime/hermes-bridge/Dockerfile runtime/hermes-bridge
```

Retain the export receipt and select its actual worker manifest digest from the OCI index. Pass
that exact selected digest in both the named context and `WORKER_IMAGE`. The
latter is retained as the controller's `org.ortak.worker.image` provenance
label. Record the relationship to the Docker-inspected worker runtime identity;
never substitute a build-log config digest for the Docker runtime image ID.

`DOCKER_CLI_IMAGE` is another mandatory immutable reference. The root task
observed the official candidate
`docker:28.5.2-cli@sha256:625d9431a9f54c5a2bc90f24f0e1c3d55b1349fd857dd85035f98c2c9acbdd4d`.
Use that explicit digest, or another independently reviewed immutable artifact.
No floating or fabricated digest is supplied as a default.

```sh
docker buildx build --load \
  --build-context "worker_image=oci-layout://$ORTAK_WORKER_OCI_DIRECTORY@$ORTAK_WORKER_OCI_DIGEST" \
  --build-arg WORKER_IMAGE="$ORTAK_WORKER_OCI_DIGEST" \
  --build-arg DOCKER_CLI_IMAGE="$ORTAK_DOCKER_CLI_IMAGE" \
  -f runtime/hermes-bridge/Dockerfile.controller \
  -t ortak-hermes-controller:private-candidate runtime/hermes-bridge
```

The named OCI context is mandatory. The immutable `WORKER_IMAGE` provenance
argument alone cannot select the Dockerfile base. The root build receipt must
prove both arguments refer to the same reviewed export.

Record the resulting controller image ID and use that immutable identity in
composition. The controller entrypoint does not import Hermes or call a model.
Its Python/SQLite and bridge modules come from the reviewed artifact; only its
Docker CLI is an added runtime dependency. The source-revision label by itself
is not execution approval. Keep the config's `executor.image` and
`executor.validated_digest` equal to the original worker identity, never the
controller derivative containing the Docker CLI.

## Fresh paths and ownership

Choose an unused absolute root, such as `/srv/ortak-private-<new-uuid>`. Create
only the new directories, with owner/group `10001:10001` and mode 0700:

- `<root>/state`: dedicated journal, WAL files and executor lock; no tokens,
  config, old journals or unrelated data.
- `<root>/profiles/<employee>`: exactly the four profile files defined in
  `HERMES_BRIDGE_V0.md`, all owned by 10001. The credential file belongs only to
  this newly selected employee; the fixture check uses a fake value.
- `<root>/controller`: `config.json` and `service-token`, owned by 10001, mode 0600.
- `<root>/checks`: writable disposable check parent, retained as evidence.

Mount state and profile paths at the **same absolute paths** inside the
controller as on the daemon host. Docker interprets a child bind mount's source
on the host. Mapping `/srv/.../state` to `/state` in the controller would make the
child mount the wrong host path. Avoid symlinks and commas in these paths.
This paragraph describes the legacy bind configuration. For a Docker-managed
journal, use the explicit selection below; changing the controller mount alone
would otherwise leave its worker children using the old host directory.

### Optional named journal volume

The controller accepts an optional `executor.journal_volume` object:

```json
{
  "name": "<exact-created-local-volume-name>",
  "created_at": "<exact-Docker-CreatedAt>",
  "owner_id": "<fresh-canonical-UUID>"
}
```

Create that volume explicitly with driver `local`, no driver options, and labels
`org.ortak.company=<company UUID>` and `org.ortak.journal_owner=<owner_id>`.
Keep both labels on the new controller too. Use Docker's default generated
hostname; custom hostnames are refused for this option. Mount the selected
volume read/write at the directory containing the controller's `--journal`
path, with `volume-nocopy`, and no nested mounts below that directory. The
directory remains private and writable by UID/GID10001. Profile/OAuth paths,
service token and worker image selection do not change.

Before opening `Journal` at startup, before acquiring executor ownership, and
before every child launch, the controller verifies the exact volume name,
creation timestamp, ownership labels, local driver/options and its own actual
volume mount. The daemon's source mountpoint is compared as metadata only.
Both selected JSON projections retain the existing1024-byte/five-second command
bound. The child receives
`type=volume,src=<name>,dst=/ortak-state,volume-nocopy`; the existing worker's
`/ortak-state/journal.sqlite` contract is unchanged. A missing, replaced,
read-only, shadowed or differently owned volume refuses admission. There is no
volume creation, initialization, migration or fallback in the runtime.

For the current incident, root's isolated comparison reproduced SIGBUS on the
host bind while the same existing images completed64 resolve/consume cycles on
a local Docker volume. Evidence:
`/private/tmp/ortak-c2-sqlite-repro-95d6d9e626224b8e955b8a041ef8337b/receipt.json`.
This is an observed deployment failure, not a reason to discard the run. Its
cold journal already held `run.completed` and the consumed tool result hash.
Same-key result ACK and event replay preserve that history without a new model
request; current Work/Office authority guards still apply to output publication.

Migration is a separate root operation: contain all original writers, preserve
the physical journal and WAL/SHM absence or bytes, copy only the selected cold
state into a fresh owned volume, and verify bytes/metadata before starting the
new controller. Retain the original source and crash evidence. A controller-only
derivative may keep the existing tested worker image unchanged; record the new
controller source/image relationship and exercise this mount gate on the actual
image before a new run. Recovery inventory must explicitly select the new volume
before claiming a later full-stack backup; the historical host bind is not its
current storage authority.

The actual dated G74 operation subsequently captured that selected local volume
and completed its offline foundation restore on 2026-09-06. Bundle
`214fd4f027a34604aeb7469d9dfb9a60` and restore
`cea594c6416d42f7a3403aa7509d2c70` passed physical raw-journal extraction and
coherent logical comparison: 25 terminal runs with valid cursors, two workspace
run/call histories and zero pending/invalid workspace rows. The source services
were restarted; the restored files stayed inert. No restored journal volume or
runtime was activated and no separate host/daemon was exercised. Raw Linux
UID/GID remain provenance when the offline files belong to the host UID; a later
activation still requires a fresh owned local volume and explicit generation
rebinding. Exact manifests, frozen operator closure, cleanup and source-owner
limits are in [the G74 recovery record](../../docs/ortak/CURRENT_PRIVATE_RECOVERY74_2026-09-06.md).

The config schema is:

```json
{
  "company_id": "<fresh-company-uuid>",
  "profiles": [{
    "employee_id": "<fresh-employee-id>",
    "directory": "/srv/ortak-private-<new-uuid>/profiles/<employee>",
    "binding": {
      "adapter": "hermes",
      "profile_ref": "<fresh-profile-reference>",
      "model": "<explicit-model>",
      "workspace_ref": "none",
      "credential_refs": ["<opaque-selected-credential-reference>"],
      "options": {}
    }
  }],
  "executor": {
    "image": "<immutable-worker-image>",
    "validated_digest": "<same-immutable-worker-image>",
    "network": "<fresh-private-runtime-network-name>",
    "docker_binary": "/usr/bin/docker"
  }
}
```

On Docker Desktop use canonical `/private/tmp/...` host paths, not the `/tmp`
symlink alias. Discover the daemon socket supplemental GID **inside a Linux
container with that socket mounted**; the macOS socket/symlink group may differ
from the Desktop VM socket group. Then verify the controller's Docker CLI
`version --format '{{.Server.Version}}'` under UID 10001 plus that group. Check
that UID 10001 can write the newly created fixture parent inside the container
before starting the harness. Only an explicitly new empty parent may receive
bootstrap ownership changes; the check itself never adopts or chowns old paths.

The daemon socket supplemental GID is operator-supplied from the selected
mounted socket's actual ownership. It grants the trusted controller Docker authority;
no worker inherits that group or socket. The runtime network is an explicitly
created private network, never `host`, `bridge`, or `none`. Real provider use
requires separately approved infrastructure egress. For the fixture check use
an internal Docker network with no provider egress.

## Container composition with native host control services

The CLI defaults to `127.0.0.1`. A container controller explicitly supplies
`--listen-address 0.0.0.0`, then publishes **only** host 127.0.0.1. The native
Rust adapter connects to `http://127.0.0.1:8650`. Bearer authentication remains
mandatory for every endpoint. The executable configuration alone never enables
runs; `--enable-validated-docker-executor` is a separate opt-in.

This service fragment is for the root task's fresh private stack. It does not
create or adopt existing company/employee resources:

```yaml
services:
  hermes-bridge:
    image: ${ORTAK_BRIDGE_CONTROLLER_IMAGE:?immutable controller image required}
    init: true
    user: "10001:10001"
    group_add: ["${ORTAK_DOCKER_SOCKET_GID:?socket GID required}"]
    read_only: true
    cap_drop: [ALL]
    security_opt: [no-new-privileges:true]
    pids_limit: 128
    mem_limit: 512m
    cpus: 1.0
    tmpfs: ['/tmp:rw,noexec,nosuid,size=67108864']
    labels:
      org.ortak.controller.company: ${ORTAK_COMPANY_ID:?fresh company required}
    ports: ["127.0.0.1:8650:8650"]
    networks: [bridge-control]
    volumes:
      - ${ORTAK_PRIVATE_ROOT:?}/state:${ORTAK_PRIVATE_ROOT:?}/state
      - ${ORTAK_PRIVATE_ROOT:?}/profiles:${ORTAK_PRIVATE_ROOT:?}/profiles:ro
      - ${ORTAK_PRIVATE_ROOT:?}/controller:/run/controller:ro
      - /var/run/docker.sock:/var/run/docker.sock:ro
    command:
      - --config
      - /run/controller/config.json
      - --token-file
      - /run/controller/service-token
      - --journal
      - ${ORTAK_PRIVATE_ROOT:?}/state/journal.sqlite
      - --listen-address
      - 0.0.0.0
      - --enable-validated-docker-executor
networks:
  bridge-control:
    internal: true
```

For foundation-only inspection, omit the execution opt-in: `run_start` remains
absent. Do not publish 8650 on all host interfaces or reuse the old profile
stack. Local file inspection is not remote credential/model health; the actual
selected profile smoke remains an independent activation gate.

## Credential-free real containment check

`checks/containment_check.py` invokes the production `Bridge`, `Journal`,
`DockerExecutor` and real `DockerEngine` on the **exact worker image**. The only
injected seam replaces the worker's module command with fixed Python probe
code. All production container ownership labels, entrypoint, UID, readonly
mounts, capabilities, limits, network and stdin transport remain unchanged.
The probe never imports Hermes, reads a provider token, or calls a model.

Run the check as UID 10001 in the patched controller image, with the selected
Docker socket group, no controller config or token, and the fresh checks parent
mounted at its identical host path. `--entrypoint python` overrides the service
entrypoint only for this explicit check:

```sh
docker run --rm --init --read-only --cap-drop ALL \
  --security-opt no-new-privileges --pids-limit 128 --memory 512m \
  --user 10001:10001 --group-add "$ORTAK_DOCKER_SOCKET_GID" \
  --network none --tmpfs /tmp:rw,noexec,nosuid,size=67108864 \
  --mount type=bind,src=/var/run/docker.sock,dst=/var/run/docker.sock,readonly \
  --mount "type=bind,src=$ORTAK_PRIVATE_ROOT/checks,dst=$ORTAK_PRIVATE_ROOT/checks" \
  --entrypoint python "$ORTAK_BRIDGE_CONTROLLER_IMAGE" \
  /opt/bridge/checks/containment_check.py \
  --image "$ORTAK_HERMES_WORKER_IMAGE" \
  --network "$ORTAK_FIXTURE_RUNTIME_NETWORK" \
  --fixture-parent "$ORTAK_PRIVATE_ROOT/checks"
```

The check creates a new random fixture directory/company/run set and leaves
its journal as evidence. It verifies readonly root/profile paths, UID 10001,
no effective capabilities, no-new-privileges, no Docker binary/socket/controller
config in the worker, and termination of a descendant that starts a separate
session. It covers running, failed-but-still-live and completed-but-still-live
cancellation, lost start receipt lookup, cancel-before-start tombstones, a
SIGKILLed controller followed by inventory-based recovery, no blind rerun,
one-event durable cursor pagination, and the production kernel deadline firing
while its controller remains absent. Docker `--init` owns namespace PID1 so the
worker default SIGALRM reliably terminates its container and descendants. Every fixture removal uses the production
exact ownership/image guard. A failed cleanup fails the check.

The output explicitly identifies this as a production executor probe. It is
not evidence of a real model response, selected provider health, approval
resume, nonempty tool permission enforcement, or deployment. Root must record
both this receipt and a later separately authorized selected-profile smoke.


## Real run-loop check with provider I/O fixtures

`checks/run_loop_check.py` is a separate image-only check. It imports the real
pinned AIAgent, executes its real constructor and `run_conversation`, and uses
the production bridge persistence path. Only the two actual provider request
methods return fixed Responses objects. It tests a final response and a forged
tool call, requiring exactly one fixture response in each case. The forged call
must persist policy denial before Hermes's unknown-tool correction/retry path.
Network and subprocess attempts remain fatal. This is not a provider response.

```sh
docker run --rm --init --network none --read-only --cap-drop ALL \
  --security-opt no-new-privileges --pids-limit 64 --memory 1g \
  --user 10001:10001 --tmpfs /tmp:rw,noexec,nosuid,size=134217728 \
  --mount "type=bind,src=$ORTAK_SOURCE_ROOT/runtime/hermes-bridge/checks/run_loop_check.py,dst=/opt/ortak-run-loop-check.py,readonly" \
  --entrypoint python "$ORTAK_HERMES_WORKER_IMAGE" /opt/ortak-run-loop-check.py
```

No profile, provider token, daemon socket, or controller config is mounted for
this check. The worker identity is taken from actual `docker image inspect`,
not a config hash printed during build. A later source/guard change requires a
new immutable worker artifact and rerunning both the constructor and loop gates.


Current worker receipt from the integration owner: the Docker-inspected image
and retained OCI layout `/private/tmp/ortak-hermes-worker-oci` agree on
`sha256:623fae9e3b38c75bc3cb94f73bc3d1c303bc3ed6a77765eb51fc17b54cc90b18`.
This 17-file source-locked candidate passed the strict constructor and real
run-loop fixture checks. Its controller/containment and selected-provider
receipts remain separate; it is not a deployed artifact pin.


Actual containment receipt: controller
`sha256:ef9a9d2a7446d9e13cdbf94cf1a2152011b5a72050e450d500356f059852d7b1`
passed all eight lifecycle/descendant/recovery/deadline checks with the final
worker 623fae identity above, SQLite 3.53.4 and zero provider calls. All fixture
containers were confirmed cleaned up. Evidence remains under
`/private/tmp/ortak-hermes-checks-20260905/containment-381ebc6a-210b-4730-b2f5-d47e57e42094`.
The earlier attestation-bearing index was superseded by the exact successful
export recipe; the matching inspected Docker/OCI digest above is authoritative.


## Real recovery-only HTTP check

`checks/http_recovery_check.py` launches the actual production controller CLI
in two separate processes under the patched image interpreter. It supplies a
fresh random company and a registry entry with zero credentials, omits the
Docker execution opt-in, and mounts no Docker socket or provider profile. The
only credential it creates is a random disposable service bearer, kept in a
mode-0600 temporary file and never printed.

The check uses authenticated loopback HTTP with bounded reads/timeouts. It
verifies authentication, recovery-only capabilities, honest unhealthy profile
inspection, lookup without admission, unavailable starts without reservations,
company scoping, cancellation before start, delayed-start refusal by tombstone,
and cursor bounds. It then SIGKILLs and reaps the first controller and starts a
second production process against the same SQLite journal, requiring the same
receipt and byte-equivalent decoded replay with no duplicate terminal event.
A success receipt is JSON containing only fixed check names and SQLite version.
This tests the real Python HTTP/CLI/recovery path; it does not exercise the Rust
HTTP client, provider health, Hermes execution or container stop ownership.

Run against the already inspected controller identity; the script can be
mounted read-only, so this additional check needs no artifact rebuild:

```sh
docker run --rm --init --network none --read-only --cap-drop ALL \
  --security-opt no-new-privileges --pids-limit 32 --memory 256m \
  --user 10001:10001 --tmpfs /tmp:rw,noexec,nosuid,size=67108864 \
  --mount "type=bind,src=$ORTAK_SOURCE_ROOT/runtime/hermes-bridge/checks/http_recovery_check.py,dst=/opt/ortak-http-recovery-check.py,readonly" \
  --entrypoint python "$ORTAK_BRIDGE_CONTROLLER_IMAGE" /opt/ortak-http-recovery-check.py
```

Loopback inside `--network none` is sufficient. No ports are published, no
external network or Unix daemon socket is available, and temporary files are
removed when the harness finishes. Root records the actual run receipt; source
compilation alone is not a passed HTTP check.


The integration owner subsequently ran this exact HTTP recovery harness on
2026-09-05: all 12 checks passed, using SQLite 3.53.4 and two real production
service processes with the first forcibly terminated by SIGKILL. Provider
calls were zero and no Docker socket was mounted. This is now an actual
Python HTTP/CLI/restart receipt; the Rust adapter and live provider remain
separate integration seams.

## Latest private E2 artifact checkpoint (2026-09-05)

This checkpoint supersedes earlier candidate artifact identities above. The
Hermes source remains the reviewed22-file lock at
`29112bef099274229cadff79cdff7bf7b99c4b77`.

- Worker: `sha256:baf828b237502da6bfdde3cd598d32b4f4f87979adbc64ee6d3fe0b548b9d79c`.
- Controller: `sha256:090758781ef2ed301556d89dbb6f13394dbe310d13e52328f5194fe3a73520f0`.
- Constructor, real Office run-loop and Codex I/O fixtures passed. A separate
  real-AIAgent Work fixture retained exact assistant text with silent Office
  intent and preserved the forged-tool denial. HTTP recovery12 and actual
  Docker containment8 passed. Those checks made0 provider calls.
- Root subsequently deployed these exact artifacts using the existing fresh
  company/profile/OAuth selection and journal. The old controller is retained
  stopped; its verified stopped-journal backup contains8 runs/22 events/2 probes.
  Worker PID6260/session28241 resumed through a frozen public launcher.
- New selected-profile health probe `de59d162-04d2-4563-8e10-0649e6c1ca89` made
  an actual Codex OAuth request and completed with exactly `OK`. This health
  witness expires normally; it is not permanent provider-health authority.
- Native100-word Office run `2d191b5b-1f92-450d-96a5-2fb444348d5f` still failed
  `provider_failed`. Its message was accepted through the corrected native
  central-routing mention path. General useful-response acceptance remains open;
  safe bounded exception-boundary diagnostics are being investigated.

Artifact checks: `/private/tmp/ortak-v0-evidence/hermes-e2-checks-a873cd8065d847bdaa71349dbad06ead`
and `/private/tmp/ortak-v0-evidence/hermes-e2-containment-93ce272df8554b8396735a3cfa57137b`.
Rollout: `/private/tmp/ortak-private-20260905/rollouts/hermes-e2-38c4035768844a9aac1e55c7f672a457`.
The earlier source/binary snapshots and failed attempts remain retained.
