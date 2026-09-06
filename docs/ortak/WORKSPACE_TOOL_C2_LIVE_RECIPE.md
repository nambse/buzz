# C2: selected Ada file-read acceptance

## Recorded private acceptance — 2026-09-06

The actual native flow completed at Work version 11 for
`56256104-b7cb-45c2-837b-6553a26367b0`, run
`fdd36ad5-0923-421b-93a8-d8179e513b8a`. The real reader read 265 bytes; the
246-byte artifact `d52c0245-b7a1-4025-a804-3aceddc1a94e` has SHA256
`8e3e798aa393f56f3ed4e1b8bb048cc3e2f0fbe43b04baf273e271d6d1aee5d2`.
Root opened that saved artifact in the native app, checked its exact acceptance code
and two-sentence summary, satisfied the criteria, recorded explicit operator
approval and completed the item. Personal review by the user is not claimed.

Evidence under the dated schema74 rollout:
`c2-live-before-review-ed4c956cfc4f4000b8bcc5acbd88d457/receipt.json` and
`c2-live-after-completion-7bd981b0f57a48dc8a75305d87be038d/receipt.json`.
The first suppressed attempt remains in history. Its completed model output was
replayed after the crash, but the cohort-off output-authority guard correctly
prevented an artifact. Keep the common cohort enabled through successful output
authorization and the acceptance receipt; do not repair a suppressed result by
SQL or bypassing current authority.

The initial host-bind deployment reproduced SQLite SIGBUS in two-container
concurrent journal access. Identical images on a Docker local volume completed
the bounded comparison. Live acceptance therefore uses controller image
`sha256:679b6f47b6ec04fa7fd8601b19f605efe0736aa840ccc63ec73cabeb643cbfbd`
and the unchanged selected worker image. Both processes use the same explicitly
owned local volume; see `runtime/hermes-bridge/CONTROLLER.md`. The superseded
controller and unused image were removed after verification; original host
journal bytes and cold evidence remain historical records, not the current store.

After acceptance, native F2 returned Ada to Sol/high with empty permissions and
Office routing enabled, revision `61430887-dcc6-4def-8435-cfd723077f69`. Fresh
capture/reconciliation enabled ordinary Office use. Current owner/config proof:
`office-restore-volume74-6c1eb5e85d274bf9ade2260a493c2ac1/receipt.json`.
Actual populated G74 capture and offline foundation restoration subsequently
passed: bundle `214fd4f027a34604aeb7469d9dfb9a60`, restore
`cea594c6416d42f7a3403aa7509d2c70`. The current volume's raw journal and coherent
rows matched; two terminal workspace journal calls and 16 physical workspace
entries were preserved. Source services resumed; no restored runtime was
activated. See [the G74 recovery evidence and limits](CURRENT_PRIVATE_RECOVERY74_2026-09-06.md).
The historical pins and
instructions below describe the original rollout; use the continuation ledger
and the final selection receipts for current resources.

This is an operator recipe, not a deployment receipt. Core 10 PostgreSQL tests,
five prepared-profile/activation PostgreSQL tests, two HTTP contracts and six
actual OS-reader tests passed. Immutable74 integration, schema parity, the new
host reader artifact, G74 accounting and live/native acceptance remain root-owned.
No credential values, current private paths or deployment IDs are supplied here.

The Files policy is **Work-only**. Use an active EmployeeRevision with
`routing.enabled=false` and an **enabled** selected company/channel/employee
cohort during the Work acceptance. The cohort is also checked by authenticated
Work admission, runtime refresh, tool use and artifact materialization; leaving
it disabled correctly refuses Start Work with 409. The per-employee routing flag
excludes new deterministic and semantic Office wakes without disabling explicit
human Work execution. Ordinary Office starts still require exact empty policy
without a workspace grant.

Keep the cohort disabled during preparation and fully drain previously admitted
Office work before enabling it with the new revision. `routing.enabled=false`
is an admission flag, not cancellation of old decisions: existing Office
dispatch, poll and delivery preserve their pinned revision policy. Use the
existing cancellation/containment workflow for any old run, and retain its
terminal receipts. Before restoring Office participation, select an empty-policy
revision with `routing.enabled=true` through F2; its profile may retain the new
workspace reference.

## 1. Freeze the selected artifacts and recovery boundary

Use the tested immutable images:

- Worker: `sha256:aebff616e80db46e4e0f22e1aecec2ef5330298f0e0771b69908bc0018cd4f6a`.
- Controller: `sha256:032e09a5a8318f3d22c82edbd9e861150362c3bea0f66cf693d4006a10a54961`.
- Installed evidence: `/private/tmp/ortak-v0-evidence/hermes-c2-build-e6b412aeb79f4b22ad101245cd31ac4f/final-receipt.json`
  covers 120 installed units, four real pinned SDK/HTTPX workspace cases and eight
  containment cases, plus the existing protocol gates. It uses synthetic provider
  I/O, not live inference.
