# B1 Channel Normalization

Status: B1a complete as library code (verified 2026-09-05, review fixes re-verified the same day); B1b open

Date: 2026-09-05

Parent plan: `REMAINING_WORK_V1.md` slice B, items 2 (real `MessageNormalizer`), 3 (DM hold/reject), 4 (disabled scorer)

This document records what the channel-normalization substep actually
delivers, how it was verified, and the one gate that must close before any of
it is composed into a running worker or central routing is switched on for any
cohort. Global central routing (`ORTAK_CENTRAL_ROUTING_ENABLED`) remains off.
Nothing here is wired into a binary.

## 1. Scope of B1a

B1a is bounded to: correct **snapshot** normalization of a claimed
`office_inbox` row from canonical Office rows, safe explicit refusals, the
disabled production scorer, a conversation-eligibility set that every routing
path is intersected with, a richer decision input hash, and a last-mile guard in
run dispatch. It deliberately does **not** rewrite the authoritative routing
transaction; see §8.

## 2. Components

| Component | Location | State |
|---|---|---|
| `PgChannelNormalizer` (production `MessageNormalizer`) | `crates/ortak-office/src/normalizer/` | Library, Postgres-tested, **unwired** |
| `Normalization` / `NormalizationRefusal` port shape | `crates/ortak-control/src/ports.rs` | Shipped |
| `NormalizedMessage.eligible_employee_ids` | same | Shipped |
| Service-to-router eligibility transport and pre-budget guard | `crates/ortak-control/src/service.rs`, `crates/ortak-router/src/lib.rs` | Shipped, unit + Postgres tested |
| `RoutingProposal.eligible_employee_ids` reapplied by `reapply_guards` and narrowing `revalidate_inputs` | `crates/ortak-control/src/routing.rs` | Shipped (snapshot, not authority; §8) |
| `DisabledSemanticScorer` | `crates/ortak-control/src/scorer.rs` | Shipped; only production `SemanticScorer` |
| `routing_input_hash` v1 | `crates/ortak-control/src/service.rs` | Shipped |
| Shared channel-kind predicate | `crates/ortak-control/src/inbox.rs` (`is_supported_channel_kind`, kind constants) | Shipped; used by normalizer and runtime guard |
| Last-mile channel-kind guard in `authorize_dispatch` | `crates/ortak-runtime/src/postgres.rs` | Shipped, Postgres-tested |
| New closed `RoutingReason` codes | `crates/ortak-domain/src/routing.rs` | `semantic_scorer_disabled`, `dm_normalization_pending`, `unresolved_provenance`, `unknown_origin`, `legacy_automation_origin`, `origin_deactivated`, `origin_not_channel_member`, `channel_not_routable`, `tag_bounds_exceeded`, `target_not_channel_member` |

## 3. Trust model of the normalizer

Every field the router treats as trusted is derived from server rows; the inbox
row's own copies are only validated against the signed `events` row under the
company's `office_company_bindings` community. A disagreement is a typed
`InboxFactMismatch` that releases the claim for bounded retry and never
normalizes from the inbox copy.

Origin resolution, in order:

1. `employee_office_bindings` for the company, **every row** (historical,
   retired, expired, unverified, disabled). A key that ever belonged to an
   employee is that employee's origin and can never be a human's.
2. Otherwise the relay's automation markers: a `bot` channel membership
   anywhere in the community, or `users.agent_type` /
   `users.agent_owner_pubkey`. Such a key is refused as
   `legacy_automation_origin` and attributed as the closed integration label
   `legacy-automation:<hex>`. Automation that is not registered as an Employee
   is never a human and never routes.
3. Otherwise `users.deactivated_at` set → refused as `origin_deactivated`.
4. Otherwise a live channel member (`channel_members.removed_at IS NULL`) or a
   `relay_members` row → known human identity `human:<hex>`.
5. Anything else → refused as `unknown_origin`. Unknown fails closed.

Identity is not access. After the origin is known (employee or human), the
message must also pass the relay's own channel write rule, taken from
`buzz-relay/src/handlers/ingest.rs::check_channel_membership`: the author is a
live member of the source channel, **or** the canonical `channels.visibility`
is `open`. An employee or human who is not a live member of a private channel
is refused as `origin_not_channel_member`, with the origin still recorded
(so a queued message whose author was removed before the worker ran it is a
visible refusal, not a wake). A missing channel row is an `InboxFactMismatch`
(retryable, no decision), and any visibility value other than the literal
`open` is treated as private: an unknown value never widens access. Relay
membership alone never grants access to a private channel.

