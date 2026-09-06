# C2: one real workspace read through Hermes

Status (2026-09-06): migration74 is integrated and the private native acceptance
passed: an actual Hermes run read the selected file, saved a deliverable, and
root opened it and recorded operator review/completion. A reproduced host-bind
SQLite SIGBUS required the controller and child to share a verified Docker local
volume. The selected worker is unchanged; the corrected controller is pinned.
The populated G74 volume capture and offline foundation restore also passed,
including retained workspace files and the completed tool journal. Restored
execution remains disabled; this is same-host storage recovery. See
[the exact G74 evidence and limits](CURRENT_PRIVATE_RECOVERY74_2026-09-06.md) and
[WORKSPACE_TOOL_C2_LIVE_RECIPE.md](WORKSPACE_TOOL_C2_LIVE_RECIPE.md) for exact pins,
evidence and the bounded rollout. The design below retains the original pre-C2
audit and planned boundaries. This acceptance is specific to the isolated
selection and does not claim general deployment readiness or personal user review.

The smallest useful next slice is a **Work run that reads an explicitly selected
text file, produces a text deliverable for human review, and shows real tool and
file-read events in Activity**. Start with one `read_workspace_text` tool and
immutable inputs. This advances C beyond the empty policy while reusing the
existing E execution/artifact/review path. File editing, terminal commands,
general network tools and approval pause/resume remain separate capabilities.

## Original pre-C2 source boundary

The reviewed source is Hermes `29112bef099274229cadff79cdff7bf7b99c4b77`, as pinned
by `runtime/hermes-bridge/hermes-source-lock.json`. Source inspection is not an
installed-image or real-provider acceptance result.

- `service.py::Bridge.validate` and `hermes_candidate.py::execute_candidate`
  require exact `EMPTY_POLICY`. Construction selects no toolsets, checks that
  `agent.tools` is empty, uses two iterations and a 120-second run budget.
- `ToollessTransport` refuses raw Responses built-in/client tool calls and
  normalized calls before upstream retry/correction. `guarded_agent_class`
  overrides five execution/delegation entry points. Removing these guards or
  selecting upstream `files` is not a supported implementation.
- Pinned `agent/agent_init.py` constructs `tools` and `valid_tool_names` from the
  global definitions registry. `_execute_tool_calls` can segment and parallelize;
  the upstream executor invokes middleware, approval machinery, environment
  selection and worker threads. Upstream `tools/file_operations.py` expressly
  implements file operations through terminal shell execution. These paths do
  not implement an Ortak workspace boundary.
- `RuntimeBinding.workspace_ref` and `PermissionPolicy.allowed_workspaces` are
  strings, not a resource registry. `ToolCapability::Files` is a policy ceiling,
  not evidence that any filesystem operation is implemented. Runtime capability
  probing currently has no file-tool feature.
- The worker has a read-only image/profile, private temporary home, a writable
  bridge journal, constrained process resources and provider network access.
  The OAuth refresh store is not mounted. There is no selected workspace mount.
- The durable bridge journal already owns start identity, cancellation and
  ordered events; its 512-event/32-KiB ceilings remain useful. It currently emits
  lifecycle/final text, not successful tool/file calls. Ortak already understands
  `tool_call.started/completed/failed` and `file.changed` with `change=read`.
  Activity rendering, persisted dense cursors and authorized SSE already exist.

## Required selected workspace contract

Add an explicit typed, server-owned `WorkspaceBinding` rather than interpreting
an existing reference as a host path. Its immutable identity contains company,
project, allowed employee, opaque workspace reference, revision, input manifest
hash and allowed operation `read_text`. Each input has an opaque file ID, logical
relative display name, media type, byte count and SHA-256. A bounded registry maps
this identity to an operator-prepared input store outside credentials and profile
directories. No model argument or ordinary HTTP field selects a host root.

The initial scope is Work-only: current project contribution/source authority,
active assignment, employee revision/lifecycle epoch, selected workspace and
current publication/retention policy must agree. An immutable run-workspace use
pins the selected input version alongside the Work execution. A resource change
requires a new version and fresh run; it cannot silently replace an admitted
file. Do not overload the memory snapshot's `memory_context` or change existing
v1/v2/v3 snapshot semantics to smuggle workspace authority into prompt text.