- Final proposal74 SHA is
  `1dc560c062aeb4f7e3076c9ce21357674166b99c6536639aae938a00e4bb9f99`.
  Core 10 PG evidence is
  `/private/tmp/ortak-v0-evidence/c2-integration-final-8afb33a5bd394b9892e42ca6a4d0874d`;
  the final five F2 PG tests passed in
  `/private/tmp/ortak-v0-evidence/c2-f2-final-fixed-ab256d3bd18247f1941bdc9ab073be50/receipt.json`.

Build and record the new **host** `ortak-workspace-reader` binary alongside the
paired74 `ortak-worker`, API and management artifacts. Copy it to a retained,
absolute versioned path; record SHA256 and owning UID. It must be a regular
single-link file, owned by the worker UID, executable by its owner and not group
or world writable. Do not replace that path during an unresolved execution:
restart containment verifies the retained path/hash/UID, then an exact execution
token with `/usr/bin/pgrep`. A deadline alone is not a stop receipt.

Before activation, G74 must classify all six main tables: `workspace_bindings`,
`workspace_files`, `run_workspace_uses`, `workspace_tool_actions`,
`workspace_tool_receipts`, `workspace_reader_executions`. The bridge's existing
SQLite journal also gains `workspace_runs` and `workspace_tool_calls`. Preserve
the whole journal/WAL policy, `executor.lock`, exact image/config/profile/OAuth
references, new input root and run-copy root. Unresolved readers, pending tool
results and nonterminal owning runs prevent a completed drain. A73-only backup
manifest is not a74 recovery receipt. Canonical deletion must know these retained
tables before an administrative purge is attempted.

Use the existing ingress gate and contained-service handoff to drain and stop the
old owners. Retain their receipts/configuration. The new controller must acquire
the same journal ownership after the old controller/provider children are stopped;
never reset the journal to make an uncertain start disappear. Existing Honcho,
Office signer and reviewed-memory selections remain the explicitly selected ones.

## 2. Prepare one fresh synthetic input and its public manifest

Root chooses the current company/project/employee IDs, a fresh revision UUID and
file UUID, a new opaque reference such as `input:c2:<fresh-id>`, fixed UTC expiry
within 30 days, and two fresh disjoint absolute roots. On macOS use canonical
`/private/tmp/...` rather than the `/tmp` symlink. No ancestor may be a symlink;
the descriptor reader also rejects unsafe ownership/write permissions.

Create only this layout, under the worker's UID:

```text
<input_root>/                              0700
  .ortak-workspace-inputs-v1                0400
  <revision UUID>/                         0700
    <file UUID>                            0400, one regular file, nlink=1
<run_root>/                                0700
  .ortak-workspace-runs-v1                  0400
```

Each marker contains exactly `ortak-workspace/v1:<company UUID>\n`. Create these
paths with exclusive creation; an occupied path is a refusal, not permission to
repair, overwrite or adopt it. Write/fsync exact UTF-8 bytes, then seal/fsync files
and parent directories. The source may be a short synthetic brief containing a
fresh acceptance code. Keep that code out of the Work prompt and existing memory
so the response requires an actual read. A trailing newline is useful acceptance
input. Do not point either root at the repository, profiles, OAuth, journals or
backups. The worker itself creates run copies and retained lock files.

The public grant template is:

```json
{
  "format": "ortak-workspace-read/v1",
  "company_id": "<selected UUID>",
  "project_id": "<selected current project UUID>",
  "employee_id": "<selected Ada ID>",
  "workspace_ref": "<new opaque reference>",
  "revision": "<fresh revision UUID>",
  "files": [{
    "file_id": "<fresh file UUID>",
    "name": "brief.txt",
    "media_type": "text/plain",
    "bytes": 0,
    "sha256": "<SHA256 of exact synthetic file bytes>"
  }],
  "manifest_hash": "<computed below>"
}
```

Set `bytes` to the UTF-8 byte length, not characters. For the one-file case, no
sorting ambiguity exists. Compute `manifest_hash` from the completed object
**without** that key using
`hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode('utf-8')).hexdigest()`.
The production adapter revalidates this hash and actually opens every selected
file before registering it. Metadata generation alone does not create a verified
registry row. Limits are 8 files, 16 KiB each and 64 KiB total; logical names are
display metadata, while only UUIDs form storage paths.

## 3. Replace all three prepared profiles coherently

Derive three **new** read-only profile directories from the already selected
public bindings. Keep company, Ada ID, original `profile_ref`, opaque credential
reference and original `oauth_directory` identical across all three. Preserve
each exact model/options combination; do not infer its model ID from a UI label.
Set only each binding's `workspace_ref` to the same new reference.

