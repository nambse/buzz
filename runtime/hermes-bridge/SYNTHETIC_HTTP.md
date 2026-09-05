# Synthetic real Hermes HTTP check

This opt-in check joins the authenticated production bridge HTTP handler,
SQLite journal, Docker executor, source-verified Hermes `AIAgent`, pinned OpenAI
SDK and the production empty-tool-policy guards. Its model server is a fixed
local **synthetic Responses fixture**, never a real model or provider health
proof. It does not activate an Employee, run the Ortak routing worker, publish
Office messages, or read deployment credentials. The fixed fixture token is
public test data and is not a provider credential.

The production endpoint configuration remains closed. A test-only child
launcher invokes the original `worker.load_hermes()`, subclasses the returned
real AIAgent, and redirects its constructor's base URL/API mode to a new
Docker DNS alias under `.api.openai.com.invalid`. The known-provider substring
selects Hermes' existing hosted-provider metadata branch, avoiding an irrelevant
Ollama `/api/show` probe against this fixture. The launcher still runs production
`worker.main()` and its journal admission, isolated home, source verification,
constructor guards, response prevalidation, durable delivery and terminal writes.
The test also replaces only the OpenAI SDK's OS-header metadata helper
`get_platform()` with `Linux`, after checking the actual `sys.platform`. Without
this test seam, Python 3.13's `platform.platform()` attempts a `uname` subprocess
for that optional header; the strict audit caught and blocked it. No SDK request,
stream, transport or response method is replaced, and no model response is
injected in memory. This seam is not a production runtime hardening fix.

The subprocess refusal comes from this fixture's additional audit hook. The
production worker does not install that hook; the observation establishes
neither a production SDK request failure nor an employee tool escape. No
production SDK monkeypatch was added. The existing tool-entry guards and
container limits remain the production boundary.

The controller runs with `--network none`; its real HTTP server/client communicate
only over container loopback and require a fresh bearer. Workers and the fixture
share a newly created, explicitly inspected **internal** Docker network with no
published ports. An additional test audit hook permits worker connections only to
the fixture IP/8080 and refuses subprocess launch. The fixture accepts only its
fixed token/model and the Responses path, rejects advertised tools, and counts
all methods. Two authenticated optional catalog GETs receive explicit 404, with no
fabricated model metadata or health claim. Request contents are never logged. The worker retains every production
container argument, limit, ownership label, readonly profile mount and stdin body.
Only the controller receives the selected Docker socket. All four profile files
are created within a new private UUID directory; no old profile or `.env` is read.

Checks:

- Wrong bearer rejected; actual profile inspection and validated executor present.
- Real SDK normal response produces ordered durable start/text/delivery/completion.
- A provider-forged terminal tool call is durably denied before correction/retry,
  with no delivery intent and only one fixture request.
- Cancelling an in-flight SDK request acknowledges only confirmed container stop.
- Repeated starts, lookup, one-event pages and terminal starts retain the same
  identity and never issue an additional fixture request.
- All three Responses requests are synthetic. Exactly two catalog lookups receive
  404; no other request, process launch or worker network destination is allowed. Cleanup verifies exact names/labels/images,
  including uncertain creation responses, then confirms absence.

The main check has a 180-second deadline plus a 60-second cleanup budget. Each
Docker command and captured response is bounded. The fixture also has its own
240-second kernel deadline; production children retain their 180-second deadline.
A failed check retains a failed receipt and its journal. No database or evidence
is deleted. Provider/network/container cleanup failure fails the check.

## Reproduce on the selected Docker Desktop daemon

Prerequisites: Python 3.11+ for local unit tests, the two exact locally available
images below, and Docker Desktop at the explicit socket. No image build/pull or
registry publication occurs. Use the reviewed source checkout for `checks`.
The Python preparation verifies that the selected new parent is empty before
changing its ownership. Never substitute an existing state/profile directory.

