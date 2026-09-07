# Bora, Deniz and central semantic routing: operator recipe

This is a source-reviewed operational recipe, not an execution receipt. Root
owns all preparation, credential access, builds, membership writes, activation,
service changes and cleanup. The shared-connection controller changes are on
HOLD for root validation/deployment. Nothing below claims Bora/Deniz exist or
that D3 quality/DM acceptance has passed.

## Selected baseline and artifact boundary

Use these names as abbreviations in the instructions, not ambient environment
overrides:

| Name | Exact selection |
| --- | --- |
| `S` | `/private/tmp/ortak-private-20260905` |
| `R76` | `S/rollouts/schema76-9d31e77457c54d2c9d219fd8f7d7b434` |
| `R74` | `S/rollouts/schema74-0bfb12ca13194005bde76e8c53b07ea6` |
| `A76` | `S/artifacts/backend76-b18b1bc6ff5643769323d5f76f1d837a` |
| `AW` | `S/artifacts/worker76-reply-f05fa5ab8fa74be89f08c47cbefb6120` |
| Company | `a4013353-a84d-49a1-8d2b-10a1caf896fe` |
| Community | `55bebe0f-90f0-44a2-a021-3b69fbb520a6` |
| Office stream | `f6bcbca6-9974-4792-8f2c-e19718f6bc11` |
| Existing controller | `2ec604cb372a9ca42708d6ca8962b3d66195929f608278cb70b19ba5c339b630` |
| Existing controller image | `sha256:679b6f47b6ec04fa7fd8601b19f605efe0736aa840ccc63ec73cabeb643cbfbd` |
| Reusable employee/scorer worker image | `sha256:aebff616e80db46e4e0f22e1aecec2ef5330298f0e0771b69908bc0018cd4f6a` |
| Honcho API | `33d4e53fd9443e6d167d465f934f3bd9a7e209bb8c80f16ce19e65774b135ff8` |
| Honcho image | `sha256:9358bd04cd45bf654a198313e05a682300d75dc645f780e85df5d5b29f367ede` |
| Honcho deployment | `efd1ad6f-df29-4346-8a2d-f2c271ff4b72`, `http://127.0.0.1:8009` |
| Runtime network | `ortak-v0-hermes-run-5214763bf281407fb8412121b4d26315` |
| Journal volume | `ortak-journal-74-7d40b392f693427caa2e5e2be61d84d9` |
| Journal generation | Created `2026-09-06T03:49:25Z`, owner `7d40b392-f693-427c-aa2e-5e2be61d84d9` |

Starting owner evidence is `R76/current-owners76-mentions.json`, SHA256
`40f58e5fb33931d78051db793b2e75ae9da7ee466fa62095ca03205dff938b40`.
It pins relay75850, API75859, management75868, worker90099 and native2348 with
their full UID/start/inode/launcher identities. The deployed mention native is
SHA256 `5ca39b832e1c289f5cf163536343fc4daccb898a2323d64ea8011c9eac1ef84f`.
These are starting selections, not permission to signal a reused PID later.
Root revalidates exact identities before every stop and records replacements.

The new shared-owner resolver belongs in a **new controller-only derivative**
of the selected controller image, with root's source/hash and focused installed
gate receipt. Its actual new digest is a pending root-produced input. Keep the
employee worker digest unchanged in `executor.image`, `validated_digest` and
`workspace_validated_digest`; never substitute the controller digest there.
The worker payload/profile format did not change. The independent semantic
listener can use the existing worker image and its already implemented module.
No new native, backend, worker, Honcho image or schema is required by this recipe.

## 1. Freeze two fresh public selections and prepare resources

Root creates one fresh operation directory `D` under `S/rollouts`, records its
UUID/ownership, and stores the selected helper/source hashes there. Use the
complete JSON shape in [the preparation guide](DISPOSABLE_EMPLOYEE_AND_SEMANTIC_ACCEPTANCE.md).
Do not copy Ada's signer, prepared memory receipt, employee manifest or OAuth
store. The two selections differ as follows:

