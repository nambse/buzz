# Minimal signed employee-memory facade

Root-approved first source slice, 2026-09-06. The private signed facade and four
routes now exist under `crates/ortak-server/src/employee_memory/`, with a default-off
`HumanGrant.can_review_employee_memory` and exact POST body bounds. Six signed PG
regressions plus one configuration test are prepared; root has not run this slice.
There is no numbered migration, deployed capability, publication, runtime use or UI.

Candidate assembly order remains immutable 1–76, then
[storage candidate](sql/employee_reviewed_memory_candidate.sql), then
[source/current-data authority](sql/employee_reviewed_memory_authority_candidate.sql).
`command_current` deliberately does not authenticate SQL-credential holders:
NIP-98 and explicit deployment ceilings are enforced by the private Principal-only
facade. The owned-namespace target port still returns false. No current deployment
configurations or recovery inventories changed.

## Recommended boundary

Add one explicit, default-off deployment capability:

```rust
// HumanGrant, alongside existing default-off capabilities.
#[serde(default)]
pub can_review_employee_memory: bool,
```

This permits new employee-owned memory previews and edited approvals within the
existing `employee_ids` and **both** source/destination `channel_ids` ceilings.
It grants no automatic publication, remote namespace, runtime access or broader
Office audience. Neither `Role::Operator`, project reviewer/owner, provisioning
rights nor employee membership implies it. A configured Reader may receive this
specific capability; do not couple it to unrelated cancellation/provisioning
privileges. Existing configurations omit it and remain disabled. The existing
32-human/64-employee/64-channel bounds remain in force.

Keep the first facade private to `ortak-server`. Its only constructor accepts
the middleware-created `Principal` and current `ApiState`; no public constructor
from a key/grant JSON, no serde for an authenticated context, and no worker or
runtime entry point. Reuse the control crate's pure employee values and canonical
SQL observation. Do not place this genuine employee namespace behind
`AuthorizedWork` or invent a project to obtain its review role.

New preview/approval requires the capability, explicit employee and both channel
ceilings, current non-agent human, active employee/current Office identity, exact
own-author decided plaintext source and both human/employee memberships. The
source resolver handles canonical private 1:1 pairs and finite ancestry; no
encrypted/group fallback. Relationship human must be the authenticated actor.

Recovery is narrower and independent of the creation capability: the original
approver may list metadata and Stop within their **remaining current employee
ceiling** and active authenticated community, including after capability removal,
channel-grant/membership loss, source deletion or employee disable. Removing all
employee access still denies that user's API access; retained background cleanup
uses the old binding. Capability loss must not hide the original approver's only
Stop affordance while their employee grant remains. No admin/other-human recovery
override is introduced.

## Exact first HTTP surface

All routes use `/api/v1/employees/{employee_id}/reviewed-memory` and the existing
signed middleware. Company/community and actor are always server-resolved.
Strict DTOs reject unknown fields; UUID/hex/time/content validation precedes any
mutation. The path employee enters the approval fingerprint and cannot be
overridden in the body. Stop/replay must compare the path employee to the
retained fact's employee before returning anything; Stop keeps the candidate's
existing fact-ID/version fingerprint rather than silently changing its bytes.

| Method/suffix | Strict request | Result |
| --- | --- | --- |
| POST `/preview` | `{source_event_id,destination_channel_id,kind,human_public_key}` | `{preview:{employee_id,audience,audience_hash,source,source_hash,observed_at,valid_before,max_expires_at}}` |
| POST base | `{operation_id,fact:{source_event_id,source_event_created_at,destination_channel_id,kind,human_public_key,expected_audience_hash,content,expires_at,reviewed}}` | `{operation_id,created,effect:{fact_id,action,result_version},fact}` |
| GET base | `?after=<optional UUID>` | `{can_approve,facts:[...],next_after}`; 16 records, SQL limit 17 |
| POST `/{fact_id}/stop` | `{operation_id,expected_version:1}` | Same receipt envelope; immutable effect version 2 |