Policy admission accepts exactly `allowed_tools:[files]`, the one selected
workspace, no employee network scopes and no approval requirements for this
feature. The existing empty policy stays supported. The runtime reports a new
specific `workspace_text_read` capability only after its executor and selected
registry are usable. Unsupported policy combinations fail at provisioning and
start, before credentials/provider I/O. The product describes this as file
reading, even though the domain's broader Files ceiling also covers future edits.

The existing F2 prepared catalog can expose this selection after its production
ports exist. Keep profile/OAuth identity and model variants unchanged: a workspace
reference is explicitly resolved, never treated as permission to mutate the
base profile binding or reuse another employee's resources.

## Tool transport and execution

Retain the real pinned AIAgent and its selected Codex OAuth/model transport.
Initialize with all upstream toolsets disabled, verify the empty construction,
then supply exactly the reviewed function schema and `valid_tool_names` through
a checked wrapper. Every provider request must carry only that schema. Validate
raw and normalized call shape/name/size before correction can expand it; reject
provider-side built-ins and every other name.

Override execution with a small Ortak-owned sequential dispatcher. Do not call
the upstream sequential/concurrent executors or `_invoke_tool` base method.
Keep delegation/concurrent bypass entry points closed. One in-flight call per
run, at most four tool calls and five model iterations, with the existing total
run deadline; a tool wait gets at most ten seconds and never extends that total.
The function takes only a selected file ID, not an arbitrary path, URL or command.
Initial bounds: eight selected files, 16 KiB UTF-8 per file, 64 KiB combined
inputs, 16 KiB per tool result, no recursive discovery or pagination loop.

Use a pull-based tool port on the already authenticated runtime bridge:

1. The child durably reserves `(start_key, call_id, arguments_hash)` in its
   journal before waiting. A changed duplicate is a conflict. It exposes the
   pending typed request through a bounded bridge read, separate from public
   Activity summaries. The child receives no database credential or broad API
   token and cannot invoke the public human API.
2. The central worker claims a durable tool action, re-derives current Work,
   Employee and workspace authority in the existing lock order, and performs the
   selected immutable read through a dedicated `WorkspaceAdapter`.
3. Before publishing a result it rechecks current authority, lease and exact
   content hash. It commits a bounded result receipt, then sends that exact result
   through an idempotent bridge resolve operation. No database fence is held over
   provider/network waits. The bridge checks the run is still running and the
   call still pending before delivering bytes to the model.
4. Response loss retries the same retained result; it neither authorizes a new
   call nor changes the selected file. A stale or cancelled run cannot consume a
   late result. Existing Work/lifecycle/reviewed-memory output gates still decide
   whether the eventual text artifact may enter human review.

This adds a real durable request/result lane; existing `RunEventPayload` alone
is not an execution command or authorization proof. A narrowly scoped additive
schema proposal is required for workspace uses and action receipts, plus SQLite
tool-call state. Allocate its migration only after review. Existing CLI/model
profiles, policy defaults and migrations72/73 remain unchanged.

## Filesystem and cancellation boundary

The provider child gets no workspace or host-directory mount. A dedicated local
file reader with no network/process launch capability owns only the selected
immutable input root. Use directory descriptors and no-follow relative opens;
reject symlinks, hard-linked inputs, devices, sockets, FIFOs, absolute paths,
parent traversal, NUL and mismatched owner/hash/size. Validate every ancestor at
import and every final opened descriptor; `Path.resolve()` followed by `open()`
is insufficient. Copy approved inputs into a run-specific immutable store before
admission; an operator changing the original path cannot change an in-flight
read. Never point this store at the repository, OAuth, profile, journal or backups.

