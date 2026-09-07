# Central semantic routing through the selected Codex OAuth provider

Status: source review and proposed implementation boundary, 2026-09-06.
The original review performed no implementation, provider call, credential read
or deployment. Its approved transport is now implemented; see
[`SEMANTIC.md`](../../runtime/hermes-bridge/SEMANTIC.md) for the owned listener,
selection and installed-artifact gates. The subsequent message projection is
documented in [`ROUTING_DECISION_READ_D3.md`](ROUTING_DECISION_READ_D3.md).
Neither slice allocates a migration or changes the five-second routing budget.
Real-provider scoring quality and actual dispatch acceptance remain separate.

The remaining D contract is actual relevant, bounded, explainable semantic
wakes. The existing deterministic router and sealed scoring boundary provide
the policy machinery. The missing composition is a real selected OAuth scorer
and product access to its decisions, including decisions which create no run.

## Source seams observed at the initial review

| Source | Current behavior and implication |
|---|---|
| `crates/ortak-routing-semantic/src/{config,request,response,lib,state}.rs` | One explicit Chat Completions deployment, Bearer token, fixed `/v1/chat/completions`; strict JSON Schema request and local candidate validation. No Codex Responses/OAuth composition or effort selection. |
| `crates/ortak-server/src/worker_semantic.rs` | Optional default-off worker selection constructs only `ChatCompletionsScorer`. Invalid selection leaves routing silent and recovery available. Environment resolution currently precedes constructor validation; a new variant should validate all public selection fields first. |
| `crates/ortak-control/src/{semantic,service}.rs` | Only the control service constructs the company/message/policy/revision-pinned input. Remote scoring is outside transactions. The total budget across refresh attempts is clamped to five seconds and the inbox claim; a late return is rejected even when it blocks a future poll. |
| `crates/ortak-router/src/lib.rs` | Deterministic precedence; only untargeted human inputs enter semantics. Exact candidate coverage, finite scores, threshold, stable score/ID order, recipient cap and chain budget remain central. |
| `runtime/hermes-bridge/ortak_hermes_bridge/service.py` | Company-bound exact full-binding registry permits reviewed model/effort variants sharing the same owned OAuth identity. Existing HTTP service has one synchronous handler, so putting a slow score call in it would delay run cancellation and recovery. |
| `runtime/hermes-bridge/ortak_hermes_bridge/{oauth_credentials,docker_executor}.py` | OAuth ownership is company/employee/profile/credential-ref, independent of model/options. Refresh is durably fenced before its single-use request, with closed retry/uncertainty states. Read-only profile inspection requires a recent actual probe; explicit probe starts an ordinary contained inference. |
| `runtime/hermes-bridge/ortak_hermes_bridge/hermes_candidate.py` | Current employee path invokes a guarded `AIAgent`, with a 120-second run budget and normal Office/Work output materialization. It is not a score-only operation. |
| `runtime/hermes-bridge/checks/codex_sdk_fixture.py` | The actual pinned run-loop/SDK fixture observes a 1,800-second per-request timeout and three provider attempts on its synthetic SSE error. Shortening an outer Rust future alone does not change those remote/local execution behaviors. |
| `crates/ortak-server/src/routes.rs`, `desktop/src/features/ortak/types.ts` | Product reads cover runs and expose a routing decision ID in provenance. There is no authorized decision/score/evidence projection or view for a zero-recipient decision. A run-only Activity view cannot demonstrate explainable silence. |

These conclusions use checked-in source and the existing installed-package
fixture contract. They do not establish provider latency, account entitlement,
structured-output support on the Codex route, or relevance quality.

## Recommended smallest production composition

Add a second `SemanticScorer` implementation in `ortak-routing-semantic`,
selected explicitly by `worker_semantic`. Reuse the bounded/redacted candidate
payload, exact-score validator, cache and circuit implementation. Keep the
existing Chat Completions selection compatible and disabled behavior unchanged.

