# Upstream maintenance for Ortak

Decision date: 2026-09-05. Upstream awareness is required; upstream product
compatibility and unattended upgrades are not.

## Different relationships, explicit revisions

| Dependency | Relationship | Upgrade rule |
| --- | --- | --- |
| Buzz | Reference/fork source inside independent Ortak | Review a bounded delta and selectively import or reimplement useful changes. No obligation to merge upstream wholesale. |
| Hermes | Runtime backend behind an Ortak-owned adapter/bridge | Pin the exact tested source revision and built image digest; advance through compatibility checks. A version label alone is insufficient. |
| Honcho | Memory backend behind an Ortak-owned adapter | Pin the tested API/schema and image revision; review scope, provenance, retention, and migration behavior before upgrading. |

## Working cadence

At the start of work on an affected integration, before merging an upstream
import, and before building/releasing a deployment, check official upstream
revisions and relevant release/security notes. During a long implementation,
repeat at a milestone boundary, not continuously between edits.

Keep three separate facts: **observed upstream head**, **reviewed revision**, and
**deployed/tested revision**. Reading a release note or fetching a branch does
not advance the latter two. A scheduled monitor can be added separately if
requested; this document does not install one.

For each reviewed delta, record source SHA/tag, affected Ortak surfaces, and an
`import`, `adapt`, `defer`, or `reject` decision with its reason. Prioritize
security/privacy fixes, data integrity, runtime lifecycle, cancellation,
permissions, idempotency, and event replay over unrelated product features.

Implement imports in an isolated branch. Preserve local Ortak changes, keep
upstream attribution and licensing, and test the changed production seam. Do
not make a build follow floating `main` or `latest`, and do not auto-deploy an
upstream change. A repository pin is not a deployment until its artifact is
built and the observed running revision/digest is recorded.

## Minimum upgrade evidence

- **Buzz:** affected event/authentication/membership behavior, data migration
  compatibility, and the touched UI surface when applicable.
- **Hermes:** employee/profile isolation, policy enforcement or explicit
  refusal, start idempotency, event correlation/replay, cancellation, approval
  semantics, and delivery deduplication. Probe advertised capabilities, then
  exercise the required behavior in the isolated stack.
- **Honcho:** non-creating adoption reads, authorized memory scope, provenance,
  idempotent writes, and any schema/retention migration.

Use focused tests plus a small deployed smoke at promotion; a large unrelated
test expansion is not required. Record what was not tested. Preserve the prior
artifact/configuration and backup state needed for rollback; a database schema
downgrade is not assumed safe.

## Checkpoint: 2026-09-05

### E2/private native integration checkpoint, 17:26 UTC

