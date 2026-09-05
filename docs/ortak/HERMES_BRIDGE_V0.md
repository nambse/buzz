# Hermes bridge v0 foundation

Status: durable foundation, real constructor, and real run-loop fixture gates
passed. Real container containment validation also passed; selected-provider
health and an actual model response are not demonstrated.

The stdlib Python package in `runtime/hermes-bridge` implements the protocol
consumed by `ortak-runtime::HermesAdapter`. It does not use the old Hermes
in-memory SSE queue and never subscribes employees independently to Office.
The default service advertises no `run_start` capability and returns unhealthy
profile inspection. These are deliberate activation failures until the real
contained executor is reviewed and exercised.

## Wire contract

Every endpoint requires one bearer credential, supplied to the process through
a mounted secret file. The service is fixed to one company. Profiles are an
operator-owned exact binding registry, including employee identity; client paths,
model options and credential references cannot select unregistered resources.
Plaintext defaults to loopback. An explicit container-only wildcard bind must
be paired with host-loopback publishing; see the controller recipe below.
No access/request logging is enabled.

- `GET /v1/capabilities`: adapter `hermes`, API `ortak-hermes-bridge/v1`, actual
  capabilities. `run_start` is absent with the shipping unavailable executor.
- `POST /v1/profiles/inspect` with `{company_id,binding}` returns
  `{profile_ref,healthy}`. Inspection does not create or modify a profile.
- `POST /v1/runs` with `{company_id,spec}` accepts the B2 `RunSpec` fields.
  Receipt: `{runtime_run_ref,started_at,status}`; extra `status` is informational.
- `POST /v1/runs/lookup` with `{company_id,run_id,idempotency_key}` returns the
  same receipt or 404. It never reserves, starts, retries or resumes work.
- `POST /v1/runs/cancel` takes the lookup body plus `reason`; response is
  `{runtime_run_ref,outcome}` with `cancelled` or `already_terminal` only after
  the executor confirms it stopped. Failure keeps a durable `cancelling` record.
  Control-plane audit owns the submitted reason; bridge persistence uses a
  fixed secret-free reason rather than storing arbitrary client reason text.
- `GET /v1/runs/{runtime_run_ref}/events?after=N&limit=100` returns
  `{events:[{cursor,occurred_at,payload}],terminal}`. Payloads use the canonical
  `RunEventPayload` `event_type` tag. Cursor strings are dense positive integers;
  `after` is exclusive, and `terminal` is true only on the final page. Cursor
  ahead, negative cursor, duplicate query fields and excessive limits fail.

The only start key is `ortak-run:{company_uuid}:{run_uuid}`. The only reference
is `ortak:{company_uuid}:{run_uuid}`. Canonical UUID spelling and identity match
are enforced; these values are never filesystem paths. A repeated key with a
changed spec conflicts. A prior cancellation with no start creates a permanent
tombstone: any late start returns the cancelled identity without execution.

## Persistence and execution seam

SQLite uses WAL, `synchronous=FULL`, a three-second busy timeout and short
`BEGIN IMMEDIATE` transactions. It has no memory fallback, expiration or
implicit retention purge. Capacity is capped at 100,000 run identities and 512
events per run; exhaustion fails closed and requires an explicit archive design
that preserves all dedup/tombstone identities. It must live on a durable local
filesystem supporting SQLite locking, not a network share.

The start identity and spec fingerprint commit before executor invocation.
No raw input, credentials or runtime exception messages enter the registry.
The executor calls `begin_execution` at the actual execution boundary, then
commits a bounded redacted output, delivery intent and completion in one
transaction at execution return. The first slice retains final output, not
streaming tokens or hidden reasoning. A partial provider response is not a
successful Office reply. Tool denial is recorded synchronously at the tool
entry before throwing a fatal exception. This is execution-side capture, not
a downstream SSE logger.

`recover(assert_stopped)` requires positive containment evidence for every
unsettled identity. Once evidence succeeds, interrupted work becomes failed;
it is never blindly rerun. Cancelling work becomes cancelled only after stop
proof. The default executor cannot prove prior work stopped and therefore does
not auto-seal old records on startup.

## Hermes candidate and unresolved activation gate