The new adapter calls a separate private, company-bound scoring listener in the
reviewed Hermes bridge image. This listener uses the pinned Codex request/header
and response-normalization seams for one provider request. It does not call
`execute_candidate`, `run_conversation`, `/v1/runs` or `/v1/profiles/probe`.
There is no employee RunSpec, session, Office signer, memory, workspace, tool,
delivery intent or employee activation in a scoring operation. The local owner
is one centrally configured deployment, never an independently subscribing
employee runtime.

Keep the scoring listener and its bounded I/O scheduler separate from the
current run/cancel HTTP handler. Use cancellable asynchronous provider I/O,
at most two active calls, immediate refusal at capacity, no queued inference
backlog and no provider/SDK retries. Do not introduce a per-score subprocess,
Docker launch or full agent constructor. Their cold-start and containment costs
would undermine the five-second contract. Existing immutable image/source
verification still applies to the lower Codex transport imports.

Proposed private selection, with no credential values:

```json
{
  "adapter": "hermes-codex",
  "deployment_id": "<uuid>",
  "origin": "<fixed private HTTPS or literal-loopback origin>",
  "bridge_token_ref": "credential://<opaque-reference>",
  "bridge_token_env": "<exact selected environment name>",
  "binding_sha256": "<full canonical registered binding hash>",
  "model": "<exact selected model>",
  "response_model": "<exact expected response model>",
  "reasoning_effort": "<explicit supported effort>"
}
```

The scorer's server-owned registry maps this deployment to exactly one existing
public profile binding and its already owned OAuth store. No request supplies a
directory or provider endpoint. Reusing an enrolled identity is an explicit
central credential selection, not a new employee identity or an implicit grant
from any candidate's presence. A deployment/model change is explicit and
invalidates the cache identity; editing an employee model does not silently
change the central router's model. The original registered variants remain
available for their original employee revisions.

Proposed private `POST /v1/semantic/score` accepts only deployment/binding
fingerprint, a bounded request identifier, prompt/schema version, remaining
budget and the existing redacted message/candidate view. Its authenticated
listener fixes the company. Company/message/revision/policy pins stay inside
Rust; they are not provider prompt fields. A success returns the exact selected
deployment/model/effort/version, bounded usage and `scores`. It never returns
arbitrary provider prose, raw exceptions, tokens or paths. Both bridge and Rust
validate exact candidate coverage and the existing five-label vocabulary.

Do not reuse the Chat Completions `response_format` envelope on the Codex route
without transport evidence. The lower pinned Responses seam must be inspected
and tested in its image. Use a compiled JSON-only scoring instruction and strict
local parsing; enable a provider schema field only if the actual selected
transport and a real request demonstrate support. Refusal, incomplete output,
tool intent, unknown fields/labels, duplicate fields/candidates, extra text,
wrong model or nonfinite scores produce silence, never a repair/retry prompt.

## Deadline and OAuth ownership

Rust retains its five-second total control budget. The adapter gives the broker
only the remaining budget with a small return margin (maximum 4.5 seconds), and
the broker spends one monotonic budget on admission, credential read, request,
SSE bytes, parsing and response. Bound the raw provider stream as well as the
normalized result; a finite token limit alone does not bound SSE metadata.
This requires one small shared-port adjustment: pass a control-issued transient
`ScoringBudget` alongside the sealed input into `SemanticScorer::score`. Today
the trait receives only the input, so an adapter cannot know how much of the
shared deadline remains on a second scoring attempt. The budget is not prompt,
cache or persisted authority; both existing and new adapters refuse an expired
budget before I/O. Control still performs the final authoritative deadline check.
Client disconnect, timeout and shutdown close upstream I/O. No detached result
can update the cache or become a later wake. Losing the result may consume a
provider request, as with the current HTTP scorer; no exactly-once provider
billing claim is made.

Token refresh cannot hide inside a five-second scoring call: the existing
refresh process permits 35 seconds. Add a bounded read-current-token method
that never refreshes and uses a short lock budget. The listener may consume only
a ready token with sufficient remaining validity. Missing/expired/uncertain
credentials return a closed unavailable reason immediately.