A gift wrap (kind 1059) is refused as `dm_normalization_pending` **before its
content column is selected**. Its outer signing key is a transport artifact, so
the decision's origin is recorded as `integration` /
`gift-wrap-transport:<hex>`, never as a verified human. This attribution has no
routing effect (a refusal names no candidate) and exists only so Activity does
not describe an ephemeral wrap key as a member.

Other derived facts:

- **Channel**: must exist, be neither archived nor deleted, and have
  `channel_type = 'stream'`, the type the supported kinds (9, 40002) are defined
  for. A DM-typed or otherwise unexpected channel is `channel_not_routable`.
- **Reply parent**: only the relay-persisted `thread_metadata` parent, which
  must be a stored, non-deleted event of a supported kind in the same community
  and channel; otherwise `unresolved_provenance`. A client `e … reply` marker
  without a persisted parent is also `unresolved_provenance`.
- **Loop root**: for an employee-authored event, `runs.root_message_id` of the
  run whose `office_publish` outbox row froze this exact event id; an employee
  event Ortak never published is refused rather than started as a human chain.
  A human message roots its own chain.
- **Mentions**: accepted `p` key tags resolved through the binding table. Names
  are never remapped to keys.
- **Bounds**: more than 64 tags or more than 16 distinct mention keys is
  `tag_bounds_exceeded`. The scan refuses; it never truncates and routes a
  different fallback.
- **Never taken from the message**: system origin, structured dispatch targets,
  Work assignments, chain counters. They stay empty.

## 4. Conversation eligibility

The normalizer computes `eligible_employee_ids`: employees whose **active
revision manifest** names an Office `public_key` and `signer_ref` that match an
`employee_office_bindings` row owned by the same employee, where that binding
is verified (`verified_at`), inside its validity window
(`valid_from <= now() < valid_until`), and its key is a live member of the
channel. These are the checks `OfficeDeliveryRepository` applies before
signing (`crates/ortak-office/src/postgres.rs`). The binding's `revision_id`
is the revision that **introduced** the key and is deliberately not compared
with `employees.active_revision_id`: provisioning reuses a key across
revisions without rewriting the binding, so a newer revision with the same
key and signer stays eligible, and a revision that names a different key or
signer is not, whatever the introducing binding still says. Lifecycle status
is not filtered here so the router can still explain an inactive target as
`employee_inactive`.

`InboxRoutingService` passes the set to the router without changing identity
resolution. The full company catalog still detects alias collisions and
resolves known explicit targets. Conversation eligibility is then applied
inside the existing eligibility guard **before** recipient or remaining-chain
capacity is spent. An unavailable target is an explained drop, never a reason
to fall through to semantic fan-out or consume an eligible colleague's slot.
Semantic requests contain only eligible candidates. Existing self/visited,
lifecycle, and routing-policy refusals retain their precedence.

The set is also carried on `RoutingProposal` so the commit transaction reapplies
it (`reapply_guards` drops a wake outside it) and so the "newly eligible
roster member" revalidation only considers employees inside it. This is a
snapshot; see §8.

## 5. Decision input hash

`routing_input_hash` (domain tag `ortak-routing-input-v1`) is a length-prefixed
SHA-256 over: message id, message kind, origin label, conversation (channel id,
or direct id plus participants), body, reply parent id and origin (or an
explicit no-reply marker), delivery-chain root id, dispatch targets, structured
mentions, Work assignments, candidate `(employee, revision)` pairs, the sorted
conversation-eligibility set, and the policy fingerprint. Mutable chain counters are excluded because the commit
reapplies them from the locked chain row. Refusals use the separate
`ortak-routing-refusal-v0` hash over id, reason, origin, and policy only, so an
encrypted wrap contributes nothing but its id.

## 6. Runtime last-mile guard

`authorize_dispatch` now refuses, before `bound_message_text` reads any
content, when the inbox kind is not a supported channel kind
(`UnsupportedMessageKind { kind }`), when the inbox row has no channel
(`MessageChannelMissing`), or when the inbox kind/channel disagree with the
canonical `events` row (`MessageProvenanceMismatch { field }`). A stale or
hand-seeded `run_dispatch` for a gift wrap therefore cannot hand ciphertext to
a runtime even if it reached a `wake` recipient row. The refusal is a bounded
retry on the outbox row like every other dispatch refusal.

## 7. Verification

Environment: Hermit toolchain (`. ./bin/activate-hermit`); `CARGO_HOME` and
`CARGO_TARGET_DIR` pointed at sibling worktrees per the operator's
instructions, `CARGO_INCREMENTAL=0`; disposable Postgres at
`127.0.0.1:55432` named only through `ORTAK_TEST_DATABASE_URL`. The
`postgres_channel_normalization` fixture refuses to run without that variable
and ignores `DATABASE_URL` and the desktop-relay default. No services were
installed or changed. No full `just ci`. Commands executed on 2026-09-05
after the review fixes, all from this worktree:

