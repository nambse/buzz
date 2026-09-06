# C2 workspace read bridge contract

Status: immutable74 and installed-image integration gates passed. The first SDK fixture
exposed pinned Hermes trimming raw tool-result whitespace. The dispatcher now
encodes its model result as canonical JSON and the installed fixture requires an
exact envelope and UTF-8 round trip. The replacement images passed the installed
SDK and containment gates; live deployment, real-provider and native file-read
acceptance remain pending. The existing EMPTY_POLICY path and its start fingerprints remain
compatible. This document does not authorize a live profile/registry replacement.

## Start and exact selection

The existing authenticated `POST /v1/runs` accepts `{company_id,spec}` unchanged.
A Work-only Files-read start adds one top-level `workspace` object:

```json
{
  "format": "ortak-workspace-read/v1",
  "company_id": "<canonical UUID>",
  "project_id": "<canonical UUID>",
  "employee_id": "<exact employee ID>",
  "workspace_ref": "<opaque selected reference>",
  "revision": "<canonical UUID>",
  "manifest_hash": "<SHA256>",
  "files": [
    {
      "file_id": "<canonical UUID>",
      "name": "inputs/readme.txt",
      "media_type": "text/plain",
      "bytes": 42,
      "sha256": "<SHA256 of exact UTF-8 content>"
    }
  ]
}
```

All object fields are exact. UUIDs are lowercase canonical nonzero values. Hashes
are lowercase 64-character hex. Files are sorted by distinct `file_id`; one to
eight files, at most 16,384 bytes each and 65,536 total. Logical names are unique,
at most 256 ASCII characters, match `[A-Za-z0-9][A-Za-z0-9._/-]*` and have no empty,
`.` or `..` components. References match `[A-Za-z0-9][A-Za-z0-9._:-]{0,127}`. These
values are identifiers and display names, never host paths.

`manifest_hash` is SHA256 of the entire object excluding `manifest_hash`, encoded
as compact JSON with lexically sorted object keys and UTF-8, without ASCII-escape
substitution. Arrays retain their order. The bridge fingerprints both this grant
and RunSpec before launch; changed replay conflicts. RunSpec and frozen memory
snapshot versions do not change.

Company/employee/workspace must match the selected exact RuntimeBinding. Work
context is required; Office conversation/reply targets must be absent or null.
Policy must be exactly Files, that one workspace, no network scopes and no
approval requirements. Every no-grant start still requires exact EMPTY_POLICY.
Profile probes may use EMPTY_POLICY even when their binding has a workspace ref.
The specific bridge capability is `workspace_text_read`. It is default-off even
for an otherwise validated Docker executor. Only after the installed C2 fixture
passes for the selected immutable worker image may the operator set optional
`executor.workspace_validated_digest` to that exact `executor.image` string,
alongside the existing matching `validated_digest`. A different digest is an
error. An absent value keeps existing configurations empty-only: the same Hermes
source revision alone cannot prove an older worker implements C2. The opt-in
attests the separately validated contained execution mechanism. The central registry and current Work authority
remain separate required admission/health checks.

## Pull and idempotent resolve

Both routes use the existing service bearer authentication. Every body contains
the exact canonical `{company_id,run_id,idempotency_key}` identity; its company
must match the configured bridge and the company/run embedded in the start key.
No child receives this service token, a PostgreSQL credential or a workspace mount.

`POST /v1/runs/tools/pending` takes only those three fields. It returns
`{"request":null}` or the following exact request:

```json
{
  "request": {
    "call_id": "call_example",
    "file_id": "<selected canonical UUID>",
    "arguments_hash": "<SHA256>",
    "ordinal": 1
  }
}
```

Call IDs match `[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}`. Ordinals are 1–4. Tool arguments
contain only `file_id`; duplicate JSON keys, extra fields and arguments over
128 UTF-8 bytes are refused. `arguments_hash` is SHA256 of exact canonical UTF-8
`{"file_id":"<uuid>"}`. The child durably reserves the request and its start
event before polling. There is one pending call at a time and at most four calls
per run. A provider reusing a call ID cannot create another execution.

`POST /v1/runs/tools/resolve` adds exact `request` and `result` fields to the three
identity fields. Successful `result` is:

```json
{
  "status": "completed",
  "content": "Exact selected UTF-8 text",
  "sha256": "<matching SHA256>",
  "bytes": 42,
  "name": "inputs/readme.txt"
}
```

