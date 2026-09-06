# Final77 encrypted Deniz pair: operator cutover

Source review, 2026-09-06. This is a root-executed recipe, not an activation
receipt. No commands, private configurations, credentials or live metadata were
read/executed to write it. Final migration77, image digests, current process
owners and the enabled pair receipt must come from the root's actual cutover.

## Keep the existing employee

`ConfidentialDmV1` is an installed **bridge capability**, not a capability written
into `employee_revisions` or `employee_runtime_bindings`. It is deliberately not
one of the seven universal activation requirements in
`crates/ortak-control/src/runtime.rs`. The worker probes it once at startup.
Replacing the controller/worker artifacts can therefore enable this protocol for
Deniz's already validated active revision without another adoption, model change,
lifecycle transition, provider probe or OAuth enrollment.

That conclusion is conditional on the current revision having all of:

- The exact previously validated Hermes runtime binding, including model/options,
  profile reference, workspace reference and credential references.
- Exactly empty `allowed_tools`, `allowed_workspaces`, `allowed_networks` and
  `approval_required`; `routing.enabled=true`.
- Current verified Office binding and active employee/lifecycle. The selected
  canonical private DM has exactly the human and Deniz as current participants.

The production SQL `ortak_confidential_runtime_binding(company,revision)` checks
these runtime/policy/routing conditions. Empty policy is required; this lane
cannot reinterpret Files permissions as empty. If the predicate is null, record
the actual discrepancy before considering a separate revision change.

Preserve all five controller profile rows: Ada's three variants and Bora/Deniz's
existing variants. Preserve Deniz's profile files and `oauth_owner` declaration
pointing to the original Ada OAuth identity/store. Office signing/decryption
uses **Deniz's own existing Office key**, never Ada's Office key. The OAuth owner
delegation and the Office identity are separate contracts.

## Artifacts and configuration before enabling a pair

1. Finish the selected current76 backup and offline restore. Retain its exact
   owners, old controller/image, local journal volume and all receipt history.
   Use the root's reviewed pause/migration/handoff machinery; no old PID command
   in a historical document is a current stop recipe.
