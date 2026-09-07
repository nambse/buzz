# Owned employee memory protocol candidate

Source contract approved by root on 2026-09-06; implementation and integration
gates pending. Existing `ortak-honcho/1` protocol response, project/conversation
records and generic memory scopes remain unchanged. This adds no runtime use,
periodic validation, deployed configuration or numbered migration.

## Namespace and transport

All new endpoints are POST, below
`/v3/ortak/workspaces/{workspace}/reviewed-employees/{employee}` with existing
workspace JWT authorization and authentication-required guard. Route employee
must exactly equal the request employee. No project or session substitution.
Each request is bounded to 32 KiB, each response to 64 KiB, and one existing
10-second service transaction budget applies. No provider or embeddings calls.

Every request contains these **required** common fields:

```text
company_id: canonical nonnil UUID
employee_id: existing closed Employee string
deployment_id: canonical nonnil UUID
binding: {adapter:"honcho",endpoint_ref,workspace,user_peer,employee_peer,options:{}}
ownership: {request_hash:64lowerhex,native_ids:{workspace:string,peers:{name:id,name:id}}}
```

Binding names retain the existing Name grammar and two distinct peers. Endpoint
ref is opaque nonempty ASCII <=256 bytes, not a URL to follow. Options must be
empty in this first employee family. Reject unknown fields. `owned_bundle` must
match the exact company/employee/workspace, peer names, original creation hash
and native IDs under its existing locks before any new family operation. A
caller-supplied deployment is a pinned transport selection, not authentication;
the Rust adapter additionally compares it with its fixed authorized deployment.

Canonical namespace string (UTF-8, sorted keys, compact serde-compatible JSON):
`{"company_id":C,"employee_id":E,"format":"ortak-reviewed-employee-namespace/1"}`.
`namespace_hash = SHA256(namespace bytes)`.
**New family binding_hash** is SHA256 of canonical
`{binding,namespace_hash,protocol:"reviewed-employee/1"}`. It is deliberately not
the old reviewed-project native-identity hash. Original native identities remain
an independent exact ownership requirement on every operation and retained row.

`POST /namespace` is read-only and returns exactly:
`{protocol:"reviewed-employee/1",company_id,employee_id,deployment_id,binding,
ownership,namespace:<canonical string>,namespace_hash,binding_hash}`.
The old `/protocol` response is unchanged. Absence of this new endpoint refuses
employee-family readiness. The response alone is not an actual-I/O witness.

## Human-reviewed records

`POST /records/{record_id}/publish` adds to the common fields:

```text
target_id: UUID
destination_channel_id: UUID
idempotency_key: "employee-reviewed:publish:{company_id}:{record_id}"
content: edited UTF8 1..4096 bytes, nonblank, no Cc except LF/TAB
content_hash,source_hash,sharing_hash: 64lowerhex
provenance: canonical employee provenance JSON string, <=4096 bytes
```

The provenance is exactly the existing control `memory::employee` v1 contract
(audience/source/approval, explicit null human for experience, exact approver
human for relationship). Require canonical roundtrip, all internal hashes,
matching company/employee/destination, source+destination same community, and
content hash. Original source author and approval human must agree in this first
conservative own-source slice. Preserve exact six-digit UTC timestamps. The
service validates identity and immutable approval claims, not current Office
ACL; the central signed facade and locked current-source resolver own that.
No client-supplied epoch grants access.

`POST /records/{record_id}/withdraw` adds only `target_id`,
`destination_channel_id`, `idempotency_key` with action `withdraw`, and the three
immutable `content_hash,source_hash,sharing_hash` commitments. No content or
provenance is needed for cleanup. It is legal before publication. If header or
tombstone exists, every target/destination/hash/ownership pin must match.

The operation `request_hash` is SHA256 of the exact canonical commitment already
used by the central candidate `ortak_employee_reviewed_request_hash`:

```text
{action, binding_hash, company_id, content_hash, employee_id, fact_id:record_id,
 format:"ortak-reviewed-employee-remote-request/1", namespace_hash, sharing_hash,
 source_hash, target_id}
```

This is a typed commitment, not a claim to hash all HTTP bytes. Destination and
approval/source fields are bound by validated sharing_hash. The exact stable key
is validated separately. Retain an internal full canonical request fingerprint
for same-key payload comparison as well; it never accepts changed bytes on retry.

Publish returns 201 once, 200 on exact replay. Withdraw returns 200. Both return
this exact metadata shape, **never text**, with `request_hash` added:

```text
{protocol:"reviewed-employee/1",company_id,employee_id,deployment_id,
 workspace_id,record_id,target_id,destination_channel_id,namespace_hash,
 binding_hash,status:"active"|"expired"|"withdrawn",
 content:null,content_hash,source_hash,sharing_hash,
 provenance:null|<canonical string>,expires_at:null|UTC6,
 erased_from_reviewed_store:bool,tombstone_at:null|UTC6}
```