| Field | Bora | Deniz |
| --- | --- | --- |
| `employee_id` / name | `bora-private` / Bora | `deniz-private` / Deniz |
| Role | English translation/localization | Turkish writing/editing |
| Responsibilities | Translate supplied text without adding facts; preserve terminology | Simplify supplied Turkish text; check clarity and tone |
| Domains | `translation`, `localization` | `writing`, `editing` |
| `output_directory` | `D/employees/bora` | `D/employees/deniz` |
| `signer_ref` | `secret://ortak-private-20260905/bora-office` | `secret://ortak-private-20260905/deniz-office` |
| `signer_env` | `ORTAK_PRIVATE_BORA_OFFICE_KEY` | `ORTAK_PRIVATE_DENIZ_OFFICE_KEY` |
| `profile_ref` | `ortak-private-20260905-bora-oauth-v0` | `ortak-private-20260905-deniz-oauth-v0` |
| Honcho workspace | `ortak_bora_a4013353a84d49a18d2b10a1caf896fe` | `ortak_deniz_a4013353a84d49a18d2b10a1caf896fe` |
| Honcho employee peer | `bora-private` | `deniz-private` |

Use the existing selected Sol/high model for these initial choices unless root
explicitly selects another supported tested binding. Both initial runtime
`workspace_ref` values are `"none"`; permissions have empty tool, workspace,
network and approval arrays, with `routing.enabled:true`. They receive no
reviewed-project/conversation sharing or Files grant from this preparation.
Keep Ada's current definition and sharing selections intact.

Both selections explicitly use the **one existing authorized connection**:

```json
{
  "oauth_directory": "/private/tmp/ortak-hermes-v0-private-20260905/oauth/ada-private",
  "oauth_owner": {
    "format": "ortak-oauth-identity/1",
    "company_id": "a4013353-a84d-49a1-8d2b-10a1caf896fe",
    "employee_id": "ada-private",
    "profile_ref": "ortak-private-20260905-ada-oauth-v0",
    "credential_ref": "secret://ortak-private-20260905/ada-codex-oauth-v0"
  }
}
```

Their `runtime_binding.credential_refs` is the one-element array containing that
same opaque credential reference. This is an operator grant to the original
connection; the historical `ada` spelling does not change its owner marker.
The controller requires compatible undelegated Ada entries with the exact same
directory. There is no token copy, marker relabel, new device login or
credential-owner field in an API request/employee runtime binding.

Select `A76/buzz-admin` with SHA256
`f0e9b8b6ea045eeeeee04cdd357db6ea7e261367039ebf135ccb00ffdbe8a032`
as the helper's key generator. Set each memory selection to the existing Honcho
deployment/origin above, endpoint reference
`service://ortak-private-20260905/honcho`, admin reference
`secret://ortak-private-20260905/honcho-admin`, environment name
`ORTAK_HONCHO_PRIVATE_TOKEN`, and user peer `operator-private`. Allocate distinct
immutable creation keys, diagnostic UUIDs and canonical timestamps once.

From the repository root, these existing commands operate on one selection at
a time; root substitutes the absolute mode0600 selection path:

```sh
python3 scripts/ortak/prepare_disposable_employee.py --selection <bora-selection.json>
python3 scripts/ortak/prepare_disposable_employee.py --selection <bora-selection.json> --action prepare
python3 scripts/ortak/prepare_disposable_employee.py --selection <bora-selection.json> --action memory
python3 scripts/ortak/prepare_disposable_employee.py --selection <bora-selection.json> --action export
```

Repeat for Deniz. Root supplies only the selected Honcho admin token in the
exact named child environment for memory/export, through its existing private
credential preparation; no token belongs in command arguments or receipts.
The first two actions need no provider/Honcho credential. The memory action
uses existing authenticated create/inspect and diagnostic remember/recall APIs.
On lost ACK, retain the same selection, create key and diagnostic key; rerun that
action once after inspecting its durable checkpoint, never invent a new create.

Retain `selection.json`, signer metadata/hash, the three public profile files,
`office-signer.json`, `controller-profile.json`, `oauth-connection.json`,
`memory/bootstrap.json`, `memory/prepared-memory.json` and
`memory/worker-memory-prepared.json`. The connection recipe deliberately says
`ownership_verified:false`; it is not a login or activation receipt.

## 2. Signed membership and one private DM

Use the existing selected human in native community settings → Members → Add
member. Paste **one exact generated public key**, role Member, and retain its
accepted signed kind9030 event. Repeat for the second key. Then open the Office
stream's members → Add members, paste the same public keys and submit accepted
signed kind9000 events. Both pickers support raw hex/npub before a kind0 profile
exists. Check each member's canonical roster presence; batch toast success alone
does not establish both individual writes. The adoption saga later publishes
each employee's signed profile with its own signer.

Open Bora's profile from the roster → Message. Existing native `open_dm`
submits kind41010 using the selected human signer and fetches the acknowledged
channel metadata. Retain the returned DM UUID and exact human/Bora public-key
pair. Do not add Deniz or Ada to that channel. Opening before adoption is okay;
do not send an execution message or capture the DM until Bora's Office binding
is active and canonical direct-channel validation succeeds. If the UI only
exposes the profile after adoption, do this immediately afterward and freeze a
second API configuration containing that newly returned channel.