2. Install the final immutable77 through the staged77 migrator (`buzz-admin
   migrate`, with the existing private database environment supplied by the
   root's bounded helper). Verify ledger/schema and retained rows as part of
   that one migration gate. Deploy only the matching77 relay/API/worker binaries.
   API and worker builds must include the `ortak-server` `encrypted-dm` feature.
3. Select tested immutable worker and controller images, here named **W77** and
   **C77**. `runtime/hermes-bridge/Dockerfile.controller` is built from the exact
   selected worker image and records `org.ortak.worker.image`. Preserve the
   pinned Hermes revision and original OAuth connection; no upstream upgrade is
   required by this cutover.
4. In a new, retained controller config, preserve `company_id`, all `profiles`,
   executor `network`, `docker_binary` and `journal_volume`. Set
   `executor.image = executor.validated_digest = W77` and add
   `executor.confidential_validated_digest = W77`. Preserve Files support only
   with its separately evidenced `workspace_validated_digest = W77`; an old
   digest there causes startup refusal. C77 is the controller image, not the
   value of these worker digest fields.
5. Hand off the one journal owner only after the old controller and owned
   children are stopped. Use the same Docker-managed local journal volume,
   existing exact public profile mounts and original OAuth mount. The new
   controller needs `--ulimit core=0:0`, `--init`, selected UID10001, read-only
   root, loopback8650 binding and the retained isolated control network/socket
   boundary. Its existing CLI remains:

   ```text
   python -m ortak_hermes_bridge --config <new selected controller config>
     --token-file <existing controller service-token>
     --journal <existing absolute journal path>
     --listen-address 0.0.0.0 --enable-validated-docker-executor
   ```

   The installed executor refuses confidential capability without Linux sealed
   memfd support and a zero core limit. Confidential children independently get
   `--ulimit core=0`, private tmpfs, bounded resources and encrypted journal I/O;
   the parent passes only purpose-derived data keys and access-only provider
   credentials, never the Office key or per-run master.
6. Make one authenticated, bounded `GET /v1/capabilities` against that exact
   controller. Require the existing seven base capabilities plus
   `confidential_dm_v1` (and `workspace_text_read` if preserving Files). One
   `POST /v1/profiles/inspect` with `{company_id,binding:<exact Deniz binding>}`
   proves that this installed registry accepts the existing profile and returns
   `healthy:true` with the exact credential references. This is not
   `/v1/profiles/probe` and does not justify repeating adoption/provider probes.
   Use the existing private bearer-token helper; no token belongs in argv/logs.
7. Restart the selected77 worker **after** controller capability readiness:
   its capability result is cached by `bin/ortak-worker.rs`. Retain ordinary
   worker memory/scorer/Office/workspace fields and add only this exact object
   inside the existing <=16KiB `ORTAK_WORKER_CONFIG_JSON`:

   ```json
   {
     "encrypted_dm": {
       "format": "ortak-encrypted-worker/1",
       "pair_ids": ["<new immutable selection UUID>"],
       "relay_origin": "ws://localhost:3038/",
       "key_bindings": [{
         "signer": {
           "company_id": "a4013353-a84d-49a1-8d2b-10a1caf896fe",
           "employee_id": "deniz-private",
           "signer_ref": "<exact existing Deniz Office signer reference>",
           "public_key": "<Deniz existing lower-hex Office public key>",
           "secret_env": "<exact existing Deniz Office environment binding>"
         },
         "office_binding_id": "<current verified Deniz Office binding UUID>",
         "key_version": 0,
         "purposes": ["dm_decrypt", "confidential_wrap", "confidential_unwrap", "dm_seal"]
       }]
     }
   }
   ```

   `key_version:0` is the initial explicit version if selected by root, not a
   version derived from the model/revision. The database selection and provider
   must match it. Reuse the exact `office_signers` public reference/env binding;
   the existing launcher resolves that one private key into the process. Do not
   add an ambient lookup or put key bytes in the JSON. Bounds are 1–16 unique
   pair IDs and 1–16 owned key bindings, all four purposes per binding. An absent
   or invalid selection leaves keyless recovery only; worker liveness alone is
   therefore not an activation witness.

## Exact pair and cohort operations

The current private native source explicitly selects channel
`be203245-5ca3-4a47-9d88-2c20fc65622a` under `http://localhost:3038`.
This is a **display selection**, not proof that the database channel is Deniz's
pair. Root must compare it with canonical current rows. No direct channel or
membership insertion is part of this recipe; reuse the actual native-created
two-person DM. There is no public pair-activation API or CLI in final77.

Run this bounded metadata-only preflight through the already selected database
helper after77 is installed; placeholders are public psql variables supplied by
root, not SQL string interpolation in application code:

```sql
SELECT e.id,e.status,e.active_revision_id,e.lifecycle_epoch,
       b.id AS office_binding_id,encode(b.public_key,'hex') AS employee_public_key,
       b.signer_ref,
       ortak_confidential_runtime_binding(e.company_id,e.active_revision_id)
         IS NOT NULL AS exact_runtime_ready,
       ch.id AS channel_id,ch.channel_type,ch.visibility,
       encode(ch.participant_hash,'hex') AS pair_hash,
       (SELECT count(*) FROM channel_members m
         WHERE m.community_id=ch.community_id AND m.channel_id=ch.id) AS retained_members,
       (SELECT array_agg(encode(m.pubkey,'hex') ORDER BY m.pubkey)
         FROM channel_members m WHERE m.community_id=ch.community_id
           AND m.channel_id=ch.id AND m.removed_at IS NULL) AS current_members
FROM employees e
JOIN office_company_bindings cb ON cb.company_id=e.company_id
JOIN employee_revisions r ON r.company_id=e.company_id AND r.id=e.active_revision_id
JOIN employee_office_bindings b ON b.company_id=e.company_id AND b.employee_id=e.id
  AND encode(b.public_key,'hex')=r.manifest#>>'{office,public_key}'
  AND b.signer_ref=r.manifest#>>'{office,signer_ref}'
JOIN channels ch ON ch.community_id=cb.community_id AND ch.id=:'channel_id'::uuid
WHERE e.company_id=:'company_id'::uuid AND e.id='deniz-private'
  AND b.id=:'office_binding_id'::uuid;
```

Require one row, current active/verified identity, `exact_runtime_ready=true`,
private/dm, exactly two retained/current members matching the selected human and
Deniz. The mutation trigger additionally enforces validity/TTL, human origin and
the sorted-public-key pair hash. Inserting even a disabled selection requires a
currently valid pair.

Freeze one new selection UUID in the operator receipt/config. Insert it disabled
once; on uncertainty read that exact ID and compare the full immutable tuple,
never generate another selection or use `ON CONFLICT DO UPDATE`:

```sql
BEGIN;
SET LOCAL lock_timeout='2s';
SET LOCAL statement_timeout='5s';
INSERT INTO encrypted_dm_selections
 (company_id,selection_id,community_id,channel_id,employee_id,human_public_key,
  employee_public_key,office_binding_id,key_version,decrypt_ref,enabled)
VALUES
 (:'company_id'::uuid,:'selection_id'::uuid,:'community_id'::uuid,:'channel_id'::uuid,
  'deniz-private',decode(:'human_public_key','hex'),decode(:'employee_public_key','hex'),
  :'office_binding_id'::uuid,:'key_version'::bigint,:'signer_ref',false)
RETURNING selection_id,generation,enabled;
COMMIT;
```

The selection trigger owns Office locking, generation and timestamps. Do not
pre-acquire a shared Office fence then attempt this mutation/lock upgrade.
At most one pair per employee can be enabled; old selection rows are retained.

The cohort must include this exact channel and Deniz and be enabled. Preserve
the other selected channels/employees. Use the existing `ortak-cohort` binary,
`ORTAK_COHORT_ENABLED=true`, existing private `ORTAK_DATABASE_URL`, and its
`ORTAK_COHORT_CONFIG_JSON` object (public fields only):

```json
{"community_id":"<selected community UUID>","action":{"kind":"status"}}
```

If the exact channel/employee is already covered, no new capture is needed. If
coverage must change, use the production sequence `disable`, `capture` with
`relay_capture_hook_installed:true` and complete selected `channel_ids` /
`employee_ids`, bounded `reconcile` pages per selected channel using the returned
`capture_id`, then `enable` with that same ID. Each action is one CLI invocation.
Never directly edit the cohort tables or reset historical unsupported1059
inbox/decision rows. Global cohort OFF also blocks this encrypted lane.

Only after image/config/API/native/recovery selections are ready and the cohort
is settled, enable the retained pair with a generation comparison:

```sql
UPDATE encrypted_dm_selections SET enabled=true
WHERE company_id=:'company_id'::uuid AND selection_id=:'selection_id'::uuid
  AND enabled=false AND generation=:'expected_generation'::bigint
RETURNING selection_id,generation,enabled,enabled_at;
```

Require exactly one row; on an uncertain response inspect the same ID first.
This advances Office authority. Finish all cohort/pair edits before sending:
later Office mutations conservatively retire already admitted confidential
runs. Only untouched wrappers **received after enabled_at** enter the new lane;
new messages must be sent after activation, not replayed from failed history.

## Native and one real acceptance

The API's existing operator grant must include both the selected channel UUID
and `deniz-private`. The protected route is NIP98-authenticated
`GET /api/v1/channels/{channel_id}/encrypted-dm/authority`; only the signed
human's exact pair is returned, with <=5s validity and no credential reference.
This endpoint alone does not prove that the worker has an installed capability.

`desktop/scripts/ortak-private-native.mjs` is the existing native build entry
(`node desktop/scripts/ortak-private-native.mjs build`, root's pinned build
environment). It already compiles the exact encrypted channel mapping and
relay→API mapping above. A runtime env var cannot retrofit the frontend mapping
into the old native artifact. Use the one new native build already being staged;
do not rebuild again when its selected mapping/source already match. The native
API resolver uses `ORTAK_ENCRYPTED_DM_API_BINDINGS`, then existing configured /
compiled `VITE_ORTAK_API_BINDINGS_JSON`; IPC cannot choose another API.

Before the first protected draft, freeze the final77 recovery selection:
`JOURNAL_CONFIDENTIAL` must pin the reviewed journal validator and
`NATIVE_CONFIDENTIAL_APP_DATA` must select the existing isolated native app-data
root. Their exact inventory shapes are
`{"format":"ortak-confidential-journal-recovery/1","validator_sha256":"<SHA256 of recovery_confidential_journal.py>"}`
and `Path('/Users/nambse/Library/Application Support/dev.ortak.private20260905')`.
Preparation separately pins the native validator and current native-owner hash.
The separate `ortak-encrypted-dm-v1/ciphertext.sqlite` leaf is created by
native at0700 directory/0600 file, not by copying the old ordinary profile. The
76 inventory keeps both selections unset; do not mutate its frozen backup.

In the isolated native app, open the already selected Deniz DM from Office.
The actual current UI heading is **Private conversation with Employee** (the
route passes the generic employee label), with **Encrypted message** and
**Send encrypted message**. The ordinary composer/timeline must be absent.
Write one fresh bounded synthetic question, wait for **Saving encrypted draft…**
to clear, then send once. **Refresh messages** opens the bounded current-pair
view. If a send is uncertain, use **Retry retained encrypted send**, which reuses
both frozen wrappers and its operation ID; do not create another message.

Root's single acceptance should correlate the real native outer/rumor with one
verified consumed decrypt job, one confidential run, protected snapshot/events,
completed/stopped execution and both reply-copy ACKs, then read the reply in
this participant view. Generic Activity remains metadata-only. Check absence
from ordinary snapshot/event/output/memory/Work content sinks once through the
existing bounded SQL evidence helper; never dump/decrypt protected payloads to
prove that absence. A source-only gate or pair-authority response cannot replace
this actual native→runtime→encrypted-reply result.

Reuse the already passed codec/admission/execution/native regressions. Remaining
validation is the changed image's installed confidential capability/volatile
handoff, exact current configuration/mounts and the one live acceptance above,
not another general test matrix or repeat employee adoption.

## Stop and recovery

To withdraw this opt-in, the operator changes only `enabled=false` for the exact
selection (optionally with its expected generation) and retains the row. The
worker discovers current denial, queues cancellation, performs keyless
lookup/cancel settlement, and retires unsent copies without inventing ACKs.
Leave the77 worker/controller available for that drain even if keys/capability
or selection are removed; missing keys must never trigger reenrollment.
Keep partial/final ACKs and ciphertext history. No down-migration or old76
binary/journal fallback is a recovery command after77 content exists.