```sh
ORTAK_CHECK_SOURCE=/Users/nambse/.codex/worktrees/a5ed/ortak.dev/runtime/hermes-bridge/checks
ORTAK_CHECK_PARENT=$(mktemp -d /private/tmp/ortak-synthetic-http.XXXXXXXX)
ORTAK_WORKER_IMAGE=sha256:623fae9e3b38c75bc3cb94f73bc3d1c303bc3ed6a77765eb51fc17b54cc90b18
ORTAK_CONTROLLER_IMAGE=sha256:ef9a9d2a7446d9e13cdbf94cf1a2152011b5a72050e450d500356f059852d7b1
ORTAK_SOCKET_GID=$(env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  /usr/local/bin/docker --host unix:///Users/nambse/.docker/run/docker.sock \
  run --rm --pull=never --network none --read-only --cap-drop ALL --cap-add CHOWN \
  --security-opt no-new-privileges --user 0:0 --entrypoint python \
  --mount "type=bind,src=$ORTAK_CHECK_PARENT,dst=/fixture" \
  --mount type=bind,src=/Users/nambse/.docker/run/docker.sock,dst=/var/run/docker.sock,readonly \
  "$ORTAK_CONTROLLER_IMAGE" -c \
  'import os,pathlib; p=pathlib.Path("/fixture"); assert not list(p.iterdir()); os.chown(p,10001,10001); print(os.stat("/var/run/docker.sock").st_gid)')

env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  /usr/local/bin/docker --host unix:///Users/nambse/.docker/run/docker.sock \
  run --rm --init --pull=never --network none --read-only --cap-drop ALL \
  --security-opt no-new-privileges --pids-limit 128 --memory 512m \
  --user 10001:10001 --group-add "$ORTAK_SOCKET_GID" \
  --tmpfs /tmp:rw,noexec,nosuid,size=67108864 \
  --mount "type=bind,src=$ORTAK_CHECK_PARENT,dst=$ORTAK_CHECK_PARENT" \
  --mount "type=bind,src=$ORTAK_CHECK_SOURCE,dst=/opt/synthetic-checks,readonly" \
  --mount type=bind,src=/Users/nambse/.docker/run/docker.sock,dst=/var/run/docker.sock,readonly \
  --entrypoint python "$ORTAK_CONTROLLER_IMAGE" \
  /opt/synthetic-checks/synthetic_http_check.py \
  --image "$ORTAK_WORKER_IMAGE" --fixture-parent "$ORTAK_CHECK_PARENT"
```

The fixture parent remains UID10001/private. Inspect its evidence using the same
controller image with only that parent mounted readonly, UID10001 and network
none. The receipt includes the exact image/source pin, SQLite version, launcher
and fixture hashes, fresh company/network names, observed checks and cleanup proof.

## Validation receipt

On 2026-09-05 the final check passed against worker
`sha256:623fae9e3b38c75bc3cb94f73bc3d1c303bc3ed6a77765eb51fc17b54cc90b18`
and controller
`sha256:ef9a9d2a7446d9e13cdbf94cf1a2152011b5a72050e450d500356f059852d7b1`,
with SQLite **3.53.4**. All four gate groups passed. The fixture observed exactly
**three Responses requests plus two catalog 404s**, zero unknown requests and
zero external provider requests. No forbidden process/network audit event occurred.
Independent daemon inventory afterward found zero remaining run containers for
all nine retained attempt scopes, zero fixture providers and zero fixture networks.
No test process remains active.

Final protected receipt:
`/private/tmp/ortak-synthetic-http-7d5453416d8141ec95b13eb9fae722bf/synthetic-http-e345843400274bbe8444362a491035e8/receipt.json`.
The earlier receipt ending `d9d92adb95b54894add81d07c42e7a14` is superseded: its
counter did not yet include unsupported HTTP methods. Earlier failed attempts
are retained and document the metadata findings above; their cleanup also passed.

Eight provider-free unit tests passed in 0.002s, covering transport argument/stdin
preservation, qualified fixture endpoint selection, SDK response/stream shapes,
bounded loopback HTTP, unsupported-method accounting, authenticated catalog404,
exact resource ownership, and refusal on daemon failure or mismatched ownership.

Frozen source SHA256 values:

| File | SHA256 |
| --- | --- |
| `checks/synthetic_http_check.py` | `a3ba279f84aa6c511510ebb0c99598fa8e3c8732126739bcdf035aae19a23457` |
| `checks/synthetic_provider.py` | `f3682c46c935fee5174cc07c15469d80611516a853858df315042e5a643119a6` |
| `tests/test_synthetic_http_check.py` | `fd91eea0a8d5e46bec48f35634acb9bda13792bb81b2e1f68458691b56ad3ed3` |

This is synthetic execution-boundary evidence. Actual selected-provider health,
real model quality, activation, runtime routing,
and a signed Office roundtrip remain separate gates.