```
cargo fmt --check -p ortak-domain -p ortak-router -p ortak-control -p ortak-office -p ortak-runtime -p ortak-work
cargo clippy -p ortak-domain -p ortak-router -p ortak-control -p ortak-office -p ortak-runtime -p ortak-work --tests -- -D warnings
cargo test -p ortak-domain -p ortak-router -p ortak-control -p ortak-office -p ortak-runtime -p ortak-work
cargo check -p buzz-relay -p ortak-observability
ORTAK_TEST_DATABASE_URL=postgres://ortak:ortak@127.0.0.1:55432/ortak cargo test -p ortak-office --test postgres_channel_normalization -- --ignored
ORTAK_TEST_DATABASE_URL=postgres://ortak:ortak@127.0.0.1:55432/ortak cargo test -p ortak-office --tests -- --ignored
ORTAK_TEST_DATABASE_URL=postgres://ortak:ortak@127.0.0.1:55432/ortak cargo test -p ortak-control --tests -- --ignored
ORTAK_TEST_DATABASE_URL=postgres://ortak:ortak@127.0.0.1:55432/ortak cargo test -p ortak-runtime --tests -- --ignored
ORTAK_TEST_DATABASE_URL=postgres://ortak:ortak@127.0.0.1:55432/ortak cargo test -p ortak-work --tests -- --ignored
```

| Check | Result |
|---|---|
| `cargo fmt --check` on the six crates above | clean |
| `cargo clippy --tests -D warnings` on the same crates | clean |
| Non-Postgres tests, same crates (`#[ignore]` suites skipped) | 106 pass |
| `ortak-office` `postgres_channel_normalization` (11 tests) | pass |
| `ortak-office` `postgres_office_delivery` (5 tests) | pass |
| `ortak-control` `postgres_control_plane` (8) + `postgres_provisioning` (9) | pass; see note |
| `ortak-runtime` `postgres_run_supervision` (6 tests) | pass |
| `ortak-work` `postgres_work` (6 tests) | pass |
| `cargo check` of reverse dependents `buzz-relay`, `ortak-observability` | clean |

Note: on the first run of the day `postgres_control_plane` failed once in
`changed_inputs_roll_back_and_outbox_leases_are_fenced` at the
`claim_due` re-lease assertion after a `fail(..., Utc::now())`; three isolated
reruns and a full rerun of both control suites passed. That test and the
outbox lease code were not changed by the B1a review fixes; the cause was not
investigated further and is recorded here rather than hidden.

Final independent Codex verification after the recipient-budget and eligibility
hash fixes reran the six-crate fmt, clippy, non-Postgres tests, and reverse
dependent check above. The Postgres suites were rerun together with:

```
cargo test -p ortak-office -p ortak-control -p ortak-runtime -p ortak-work --tests -- --ignored
```

With the same explicitly exported disposable database URL, all 45 Postgres
tests passed (151 tests total with the 106 non-Postgres tests); no timing failure
recurred in this final run. `git diff --check` also passed. The two compact
recipient-limit regressions prove that an ineligible first mention does not
consume the sole recipient slot or last chain wake. The input-hash regression
also proves that changing only conversation eligibility changes the hash.

Falsifiability of the two review-fix regressions was checked by mutation:
with the eligibility query temporarily reverted to
`b.revision_id = e.active_revision_id` and the access rule temporarily made
unconditional, exactly the two new tests failed and the other nine passed;
restoring the code brought all eleven back to green.

Scenarios that bind the changed seams (all through `InboxRoutingService` over
`PgControlPlane` and the production normalizer, or through `RunSupervisor`
over `PgControlPlane`):

- Private channel, human author who is also a relay member: a message
  accepted while the author was a member and routed after the author was
  removed is one silent `origin_not_channel_member` decision with origin
  `human:<hex>`, no candidate, no dispatch. The same non-member relay member
  wakes Cem once the channel is `open`. Cem's own published reply (persisted
  provenance, key mention of Zeynep) is refused the same way after Cem leaves
  the private channel, rooted at its own id, with no extra chain visit.
- Eligibility follows the active manifest: revision B activated with the key
  and signer revision A introduced wakes Cem, the decision pins revision B,
  and the binding's `revision_id` still names A. A revision naming a key with
  no binding, then one naming the same key with a different signer, are each
  `target_not_channel_member` silences with no dispatch; no revision change
  rewrote the binding.