Every read and result buffer has a byte/deadline cap. Use a separately contained
reader process when blocking filesystem I/O cannot be interrupted; a killed
Python thread is not containment. Cancellation tombstones the tool call, blocks
new reads/results and stops/reaps both the provider child and any owned reader.
Recovery inventories exact run/child/store identities before accepting work.
It settles interrupted pending calls without rerunning a model or selecting a
new input. No completed/cancelled acknowledgement precedes containment evidence.

The input registry, pending/resolved/interrupted action journal and retained
run-workspace uses join G drain, backup/restore and community-purge accounting.
Current reads stop after binding/source removal; retained provenance must not
force deletion of canonical history or silently disappear from cleanup inventory.

## Events and acceptance

Persist the tool start before filesystem access. Complete its receipt and
`file.changed(read)`/`tool_call.completed` in one journal transaction; append a
finite failure event on refusal. Use the same call ID throughout. Public events
contain logical file name, size/hash and bounded redacted summaries, not absolute
paths or raw file contents. The model receives only the authorized bounded input.
No `terminal.*`, file-created/edit, or permission-resume success event is emitted
for this read-only feature.

Required gates are falsifiable against production seams:

- Installed pinned-image synthetic provider emits one permitted file call and a
  final response. Verify actual file access, exact schema/model/effort, ordered
  durable events and a text artifact requiring human acceptance. Every denied
  legacy tool entry, invented name, built-in, malformed or oversized call stays
  unable to touch a file/process/network scope.
- Real filesystem fixtures cover traversal, symlink/hard-link and ancestor
  replacement, devices/FIFOs, oversized/mutated inputs and sibling run/company
  stores. Inject read hang and prove exact child containment with bounded state.
- Actual PG/bridge tests cover current project/assignment/employee/workspace
  revocation before read and before result commit, call-ID collision, lost result
  ACK, restart, cancel while waiting and late output. No duplicate read-result
  receipt or artifact, no approval auto-acceptance, no stale replay.
- Native: select the prepared Files-read employee revision; queue a Work item
  referring to a known selected input; see real tool/read progress, reload during
  the call, reconnect at the durable cursor, inspect the resulting text artifact
  and accept it as a human. A separate run is cancelled while the file call waits;
  Activity must show interruption/acknowledgement and no late deliverable.

Suggested ownership: domain/control typed binding, capability and action ports;
runtime current-authority orchestration and WorkspaceAdapter; bridge checked
tool dispatcher/journal/protocol; server worker composition plus existing scoped
Activity projection; a small native capability/selection explanation. Root owns
schema integration and installed-image/real-provider/native gates. A subsequent
create-only run-draft file tool needs its own durable file-write receipt and
artifact handoff; terminal and approval-resume remain separately reviewed work.

Proposed bounded source seams, before any implementation:

| Area | Proposed files and production test seam |
| --- | --- |
| Types and capability | `crates/ortak-control/src/workspace.rs`, additions to existing `runtime.rs`; serialization and unsupported-policy tests |
| Current authority and receipts | `crates/ortak-runtime/src/workspace_tools.rs`, `src/postgres/workspace_tools.rs`; `tests/postgres_run_supervision/workspace_tools.rs` with real Work/lifecycle revocation |
| Bridge transport and exact calls | `runtime/hermes-bridge/ortak_hermes_bridge/workspace_tools.py`, `journal_tools.py`; narrow integration in existing candidate/service/journal modules and actual bridge socket tests |
| File reader | One fixed descriptor-based, isolated reader module, invoked only from selected `WorkspaceAdapter`; actual filesystem/process-boundary tests, no shell tool implementation |
| Worker composition | `crates/ortak-server/src/worker_workspace_tools.rs`; bounded claim/drain/result delivery tests |
| Installed artifact | `runtime/hermes-bridge/checks/workspace_tool_check.py`; real pinned constructor/transport/tool-result round trip with synthetic provider and owned temporary inputs |
| Native acceptance | Existing `RunPanel.tsx`, `activity.ts`, Work `ExecutionPanel.tsx` and artifact detail; extend only the capability explanation and missing tool/read presentation proven necessary by the real interaction |

The workspace-use/action schema number and any versioned runtime-wire extension
must be agreed before these files are wired. None is part of migrations72/73.
