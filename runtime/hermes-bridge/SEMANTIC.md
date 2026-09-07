# Explicit central Codex OAuth scoring

This is the transport/selection slice of
[`SEMANTIC_OAUTH_COMPOSITION_D3_PLAN.md`](../../docs/ortak/SEMANTIC_OAUTH_COMPOSITION_D3_PLAN.md).
It is off until the operator starts this separate service and selects it in the
central worker. It creates no employee run, Office reply, memory record, local
execution child or score-job journal. Current routing policy remains the only
authority to wake employees.

The service uses the pinned Hermes Codex format conversion, required identity
headers and response normalizer with one cancellable HTTPX request. It never
constructs `AIAgent` or enters its retry/run loop. Its only upstream endpoint is
`https://chatgpt.com/backend-api/codex/responses`. The inspected Codex transport
omits `max_output_tokens` on this route; the service therefore bounds raw SSE to
64 KiB, 512 events and 4.5 seconds instead of claiming a provider token ceiling.
The Rust control service still owns a maximum five-second total budget across
revalidation attempts. Malformed, incomplete, foreign-model, tool-intent or late
results fail closed. A single permitted evidence label accompanies each score.

The selected endpoint was observed on 2026-09-06 returning HTTP 200 without
`Content-Type` or `Content-Encoding`, while its bounded body contained valid
Responses SSE (`response.created`, then `response.in_progress`). The retained
diagnostic is `D/native-routing/provider-diagnostic3/receipt.json`; it establishes
that response shape, not a completed score or an intermediary's behavior.
Missing `Content-Type` therefore enters the existing strict Responses parser.
An explicitly different or empty media type, or any explicit encoding other
than `identity`, remains refused before reading the body. Missing headers never
turn HTML, malformed or incomplete data into success: raw-byte/event limits,
completed response/model/tool/score validation and the original deadline all
remain required. No provider/model/header impersonation or retry change is made.
The first native routing failure remains historical; this source correction
does not rewrite its decision or establish subsequent live quality acceptance.

## Separate service selection

Use a newly built, immutable reviewed worker image containing
`ortak_hermes_bridge.semantic` and `checks/semantic_transport_check.py`.
It does not need a Docker socket or a runtime journal mount. The scoring process
must be separate from the employee controller's single HTTP handler, so scoring
cannot occupy the cancellation/lookup lane.

Provide a private server-owned JSON configuration with exactly these root keys:

```json
{
  "company_id": "<company-uuid>",
  "profiles": [
    {
      "employee_id": "<existing-owned-identity>",
      "binding": "<the complete existing public binding object>",
      "oauth_directory": "<exact existing private OAuth directory>"
    }
  ],
  "semantic": {
    "deployment_id": "<new-scoring-deployment-uuid>",
    "binding_sha256": "<sha256 of sorted compact full binding JSON>",
    "response_model": "<exact expected provider response model>"
  }
}
```

The `binding` placeholder above is an object in an actual configuration. Model
and `options.reasoning_effort` are read from that exact registered binding.
Selection refuses unknown variants before opening any OAuth store. Reusing the
already enrolled identity is explicit: its company/employee/profile/credential
ownership marker must still match. No profile is copied, created or relabeled.

Run as the existing private UID, with `--init`, an empty tmpfs home, the fixed
provider network and only the explicitly selected OAuth directory's private
parent mounted writable. OAuth refresh uses atomic file replacement in that
same owned store. Mount the public service configuration read-only, and a
distinct 0600 service bearer-token file. No secret belongs in command arguments:

```sh
python -m ortak_hermes_bridge.semantic \
  --config /private/semantic-config.json \
  --token-file /private/semantic-service-token \
  --port 8651 --listen-address 0.0.0.0 \
  --enable-selected-semantic-oauth
```

The existing central worker's optional `semantic` selection becomes:

```json
{
  "adapter": "hermes-codex",
  "deployment": {
    "deployment_id": "<same-scoring-deployment-uuid>",
    "origin": "<fixed HTTPS or literal-loopback service origin>",
    "model": "<exact registered model>",
    "response_model": "<same exact expected response model>",
    "reasoning_effort": "<exact registered effort>",
    "binding_sha256": "<same full binding hash>",
    "bridge_token_ref": "credential://<opaque service reference>"
  },
  "bridge_token_env": "<exact selected worker environment name>"
}
```

The old `{deployment,token_env}` Chat Completions selection remains compatible.
No absent/invalid selection falls back to another model, provider or credential.

## Credential maintenance, status and G capture

Explicit service enablement also starts one owned credential-maintenance thread
for this one OAuth store. It checks every 15 seconds and refreshes ahead of
expiry using the existing `OAuthStore` lock, durable `refreshing` fence,
generation, retry-at and uncertainty/relogin states. The existing bounded
refresh subprocess has a 35-second deadline and exact reap. It receives tokens,
never message/candidate content. Ordinary scoring only performs a nonblocking
ready-token read; it cannot refresh or wait behind a refresh.

`GET /v1/semantic/status` requires the private bearer token and returns only the
deployment/binding fingerprint, whether the listener accepts work, active-score
count and **last observed** maintenance state. It performs no token read or
provider call and makes no model-health claim. There are at most four HTTP
connections and two scoring calls; capacity is refused immediately. Scores and
provider prose are not persisted by this listener.

For G drain/capture, stop this scoring listener in addition to the existing
worker/controller/native ingress. SIGTERM first closes admission, cancels and
joins all owned score tasks, and closes upstream HTTP streams. It then joins
the credential owner; configure a stop grace of at least 45 seconds. A failed
credential-owner join is a failed shutdown, not a drain acknowledgement.
Capture/restore the already owned OAuth directory and its existing phase and
generation with the same private backup policy as the employee controller.
There is no new PostgreSQL table or separate credential copy. A killed refresh
with an uncertain phase requires the existing explicit login recovery; never
replay a possibly consumed refresh token.

Do not claim a live capture is drained from `active_scores=0` alone: the listener
must be stopped and the credential owner gone. Startup status and token presence
do not establish provider model entitlement or relevance quality.

## Gates

Local production-protocol tests:

```sh
PYTHONPATH=runtime/hermes-bridge python3 -m unittest discover \
  -s runtime/hermes-bridge/tests -p 'test_semantic*.py'
```

The installed-artifact gate must run in the new image with no credentials and
no external network (the gate permits only its exact owned loopback listener):

```sh
python /opt/bridge/checks/semantic_transport_check.py
```

It retains the real pinned conversion/header/normalizer and actual HTTPX
request/stream path. Only the socket transport is synthetic. Five cases cover
Sol/high, Astra/max, tool refusal, one auth failure without retry, and timeout
closure. Four installed Python lifecycle cases additionally prove the four
connection admission cap under overflow, active-score shutdown, and maintenance
join after transport cleanup failure, and exact socket abort when a slow peer
does not complete graceful close. Output contains fixed metadata only. It
is not a real-provider gate.

Central Rust gates are `ortak_routing_semantic` tests under `tests::hermes::`
and control `every_rescore_receives_the_original_control_deadline`, plus the
existing semantic/deadline/current-authority PostgreSQL regressions. Root owns
Cargo/image builds and the separately authorized real-provider test. Deployed
quality, bounded multi-employee wakes and the zero-recipient decision UI remain
open until those actual acceptance steps succeed.