Each new OAuth profile directory contains exactly these three public files:

| File | Exact content |
| --- | --- |
| `ORTAK_DISPOSABLE_PROFILE.json` | Existing `{company_id,employee_id,profile_ref}` identity |
| `ORTAK_RUNTIME_BINDING.json` | Full selected binding, including model/options and new workspace reference |
| `ORTAK_PROVIDER.json` | Existing `{"provider":"openai-codex","credential_ref":"<original opaque ref>"}` |

Use read-only files/directories and the same existing mount visibility contract
for the controller and Docker daemon. Do not create `provider-token`, copy OAuth
state, enroll again, relabel the OAuth marker or run the fresh-employee/signing
bootstrap helper. Old profiles and configurations remain retained.

Replace the controller's three `profiles` entries together:

```json
{
  "employee_id": "<same Ada ID>",
  "binding": "<full selected binding object for this variant>",
  "directory": "<new absolute read-only profile directory>",
  "oauth_directory": "<same original configured OAuth directory>"
}
```

`binding` above is an object when applied. Do not append the new variants to old
workspace bases under the same `profile_ref`: `profile_registry` rejects that
mixed identity. Retain the existing explicit company/private network and service
credential mount. Set all three executor fields to the identical tested worker
image string: `image`, `validated_digest`, `workspace_validated_digest`. The
controller image pin is separate. Absent `workspace_validated_digest` leaves
`workspace_text_read` disabled. Enable the existing
`--enable-validated-docker-executor` flag only for this evidenced selection.

## 4. Publish verified inputs before Files activation

Add this object to the existing selected worker configuration as `workspace`:

```json
{
  "reader_binary": "<retained absolute host-reader path>",
  "reader_sha256": "<exact reader SHA256>",
  "input_root": "<fresh input root>",
  "run_root": "<fresh run-copy root>",
  "grants": ["<the full grant object from step 2>"],
  "expires_at": "<fixed UTC RFC3339 timestamp>",
  "register_selected_inputs": true
}
```

`grants` contains objects, not strings. Preserve the existing memory full receipts,
reviewed opt-ins, Office signer/relay and bridge selection. Whole worker JSON is
bounded to 16,384 bytes; verify the resulting encoded size. Use the normal selected
launcher with `ORTAK_WORKER_ENABLED=true`, `ORTAK_WORKER_CONFIG_JSON` and its
existing protected environment. There is no register-only worker mode: keep
ingress closed and all prior work drained during this startup.

The startup probes the exact bridge capability, verifies the actual pinned OS
reader/files and commits the binding+files in one registry transaction. Confirm
exact company/project/employee/ref/revision/hash, non-revocation and fixed expiry.
The project must have its matching current `project_api_bindings` and Office
community binding. A retained old registry row alone cannot pass F2 health.

After successful publication, save the final worker configuration with
`register_selected_inputs:false` and the **same** expiry and grant. Restarting
the initial `true` configuration is an idempotent replay only if every retained
byte and expiry is identical. Do not compute a new expiry on each restart.
An expired/withdrawn revision requires a fresh explicit revision and publication.

## 5. Select the Files policy through F2 and native UI

Prepare a complete replacement catalog containing all three model choices with
fresh immutable catalog IDs. Retain each exact Honcho creation receipt, Office
signer, credential-selection reference and employee identity. Change the runtime
workspace reference consistently, set `configuration.manifest.employee.routing.enabled`
to **false** in every Files choice, and use exactly this permission policy:

```json
{
  "allowed_tools": ["files"],
  "allowed_workspaces": ["<new opaque reference>"],
  "allowed_networks": [],
  "approval_required": []
}
```

Catalog format is `{community_id,entries:[{id,label,configuration}]}` with the
existing full `ProvisioningConfig` object in `configuration`. Import through
`ortak-management` using `ORTAK_MANAGEMENT_ENABLED=true`,
`ORTAK_MANAGEMENT_ACTION=import_catalog`, `ORTAK_PREPARED_CATALOG_JSON` and the
selected protected database environment. Import retires omitted choices; include
all three instead of accidentally replacing the catalog with one model.

Run the normal selected management worker (`ORTAK_MANAGEMENT_ACTION=work`,
`ORTAK_MANAGEMENT_COMMUNITY_ID`). In native Employees, select the prepared choice,
save its draft and execute **Update** against Ada's current revision/lifecycle
epoch. If Ada is currently disabled, use the existing explicit re-enable action.
Wait for the durable command to succeed and confirm its new revision/policy.
Confirm the active manifest's routing flag is false, not only the prepared
catalog's flag. Importing a catalog does not change the active revision.
This explicit operation performs a real selected runtime probe, contains that
probe, prepares memory and runs the sealed fresh health gates. Ordinary health
does not substitute for it. Workspace loss during probing or before final commit
must refuse activation and retain any unresolved cleanup obligation.

