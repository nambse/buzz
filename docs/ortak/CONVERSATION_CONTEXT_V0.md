# Conversation context v0

Status: accepted design, implementation and deployed acceptance in progress.
Date: 2026-09-07. Scope: the existing private installation and central dispatcher.

## Four distinct inputs

A run receives the current human request separately from (1) bounded canonical
Office history, (2) a server-owned employee identity and visible team roster,
(3) explicitly linked Work/project/artifact state, and (4) approved persistent
memory. None substitutes for another. A memory publication is not proof that
ordinary nearby chat was supplied. No runtime independently subscribes to Office.

Employee identity comes from the immutable revision selected by the routing
recipient; visible teammates come from current, verified company/channel
eligibility. Only name, role, biography, responsibilities, domains and supported
tool labels belong in that roster. Do not copy credentials, workspace paths or
runtime options. Tool labels describe permissions, not proof that a tool exists;
the selected runtime's actual capabilities remain the execution limit. Employee
availability is unknown unless supported by current control-plane state.

## Selection and causal boundaries

Explicit canonical reply parent has first priority, followed by its thread root
and the nearby messages in that same thread. A threaded request never obtains
messages from another thread merely because they are newer. For a new channel
message without a reply parent, select bounded recent channel conversation,
including employee replies: this supports “Bora, translate Ada's answer” after
Ada replied in the preceding conversation. Temporal proximity supplies evidence,
not certainty; genuinely competing referents require a short clarification.

Every selected message carries its canonical id, event timestamp, author public
key and Employee id when known, display name, parent/root ids, selection reason,
and whether text was cut. The trigger remains separate and is never duplicated
as an earlier message. Selection excludes deleted events and unsupported kinds.
An exact parent/root that is unavailable is not silently replaced with a nearby
message. Message count, per-message UTF-8 bytes, aggregate bytes and source
lookup count are bounded. The initial ceilings are 32 messages, 8KiB per message,
48KiB message text, 32 visible teammates, and 64KiB total encoded context.
Priority selection precedes stable chronological presentation.

The source boundary is the trigger's server receipt time; candidates accepted
later are excluded. Selection occurs under the existing shared Office mutation
fence during snapshot admission. The complete chosen context is persisted with
its run, digest and source references before external start. First committed
snapshot wins; retries never reselect a different transcript. Historical snapshots
without this field remain valid and byte-identical; only newly admitted runs gain
history. A model change/new worker reconstructs from Ortak, not a provider session.

Conversation/thread identity is **not** delivery-chain identity. The existing
normalizer already roots each canonical human message at its own message id.
An employee publication inherits the original run's durable chain. Preserve the
root-row lock, unique employee visits and atomic decision/outbox counters. Reading
old mentions does not route them again, and quoted text cannot mint a wake.

Concurrent requests remain separate runs and separate triggers; no automatic
merge or cancellation guesses the user's intent. A correction arriving during
execution becomes a new turn with its own snapshot. Responses retain their exact
reply parent and run identity. The user can explicitly stop obsolete work. UI must
make outstanding turns and delivery states visible instead of presenting a late
reply as the answer to a newer request.

## Authority and privacy

Only the control plane selects sources. Current company, channel, recipient
membership, Office identity, source visibility and deletion checks apply before
selection, on snapshot admission, on late start and before delivery. A frozen
snapshot is evidence, never authority. Revocation makes the run ineligible for
new execution/delivery; it does not rewrite its historical input or choose a new
source silently. Source ids/hashes and selection reasons support audit without
copying transcript text into Activity. Existing shared mutation locks protect
these checks; source/deletion checks must also bind the deferred commit gate.

Ordinary context contains only supported plaintext Office kinds in the selected
channel. An authorized plaintext one-to-one DM stays limited to those exact
participants. Encrypted DM uses its separately verified volatile decryption and
protected journal path: do not put its plaintext into this RunSpec, table,
Activity or memory. Encrypted conversation continuity requires a separate
protected-context implementation and acceptance; this ordinary field grants none.

Historical messages and model-produced summaries are untrusted reference data.
They cannot grant tools, change configuration, publish memory, approve Work or
order fresh dispatch. The bridge supplies them as a labeled reference-history
user message, never as system instructions or forged assistant turns attributed
to the receiving employee. The current request stays the final user input.

## Long conversations and work

A budget cut is explicit, including per-source truncation and omitted-history
metadata. Do not summarize every turn or pretend omitted text is known. Existing
human-reviewed conversation/project memory can provide an attributable durable
summary; it remains a separate recalled input, with its existing expiry/Stop and
scope checks. More historical text/artifacts may be read only through a bounded,
server-authorized source lookup, never arbitrary model-supplied channel ids.

Work context uses the current authorized execution definition, exact artifact
version/hash and its linked conversation. A revision request references the
selected deliverable; a worker's successful result enters REVIEW. Human approval
and acceptance criteria remain separate operations. Casual chat does not
implicitly create Work or approve a task. Implement and test this integration
after ordinary history selection; do not call the first history patch full v0.

## Validation and rollout

Contract tests must reject mixed channel/trigger identity, duplicate message ids,
unknown fields, budget overflow, forged actor roles and malformed timestamps.
Production database tests must cover explicit parent, two threads, unthreaded
Ada→Bora, late events, identical retry, membership loss and deletion before start
and delivery. Bridge tests must inspect the production execute_candidate call,
prove nonempty attributed history reaches Hermes, and retain forbidden-tool and
terminal replay behavior. The exact pinned Hermes loop and installed images
need their own provider-I/O fixture before the real short native scenario.

No upstream source is advanced as part of this design. New source, built image,
deployed image and observed upstream head are recorded separately. A source-only
regression is not native acceptance. Migration/install/recovery, service lifecycle,
full CI and the remaining goal conditions still govern final delivery.