The content excludes NUL and must exactly match the admitted file's hash, byte
count and name. Failure is `{status:"failed",code:"..."}` with only one of:
`authority_changed`, `workspace_unavailable`, `file_unavailable`, `input_changed`,
`deadline_exceeded`, `cancelled`. Successful acknowledgement is
`{acknowledged:true,call_id,arguments_hash}`.

The central worker owns current-authority claim/read/revalidation and its durable
result receipt before this call. Changed requests/results conflict. A new result
cannot enter a terminal, cancelling or expired run. An identical previously
committed result returns the same ACK even after consumption/termination; this
only replays acknowledgement and never releases more bytes to a model.

## Journal, execution and recovery

`workspace_runs(start_key,grant_json)` pins metadata in the existing SQLite
journal. `workspace_tool_calls` keys `(start_key,call_id)` plus unique ordinal,
with request, deadline, state and result fingerprint. States are `pending`,
`resolved`, `consumed`, `interrupted`. The private `result_json` field exists only
while awaiting model consumption; consume/failure/cancel erase it. `result_hash`
remains to verify exact ACK replay. These tables join the whole-journal backup
and future G drain inventory; pending/resolved rows require run-owner settlement.

Result persistence and `file.changed(read)`/`tool_call.completed` events commit
atomically. Failed reads emit `tool_call.failed`. Public events contain safe
logical names, file IDs, byte counts and hashes; no content or host path. Model
consumption rechecks current running state, call deadline and exact result hash.
Cancellation/failure closes outstanding requests in the same transaction.
Final output cannot overtake an unresolved tool. Restart recovers exact owned
processes and seals interruption; it never reruns a model or tool.

The real pinned AIAgent starts with empty upstream toolsets, then receives exactly
one reviewed function schema. Both input and serialized provider tool lists are
checked. Parallel calls are disabled; raw provider built-ins, other names,
multi-call batches and invalid normalized calls are fatal before dispatch. The
Ortak-owned main sequential dispatcher supplies an ordinary tool-result message;
upstream Files, Shell, environment, approval, parallel and delegation executors
remain unreachable. Four bypass methods retain their fatal guards.

The model-facing message content is compact, sorted-key UTF-8 JSON containing
the exact validated successful result above plus its request's `file_id`.
This is distinct from the unchanged central resolve body and receipt hash.
Pinned Hermes normalizes outer message whitespace in
`agent/conversation_loop.py`; encoding the content inside a JSON string preserves
leading/trailing spaces, CR/LF, tabs, quotes, backslashes and Unicode exactly.
The installed SDK fixture checks the full serialized envelope, then decodes the
content and independently verifies its original bytes, length and SHA256.

The loop allows five model iterations; each tool wait is at most ten seconds,
with short bounded SQLite lock waits. The workspace child has a kernel-enforced
120-second lifetime as well as the loop budget. The existing controller proves
whole-container stop and reaps its process before cancellation acknowledgement.
The child sees no workspace mount or PostgreSQL/service credential. Filesystem
containment belongs to the separate central reader, not this Python module.

## Selected profile update and validation

Adding a workspace to an existing profile does not relax profile/OAuth ownership.
An operator can prepare new read-only profile directories with the same
company/employee/profile_ref and a coherent new workspace binding for **every**
selected model variant. Retain old directories/configuration and reference the
same existing OAuth directory; do not copy credentials or relabel its identity.
Quiesce/drain before replacing the registry, then use sealed F2 Update with fresh
selected profile probing, workspace checks and health gates. Mixed workspace
bases under one profile_ref remain refused.

Run the local source regression suite with:

```sh
PYTHONPATH=runtime/hermes-bridge python3 -m unittest discover -s runtime/hermes-bridge/tests -q
```

Fifteen new cases cover grant/start pinning, actual authenticated loopback HTTP,
lost ACK/terminal replay, cancellation before/after resolution, crash recovery,
concurrent/four-call limits, expiration, atomic rollback, raw/normalized policy
guards, exact model-result byte preservation, exact Docker stdin with no additional mount and a second exact-image capability
opt-in before profile/credential I/O. The existing 105 cases
still pass. Root's new worker/controller image gate runs
`python /opt/bridge/checks/workspace_tool_check.py`: four real pinned AIAgent +
OpenAI SDK + HTTPX scenarios with a synthetic socket transport and central read.
They prove complete, cancel, authority refusal and forged-tool behavior; they
do not claim live-provider, OS-reader containment, full-stack or native acceptance.