For sustained operation, explicitly enabling this scorer must also enable one
owned credential-maintenance lane for its selected OAuth identity. It refreshes
ahead of expiry using the existing durable `refreshing` fence and process
deadline. It carries no message/candidate data, is not a background inference,
and cannot replay a silent Office message. Serialize it with employee/probe
refresh through the same OAuth lock; cap it to one refresh, use the existing
retry-at backoff, and preserve relogin/uncertain recovery. No model-health probe
is run on an ordinary score, status or product read. A failed or unavailable
maintainer leaves scoring unavailable, not a hidden alternate credential path.
The maintenance subprocess must be included in shutdown/containment and G
capture inventory; ordinary Rust scorer cancellation cannot be its owner.

No new Postgres score-job table is necessary for the initial stateless transport:
canonical routing decisions already record durable results and no score call
creates an employee run or local child. New durable credential/child work, if
implementation introduces any beyond the existing OAuth state, would require a
separate reviewed journal and G/retention integration before shipping.

## Explainability and acceptance

The smallest product read is a signed, current-authority decision lookup by
Office message ID, including zero-recipient decisions. Return only mode/reason,
policy/scorer versions, model/effort, latency/bounded usage and currently visible
candidate actions/scores/evidence. Reuse canonical company/channel/source
authorization before reading any row; hidden source/employee IDs and retained
data after purge must not leak. A message-level “Routing” view can show the
result even when there is no run. Existing SQL stores most projection data;
any new audit action requires an additive schema change, not an unsupported
string in the existing audit CHECK. A durable decision stream is still a
separate M2 requirement; a run SSE connection is not that stream.

Implementation and acceptance should be split at these concrete seams:

1. **Transport and selection:** Rust sealed input through the actual private
   listener and pinned Codex SDK/HTTP boundary. Prove exact model/effort/owned
   credential route, redacted 64-KiB maximum payload/result, 32-candidate limit,
   zero tools/context, one provider request, no redirect/proxy/fallback, and no
   credential lookup for absent or statically invalid selection.
2. **Deadline and recovery:** real streaming stalls/dribbles, malformed SSE,
   disconnect and contention. Prove result rejection by the actual five-second
   control service, released I/O slots, responsive ordinary run cancellation,
   refresh-lock contention, token rotation, process death during refresh,
   durable relogin/uncertainty and no replay of the original silent message.
3. **Routing PostgreSQL boundary:** signed human input and the production scorer
   transport; zero/one/capped recipients, exact one decision/outbox reservation,
   unchanged deterministic precedence and no scorer calls for employee-origin,
   unsupported DM, system/integration, ineligible or explicit-target messages.
   Hold scoring while membership, revision, lifecycle epoch, policy or chain
   state changes; prove current revalidation and no late/duplicate wake.
4. **Product privacy:** actual signed decision reads and UI affordance for both
   scored silence and wakes; cross-company/channel denial, hidden candidate
   redaction, role/source revocation and canonical purge. The response must not
   serialize the full stored decision/input hash/manifest or provider body.
5. **Real selected-provider quality gate:** first evaluate the already reviewed
   Sol/high variant against a small, human-labeled company roster. Include
   unrelated chatter, one clear responsibility, overlapping responsibilities
   beyond the cap, ambiguous short input, Turkish/English and instruction-like
   candidate/message text. Record all cases, scores, decisions, exact selection,
   latency, usage and timeouts. Then demonstrate actual signed Office inputs,
   bounded dispatch, employee replies, and the decision UI on the selected stack.

There is no evidence yet that Sol/high, Astra/max or any available account model
meets this short deadline. A successful 75-second employee/probe inference does
not satisfy it. If the chosen variant misses the budget, the observed result is
silent/unavailable; test another explicitly selected model/effort variant before
enabling semantics. Do not extend the budget, fabricate scores or mark slice D
accepted from transport fixtures or a single favorable provider response.
