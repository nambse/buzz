# Fresh employees and central semantic acceptance

The source helper is `scripts/ortak/prepare_disposable_employee.py`. Its default
action only validates an explicitly selected public document and prints a plan.
It does not inspect an OAuth store, read an environment credential, create files,
call a provider, start Docker, change Office membership or activate an employee.
The root build owner separately passed the C2 candidate's 120 installed tests,
four SDK workspace scenarios and eight containment gates. Those are component
fixtures, not enrollment, semantic quality or employee activation evidence.

## Explicit fresh selection

Save the following public document as a private mode0600 file, replacing every
placeholder. The company and existing Honcho deployment stay the selected ones;
the employee, output root, OAuth directory, profile/credential references,
memory workspace, creation key and diagnostic identity are fresh selections.
The same helper supports a separately selected third employee.

```json
{
  "format": "ortak-disposable-employee-prepare/1",
  "company_id": "<existing company UUID>",
  "employee_id": "<fresh lowercase employee ID>",
  "output_directory": "<new canonical absolute private directory>",
  "signer_ref": "secret://<fresh employee>/office-signer",
  "signer_env": "ORTAK_NEW_EMPLOYEE_SIGNER",
  "runtime_binding": {
    "adapter": "hermes",
    "profile_ref": "<fresh exact profile reference>",
    "workspace_ref": "none",
    "model": "gpt-5.6-sol",
    "credential_refs": ["secret://<fresh employee>/codex-oauth"],
    "options": {"reasoning_effort": "high"}
  },
  "oauth_directory": "<separate fresh canonical absolute OAuth directory>",
  "worker_image": "sha256:<reviewed immutable worker digest>",
  "key_generator": {
    "path": "<absolute frozen buzz-admin binary>",
    "sha256": "<binary SHA256>"
  },
  "memory": {
    "deployment_id": "<existing selected Honcho deployment UUID>",
    "origin": "http://127.0.0.1:8009",
    "token_ref": "secret://<selected deployment>/honcho-admin",
    "token_env": "ORTAK_HONCHO_SELECTED_TOKEN",
    "binding": {
      "adapter": "honcho",
      "endpoint_ref": "service://<selected deployment>/honcho",
      "workspace": "<fresh employee memory namespace>",
      "user_peer": "operator-private",
      "employee_peer": "<fresh employee peer>",
      "options": {}
    },
    "creation_key": "<new retained original create key>",
    "validation_run_id": "<new retained diagnostic UUID>",
    "validation_recorded_at": "<canonical UTC timestamp>"
  }
}
```

Initial profiles are intentionally empty-policy profiles. Their required Rust
`RuntimeBinding.workspace_ref` string is `"none"`, not JSON null; this value
does not grant workspace/file access.
This prepares D3 acceptance without depending on schema74 or its reader service.
Only literal loopback HTTP Honcho origins are supported by this bounded helper.
The selected private parent must already be owned by the invoking operator and
mode0700. An unmarked nonempty output directory is refused before even adding a
lock file. Existing marked intent cannot be changed on retry.

Atomic writes retain one target-associated `.pending-<filename>` checkpoint.
A retry completes only a bounded canonical JSON leaf owned by the current UID,
with one link and the exact selected immutable payload (or a validated monotonic
memory journal transition). A staged signer is retained instead of regenerating
its key. Unknown names, incomplete bytes, changed selection, links or ambiguous
state remain retained and refused before credentials or external writes. These
states require root to inspect the exact private checkpoint; the helper never
deletes an unknown file or silently adopts an old output tree.

```sh
python3 scripts/ortak/prepare_disposable_employee.py --selection <selection.json>
python3 scripts/ortak/prepare_disposable_employee.py --selection <selection.json> --action prepare
```

`prepare` verifies the frozen key-generator hash, bounds its captured output to
1024 bytes and five seconds, and persists the signer once in `signer.json`,
mode0600. It never prints that generated output or secret. The profile contains
exactly the three public OAuth profile files; its directory is0555 and files0444.
Mount that directory read-only in the controller's exact registry and child.
It also writes public selections `office-signer.json`, `controller-profile.json`
and `oauth-enrollment.json`, each0600. Retrying derives missing exports from the
same signer and intent. No generated key is passed on a command line.

`oauth-enrollment.json` is an exact **command recipe**, not a login receipt.
Root creates a separate fresh OAuth parent as UID10001/mode0700, mounts only that
parent writable in the reviewed worker image, and runs its existing
`python -m ortak_hermes_bridge.oauth_login` command with a real TTY and `--init`.
The human completes the device flow. The helper never launches that container,
reads tokens or infers enrollment from a file's presence. Never copy Ada's store,
edit its ownership marker, or reuse its employee/profile/credential tuple.
See [OAUTH.md](../../runtime/hermes-bridge/OAUTH.md). A model variant for the same
new employee may later reuse that new employee's owned store through an explicit
registry/catalog revision; a different employee uses either its own enrollment
or the explicit shared-connection selection below.

