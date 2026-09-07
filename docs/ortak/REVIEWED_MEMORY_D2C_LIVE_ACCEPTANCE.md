# Root-only selected reviewed-memory acceptance

Prepared on 2026-09-06 from the production source. This is an execution recipe,
not a record of live recall or human review. Root owns the schema73/Honcho rollout,
exact worker opt-in, native interaction and evidence capture. No script, process,
provider request or live mutation was run while preparing this document.

## Fixed existing evidence and fresh selections

Use company `a4013353-a84d-49a1-8d2b-10a1caf896fe`, employee `ada-private` and
project `3a06f7cf-ff9c-4deb-bb8c-7ef422eb9b6e`. The already completed Work item is
`4419e4fc-58a3-4570-8f9d-11767a9ff1c5`; its actual Sol run is
`e1209baf-9c50-4f74-a2e1-3acf73006aad`, and its saved artifact is
`5c2a86f0-d6f3-44f7-ba21-718f8804f016`. The retained artifact is 377 UTF-8 bytes,
SHA256 `b685e90b3c885b6d17661e708e9278f85a0b5240b0ce709e1c673970906bbbcd`.
Recheck current signed visibility and exact bytes before using it as evidence.

Leave old fact `853d06ef-b81e-4e68-be60-fb985c988d68` revoked. Its original approval,
publication/withdrawal and zero remote content rows remain a separate baseline.
Create a new fact and a new Work item; do not reopen or modify the completed item.

Before acting, retain an owner-private acceptance intent with fresh operation IDs,
an alphanumeric lookup label of 2–32 bytes (for example `d2c` plus 12 fresh hex
digits), a different fresh answer marker, and an explicit expiry 2–24 hours ahead.
These are nonsecret test data. The answer marker must appear only in the newly
reviewed fact and private expected-result evidence, never in the new Work title,
description, criteria, Office messages or runtime profile. Reuse each exact
operation/body after an uncertain acknowledgement; never regenerate IDs to retry.

## Earliest action and prerequisites

The earliest native action is to open the existing saved deliverable, verify its
content, and explicitly approve a fresh edited fact for Ada in this project. This
local approval performs no Honcho write and can precede runtime opt-in. Publication
must wait for the selected live memory target; execution must also wait for the
schema73/backend/Honcho candidate and exact project runtime-use gate.

Root records the deployed artifact IDs, current Ada revision/lifecycle epoch and
current native bundle, then publishes a new immutable worker configuration retaining
the original full Honcho creation receipt. For Ada alone, the intended additions are:

```json
{
  "reviewed_projects": ["3a06f7cf-ff9c-4deb-bb8c-7ef422eb9b6e"],
  "reviewed_runtime_projects": ["3a06f7cf-ff9c-4deb-bb8c-7ef422eb9b6e"]
}
```

Merge these fields into the selected employee entry without replacing other
explicit selections. The second set must be a subset of the first. The owning
worker must advertise a live target with `enabled=true`,
`runtime_consumption_enabled=true` and `valid_until>clock_timestamp()`; record its
target ID, consumption epoch and binding hash. Do not set these DB fields by hand.
No employee/profile/model/OAuth/semantic/workspace change is part of this recipe.

## Native actions and exact existing API contracts

Use the configured private native Work screen and its current human NIP-98 signer.
All mutations below already exist as authenticated HTTP routes; none requires a
new API or a browser-console mutation. Keep the approval, publication, execution,
artifact inspection and Stop using actions native for this acceptance record.
Headless signed API replay/read evidence alone is not native acceptance.

1. Select the existing project and completed item. In **Reviewed project memory**,
   choose Ada, choose its **Saved deliverable** as evidence, and review this edited
   test annotation before selecting **Approve fact**:

   > Operator-reviewed acceptance annotation for the saved deliverable.
   > Lookup label: `<lookup>`. Verification answer: `<answer>`.
   > This is a fresh operator test annotation, not a quotation from the artifact.

   Retain the actual approving operator public key/time. Do not describe this as
   the user personally reviewing it. The API derives company/actor/source hashes:

   ```text
   POST /api/v1/projects/{project}/reviewed-memory
   {"operation_id":"<fresh UUID>","fact":{"employee_id":"ada-private",
    "source":{"kind":"artifact","artifact_id":"5c2a86f0-d6f3-44f7-ba21-718f8804f016"},
    "content":"<exact reviewed annotation>","expires_at":"<explicit future UTC>","reviewed":true}}
   ```

2. Retain the returned new fact ID/version1. Read the separate publication consent,
   check it, and select **Publish reviewed fact**. Refresh until the UI reports
   publication acknowledged and selected Work use enabled. Merely saving the fact
   or a pending job is insufficient.

   ```text
   POST /api/v1/projects/{project}/reviewed-memory/{new_fact}/publish
   {"operation_id":"<fresh UUID>","expected_version":1,"confirmed":true}
   GET /api/v1/projects/{project}/reviewed-memory?employee_id=ada-private
   ```

3. Create a new Work item in the same project, with lookup label first in its title.
   Use this body as a template, substituting only the lookup label (not the answer):

   ```text
   POST /api/v1/projects/{project}/work-items
   {"operation_id":"<fresh UUID>","title":"<lookup> reviewed recall",
    "description":"For lookup label <lookup>, return only the verification answer from the approved project memory. If no such memory is supplied, return NOT_FOUND. Do not infer an answer.",
    "priority":"normal","criteria":["The saved answer exactly matches the separately reviewed memory annotation."],
    "approvals":[{"gate":"review","required":true}]}
   ```

   Assign Ada as owner, mark the fresh definition Ready, then use **Start employee
   execution**. Read each current version from the latest response; do not guess it.
   The runtime query uses the first16 unique bounded words of title/description;
   lookup-first placement avoids truncating the sole distinctive match.

   ```text
   POST /api/v1/work-items/{new_item}/assignments
   {"operation_id":"<fresh UUID>","expected_version":<current>,"employee_id":"ada-private","role":"owner"}
   POST /api/v1/work-items/{new_item}/transitions
   {"operation_id":"<fresh UUID>","expected_version":<current>,"target":"ready","reason":"Definition reviewed for selected-memory acceptance"}
   POST /api/v1/work-items/{new_item}/executions
   {"operation_id":"<fresh UUID>","expected_version":<current>,"employee_id":"ada-private"}
   ```