`hermes_candidate.py` contains the direct `AIAgent` call and denial subclass for
compatibility inspection at candidate release revision
`29112bef099274229cadff79cdff7bf7b99c4b77`; it does not import desktop Hermes.
The source reviewed is the official pinned
[run_agent.py](https://github.com/NousResearch/hermes-agent/blob/29112bef099274229cadff79cdff7bf7b99c4b77/run_agent.py),
especially constructor controls and tool dispatch near lines 7897–8019.
This is not an accepted deployment pin or a full source audit. The current test
image's exact source remains unknown, as recorded in runtime discovery.

The candidate accepts only an explicitly empty four-field `PermissionPolicy`.
Any tool/workspace/network grant or approval-resume policy is unsupported.
It overrides the inspected batch, sequential, concurrent, single-tool and
subagent dispatch entries before construction, and refuses a missing boundary
or non-empty/unrecognized tool-definition field. Actual selected-image tests
must still prove these hooks bind all production dispatch paths. Memory,
background review, context files, soul loading, checkpoints and trajectories
are disabled; server-side typed reply delivery remains outside Hermes tools.

A real contained executor is still required: digest-pinned image, fresh exact
company/employee profile binding, credential isolation, bounded process tree,
post-start revocation and stop confirmation, execution journal at the child
boundary, and restart reconciliation. No existing Cem/Zeynep profile, secret,
gateway, memory or service was read or changed by this implementation. No live
model call was made. Root capabilities alone cannot establish profile health.

## Local verification and launch

Use Python **3.11 or newer** for these local commands (`python3 --version`).
Source verification uses `hashlib.file_digest`, which is unavailable in older
macOS system Python installations. The contained image pins Python 3.13;
local fixture tests do not replace that image's validation.

```sh
PYTHONPATH=runtime/hermes-bridge python3 -m unittest discover -s runtime/hermes-bridge/tests -v
python3 -m compileall -q runtime/hermes-bridge/ortak_hermes_bridge
```

18 production-seam tests passed on 2026-09-05: concurrent reservations, stable
lookup, payload conflict, delayed start tombstones, cancellation on both sides
of execution admission, stop-before-ack, atomic terminal rollback, dense replay
and final-page terminal flags, restart refusal without stop evidence, failure
instead of rerun, redaction, scoped bindings and policy refusal, truthful
unavailable capabilities, and every candidate denial entry. The injected class
test verifies call ordering, not actual Hermes model or container behavior.

To inspect the unavailable protocol locally, supply a private config with
`company_id` and `profiles:[{employee_id,binding}]`, then run:

```sh
PYTHONPATH=runtime/hermes-bridge python3 -m ortak_hermes_bridge \
  --config /path/to/private/config.json --token-file /path/to/mounted/token \
  --journal /path/to/dedicated/state/journal.sqlite --port 8650
```

No sample credential value is supplied. This launch is a protocol foundation,
not a usable employee runtime; activation must fail while `run_start` is absent.

## Contained executor candidate (still disabled by default)

`docker_executor.py` and `worker.py` now implement the next bounded layer.
The default CLI still selects `UnavailableExecutor`. The integration owner must
explicitly construct `DockerExecutor(..., validated_digest=the_exact_image)`
only after validating that immutable artifact; this parameter records a
release-gate decision and is not itself evidence that the gate passed.

The engine accepts only `repository@sha256:<64 hex>` images, checks the
`org.ortak.hermes.revision` image label, and refuses default/host networking.
Each run starts a named container with company/start-key ownership labels,
`--entrypoint python` (never upstream `/init`), read-only root filesystem,
capabilities dropped, no new privileges, PID/memory/CPU limits and disabled
Docker logs. CLI output goes to `/dev/null`; bounded inspect commands retain
at most 1,024 bytes while reading and have a five-second deadline. Payload is
supplied by anonymous stdin storage, never an argument or request log.

Exact child command:

```text
python -m ortak_hermes_bridge.worker --journal /ortak-state/journal.sqlite
```

Stdin is the same `{company_id,spec}` body accepted by the service. The journal
filename may differ; its parent is always `/ortak-state`. The service's dedicated
state directory mounts there read/write. UID/GID is 10001:10001; the deployment
must pre-provision state permissions for that UID. The selected disposable
profile directory is mounted **read-only** at `/profile`, containing exactly:

- `ORTAK_DISPOSABLE_PROFILE.json`: `{company_id,employee_id,profile_ref}`.
- `ORTAK_RUNTIME_BINDING.json`: the exact server-owned `RuntimeBinding`.
- `ORTAK_PROVIDER.json`: `{provider,credential_ref}` matching its sole binding
  credential reference. This candidate supports API-key `openai`/`openrouter`.
- `provider-token`: the selected profile's mounted credential, bounded to 4,096
  bytes. There is no credential-copy or old-profile adoption operation.

Configuration files must be regular, not symlinks, owned like the profile root,
bounded, and agree with the exact employee/company/binding. Unexpected files,
including `.env`, `config.yaml`, `auth.json` or MCP configuration, are refused.
OpenAI Codex OAuth is not wired by this worker. A locally valid token file is
not remote-provider authorization proof; a real profile smoke remains required.

Each child creates a new `/tmp/hermes-home`; `HOME` and `HERMES_HOME` point
there, not at the mounted profile. The worker writes only a fixed bootstrap
config disabling environment probing and lazy dependency installation, then
imports Hermes with a sanitized environment. No selected-profile config loads.
It refuses `/opt/hermes/.env`, verifies `/opt/hermes/ORTAK_SOURCE_REVISION`, and
requires `AIAgent` to load from `/opt/hermes`. The wrapper image must already
contain that source and package; it never downloads or upgrades at execution.
The SQLite admission gate commits before the lazy third-party import.

Container capacity is four active runs, with a 180-second supervisor ceiling
and a matching kernel SIGALRM in each worker. Docker `--init` owns namespace
PID1, allowing default-fatal SIGALRM to terminate the worker and its container
even when the controller is absent. This is distinct from the forbidden Hermes
upstream `/init` gateway entrypoint. Cancellation first seals the durable key, then validates the existing
container's exact company/start-key/image identity before force-removing it and
reaping the local Docker CLI. A daemon error never proves container absence.
**A failed or completed journal record is not stop evidence:** cancellation
still invokes containment for every actual start identity. A pure pre-start
cancellation tombstone never launched a process and can acknowledge directly.

The executor holds one exclusive file lock for its dedicated journal. Startup
inventories its labeled containers, including terminal journal records, refuses
unregistered or differently owned resources, tombstones unsettled work before
containment, and never reruns interrupted starts. Failed stop proof keeps the
executor unavailable. No old test-stack resources are targeted by this code.

30 focused local tests now pass, including the new production engine seam:
explicit entrypoint and limits, private per-run home, readonly selected profile,
no credential/input in argv, exclusive ownership, profile binding/token/file
refusal, terminal-container restart recovery, known-failed cancellation stop,
unknown/mismatched container preservation, daemon failure, and bounded output
capture before EOF. Engine operations use test doubles; no actual Docker
container, Hermes model, provider token or external profile was exercised.

## CLI handoff checkpoint — image build not started

The integration owner downloaded the actual official revision archive. Its
SHA-256 is `76b99a8be9b77d66833c3cfe2b35c6d6f6a58e4ff9637ef8effcfc1f420ab35a`.
`runtime/hermes-bridge/hermes-source-lock.json` records that receipt and hashes
of 12 relevant source/lock files. Verification against the extracted archive
passed. Local archive contents supersede earlier cached web line offsets:
`model_tools.py:427` distinguishes an empty toolset from defaults,
`agent/conversation_loop.py:7668` enters `_execute_tool_calls`, and the real
sequential/concurrent executor has direct handler branches, so all five denial
overrides remain necessary.

The new Dockerfile builds from the pinned uv/Python base already recorded in
upstream's Dockerfile, compiles checksum-pinned SQLite 3.53.4 to avoid the
reported SQLite WAL-reset bug, installs core dependencies with
`uv sync --python /usr/local/bin/python --frozen --no-dev --no-install-project`, verifies actual source-file
hashes, and runs `candidate_smoke` with networking disabled. That smoke invokes
the **real** AIAgent constructor and all denial boundaries, and rejects network
or subprocess attempts. It has been authored but **has not run**: no Docker
build or container was started before the requested CLI pause.

Candidate build command after the CLI handoff (paths identify retained local
artifacts, not a deployed service):

```sh
docker buildx build --load \
  --build-context hermes_source=/private/tmp/ortak-hermes-source-29112bef/hermes-agent-29112bef099274229cadff79cdff7bf7b99c4b77 \
  -f runtime/hermes-bridge/Dockerfile \
  -t ortak-hermes-candidate:29112bef runtime/hermes-bridge
```

The local tag is a build handle. Execution accepts only the immutable resulting
`sha256:<image-id>` or a registry `repository@sha256:<digest>`; a floating tag
cannot activate it. Record build output and real-class guard results before
selecting the digest. A build success alone is not the required live
provider/cancellation/restart smoke.

The CLI now supports explicit `--enable-validated-docker-executor`, default off.
Private config adds `executor:{image,network,validated_digest,docker_binary}`;
the last field is optional and defaults to `/usr/bin/docker`. Config by itself
never enables execution. The service still binds plaintext only to loopback.
The controller must have its own Docker CLI/daemon access and use the fixed
SQLite library; this minimal worker image does not include controller packaging.
The service CLI refuses SQLite versions older than 3.51.3. Worker and guard
smoke sanitize inherited environment while retaining only the fixed
`LD_LIBRARY_PATH=/opt/sqlite-fixed/lib`, alongside fixed home/locale/PATH values.
SQLite is also imported before sanitization, so the patched library is already
loaded. No Docker socket or daemon access is mounted into an execution child.

Final local verification before pause: **37 tests passed**, Python compilation
passed, and all 12 actual source hashes verified. Additional tests prove source
byte tampering cannot be hidden behind a revision marker, source `.env` and
symlinks are refused, config alone cannot enable execution, invalid company
configuration never creates an executor, and missing validation fails closed.
No active child, background build, network operation, or test process remains
from this lane. No commit was made; integration review and checkpoint commit
remain with the root task.


## Resumed image validation and controller composition

The root task exercised the real Docker build after the CLI pause. The pinned
Python 3.13 base requires explicit `uv sync --python /usr/local/bin/python`,
because the archive's `.python-version` selects 3.11. The actual pinned source
supports 3.13; downloads remain disabled. The real constructor smoke exposed
three initialization paths that the earlier injected-class tests could not:

1. `agent_init.py` enters direct credential construction only when **both**
   `api_key` and `base_url` are supplied. Production and smoke now share
   `agent_constructor_kwargs`, selecting the fixed OpenAI or OpenRouter API
   endpoint with the selected key. Ambient provider routing and OAuth aliases
   are not used. The smoke checks that the real constructor retained both.
2. An unconditional optional Bedrock import attempts lazy dependency installs.
   Production and smoke share environment isolation with
   `HERMES_DISABLE_LAZY_INSTALLS=1` and no `HERMES_LAZY_INSTALL_TARGET`, plus the
   fixed `security.allow_lazy_installs:false` bootstrap config.
3. The default environment probe starts subprocesses in a background thread.
   The new exclusive temporary home contains fixed
   `agent.environment_probe:false`; the real smoke asserts it is disabled.

`tools/lazy_deps.py` and `tools/env_probe.py` are now hash-locked alongside the
12 original seams. All **14 actual source hashes** verify. No guard is relaxed
for optional startup effects. The real constructor's no-network/no-process
smoke still needs to pass on the resulting immutable image before acceptance;
remote provider health and a real model response have not been demonstrated by
this lane. OpenRouter additionally prewarms metadata; the constructor smoke
currently exercises the OpenAI API-key route.

`runtime/hermes-bridge/CONTROLLER.md` provides a concrete separate controller
image and native-host service composition. `Dockerfile.controller` requires
explicit immutable worker and Docker CLI artifacts, retains the fixed SQLite,
and adds Docker only to the controller derivative. The service's new
`--listen-address` defaults to 127.0.0.1; 0.0.0.0 is an explicit option for a
container published only on host 127.0.0.1. Every route still requires bearer
authentication. Identical absolute state/profile mount paths preserve the
host daemon's child-bind semantics. The child gets no daemon socket or service
config/token. The original worker identity remains the executor's image.

The credential-free `checks/containment_check.py` binds the real production
Docker owner and exact worker image, substituting only a fixed probe command.
It tests cancellation of a descendant in a separate session, known-terminal
but still live containment, pre-start tombstones, lost receipt lookup,
controller SIGKILL and restart without rerun, kernel deadline after controller
loss, and dense ordered replay. It creates only fresh random fixture resources
and retains their journals. It has been authored and syntax-checked; the root
task must record the actual Docker check receipt separately. Its probe makes
zero Hermes imports or provider calls and does not substitute for a later real
model smoke.

Latest focused local validation: **45 tests passed**, compilation passed,
14 actual source hashes verified. A real harmless child process confirms the
production kernel timer exits via SIGALRM; root's container harness must prove
the namespace and descendant case. All owned files are saved, with no commit,
provider call, old-profile access, or active process from this lane.


The first real worker image subsequently passed the strict constructor and all
five execution-entry guards, with SQLite 3.53.4 and 14 source hashes. Its real
run-loop fixture passed ordinary output but exposed an earlier path: Hermes
corrected an unadvertised tool name and retried before execution dispatch.
`ToollessTransport` now wraps the real per-agent transport and durably denies
raw Responses call items or Chat tool/function calls before validation, plus
normalized tool calls before correction. The original execution guards remain.
The real codex/chat transport and registry files are now locked: **17 source
hashes verify**, and **46 local tests pass**. The worker must be rebuilt and the
real run-loop fixture rerun for this new boundary; the earlier constructor-only
artifact is not a final execution pin.

Controller local-image resolution on Docker Desktop required another concrete
packaging change: `Dockerfile.controller` consumes the mandatory named
`worker_image` OCI-layout context selected by digest. The root task can export
the reviewed worker without publishing to a registry. `WORKER_IMAGE` remains a
mandatory immutable provenance argument and must agree with that selected OCI
context; the actual Docker-inspected worker identity still drives execution.
The controller recipe records the exact export/build and both container checks,
including Docker Desktop's inside-container socket GID and canonical bind paths.


## Verified worker candidate receipt

The integration owner rebuilt the transport-guard revision and recorded both
Docker's inspected image identity and the selected OCI manifest as:

`sha256:623fae9e3b38c75bc3cb94f73bc3d1c303bc3ed6a77765eb51fc17b54cc90b18`

The retained OCI layout is `/private/tmp/ortak-hermes-worker-oci`. The strict
network-disabled real AIAgent constructor and five execution-entry denial
checks passed with SQLite 3.53.4 and 17 verified source files. The separate real
run-loop check also passed: ordinary final output committed through the bridge,
a forged tool response persisted policy denial before correction, exactly two
fixed fixture responses were consumed across those cases, and zero provider
requests or network calls occurred. This is real Hermes constructor/loop
compatibility evidence with provider I/O fixtures, not a real model response.

The controller derivative is being built from this exact OCI context. Its
real Docker lifecycle/descendant/restart/deadline check remains a separate gate.
This candidate has not become a deployed Hermes revision or authorized old
employee adoption. No earlier build-log config hash is an authoritative image
identity; the inspected and OCI-agreed digest above supersedes earlier build
handles for the current source revision.


The final matching Docker/OCI identity above uses an explicit provenance-free
simultaneous image and OCI export. Earlier attestation-bearing index/config
values were superseded. The controller image is
`sha256:ef9a9d2a7446d9e13cdbf94cf1a2152011b5a72050e450d500356f059852d7b1`.
The integration owner ran the real Docker containment harness against those
exact artifacts. All eight checks passed: stop running, stop failed, stop
completed, cancel before start, controller SIGKILL recovery, restart without
blind rerun, dense durable replay, and child deadline without the controller.
SQLite was 3.53.4; provider calls were zero. All fixture containers were confirmed
stopped/removed. The retained evidence directory is
`/private/tmp/ortak-hermes-checks-20260905/containment-381ebc6a-210b-4730-b2f5-d47e57e42094`.
This establishes actual process containment and recovery; selected-provider
health, actual model response, product activation and deployment remain
independent gates.


The recovery-only HTTP gate also passed against the real controller artifact:
12 authentication, scope, unavailable-start, tombstone, receipt and replay
checks; SQLite 3.53.4; two production CLI processes separated by confirmed
SIGKILL/reap; zero provider calls and no Docker socket. The first process's
durable cancellation and receipt were unchanged through the second process's
live HTTP endpoint. This check deliberately omitted the execution opt-in and
reported the fixture profile unhealthy. It verifies the actual Python HTTP and
startup path, not the Rust client's HTTP transport or selected-provider health.