The three hash commitments remain present for withdrawal-before-publication;
provenance/expiry remain null until a publication header exists. Publication after
withdrawal may retain the exact header/receipt, but **never restore content**.
Provenance contains no original or edited text. No runtime implication follows
from an ACK. Original native ownership, header and tombstone are immutable;
content INSERT is forbidden after tombstone, DELETE requires tombstone, and
deferred guards require atomic operation+header/content or tombstone+no-content.
Per namespace cap 1024 distinct record IDs; action uniqueness is record+action.
One scheduled withdraw handles both expiry and revocation; no third expire key.

`POST /recall-selected` adds `destination_channel_id`,
`human_public_key` (required nullable field), and `record_ids` (1..8 distinct UUIDs).
No broad recall/list or query ranking endpoint in this first slice. It returns
`{records:[<metadata shape, with active content and canonical provenance>],
truncated:bool}`; no request_hash per record. It only selects the explicit IDs,
active/unexpired content, same destination, and for relationship exact nonnull
human equality. Preserve submitted ID order; at most eight records/8192 text
bytes. Missing/ineligible IDs remain absent. It cannot authorize a caller's human
claim: central current run-origin resolution is required before use. Runtime
composition remains disconnected in this slice; this endpoint is only a strict
adapter primitive and controlled protocol acceptance seam.

## Finite explicit namespace diagnostic

Diagnostic data is a distinct subfamily, never selectable as reviewed memory.
An operation has one caller-journaled UUID, revision, lifecycle and a synthetic
challenge (64 lowercase hex, not user text). No periodic or per-read probes.

`POST /diagnostics/{operation_id}/write` adds common fields plus:
`employee_revision_id:UUID,employee_lifecycle_epoch:nonnegative i64,challenge:64hex`.
The diagnostic request hash is SHA256 of canonical
`{format:"ortak-reviewed-employee-diagnostic/1",operation_id,namespace_hash,
binding_hash,employee_revision_id,employee_lifecycle_epoch,challenge}`.
One immutable header and synthetic content commit atomically, replay exact only.
Retain ownership/deployment, hash and revision/lifecycle. Cap 128 diagnostic IDs
per namespace; no automatic deletion/reset of retained diagnostic history.

`POST /diagnostics/{operation_id}/read` and `/withdraw` use the same required
common fields plus `employee_revision_id,employee_lifecycle_epoch,challenge_hash`
where challenge_hash=SHA256(challenge UTF8). They require the immutable matching
operation; read never creates. Withdraw may precede uncertain write, stores an
irreversible tombstone even if write never committed, and returns confirmed
synthetic-content absence. Delayed write cannot resurrect it. Pins/commitments
must agree if a header already exists. The write/read request hash is not reused
as the cleanup request hash; cleanup hash is SHA256 canonical
`{format:"ortak-reviewed-employee-diagnostic-withdraw/1",operation_id,
 namespace_hash,binding_hash,employee_revision_id,employee_lifecycle_epoch,
 challenge_hash}`.

All diagnostic responses have exactly the namespace response identity fields
plus `operation_id,employee_revision_id,employee_lifecycle_epoch,challenge_hash,
write_request_hash:null|64hex,withdraw_request_hash:null|64hex,
challenge:null|64hex,erased:bool,tombstone_at:null|UTC6`.
Only `/read` may return challenge and only before withdrawal. Write/withdraw ACKs
never include it. The adapter must perform one exact readback and validate the
confirmed cleanup ACK before minting its private process-local witness. If any
step is uncertain, only same-operation cleanup recovery is allowed, never a
readiness claim. After completed cleanup/restart, validation needs a fresh
explicit durable operation; an old tombstone cannot prove a new readback.
All retries are bounded by the caller's retained attempt budget; no remote retry
loop or provider action. Witness freshness is at most 55 seconds **for admitting
initial namespace registration only**, tied to exact namespace/native identities,
revision and lifecycle. The diagnostic receipt remains immutable registration
evidence. Ordinary publication/selected recall rechecks retained registration,
current binding/authority and read-only namespace ownership without another
diagnostic. A separate explicit deployment-selection deadline governs continued
target availability and is never extended by a health read. Model-only changes
keep namespace identity but invalidate current authorization evidence where
required. Historical cleanup uses the original owned binding without reacquiring
current source/employee authority.

## Central boundary and integration

Worker configuration selects a default-empty finite set of destination channels.
It does not schedule diagnostic validation implicitly. Only the private adapter
witness can be passed to target registration; source SQL checks current data and
declares its trusted application/SQL-credential boundary honestly. Publication
requires explicit original-human instruction, exact new fact approval, current
source/destination authority and current owned target. Claim/prepare/ACK/failure
retain exact live lease/attempt identity; cleanup never switches binding after
revocation. Future runtime pins must include actual requester, source/destination
epochs and target consumption epoch. No manual-Work fallback or local text copy.

Honcho table names and main candidate additions will be listed at source handoff.
They require deletion/backup inventories and actual installed-protocol tests
before deployment. Existing already-owned employee resources may be reused only
after exact original creation and current native-identity agreement; generic
peer existence or process health is never ownership evidence.