Before enabling the selected cohort, retain proof of no unresolved Office
`run_dispatch`, Office-origin nonterminal run, `runtime_office_outputs` or
`office_reply` delivery, and no provider child from that work. An expired lease
alone is not closure. Reconcile or cancel these through their existing durable
paths; do not reset or delete historical decisions. Retained settled memory or
withdrawal obligations follow their separate recovery rules.

Use the bounded `ortak-cohort` capture/reconciliation/enable actions for the exact
current channel and employee selection. Complete each retained capture window
before enable; do not use a direct SQL state change. Confirm cohort enabled and
Ada active with `routing.enabled=false`. A fresh authorized explicit Ada mention
must record `RoutingDisabled`, with zero wake recipients, no new runtime dispatch,
no employee reply and no provider child. An untargeted message in an Ada-only
cohort must be silent without semantic I/O. Other employees, if explicitly in the
cohort, retain their own current routing policies.

## 6. Smallest real file-read acceptance

With the cohort enabled and Ada's routing flag still false, create/promote one
Work item in the selected authorized project, assign Ada, define human acceptance
criteria and move it to Ready. Its description can say:

> Call `read_workspace_text` for file ID `<file UUID>`. Return the acceptance
> code and a two-sentence summary from that file. If the read fails, report the
> failure. Do not infer the file contents.

Start execution through the native Work action, not a direct bridge request.
Verify actual Activity `tool_call.started`, `file.changed` with `change=read`, and
`tool_call.completed` share the admitted call identity, then a real text artifact
arrives in Review. Check the fresh code against the file. Inspect the artifact and
perform human acceptance; the execution must not mark criteria satisfied itself.

Retain bounded metadata evidence: run/revision/epoch, immutable workspace-use
hash, action call/file/hash/ordinal, stopped prepare/read executions, one immutable
result receipt per call, delivered state, dense Activity cursors, artifact identity
and Work history. Public tool events must not expose a host path or file content.
The authorized text artifact may contain the requested file-derived answer.
One model-selected call is the minimal demonstration; up to four calls are
allowed, while duplicate retries must not add another receipt for the same call.

Reload/reconnect and confirm the same persisted events and artifact. A separate
native cancellation may prove provider containment and absence of a late
deliverable. Do not label it an in-flight file-read cancellation unless a reader
was actually observed: short reads can finish before the click. The controlled
PG and OS-reader tests independently cover blocked I/O, expiry and restart proof.

## 7. Settle and return to ordinary Office use

Keep the C2-capable controller, original journal, reader binary and roots available
until every admitted run, action and reader is terminal. Removing the worker's
workspace selection triggers stop/recovery of existing uses; it is not a stop
acknowledgement or a deletion. Missing reader binaries/credentials leave durable
cleanup pending. A lost result ACK retries the same retained receipt after actual
stop proof; it does not reread a file or deliver another model message.

Prepare/import all three **empty-policy** choices using the same new workspace
base and `routing.enabled=true`, then perform sealed F2 Update after all C2 work
has settled. Keep the selected cohort disabled during that transition and use a
fresh completed capture/reconciliation receipt before enabling it again. Only
the successful active revision enables Office participation. Keeping the binding
reference does not grant a tool under empty policy. Do not mix the old base
profiles back into this registry to bypass the update.

Preserve input hashes, markers, run copies, lock files, receipts and stopped
history through G74. This slice has no public workspace withdrawal/delete UI or
automatic filesystem eraser. Removing configuration does not revoke the retained
registry row; its explicit fixed expiry still closes future use. Do not issue
ad-hoc SQL DELETEs or claim physical erasure after removing a selection. Any later
retention cleanup needs the owned stop/retention proof and the reviewed deletion
workflow for that scope.

Production seams: `worker_workspace_tools.rs`, `workspace_reader.rs`,
`ortak-runtime/src/postgres/workspace_tools/`, `prepared_runtime.rs`,
`ortak-control/src/postgres/provisioning/activation.rs`,
`ortak_hermes_bridge/{service,docker_executor,workspace_tools,journal_tools}.py`.
The exact wire and bounds are in [WORKSPACE_TOOL_C2_WIRE.md](WORKSPACE_TOOL_C2_WIRE.md).

The configuration combination needs its own regression: the existing C2 tests
use the authenticated executions endpoint, but their employee fixture implicitly
enables the cohort. Exercise a Files employee with routing false through the
signed Start Work request and real supervisor/selected-read/result/artifact
seams; keep criteria pending until human acceptance. In the same scope, assert
fresh explicit/untargeted Office inputs cannot dispatch that employee, and that
cohort disable still refuses a new Work request without persisting a run and
fences an existing Work use/output. Direct outbox insertion cannot prove this
operator contract.