Official commit endpoints were checked again before the next image build.
Observed Buzz main is now
[`dad5a33865fc81a2e55b3b60746632f615ec1e3a`](https://github.com/block/buzz/commit/dad5a33865fc81a2e55b3b60746632f615ec1e3a),
one commit after the last observation. **Adapt** its two-file packaged-assets
fix in the isolated `codex/ortak-v0-delivery` checkout: Tauri receives a path
relative to its config directory, preventing Windows drive letters from being
parsed as URLs and producing an apparently successful package with no assets.
The regression separately writes through the producer path and reads through
the config-resolved consumer path. Local private-build runner arguments and
per-invocation isolation are retained. This is source integration; the already
running macOS bundle predates this change, and no Windows execution is claimed.

Hermes main is now
[`9dd6634c5635321cf38840cc30e9b51226689128`](https://github.com/NousResearch/hermes-agent/commit/9dd6634c5635321cf38840cc30e9b51226689128),
81 commits after the last observation. The official release list still selects
[v2026.8.31](https://github.com/NousResearch/hermes-agent/releases/tag/v2026.8.31).
**Defer** that main-branch delta and retain reviewed source
`29112bef099274229cadff79cdff7bf7b99c4b77`. The bounded review read full diffs for
credential-pool status-reset persistence, its auth-store merge, and prompt-builder
permission-error/backend-probe changes; it also inspected partial gateway-media
and Docker environment diffs. This is not a review of all81 commits. The chosen
Ortak worker disables upstream credential recovery/pools, context-file discovery,
environment probes, tools and gateway publication. Its containment owner is the
Ortak controller; upstream Docker teardown or compatibility flags do not replace
that owner or relax its configured protections. Revisit these deltas before any
tool-enabled profile is accepted.

Honcho main remains `be54355545b64ddb10203829d323861f52423685`; selected source and
the deployed extension image remain unchanged. Public metadata receipts and
bounded compare responses are retained under `/private/tmp/ortak-v0-evidence`
as `upstream-e2-checkpoint-20260905.json` and `upstream-e2-*.json`. New bridge
artifacts freeze Ortak's E2 output/diagnostic changes against the same22-file
Hermes source lock. Their built/tested/deployed image IDs must be recorded
separately; this observation does not deploy an upstream revision.

### Later private integration checkpoint, approximately10:25 Istanbul

Official Git refs and bounded GitHub compare responses were checked again before
promoting further local changes. Buzz `main` is still `f038cbbb0d4092a72ffd93f17916f84d2b39bb43`.
Hermes `main` is now `5ac75e91e2012497db474835a58e0139e89047cd`, eighteen commits
ahead of the earlier observed `f159e581c7afd22a5c94652c569e3859f1b994d2`.
Honcho `main` is `be54355545b64ddb10203829d323861f52423685`, fourteen commits
ahead of the selected3.1.1 commit. Annotated tag dereferences still resolve to
Hermes `29112bef099274229cadff79cdff7bf7b99c4b77` and Honcho
`5d992bc65afcfbc05a5911ab4edbaa88ef64c690`; tag object IDs are not source commits.
Honcho's GitHub latest-release endpoint returned404, so it supplied no latest
release evidence at this check.

Decision: **defer these main-branch deltas**, retain tested pins. The bounded
Hermes review inspected the new Relay streaming accumulator, browser/search
toolset membership change and Electron window-open denial patch. The desktop
patch applies to upstream Electron surfaces, which the selected headless
worker does not launch. The accumulator protects Relay's recorded streaming
response; any future import still needs Ortak journal/output and permission
checks. The current worker permits no tools. This is a three-file review,
not a full review of the eighteen commits. Honcho's comparison metadata shows
search, Qdrant, harness, MCP, telemetry and sandbox changes; their implementation
has not passed the extension's atomicity and scoped-memory gates.

The actual private artifacts remain the Hermes worker
`sha256:623fae9e3b38c75bc3cb94f73bc3d1c303bc3ed6a77765eb51fc17b54cc90b18`
and Honcho extension
`sha256:cc8b4a29c0adda08978886e205ff5c5ff0a13923e4ed15e1626b24194d0c0c21`.
No floating upstream was imported, built or deployed. Honcho build-helper/context
hardening after that artifact remains separately documented as not rebuilt.

Sources: [Hermes observed delta](https://github.com/NousResearch/hermes-agent/compare/f159e581c7afd22a5c94652c569e3859f1b994d2...5ac75e91e2012497db474835a58e0139e89047cd),
[Honcho selected-to-observed delta](https://github.com/plastic-labs/honcho/compare/5d992bc65afcfbc05a5911ab4edbaa88ef64c690...be54355545b64ddb10203829d323861f52423685).

### Earlier discovery observation

- Buzz upstream `main` remained at
  [`f038cbbb0d4092a72ffd93f17916f84d2b39bb43`](https://github.com/block/buzz/commit/f038cbbb0d4092a72ffd93f17916f84d2b39bb43).
  GitHub's comparison with the already reviewed checkpoint was identical
  (`ahead_by: 0`). See `BUZZ_IMPORT_2026-09-05.md` for the eight accepted and five
  deferred changes. No additional Buzz import was needed at this checkpoint.
- Hermes's latest published release was
  [`v2026.8.31` / 0.21.0](https://github.com/NousResearch/hermes-agent/releases/tag/v2026.8.31),
  published 2026-08-31 and resolving to commit
  [`29112bef099274229cadff79cdff7bf7b99c4b77`](https://github.com/NousResearch/hermes-agent/commit/29112bef099274229cadff79cdff7bf7b99c4b77).
  Observed `main` was
  [`f159e581c7afd22a5c94652c569e3859f1b994d2`](https://github.com/NousResearch/hermes-agent/commit/f159e581c7afd22a5c94652c569e3859f1b994d2).
  Neither is yet an accepted Ortak runtime deployment pin.
- A bounded review of three Hermes runtime files at that observed main found
  that its event handler still consumes an in-memory queue without cursor or
  `Last-Event-ID` replay. Its SQLite idempotency/status store is separate from
  an event journal and may fall back to memory on storage failure. Default
  terminal-record retention is 24 hours, not an unlimited exactly-once promise.
  Profile-prefixed execution and approval/stop handlers exist, but Ortak's
  per-run permission enforcement and an API-only clean-stack configuration
  remain unverified. Upgrading alone does not close the replay requirement.
  Sources: [run handler](https://github.com/NousResearch/hermes-agent/blob/f159e581c7afd22a5c94652c569e3859f1b994d2/gateway/platforms/api_server_runs.py#L622-L661),
  [idempotency store](https://github.com/NousResearch/hermes-agent/blob/f159e581c7afd22a5c94652c569e3859f1b994d2/gateway/platforms/api_server_run_idempotency.py#L44-L101),
  [profile API](https://github.com/NousResearch/hermes-agent/blob/f159e581c7afd22a5c94652c569e3859f1b994d2/gateway/platforms/api_server.py#L1216-L1379).
  This was not a full review of the 5,234 commits since the release, nor a
  comparison proving which revision the existing test image contains.
- The existing test container reports Hermes 0.21.0 but has no Git metadata.
  Do not infer that its source equals the release commit merely because the
  labels match. A clean build must make its exact source and artifact identity
  reproducible and observable.

### Deployed bridge artifact follow-through

The reviewed17:26 checkpoint subsequently produced tested worker
`baf828b237502da6bfdde3cd598d32b4f4f87979adbc64ee6d3fe0b548b9d79c`
and controller `090758781ef2ed301556d89dbb6f13394dbe310d13e52328f5194fe3a73520f0`.
Root deployed those exact images after credential-free constructor/run-loop,
Work output, OAuth fixture, HTTP recovery and Docker containment gates. The
selected22-file Hermes source lock remains29112bef; no newly observed upstream
main revision was imported. Actual selected OAuth health returnedOK, while
nontrivial Office output still fails at an unresolved exception boundary.
[The current controller checkpoint](../../runtime/hermes-bridge/CONTROLLER.md)
records the scoped receipts and limits. This is a deployed bridge change, not
an upstream Hermes or Honcho version upgrade.

## Integration observation —2026-09-05 19:40 UTC

Observed Buzz main `3c7f288c60d67df78577b237e27c3dfc8831aaa1` advances the prior
reviewed `dad5a338…` by one release commit (#7381). All six file diffs were
reviewed: release metadata/changelog and desktop version0.5.23 only. No runtime
logic changed; no release-version metadata was imported into independent Ortak.
Retained comparison: `/private/tmp/ortak-v0-evidence/buzz-delta-3c7f288-20260905.json`.
Hermes main remains `9dd6634c5635321cf38840cc30e9b51226689128`; selected
v2026.8.31 still peels to `29112bef099274229cadff79cdff7bf7b99c4b77`.
Honcho main remains `be54355545b64ddb10203829d323861f52423685`, with selected
3.1.1 source unchanged. No floating upstream was merged or deployed.

Current tested/deployed Ortak Hermes artifacts are worker
`sha256:7054b37a1f2f434d86f744e874eaf8aac4f0ca85ace4b9b7dbc67e7fd1d15738`
and controller
`sha256:1863dc84ed8f301acc6e7c58401efb619cd1ac7b3fb90f9914f12f6252cc5cb4`.
This adapter increment corrects the pinned credential-recovery tuple contract
and the Docker auto-removal inspection race, and adds bounded private error
coordinates. The real pinned constructor/conversation/SDK fixtures, including
three synthetic SSE APIErrors, plus12 HTTP and8 containment checks passed.
Source/image proof:
`/private/tmp/ortak-v0-evidence/hermes-diagnostics-build-ae38475f5caa40e5af47ed353419abb5`.
The same deployed pair passed one actual81-word Office/memory response and one
native cancellation with contained process stop; these do not prove general
provider reliability. Effective Codex request timeout is1800s for all four
HTTP phases, overriding factory15/None/15/10 defaults; no timeout or model
selection change was made in this diagnostic increment.


## Model variant artifact checkpoint — 2026-09-05 20:50 UTC

The Ortak bridge now admits explicit immutable model/options variants under the
same employee/profile/OAuth ownership. No upstream source was imported; the
selected Hermes revision and 22-file source lock remain unchanged. The previous
19:40 upstream observation remains the last upstream-head check.

New tested, **not deployed**, images:

- Worker: `sha256:8ee1899da85d40e26db381160f9fef50f4ba69a029699f77c7aced590b3a00f1`.
- Controller: `sha256:dbc9bcf93f7681110052da3a437ab2920906b0c171dfacc8bf07a35f51cec247`.

All94 local bridge tests passed. The exact new artifacts passed constructor,
Office/Work conversation, Codex SDK fixture,12 HTTP recovery and8 real Docker
containment checks. Four additional variant tests ran against the controller's
installed production package, with synthetic OAuth/engine fixtures and no provider
calls. They prove exact variant selection, unchanged enrollment/profile bytes,
separate readiness witnesses and old-run binding preservation; they do not prove
account access to every model. Evidence:
`/private/tmp/ortak-v0-evidence/hermes-diagnostics-build-4d896cedf63e4b09a860c859f3e1659a`.

The live private controller still uses1863dc84… and its worker uses7054b37a….
A selected configuration rollout and native model-choice acceptance remain open.


## Integration observation — 2026-09-05 22:23 UTC

Buzz main remains`3c7f288c60d67df78577b237e27c3dfc8831aaa1`; Honcho main remains
`be54355545b64ddb10203829d323861f52423685`. Hermes main is now
`ee5b5ec21e576ccf9b941f9ff71330418415a5cb`, three commits ahead of the previous
observation. The28-file delta adds terminal yield-to-background behavior plus
RSS/Reddit skill and documentation changes. The seven runtime diffs
(agent interrupt control; environment/base-output; interrupt/process registry;
terminal foreground/background) were reviewed. The selected Ortak runtime has
no terminal/tool capability, so those changes are not imported; the new skill
bodies are not adopted or treated as instructions. No Codex transport/header/
normalizer or selected OAuth leaf change is present in this observed delta.
This is a relevance review, not validation of the new upstream background-process
handoff. [Upstream comparison](https://github.com/NousResearch/hermes-agent/compare/9dd6634c5635321cf38840cc30e9b51226689128...ee5b5ec21e576ccf9b941f9ff71330418415a5cb).
The full comparison is retained privately at
`/private/tmp/ortak-v0-evidence/hermes-delta-ee5b5ec-20260905.json`.
Selected Hermes29112bef and Honcho5d992bc source revisions remain pinned.

Actual schema69 deployment uses Hermes worker8ee1899d/controllerdbc9bcf9 and
Honcho runtimefebea560, as recorded by the schema69 rollout, superseding the
historical19:40 artifact selection above. New Honcho selected-recall image
`sha256:9358bd04cd45bf654a198313e05a682300d75dc645f780e85df5d5b29f367ede`
has passed25 PG tests,12 installed local tests, exact source-file hashes and
runtime initialization; it is built/tested, not deployed. New Hermes semantic
candidate8cf05437/a02a915a passed its five pinned transport and three lifecycle
fixtures plus existing loop/HTTP/containment gates; a subsequent slow-client
shutdown hardening review is pending another image generation. It is not
currently deployed or real semantic-provider acceptance.


## Tested semantic artifact update — 2026-09-05 22:46 UTC

The slow-client cleanup correction is now included in tested worker
`sha256:ac23eb257af263294d8a233a87d806d7917cabbcbfc5410c0194122957db7153`
and controller
`sha256:f740052a21783740db5606f9cda76454ed4d936655bcbd9d4ac0688aef265c4c`.
Nine installed semantic transport/lifecycle checks, the existing constructor/
Office/Work/Codex/recovery/containment gates and105 installed unit tests passed
with synthetic fixtures and zero provider calls. Proof:
`/private/tmp/ortak-v0-evidence/hermes-diagnostics-build-baa8a04f26554e2ca38575e895f5b66d`.
These replace the previous test candidate only; live Hermes remains
worker8ee1899d/controllerdbc9bcf9. Selected upstream source29112bef and the
22-file lock remain unchanged; the22:23 upstream-head observation still applies.

### 2026-09-06 00:35 UTC integration check

Buzz main remains `3c7f288c60d67df78577b237e27c3dfc8831aaa1`; Honcho main
remains `be54355545b64ddb10203829d323861f52423685`. Hermes main advanced to
`245e48008fa814b3251f50755eb656bd9fb86cb1`. Root reviewed all six changed files
in the four-commit delta from `ee5b5ec`: scoped bot reset selection, MCP alias
collisions with static toolsets, merged listing behavior and their tests/docs.
The Ortak runtime does not run the upstream desktop/bot plugin or dynamic MCP
toolset discovery; C2 supplies one fixed reviewed schema after empty-toolset
construction. No selected Codex/OAuth leaf changed in this delta. No import or
floating deployment was performed. [Reviewed comparison](https://github.com/NousResearch/hermes-agent/compare/ee5b5ec21e576ccf9b941f9ff71330418415a5cb...245e48008fa814b3251f50755eb656bd9fb86cb1).
Retained response:
`/private/tmp/ortak-v0-evidence/hermes-delta-245e480-f91ef67f50a34455a7b9495247355ec3.json`.

Selected Hermes revision remains `29112bef099274229cadff79cdff7bf7b99c4b77`.
The new tested, not deployed, C2 worker/controller images are
`sha256:aebff616e80db46e4e0f22e1aecec2ef5330298f0e0771b69908bc0018cd4f6a`
and `sha256:032e09a5a8318f3d22c82edbd9e861150362c3bea0f66cf693d4006a10a54961`.
Their installed120 unit tests, four SDK workspace cases and eight actual
containment cases passed with the exact pinned source. Current deployed Hermes
images remain the schema69 selection until an explicit later rollout. Selected
Honcho revision remains `5d992bc65afcfbc05a5911ab4edbaa88ef64c690`; the tested
D2c image remains staged, pending the separately prepared73 rollout.

### 2026-09-06 02:02 UTC integration check

Public Git `HEAD` and `main` remain unchanged from the00:35 checkpoint:
Buzz `3c7f288c60d67df78577b237e27c3dfc8831aaa1`, Hermes
`245e48008fa814b3251f50755eb656bd9fb86cb1`, and Honcho
`be54355545b64ddb10203829d323861f52423685`. There is no new source delta to
review; the prior bounded review scope is unchanged. **Defer** upstream
advancement and retain selected/tested Hermes
`29112bef099274229cadff79cdff7bf7b99c4b77` and Honcho
`5d992bc65afcfbc05a5911ab4edbaa88ef64c690`. Their annotated release tags still
peel to those source commits. This check performed no imports, builds or
deployments and makes no claim about a newer live artifact selection.

The official latest-release endpoints returned
[Buzz desktop-v0.5.23](https://github.com/block/buzz/releases/tag/desktop-v0.5.23)
and [Hermes v2026.8.31](https://github.com/NousResearch/hermes-agent/releases/tag/v2026.8.31),
with unchanged published/updated metadata. Honcho's latest-release endpoint
returned404. Each repository's public security-advisory endpoint returned an
empty list. Raw bounded public responses, Git refs, hashes and separate
observed/reviewed/selected fields are retained at
`/private/tmp/ortak-v0-evidence/upstream74-checkpoint-sxfgtjly/receipt.json`.

### 2026-09-06 06:56 UTC pre-rollout76 checkpoint

Public Git `HEAD` and `main` are unchanged from02:02 UTC:

| Repository | Observed main | Review status |
| --- | --- | --- |
| [Buzz](https://github.com/block/buzz/commit/3c7f288c60d67df78577b237e27c3dfc8831aaa1) | `3c7f288c60d67df78577b237e27c3dfc8831aaa1` | Prior release-only six-file review remains the latest; no new delta. |
| [Hermes](https://github.com/NousResearch/hermes-agent/commit/245e48008fa814b3251f50755eb656bd9fb86cb1) | `245e48008fa814b3251f50755eb656bd9fb86cb1` | Prior bounded runtime/toolset reviews remain the latest; this does not claim full selected-to-main validation. |
| [Honcho](https://github.com/plastic-labs/honcho/commit/be54355545b64ddb10203829d323861f52423685) | `be54355545b64ddb10203829d323861f52423685` | Selected source remains reviewed/tested; the previously deferred main delta has not gained implementation validation. |

There is no new upstream source delta affecting the Office, reviewed-memory or
Hermes context-transport surfaces touched by76. Decision: **defer upstream
advancement**, retaining the exact tested Hermes
`29112bef099274229cadff79cdff7bf7b99c4b77` and Honcho
`5d992bc65afcfbc05a5911ab4edbaa88ef64c690` source pins. Their selected annotated
tags still peel to those commits. Existing deferred changes do not become
accepted because Ortak's conversation-context implementation changed.

The official latest-release endpoints still report
[Buzz desktop-v0.5.23](https://github.com/block/buzz/releases/tag/desktop-v0.5.23)
and [Hermes v2026.8.31](https://github.com/NousResearch/hermes-agent/releases/tag/v2026.8.31),
with the same release target and published/updated metadata as02:02 UTC.
Honcho's latest-release endpoint returned404. Each repository's public
security-advisory endpoint returned an empty list; this is an observation of
published advisories, not a broader security assurance.

This checkpoint performed read-only public Git-ref and bounded official GitHub
metadata requests. It did not fetch/import source, upgrade a pin, build/test an
artifact, inspect or change a running deployment, or access OAuth/provider
credentials. Deployed artifact selections remain governed by their separate
rollout receipts; no live image or model selection is inferred from this check.
Exact ref output, public responses and separate observed/reviewed/selected
fields are retained at
`/private/tmp/ortak-v0-evidence/upstream76-checkpoint-ed4qalzs/receipt.json`
(SHA256 `e3244fbbe210aad5665e93f5d1b2a0c323193c544e22c3eda5b8863c667d30bf`).

### 2026-09-06 08:48 UTC shared-connection controller checkpoint

Hermes main advanced to `9a84bee265daad14340a80d7585928cd8ea1f9eb`. Root read all six changed files in the three-commit delta: SSH prompt probes now isolate their control socket, skip remote file synchronization/session setup, and clean up their own connection. No OAuth/Codex leaf changed. The selected Ortak worker does not expose the upstream SSH/terminal toolsets. Defer upstream advancement; retain Hermes `29112bef099274229cadff79cdff7bf7b99c4b77` and the tested worker `aebff616e80db46e4e0f22e1aecec2ef5330298f0e0771b69908bc0018cd4f6a`. This checkpoint concerns an Ortak controller-only connection mapping. Latest published Hermes release remains v2026.8.31. [Reviewed delta](https://github.com/NousResearch/hermes-agent/compare/245e48008fa814b3251f50755eb656bd9fb86cb1...9a84bee265daad14340a80d7585928cd8ea1f9eb).

### 2026-09-06 12:13 UTC final feature artifact checkpoint

Observed public main refs: Buzz remains
`3c7f288c60d67df78577b237e27c3dfc8831aaa1`. Honcho remains
`be54355545b64ddb10203829d323861f52423685`. Hermes advanced to
`0cb996d977187b7e82e2d7126a0018dc6d9d5ae9`:40 commits and52 changed files
since9a84bee. Root reviewed17 full runtime/tool/gateway/update patches from the
bounded public comparison. Changes cover per-model OpenRouter routing and Nous
preference isolation; logical ordering of compacted display messages; refusal
to accidentally empty the upstream memory store; older-systemd scope arguments;
delegate progress labels; updater completion receipts; and TUI reconnect/orphan
timer ownership. Other desktop/test/documentation changes were inventoried but
are not claimed as a full source review. No Codex OAuth leaf changed in this
comparison. The selected Ortak paths do not use those upstream UI, updater,
delegation, terminal or memory tool entry points.

Decision: defer upstream advancement. Hermes remains pinned to29112bef and
Honcho to5d992bc; existing deployed worker/controller/Honcho image IDs remain
aebff616/b7401e7d/9358bd04 until the separately verified Ortak feature images
are selected. No floating source or image is imported or deployed. This ref
checkpoint does not assert a fresh release/advisory review.
[Observed comparison](https://github.com/NousResearch/hermes-agent/compare/9a84bee265daad14340a80d7585928cd8ea1f9eb...0cb996d977187b7e82e2d7126a0018dc6d9d5ae9).
The bounded response is retained at
`/private/tmp/ortak-v0-evidence/upstream-final-source-06cc69aa03514efc93f1c10f8e4b772e/hermes-compare.json`.

Bounded official response and review receipt: `/private/tmp/ortak-v0-evidence/upstream-d3-619576cb61eb460e91419387e6b6189e/receipt.json`. No import, worker rebuild or upstream deployment occurred in this checkpoint.

### 2026-09-06 16:11 UTC release checkpoint

Read-only official Git `HEAD`/`main` observations at this checkpoint:

| Repository | Observed main | Delta from the prior checkpoint |
| --- | --- | --- |
| [Buzz](https://github.com/block/buzz/commit/3c7f288c60d67df78577b237e27c3dfc8831aaa1) | `3c7f288c60d67df78577b237e27c3dfc8831aaa1` | Unchanged; prior release-only review remains applicable. |
| [Hermes](https://github.com/NousResearch/hermes-agent/commit/8d4b7f874d59841394536c72445bf7d0c6c18f2c) | `8d4b7f874d59841394536c72445bf7d0c6c18f2c` | 73 commits and 123 changed files since `0cb996d977187b7e82e2d7126a0018dc6d9d5ae9`. |
| [Honcho](https://github.com/plastic-labs/honcho/commit/be54355545b64ddb10203829d323861f52423685) | `be54355545b64ddb10203829d323861f52423685` | Unchanged; previously deferred main changes gain no additional validation. |

The bounded Hermes review read the complete comparison patches for 13 files:
`agent/{error_classifier,turn_recovery,turn_context,turn_context_compaction,turn_preflight,codex_responses_adapter,context_compressor,native_compaction,chat_completion_helpers}.py`,
`agent/transports/chat_completions.py`, `agent/verify/runner.py`,
`gateway/platforms/base.py`, and `gateway/run_busy.py`. This is not a full review
or test of the 73-commit delta. The official
[comparison](https://github.com/NousResearch/hermes-agent/compare/0cb996d977187b7e82e2d7126a0018dc6d9d5ae9...8d4b7f874d59841394536c72445bf7d0c6c18f2c)
response was 744,592 bytes, SHA256
`92ef071d412e517f69cc8e59963abbf545d05c3613c3651076d27fd825019279`.

Relevant future candidates are exact Codex masked encrypted-reasoning replay
rejection classification; Retry-After handling for retryable provider failures;
and native-compaction checkpoint deferral until real usage is available.
Gateway changes keep rate-limited sends out of plaintext fallback and return
long cooldowns to the delivery owner; compose verification now refuses observed
running containers or an unanswered daemon probe. Ortak does not use these
upstream gateway, compose verification or TUI owners. Its semantic scorer
explicitly disables reasoning replay/native compaction and owns one bounded
request. Ordinary and confidential workers start with empty conversation
history; Files may still make bounded subsequent tool turns, so reasoning-replay
recovery remains relevant to a future compatibility gate rather than being
declared universally unreachable.

Only two changed paths intersect the selected 22-file Hermes lock:
`agent/codex_responses_adapter.py` (native-checkpoint detection) and
`agent/transports/chat_completions.py` (Gemini thinking-level selection).
No OAuth/auth-store, Codex header or credential-runtime leaf changed in this
comparison. Decision: **defer** upstream advancement. No new blocker for the
current pinned flows was demonstrated by this bounded source review; future
imports must preserve Ortak's finite retries, current authority, exact output
receipts and containment. Neither a provider retry nor an upstream recovery
classification is treated as successful delivery.

Observed/reviewed main remains separate from deployed/tested source: Hermes
stays `29112bef099274229cadff79cdff7bf7b99c4b77` and Honcho stays
`5d992bc65afcfbc05a5911ab4edbaa88ef64c690`. Their selected annotated tags
still peel to those commits. Current Ortak artifact ownership remains governed
by the rollout receipts; no running process or image was inspected here.

Official latest-release endpoints still report
[Buzz desktop-v0.5.23](https://github.com/block/buzz/releases/tag/desktop-v0.5.23)
(published/updated September 5, 18:40:15 UTC) and
[Hermes v2026.8.31](https://github.com/NousResearch/hermes-agent/releases/tag/v2026.8.31)
(published August 31, 19:29:49 UTC; updated 19:57:53 UTC).
[Honcho's latest-release endpoint](https://api.github.com/repos/plastic-labs/honcho/releases/latest)
returned404. The public advisory endpoints for
[Buzz](https://api.github.com/repos/block/buzz/security-advisories),
[Hermes](https://api.github.com/repos/NousResearch/hermes-agent/security-advisories)
and [Honcho](https://api.github.com/repos/plastic-labs/honcho/security-advisories)
each returned an empty list; that is published-metadata evidence, not a security
assurance. No source checkout, import, dependency installation, test, build,
container or deployment action occurred. Only checkpoint documentation changed.
