# Fresh Codex OAuth profile

The bridge uses the pinned Hermes `AIAgent` Codex Responses transport. It does
not launch Codex CLI, use the Codex desktop account, discover another profile,
or copy any `auth.json`. The provider is `openai-codex` and the fixed endpoint
is `https://chatgpt.com/backend-api/codex`.

The selected model and `binding.options.reasoning_effort` reach the actual
constructor and request transport. Both are explicit and changeable by updating
the controller registry and exact public profile binding together. Unsupported
efforts fail before provider I/O. In particular, `ultra` is not an alias for
`max`. The published [GPT-6 Astra model documentation](https://developers.openai.com/api/docs/models/gpt-6-astra)
lists `low`, `medium`, `high`, `xhigh`, and `max`. The narrow exact-model bridge
adaptation preserves Astra's selected effort over the older pinned transport's
`max` → `xhigh` clamp; it never substitutes a model. Account entitlement is
established by the explicit real probe below. Model documentation is not an
entitlement guarantee. The [official Codex authentication guide](https://learn.chatgpt.com/docs/auth)
describes the ChatGPT device login family used by the pinned Hermes flow.

## Build and source review gate

Keep Hermes source revision `29112bef099274229cadff79cdff7bf7b99c4b77` and the
existing immutable dependency pins. `hermes-source-lock.json` now verifies 22
reviewed files, including the OAuth device/refresh implementation, Codex headers,
runtime/adapter and reasoning normalization. No floating upstream is imported.
Build new worker/controller tags and a **new** OCI export directory using
[CONTROLLER.md](CONTROLLER.md); retain earlier artifacts.

Before login, run in the new immutable worker with no credentials and no network:

```sh
python -m ortak_hermes_bridge.candidate_smoke
python /opt/bridge/checks/run_loop_check.py
python /opt/bridge/checks/codex_oauth_check.py
```

The last check retains the real pinned constructor, Codex transport, run loop,
tool denial and durable journal, with explicit metadata and provider-I/O fixtures.
It asserts the actual request is `gpt-6-astra` / `max` and blocks ambient
credential refresh/recovery. It is **not** a successful OAuth or provider check.
Also rerun the existing exact-image containment/controller/recovery gates.
The root release receipt must record the new image identities and results;
source/unit-test completion alone does not mark these artifacts deployed.

## Explicit enrollment

Use a fresh private parent owned by `10001:10001`, mode 0700. Inside it, choose
an unused canonical absolute OAuth directory. Keep this directory separate from
the journal and worker profile. Mount its parent writable only in the login and
controller containers. The worker never receives this mount. Follow the existing
same-absolute-path bind convention for the public worker profile and journal.

Run this module in the reviewed image as UID 10001, with `--init`, a real TTY,
an empty private home/tmpfs, and provider connectivity. Substitute public exact
identity references, never tokens, in these arguments:

```sh
python -m ortak_hermes_bridge.oauth_login \
  --directory /absolute/fresh/oauth/employee \
  --company <company-uuid> --employee <employee-id> \
  --profile-ref <profile-ref> --credential-ref <opaque-credential-ref>
```

The command invokes the reviewed Hermes `_codex_device_code_login` directly.
It shows official device-login instructions; the user completes browser login.
Tokens are written only to the new private store. No browser/host credentials
are inherited. A fixed completion message means enrollment succeeded, not that
the selected model works. The command accepts an existing directory only when
its private Ortak ownership marker matches every identity field. Fresh login is
also the recovery action for this same explicitly owned store.

The controller's profile entry adds `oauth_directory` to the existing
`employee_id`, `directory`, `binding` fields. `binding` includes the exact model,
`options: {"reasoning_effort": "max"}` (when selected), and exactly one opaque
`credential_refs` entry. The worker profile directory has exactly these three
public JSON files, matching that entry:

- `ORTAK_DISPOSABLE_PROFILE.json`: company, employee and profile reference.
- `ORTAK_RUNTIME_BINDING.json`: the full exact binding.
- `ORTAK_PROVIDER.json`: `{"provider":"openai-codex","credential_ref":"<same-ref>"}`.

There is no `provider-token` file in an OAuth worker profile. The controller
selects its exact owned OAuth store and gives the child only the access token
through anonymous bounded stdin. Refresh tokens never enter a child, command
line, journal, HTTP response or Docker log. Explicit API-key profiles retain
their original four-file format.

The immutable controller registry accepts up to 64 exact binding variants.
Variants may share a `profile_ref` only when their employee, base binding
(including workspace and credential references), and OAuth directory match.
Only model and options may differ. Each variant has its own public profile
directory and exact `ORTAK_RUNTIME_BINDING.json`; it reuses the same owned OAuth
store. Adding a variant does not alter an existing profile, enrollment, employee
identity or a run's pinned revision. Unknown bindings and duplicate exact
bindings are rejected. Every variant needs its own current real health witness.

## Explicitly shared existing connection

Employee identity does not require a separate ChatGPT login. A trusted operator
may add this optional field to a consuming controller profile, together with the
existing owner's exact `oauth_directory`:

```json
"oauth_owner": {
  "format": "ortak-oauth-identity/1",
  "company_id": "<same company UUID>",
  "employee_id": "<existing owning employee>",
  "profile_ref": "<existing owning profile reference>",
  "credential_ref": "<exact same opaque reference in the consuming binding>"
}
```

The owner must already have an undelegated profile in this same frozen registry.
Several compatible owner model variants are allowed. The company, credential
reference and directory must match exactly; unregistered owners, changed paths,
cross-company grants and delegation chains are refused before credential reads.
The consumer retains its own employee ID, full model/policy binding, public
profile, signer and memory. Its three public worker-profile files remain in the
original format. An API request or RunSpec cannot add or change this grant.

This resolves the original OAuth marker and store without copying tokens,
relabeling ownership, enrolling again or changing the worker image. The existing
lock, refresh uncertainty state, account and generation are shared by all
consumers. Only the access token enters each worker's anonymous stdin. Ordinary
inspection remains read-only; activation still requires the consumer's own
completed and contained real profile probe. Delegated probe selection adds an
owner/directory fingerprint to the existing selection JSON, so an operator
remapping cannot reuse an old witness even if token/generation values coincide.
Undelegated profile behavior and probe fields are unchanged.

Grant changes and revocation require a new operator registry selection and a
coordinated controller cutover after existing work settles or is contained.
There is no registry hot reload, credential discovery or automatic fallback.
The separate semantic scorer continues selecting the original owning identity
explicitly; this controller delegation does not widen its configuration.

## Explicit real probe and current health

Ordinary `/v1/profiles/inspect` is read-only: it makes no model, catalog or
refresh calls. Its response includes `credential_references`, containing only
the exact registered opaque reference when the selected enrollment is readable
and in the ready phase. An expired access token can still be resolvable through
explicit run-time refresh. Missing, malformed, uncertain or wrong-owner stores
return no references. This field contains no credential value or secret path.

To perform a real provider request, persist a new probe UUID in the operator's
receipt, then run inside the controller (after explicit model/effort selection):

```sh
python -m ortak_hermes_bridge.profile_probe \
  --config /absolute/controller/config.json \
  --token-file /absolute/controller/service-token \
  --employee <employee-id> --probe-id <persisted-new-uuid> --port 8650
```

When the employee has multiple registered variants, also pass
`--binding-sha256 <exact-public-binding-hash>`. The hash is SHA-256 of the full
binding encoded as UTF-8 JSON with sorted keys and compact `,`/`:` separators.
An omitted or ambiguous selection is refused before admission; the API already
selects the exact full binding and needs no additional field.

This authenticated `POST /v1/profiles/probe` accepts exactly `company_id`, the
full registered `binding`, and `probe_id`. It starts one ordinary contained run
with no tools and a fixed connection-check prompt. Probe metadata and run
reservation commit in a **single SQLite transaction before launch**. The CLI
prints only the run identity and status. Inspect the normal run/event endpoints
for completion. A lost admission response is retried with the same UUID; it does
not launch another run. Failed/cancelled admissions remain durable. An identity
collision or changed selection returns conflict, requiring a new operator UUID.

Health becomes true only after that explicit run completes and containment is
proven stopped. The witness expires 120 seconds after durable completion and is
bound to company, employee, complete binding, model, effort, worker image,
OAuth generation, access-token hash and account hash. Model/effort, image or
auth-generation changes require a fresh real probe. Hashes are stored in probe
metadata; tokens and raw account identifiers are not. A token's presence,
unverified JWT metadata, an image-only fixture or an ordinary successful run
cannot manufacture this health witness. Do not periodically probe implicitly
from a health endpoint; an operator or explicitly authorized workflow must
request each chargeable model check.

## Refresh and recovery

The private store and files require exact UID, directory mode 0700, file mode
0600, nonsymlink paths and single-link files. A per-profile flock serializes
enrollment and refresh across processes with bounded acquisition time. Updates
use a fresh 0600 file, file fsync, atomic rename and directory fsync.

The controller refreshes only during an explicit run/probe when access expiry is
near. It commits `refreshing` before contacting the official token endpoint via
Hermes `refresh_codex_oauth_pure`. Successful rotation durably increments the
generation. A 429 retains a durable 60-second backoff. A lost response, process
death, unsuccessful durable write or uncertain rotation fences token reuse and
requires explicit fresh login. The worker's ambient refresh, credential pool,
model switching and fallback routes are disabled. Closed failure codes replace
provider exceptions; private state remains available for controlled recovery.

## Validation status at source handoff

Local bounded tests passed: 70 tests, including parallel refresh, crash/lost
response fences, ownership and link/mode rejection, unsupported effort refusal,
exact constructor/transport selection, access-only worker IPC, probe transaction
rollback, duplicate admission, expired/mismatched witness and containment gates.
No real credential was inspected, login performed, provider request made, image
built or private stack activated by this source change. New image checks and the
explicit authenticated probe remain release/integration gates owned by root.