This path is a private **server-readable** one-to-one DM. It is not NIP-17
gift-wrap decryption. Native `add_relay_member`, `add_channel_members` and
`open_dm` are all allowed by the compiled private boundary. No inherited agent
creation/attach/gateway flow is needed. A `buzz` CLI is not required; do not
assume A76 includes it. Existing `buzz-admin add-member` is also not a substitute
for proving the signed native event/roster path: that CLI can return success
after a membership roster publication warning.

## 3. Stage complete configs and the controller cutover

Make new immutable copies under `D`; leave all previous files/receipts intact:

| Selection | Exact delta |
| --- | --- |
| Controller config | Copy `R74/journal-volume-7d40b392f693427caa2e5e2be61d84d9/controller/config.json`; retain three Ada variants/executor settings, append the two helper `controller-profile.json` entries |
| API config | Copy `R74/config/api74.json`; preserve existing human/origins/roles and append both employee IDs and the DM UUID to that operator's explicit grants; management and execution flags remain true |
| Worker config | Copy `R76/config/worker76-conversation.json`; append both signer mappings and each original `worker-memory-prepared.json` employee entry under the same deployment; preserve Ada's reviewed selections, original workspace roots/reader and fixed expiry |
| Catalog | Complete currently enabled Ada choices plus two fresh immutable entry UUIDs/labels, each containing a complete valid `ProvisioningConfig` |
| Launchers | New copies of current worker/management/API launchers, pinned to the new configs and **actual newly created controller full ID/image**; preserve current Honcho ID/image |

For each new catalog entry set `mode:"adopt"`, `dry_run:false`, a fresh stable
operation key, manifest provisioning `adopt`, and its own signer/manifest/memory
bindings. Set `runtime_credentials:{"source":"hermes_profile"}` so OAuth stays
inside the controller. Use bridge origin `http://127.0.0.1:8650` and
`ORTAK_HERMES_BRIDGE_TOKEN`. Its Office identity configuration contains only that
employee and the prepared home stream; the DM's later execution authority is
derived independently from canonical membership/employee binding and cohort.
Use the complete prepared-memory object, not Ada's diagnostic or native IDs.

The current management launcher only injects Ada's signer; copying its public
config alone is insufficient. New worker **and management** launchers must
read each new mode0600 signer through the selected private-file helper, verify
its public key against that employee's exported mapping, and place only its
secret value in the exact selected signer environment. Keep the existing
minimal DB/bridge/Honcho credential ownership; no raw provider credential enters
these processes. The old launchers hard-pin controller2ec604… and will correctly
refuse its replacement. Finalize their actual full-ID pins only after creation.

Before changing the controller, root stops new product input and proves no
nonterminal runs, pending dispatch/output/cancellation, provisioning probes,
workspace actions/readers, due/leased/uncertain export work or active provider
children. A future scheduled withdrawal remains a retained obligation. Then
disable the cohort using the existing command and retain the observed state.
Do not disable while a Work output is still owed: global cohort OFF also fences
Work output authority. Stop the exact worker/management owners and old
controller after identity revalidation; do not reopen its original obsolete
host-bind journal. Preserve the current named-volume journal and its companion
files under the existing bounded cold-snapshot procedure, with terminal journal
and child containment recorded separately.

Root creates one uniquely named controller from the **new tested controller
digest**, preserving original limits, network, port8650, service token and
UID/GID10001 plus the default generated hostname. Keep labels `org.ortak.company=<company>` and
`org.ortak.journal_owner=7d40b392-f693-427c-aa2e-5e2be61d84d9`; add a distinct
rollout ownership label. Retain exact original public/profile mounts, the
existing OAuth parent writable, Docker socket, and append only each fresh
public profile leaf read-only at its same absolute path. Never mount the
employee output root containing the signer. Select the new controller config
read-only at its explicit path. Preserve the named journal mount:

```text
type=volume,src=ortak-journal-74-7d40b392f693427caa2e5e2be61d84d9,dst=/private/tmp/ortak-hermes-v0-private-20260905/state,volume-nocopy
```