`kind` is exactly `experience` or `relationship`. `human_public_key` is explicitly
null for experience and exactly the actor's lowercase key for relationship.
Preview resolves the partition from the decided inbox; the approval echoes the
displayed exact timestamp and rechecks it against that inbox. The user selects a
message, never types its ID/time. Source/audience hashes are computed by the
server; only the displayed audience hash is submitted as an expectation.
No source text is returned or prefilled. Preview `source` is the existing pure
locator/author/evidence object; it has no approval yet and is not full provenance.

Canonical approval fingerprint remains exactly the held candidate's
`ortak-reviewed-employee-command/1`: `action`, operation ID, path employee and the
listed draft fields. Do not add observation time, freshly resolved evidence,
current epoch or a source-derived value to the immutable submitted identity.
The fact's edited content/provenance hashes are separately computed at creation.
Keep the 4 KiB text and 32 KiB escaped canonical-command ceilings; require
nonblank reviewed text and the existing reviewed-text control/redaction policy.
Lossless UTC microseconds, finite <=90-day expiry and the tighter current deadline
are new-admission checks, not replay checks.

`effect` is the original immutable receipt; `fact` is a current authorized view,
which may now be stopped or hidden. Minimal stable fact fields are ID, employee,
kind, version, status, approval/expiry/revocation timestamps and `can_stop`.
Content, source, audience/provenance and their hashes are nullable and withheld
together if capability, either channel ceiling, source evidence or current
membership is missing. Expiry alone need not erase otherwise readable history.
Only the original approver's rows are selected before LIMIT; employee grant and
company checks also precede LIMIT. List/recovery never needs a fresh preview.
There are no publish, runtime-enable or generic memory endpoints in this cut.

## Authentication, replay and atomicity

Reuse [auth.rs](../../crates/ortak-server/src/auth.rs), including exact Host/origin,
NIP-98 URL/method/body binding, required unique payload tag for POST, Schnorr/event
ID validation, ±60-second timestamp and scoped replay guard (120-second floor).
NIP-98 replay and command idempotency are separate: retry the same immutable
operation/body with a **fresh** signed event. Reusing the original auth event
continues to return 401 before the handler. The original stored `auth_event_id`
is audit linkage; a replay does not overwrite it with the new request's event.

Add a route-specific 32 KiB HTTP body limit for these employee-memory POSTs;
current non-Work routes have only 4 KiB, which cannot hold a valid 4 KiB edited
text plus JSON/envelope. Keep every other route's body limit unchanged. Bound
responses through the existing 256 KiB projection, using the reviewed-text
policy and 16-row page to fit the worst escaped content plus provenance.

Transaction order: existing shared Office fence in a separate statement;
employee-memory operation advisory lock keyed by company/actor/operation;
receipt lookup and exact action/submitted-byte/hash comparison; then scope
registration/ordered scope locks, fact/target/job locks as applicable. Use the
existing bounded facade pattern (5-second operation, 2-second statement and
500ms lock timeout). Before lookup, enforce fresh authentication and the current
employee recovery ceiling. A matched receipt returns without new-source,
new-approval capability or new-expiry validation, with a suitably hidden current
fact view. A missing receipt requires the new-admission capability and all
canonical checks. Changed submission returns 409; do not allocate a new operation
ID automatically after timeout/503. Successful Stop and receipt commit together.
Races either serialize to one receipt or fail without a partial fact/job write.

## Resolving the closed SQL command port

Its present parameters cannot prove authentication. The actual signed event,
replay decision and deployment `HumanGrant` do not exist as authoritative SQL
inputs/rows. Persisting a caller-supplied claim, adding a GUC, checking only the
public actor/event hash, or inserting a self-certified admission row would not
repair this. Do not replace its body with `true`.