- `Cem, …` and a `p` mention of Cem when Cem's active, verified key left the
  channel: one silent decision, `target_not_channel_member`, Cem explained as a
  drop, no scorer, no dispatch, Zeynep not woken. A mixed `Cem ve @zeynep`
  wakes Zeynep once and explains Cem. An untargeted message scores only
  Zeynep. A retired binding's key rejoining the channel does not restore
  eligibility.
- `bot`-role member, `users.agent_owner_pubkey` agent, and deactivated user
  authoring a message with a Cem key mention: refused as
  `legacy_automation_origin` (origin `integration` /
  `legacy-automation:<hex>`) or `origin_deactivated`; a plain human member
  routes.
- 17 distinct mention keys: `tag_bounds_exceeded`, no dispatch.
- Reply whose persisted parent was deleted: `unresolved_provenance`.
- Archived channel, then `dm`-typed channel: `channel_not_routable`.
- Gift wrap: `dm_normalization_pending`, origin `integration` /
  `gift-wrap-transport:<hex>`, one decision, replay-safe.
- Runtime: a `wake` dispatch seeded for a kind 1059 event is refused with
  `UnsupportedMessageKind { kind: 1059 }` and no run row; a supported kind
  without channel scope is `MessageChannelMissing`; an inbox channel that
  disagrees with the canonical event is `MessageProvenanceMismatch`.
- Pure: `reapply_guards` drops a wake outside the eligible snapshot;
  `revalidate_inputs` ignores an ineligible employee joining the roster;
  every hashed field changes `routing_input_hash`.

Pre-existing scenarios (direct name, disabled scorer silence, historical
employee key with persisted root, reply parent same-channel/cross-channel/
cross-company, spoofed tags, inbox fact mismatch) still pass with the fixture
now recording Cem and Zeynep as live channel members.

## 8. B1b: mutable Office authority fencing (open, required next)

The normalizer's reads are a **snapshot taken outside any transaction**. The
authoritative `commit_routing` transaction locks the inbox row and the root
chain row and refreshes policy and the employee roster (lifecycle, active
revision, routing flag), but it does **not** re-read under those locks:

- channel membership (`channel_members.removed_at`) of the target employees,
- the employee Office binding validity/verification that made them eligible,
- channel state (`archived_at`, `deleted_at`, `channel_type`, `visibility`),
- the author's channel/relay membership, automation, and deactivation facts,
- the active revision manifest key/signer that matched the binding,
- the reply parent's existence, deletion, kind, and channel.

Between the snapshot and the commit an employee can be removed from the
channel, a binding can be retired, or the channel can be archived, and the
commit will still reserve the visit and write the dispatch outbox row.
Carrying `eligible_employee_ids` on the proposal and reapplying it in
`reapply_guards` does **not** close this: it re-checks the snapshot, not the
live rows. Running the normalizer again before commit does not close this.
Nor does merely moving those reads inside the existing READ COMMITTED
transaction: Office mutation writers do not share the delivery-root lock.

B1b is still a design/implementation gate, not a selected locking algorithm:

1. Define a coordinated row/range locking or version-fencing protocol for
   Office mutations and routing commits, covering both existing rows and
   absent-row insertions (for example, a newly registered employee key).
   Review deletion-fence ordering and deadlock behavior. Do not assume a
   SERIALIZABLE switch is compatible with the retained schema's isolation
   requirements, or silently adopt global table locks without evaluating cost.
2. Under that fence, compare the complete authoritative channel, author,
   binding, eligible roster, parent, and publish-root facts with the proposal.
   Changed inputs roll back and force a fresh snapshot; an inaccessible target
   cannot reserve a visit or produce a dispatch.
3. Revalidate and fence queued-run preparation at runtime admission, too.
   Define the linearization point and post-admission revocation/cancellation
   behavior; do not hold a database transaction across a runtime network call.
4. Exercise mutations both before the reread and after it but before commit,
   including target removal, author removal/private visibility, binding
   retirement or insertion, parent deletion, and channel archival. Assert no
   unauthorized dispatch or runtime admission, not merely that a reread ran.

Until B1b is verified, `PgChannelNormalizer` and `DisabledSemanticScorer`
stay unwired: no composition root constructs them, and central routing must
not be enabled for any cohort. Marking B1a complete does not authorize
activation.

## 9. Out of scope here

Composition root, inbox reconciler, server-owned cohort selection, Hermes
adapter, persisted delivery target, real signer/publisher, employee
activation, and trusted DM normalization remain as listed in
`REMAINING_WORK_V1.md`.