Its command remains `python -m ortak_hermes_bridge` with explicit config,
service-token and `/private/tmp/ortak-hermes-v0-private-20260905/state/journal.sqlite`
paths, port8650, listen-address0.0.0.0 and
`--enable-validated-docker-executor`. Use `--init`; no volume initialization,
copy-back, host journal fallback or simultaneous second controller owner.
Record actual container ID/image, volume generation, daemon mount projection,
capabilities and unchanged historical journal before starting updated host
launchers. The old controller stays stopped until its receipt-backed cleanup.

## 4. Import and native adoption

Catalog import is a supported bounded command in the existing A76 binary. Root
uses a minimal selected environment with these fields and captures the result:

```text
ORTAK_MANAGEMENT_ENABLED=true
ORTAK_MANAGEMENT_ACTION=import_catalog
ORTAK_PREPARED_CATALOG_JSON=<exact full public catalog file bytes>
ORTAK_DATABASE_URL=<existing selected private DB credential, never printed>
executable: A76/ortak-management
```

Omitted catalog entries are retired; preserve every retained enabled Ada choice
instead of importing just Bora/Deniz. Existing entry UUID/config fingerprints
are immutable. No direct Employee/active-revision SQL insertion is necessary.

Start the new management launcher in root's persistent session with
`ORTAK_MANAGEMENT_ACTION=work` and the same selected community. Restart the API
with its new grants; start the unchanged AW worker binary with its expanded
config and new dependency pin. Keep cohort OFF during adoption. Native
Employees → Adopt prepared resources selects Bora's prepared choice, creates
the draft and confirms its durable command; repeat for Deniz. Retain draft,
catalog, command and operation IDs, the exact request bytes, 13-step progress,
new active revision/lifecycle, fresh completed/contained provider probe, signer,
membership, profile-publication and original-memory checks. Retry an uncertain
command with its original key/body, not a new draft. Plain Create is unsupported
by this prepared-resource path; it is not an activation shortcut.

After adoption, verify Ada/Bora/Deniz are separately visible with distinct
public keys and memory owners. Reopen the exact DM if necessary; require exactly
two retained participants and one active employee identity. Add a newly obtained
DM UUID to the API grant before the acceptance message if it was not known at
the initial API restart. No runtime model is permitted to change this audience.

## 5. Separate semantic listener and final cohort generation

The source already contains `ortak_hermes_bridge.semantic`; root can run it in
the reusable worker image. Preserve the prepared central choice: deployment
`a69839bd-7e1f-4978-8ad6-1fefbd401f0a`, model/response `gpt-5.6-sol`, effort
`low`, loopback origin `http://127.0.0.1:8651`. Bind its **complete selected
public model binding** and recomputed compact sorted JSON hash together; the
historical prepared hash `1f513db8101e06b1084656970178f6c07797533131310944ef2ab85cbeabda03`
is not proof that a changed binding still matches. Keep the original explicit
Ada company/employee/profile/credential owner. The scorer needs neither Bora's
nor Deniz's consuming grant.

Use exactly the config/worker `semantic` shapes in
[SEMANTIC.md](../../runtime/hermes-bridge/SEMANTIC.md). Root creates one distinct
service bearer in a mode0600 private file owned by the scorer UID10001 and selects its opaque reference/env
name in the worker. The scorer has `--init`, UID10001, bounded resources/logs,
empty tmpfs home, fixed provider network, loopback-only host binding8651, a
read-only public config/token mount and only the original OAuth parent writable.
It has **no Docker socket, journal, employee signer, memory or workspace mount**.
The existing command is:

```sh
python -m ortak_hermes_bridge.semantic \
  --config <exact-public-config-path> --token-file <private-service-token-path> \
  --port 8651 --listen-address 0.0.0.0 --enable-selected-semantic-oauth
```

Record actual image/container ID, public binding hash, token reference (never
value), source/mount inventory and bearer-authenticated `/v1/semantic/status`.
Status is local metadata, not a model-health request. Maintenance and runtime
refresh share the original OAuth lock/state/generation; no second store is
created. Recovery must account for this new listener/refresh owner: stop grace
at least45 seconds and explicit task/maintenance termination, not merely
`active_scores=0`. Its two scoring slots and four HTTP connections remain fixed.
Add the exact scorer selection to a new worker config and restart that worker
while routing remains closed.

Capture/reconcile/enable uses `A76/ortak-cohort` with root's selected private DB
environment and `ORTAK_COHORT_ENABLED=true`. Supply one exact public JSON value
as `ORTAK_COHORT_CONFIG_JSON` for each action:

```json
{"community_id":"55bebe0f-90f0-44a2-a021-3b69fbb520a6","action":{"kind":"capture","relay_capture_hook_installed":true,"channel_ids":["f6bcbca6-9974-4792-8f2c-e19718f6bc11","<actual-bora-DM-UUID>"],"employee_ids":["ada-private","bora-private","deniz-private"]}}
```