Recommended smallest change, **only after root accepts the boundary and the
private signed facade exists**: move the authentication claim entirely to that
facade and replace the false SQL call with an accurately named
`ortak_employee_memory_command_current(company,employee,actor,action)` predicate.
It checks current bound active company/community, current non-agent human and
existing employee; new admission additionally requires active employee. Exact
own-source/current memberships, canonical bytes, original approver, immutable
command/operation and atomic effect checks stay in their existing SQL guards.
It does not inspect or pretend to authenticate an `auth_event_id`.
Unknown/null actions refuse; the first facade exposes approve/Stop only.

This uses the repository's existing trusted application-to-database boundary:
private signed handlers supply authenticated actors, while SQL validates current
data and concurrency. It is not database-verifiable NIP-98 authorization against
arbitrary holders of the service's SQL credentials. If root requires that stronger
boundary, retain the refusing port and separately design restricted database
roles or independently verifiable admission; a new unprotected receipt table is
not a substitute. Target ownership/protocol stays closed regardless of this
decision, and no publication/runtime capability follows from the facade.

Deployment grants are held in immutable `ApiState`, not hot-read from a changed
JSON file. Grant changes must use the existing owned API rollout/drain procedure;
editing a file does not revoke an old running process's `Principal`. No new
config watcher, database grant mirror or signing secret is proposed.

## Bounded ownership and validation proposal

| Owner slice | Exact files |
| --- | --- |
| Server ingress/config (root or assigned owner) | `crates/ortak-server/src/config.rs` (one default-off field); `auth.rs` (exact body-limit branch); `lib.rs` and `routes.rs` (module + four routes). |
| Private employee facade (one agent) | New `crates/ortak-server/src/employee_memory/{mod.rs,types.rs,authority.rs,source.rs,operations.rs,reads.rs}`. Handlers/private authenticated context, strict DTOs, current source resolution, replay/atomic writes, current/redacted views. |
| SQL integration (root) | Assemble held storage/source candidates; replace only the misleading command-auth predicate once agreed; allocate migration/parity/deletion/G work separately. No new project roles or fake projects. |
| Focused validation (same facade owner, root executes) | New `crates/ortak-server/tests/authenticated_routes/employee_memory.rs` plus bounded children, mounted in `tests/postgres_authenticated_routes.rs`; that file is also the only current non-config Rust `HumanGrant` literal owner and needs explicit false defaults for legacy fixtures. Narrow config-default/body-limit regressions stay in the existing server test modules. |
| Native later (separate owner) | Add selected-message review and employee recovery affordances after DTO freeze; no modification of held Work/conversation promotion dialogs for this backend cut. |

Acceptance should use the real signed router: default-false/Operator-alone deny;
explicit capability + own stream/DM source permits one edited approval; other
author/company/human or either missing ceiling denies; lost ACK + fresh NIP-98
same-key replay returns one receipt; repeated auth event is 401; changed draft is
409; source/employee/capability loss preserves only allowed metadata/Stop; held
membership/identity/expiry race cannot commit a new fact; Stop + receipt is atomic;
legacy project/conversation responses and absent-capability configs retain their
behavior. No test or build was run for this proposal.

## Source handoff and execution boundary

The implementation owns only `ortak-server/src/employee_memory/**`, the default-off
HumanGrant field, exact signed body-limit/router wiring, and its authenticated test
module. The storage candidate gained the matching current-data predicate calls
and an original-approver pagination index. No target port, immutable migration,
legacy memory table, runtime selector, native surface, configuration grant or
recovery inventory was opened.

Root's focused test selector is `employee_memory::` in the
`postgres_authenticated_routes` test binary: one non-PG capability-default test
and six ignored signed PostgreSQL tests. Apply immutable 1–76 normally first,
then `employee_reviewed_memory_candidate.sql`, then
`employee_reviewed_memory_authority_candidate.sql` to an explicitly disposable
port55432 database. The signed fixtures call the production router and canonical
SQL observation; they do not insert facts, operation receipts or remote ACKs.
No test, Cargo or SQL command was executed during this source implementation.
Static rustfmt only parsed/formatted the new Rust source.