4. Record the actual new run ID. Observe its Activity and memory panel, including
   the new fact ID, approval ID/operator and current text. Open the saved deliverable;
   compare its UTF-8 text to the private answer marker. A successful runtime must
   leave the new item in REVIEW with criterion and required approval still pending.
   Root may explicitly inspect, satisfy, approve with an honest operator reason,
   and complete the new item; none of those decisions is inferred from run success.

5. Save pre-withdrawal evidence, then use **Stop using** for the new fact. Wait for
   its withdrawal ACK and the UI's reviewed-store text removal message. Refresh
   the old run memory view: its use receipt remains, `current=false`, content is
   withheld. Reload the native view to verify the retained result. Do not erase
   the approved Work artifact or historical snapshot to imitate forgetting.

   ```text
   POST /api/v1/projects/{project}/reviewed-memory/{new_fact}/stop
   {"operation_id":"<fresh UUID>","expected_version":1,"reason":"Selected-memory acceptance complete"}
   ```

## Evidence required before claiming PASS

Use fresh private evidence leaves, with bounded read-only repeatable-read SQL and
exact company/project/employee/fact/run filters. Retain hashes/booleans and public
IDs in the summary; full snapshot/artifact bytes stay in 0600 evidence files.

| Stage | Required persisted/native evidence |
| --- | --- |
| Reviewed and published | New fact source equals the existing artifact; actual approval operation/operator/time and content hash; one publish job/receipt acknowledged; selected target ID/binding/epoch. In Honcho, exactly the new `record_id` under the selected owned workspace/project/company/employee, matching content/source/binding hashes, approval ID/operator and expiry; one content row, no tombstone, one publish operation. |
| Actual consumed input | One new run and Work execution, exact project/employee/revision/lifecycle; `run_reviewed_memory_uses` contains the new fact (and no old revoked fact). Its ordinal, hashes, approval, target and epoch equal snapshot `reviewed.records[].pin` and the native Honcho header. |
| Frozen bytes | `run_context_snapshots.spec_hash=sha256(spec_bytes)`; export exact `spec_bytes` once (maximum256KiB), parse with Python JSON rather than SQL jsonb field extraction, and check version3, Work origin, exact fact content/hash and rendered `spec.context.memory_context` entry of type `reviewed_project_memory`. Capture the same byte hash after terminal/reload/withdrawal. |
| Provider and output | Actual Hermes start key/reference, run terminal `completed`, dense terminal Activity and one materialized `runtime_work_outputs` receipt. Saved artifact hash equals its bytes and expected answer; item first reaches REVIEW with pending human gates. Signed GET of run/detail and artifact agrees with the native view. Work output has no Office reply or automatic post-artifact memory write. |
| Withdrawn | Fact version2/revoked; one withdraw ACK; Honcho content rows0, retained header1, tombstone1 and publish/withdraw operations retained. Old fact853d remains independently revoked/content0. Original run use and snapshot hashes remain; current run-memory text is withheld. |

The public reads are `GET /api/v1/runs/{run}`,
`GET /api/v1/work-items/{item}/executions`, and
`GET /api/v1/work-items/{item}/artifacts/{artifact}`. The public reviewed
`POST .../recall` is a local approval-registry preview: it does not prove runtime
Honcho recall. Honcho's owned internal route is
`POST /v3/ortak/workspaces/{workspace}/reviewed-projects/{project}/recall-selected`,
with company/employee/query and exact `record_ids`; it is not a native public API.
Runtime calls it through `worker_memory/selected.rs`, with no local-text fallback.

There is no durable per-read Honcho request journal. Matched native record IDs,
provenance and immutable consumed snapshot prove selected returned input; do not
claim that SQL captured the original HTTP request body. Any separate root-owned
selected-recall inspection is supplemental and must be labelled as such. Source
transport/image gates prove the ID allowlist is applied before the remote limit.

Existing evidence patterns: private
`provisioning/native-work-sol-20260905/{receipt,deliverable}.json` and
`provisioning/native-reviewed-memory69/{published,withdrawn}.json` are historical
references, never output targets for this run. Reuse the bounded `Commands.run` /
`HonchoCommands.run` transport pattern used by
[`record_private_recovery_resume.py`](../../scripts/ortak/record_private_recovery_resume.py),
with root's currently selected immutable owner registry and fresh labels. That
script itself is deliberately pinned to the old G/Work/fact IDs and must not be
rerun or relabelled as this acceptance helper. NIP-98 signing remains in the
existing native [`client.ts`](../../desktop/src/features/ortak/client.ts) path;
do not print keys, auth headers, tokens, profile contents or full secret receipts.

This one flow proves nonempty project recall for Ada and explicit withdrawal.
It does not replace the existing cross-scope/revocation/crash PG gates, prove a
live second employee or sibling-project negative, or claim runtime-consumption
opt-out/re-enable and lost-start recovery were exercised live. If an actual
provider call fails, retain that failed run; a new intentional retry gets its
own execution receipt and is reported separately.