Retain every already selected channel too if the actual current selection has
additional channels; capture replaces the whole selection. Persist the returned
capture UUID, then for **each** selected channel invoke
`{"kind":"reconcile","capture_id":"<same>","channel_id":"<selected>","limit":256}`
inside that same community envelope until its bounded finite completion receipt.
Finally invoke `{"kind":"enable","capture_id":"<same>"}`. Record each
intent/result and a status readback. Unknown ACK means inspect/resume that
capture; never recapture blindly. No policy setter/schema change is needed:
existing default threshold0.72 and recipient cap2 provide the intended test.

## 6. Visible bounded acceptance and receipts

Freeze this small prompt set before execution. Send fresh top-level human
messages without mentions, aliases, reply links or assignment tags, so the
semantic path actually runs. Expectations below are falsifiable quality goals,
not preclaimed model outcomes; retain failures and timeouts without prompt
tuning, automatic retries or widened deadlines.

| Case | Exact prompt | Expected durable observation |
| --- | --- | --- |
| 0 | `Kayıt notu: Pencere kenarında yağmur sesi var.` | Semantic scored0, no runs |
| 1 | `Bu cümleyi anlamını değiştirmeden İngilizceye çevirin: Toplantı yarın saat onda başlayacak.` | Bora only |
| 2 | `Şu toplantı duyurusunu Türkçede sadeleştirip İngilizce karşılığını hazırlayın: Toplantıya katılım sağlamanız rica olunur.` | Deniz and Bora |
| Cap overflow | `Yeni ürün sürümü için üç kısa katkı hazırlayın: öncelik ve teslim sırası, Türkçe kullanıcı duyurusu, İngilizce karşılığı.` | All three qualify, exactly two wake; third has `recipient_limit_reached` |
| Private DM | `Bu cümleyi İngilizceye çevir: Toplantı yarın saat onda başlayacak.` | Bora-only `direct_message`, one same-DM reply, no semantic request |

Native More actions → View routing decision must show each persisted decision,
including zero-recipient silence and dropped candidate. Retain signed source
event IDs, decision model/effort/hash/latency/evidence, selected revisions,
run/reference IDs, exact reply/event IDs and per-employee memory ACKs. Employee
replies must not create another fan-out or repeat a root visit. DM evidence also
contains exact participant fingerprint/channel and confirms neither Ada nor
Deniz woke. The operator opening these views is actual native acceptance; it is
not a claim that the user personally reviewed the content.

Finish with a fresh current-owner/config/resource receipt: actual controller,
scorer/maintenance, API/management/worker/native owners; two private signer roots,
public profile grants, original shared OAuth store, distinct Honcho resources,
catalog/API/cohort and immutable run evidence. Existing G snapshots remain
historical. The current G selection is an exact one-employee selection, so it
must be explicitly extended to these resources before claiming a final full
backup; this recipe does not make an old frozen capture operator discover them.

## Pending concrete inputs and scoped cleanup

Root supplies the new tested controller digest/creation receipt, a fresh `D`,
two generated public keys and prepared-memory receipts, catalog UUIDs, actual
DM UUID, exact current enabled catalog/cohort, and scorer service-token file/ref.
These are operator-generated outcomes, not new user account/login questions.
No missing SQL activation/API route was found. New immutable launchers and
public config assembly remain required operational work; the old hardcoded
single-Ada launchers cannot be run unchanged against the new controller.

Keep a finite cleanup ledger containing only resources root created for this
operation: full container/image IDs, creation label/receipt and purpose. After
the actual new stack is healthy and old obligations are contained, root may
remove the exact stopped superseded controller and exact stopped temporary
proof/probe containers after fresh ID/image/label/running=false/PID0 checks.
Use explicit `docker container rm <full-ID>` without `--volumes`; do not use a
name wildcard, broad label sweep or forced removal of an unconfirmed child.
The selected scorer/controller and any still-owned run child are not temporary
cleanup candidates. Failed/unknown attempts remain in the ledger until exact
containment is established.

An obsolete controller image is removable only after no retained/current
container references it, no pending build or rollback selection needs it, and
its required provenance/export is retained. Use its exact image ID. Preserve
the reusable employee/scorer worker and Honcho/datastore images, the original
named journal and all other volumes, new employee roots, the single OAuth store,
all backups/receipts, and external Cem/Zeynep resources. No blanket container,
image, volume, system or builder prune is part of this operation.