To use an already authorized connection, the public helper selection may add
`oauth_owner` containing the existing exact `ortak-oauth-identity/1` marker
fields (`company_id`, `employee_id`, `profile_ref`, `credential_ref`, `format`).
Select that owner's existing OAuth directory and the same opaque runtime
credential reference. The consuming employee ID, profile, signer and memory
remain fresh. Preparation writes `oauth-connection.json`, with
`ownership_verified:false`, and no enrollment command. It never opens the OAuth
store or changes its marker; root must register the exact grant in the reviewed
controller and complete fresh consumer activation/probe gates. The controller
requires an existing undelegated owner in the same company and directory,
rejecting different references, unregistered owners and chains. See
[explicit shared connections](../../runtime/hermes-bridge/OAUTH.md#explicitly-shared-existing-connection).

## Fresh memory and activation

After root selects the existing Honcho admin credential in the exact named
environment, invoke one attempt:

```sh
python3 scripts/ortak/prepare_disposable_employee.py --selection <selection.json> --action memory
python3 scripts/ortak/prepare_disposable_employee.py --selection <selection.json> --action export
```

`memory` freezes `memory/bootstrap.json` before credential lookup or HTTP. It
uses the existing owning `/v3/ortak/resources/create`, then inspect and the exact
diagnostic remember/recall routes. A lost ACK reuses the original create/write
keys. Once created, missing or replaced native resources refuse without another
create. Completed retries and `export` use protocol/current ownership inspection
only. Both exported configurations contain the same immutable creation receipt
and diagnostic identity: `memory/prepared-memory.json` for F2 and
`memory/worker-memory-prepared.json` for the central worker. Total attempt time
is20 seconds; responses are bounded to64KiB, with no redirects or automatic retries.

Root must still prepare current **relay membership and channel membership** for
the fresh Office public key. The identity adapter supports Adopt and refuses
membership creation/deletion. Use the existing selected operator/Nostr membership
path; do not start inherited agent wake/attach/persona provisioning. Add this
employee's exact signer selection and memory receipt to the central worker,
and append its complete readonly profile to the controller registry, preserving
existing variants. The signer secret enters only its selected process environment.

Import a complete prepared catalog containing both retained Ada choices and new
employee choices; import replaces the enabled catalog, so omitting Ada would
retire her choices. Update the API operator's explicit employee/channel grants
with `can_manage_employees:true` and `can_execute_provisioning:true`. The current
management worker then admits the native **Adopt prepared resources** flow:
draft with expected revision null, command with stable idempotency key, durable
13-step progress and fresh profile/memory/signer/membership checks. The saga
reserves an absent Employee itself; do not hand-insert an active revision or rerun
the Ada-only draft bootstrap. Preserve the operation/draft/catalog IDs on retry.

Changing the routing cohort pauses claims and creates a new capture generation.
Retain its ID, reconcile every selected channel to its finite completion receipt,
then enable that exact generation with both active employees selected. Do this
only after old runs/output obligations are drained. Adding an API audience alone
does not add an employee to the routing cohort.

## Central scorer and actual acceptance

Root's prepared public selection is `hermes-codex`, deployment
`a69839bd-7e1f-4978-8ad6-1fefbd401f0a`, origin`http://127.0.0.1:8651`,
model/response`gpt-5.6-sol`, effort`low`, binding hash
`1f513db8101e06b1084656970178f6c07797533131310944ef2ab85cbeabda03`.
It explicitly selects company`a4013353-a84d-49a1-8d2b-10a1caf896fe`,
employee`ada-private`, profile`ortak-private-20260905-ada-oauth-v0` and its
existing owned enrollment. These public fields were supplied by root; no private
configuration or OAuth data was inspected in this source task. Runtime selection
must still verify the complete binding hashes to that registered variant.

This scorer choice is independent of employees' Sol/high or other selected
runtime models. Start only the separate score listener described in
[SEMANTIC.md](../../runtime/hermes-bridge/SEMANTIC.md): no Docker socket, journal,
Office key or memory/workspace mount, a distinct service bearer and the one
selected OAuth parent. Add it and its maintenance owner to the new G inventory
before enabling it in the central worker. The five-second Rust budget and
4.5-second provider deadline remain fixed; status is not a health inference.

First issue a small explicit real-provider quality set to this listener without
enabling dispatch: unrelated/ambiguous text, one clear responsibility per
employee, overlapping work, Turkish/English and instruction-like input. Use the
actual prepared public definitions; save every result, refusal and timeout. No
retry/repair, alternative provider or deadline expansion hides failures.

Then use fresh **top-level untargeted human** Office messages: no mentions,
aliases, replies or assignment references that would bypass semantics. In native
More actions → View routing decision, verify a persisted scored0 outcome with no
run, a1-recipient outcome, and a2-recipient overlap outcome with exact Office
replies and memory ACKs. Capture selected model/effort, reason, bounded evidence,
latency, usage/cache status, decision/run IDs and no duplicate root visits.
Explicit mention/reply and employee-origin reply cases must still bypass scorer
fan-out. Disable/removal must fence future dispatch; failures stay explainable.

The current default cap is2. Two employees prove0/1/2 outcomes, but cannot prove
that a third qualifying recipient is dropped. `CompanyDirectory` only reads
policy and the cohort CLI has no audited policy setter. Use a third separately
prepared/enrolled employee for a true cap-overflow case; do not add a policy API
or silently change policy solely to make an acceptance fixture pass.

## Source validation and remaining operator work

Twelve new tests drive the real helper, bounded generated-key subprocess,
private/public files, exact memory state machine and authenticated loopback HTTP.
They cover default-plan inactivity, restart and immutable selection, unmarked
old roots, existing OAuth refusal, link/digest rejection, separate employees,
lost create/write ACKs, identical receipt exports and replaced native resources.
The existing eighteen Ada memory-bootstrap tests also pass unchanged.

```sh
python3 -m unittest discover -s scripts/ortak -p test_prepare_disposable_employee.py -q
python3 -m unittest discover -s scripts/ortak -p test_bootstrap_private_memory.py -q
```

The helper's output root adds the signer, immutable selection/profile, enrollment
recipe and memory intent/receipts to private backup inventory. The separately
owned OAuth directory adds its identity/state/lock and any refresh owner. Root
must select and record these exact paths before enrollment, catalog activation
or future capture. This helper does not install new launchers, grant project
access, enable memory sharing, start the scorer, modify schema or deploy anything.
