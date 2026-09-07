# Employee-owned reviewed memory: one explicit destination

Status: isolated pure source contract and six prepared Rust tests, 2026-09-06.
The module is wired under `memory::employee`; no SQL allocation, store, API,
runtime integration, provider call or executed test is included. Existing
project/conversation facts and legacy `MemoryScope` bytes
remain unchanged. Root integration owns validation and any migration number.

[Architecture §4.5](ARCHITECTURE_V0.md) requires employee experience and explicit
human/employee relationship memory. The [remaining memory plan](MEMORY_AND_DM_NEXT_D_PLAN.md)
requires source-review and destination-sharing authority, an owned non-project
store, provenance and revocable use. The existing generic Honcho transport accepts
`EmployeeExperience` and unqualified `Relationship`; that supplies none of these
approval/identity guarantees and must not be connected directly to runs.

The first new scope has one durable employee and exactly one explicitly approved
destination channel. Relationship also names one human. It neither invents a
project nor grants employee-global use. Employee-private retention permission is
not a sharing approval: this first slice contains no private-retention branch
and cannot convert private material without the explicit sharing review below.
Automatic extraction, embeddings, generic peer representations and company truth
are outside this contract.

## Exact pure identities

Source is isolated in `crates/ortak-control/src/memory/employee/`. Public values
have private fields and explicit constructors/parsers; no generic serde entry
point widens legacy scopes. All constructors describe claims, not authorization.

```text
audience format = ortak-reviewed-employee-audience/1
company_id, durable employee_id
kind = experience | relationship
human_public_key = null for experience; explicit 32-byte key for relationship
destination_community_id, destination_channel_id (both mandatory)

source
community_id, channel_id, event_id, event_created_at
author_public_key, evidence_hash

approval format = ortak-reviewed-employee-sharing/1
approval_id, approved_by, content_hash, expires_at

provenance format = ortak-reviewed-employee-provenance/1
approval, audience, audience_hash, source, source_hash
```

The source is an exact decided plaintext Office event and PostgreSQL partition,
not a delivery-chain root, project ID, source-text label or inferred session.
Source and destination communities must agree; their channels may differ.
Relationship provenance requires its declared approver to equal its explicit
human. A future signed facade must independently bind that key to the actual
authenticated actor; pure equality is not evidence of a review.

Every JSON object uses lexicographic key order, compact UTF-8 and standard
`serde_json` string escaping. UUIDs are nonnil canonical lowercase forms; event,
public-key and hash strings are exactly 64 lowercase hexadecimal characters.
Timestamps use UTC `YYYY-MM-DDTHH:MM:SS.ffffffZ`, years 1970–9999, with lossless
microseconds and no leap-second encoding. The parser checks 2 KiB audience/4 KiB
provenance limits before decoding, rejects duplicate/unknown fields, and compares
re-encoded bytes exactly. It does not normalize noncanonical input or consult
the current clock, so valid expired history remains structurally readable.

`audience_hash` is SHA256 of audience bytes only. `source_hash` is SHA256 of
canonical `{audience_hash,format:"ortak-reviewed-employee-source/1",source}`.
Thus every original locator/author/evidence field is bound, while a changed
review does not alter source identity. `sharing_hash` is SHA256 of the complete
provenance bytes, including edited-content hash, approver, approval ID and expiry.
There is no source body, edited text, model, revision, runtime, credential or
provider selection in these identity values. A digest alone proves no source
truth, current permission or committed approval.

## Required next production boundary

The facade must resolve the source from current company/community binding,
decided inbox and exact canonical event ID/time/author/kind/channel; it computes
the evidence digest itself. It must prove current source-review authority and
separate permission to share into the destination, plus current human and
employee visibility/membership. Mere employee readability is insufficient.
The exact source-review/destination-share role policy still belongs to the next
authorized facade design; no existing project-role grant is fabricated here.

The approval operation must atomically persist the edited text (existing 4 KiB
ceiling), provenance and operation receipt with a finite expiry (existing 90-day
maximum). Future current checks compare exact approved bytes/hashes, source,
destination, current memory identity/ownership and explicit target opt-in.
Model-only revision changes preserve employee identity; they do not create new
binding ownership. A relationship run additionally resolves the actual human,
which must equal the approved relationship human. Destination-channel membership
alone does not authorize relationship disclosure.

No runtime use may begin until a genuine owned employee namespace and reviewed
publish/withdraw protocol exist. Retain source and sharing pins plus monotonic
source/destination/target revocation epochs in frozen use; removal followed by
restoration cannot revive an old use. Recheck before selected recall, freeze,
admission, current-use/output and post-ACK memory writes. Cleanup retains exact
owned target and receipt-only recovery after revocation, without renewed access.
Current loss denies use immediately; it does not invent remote-erasure ACKs.

The focused selector is `memory::employee::tests`. Its literal JSON vectors were
authored independently and reference SHA-256 digests computed over those bytes;
the tests call production constructors/parsers, with no parallel serializer.
No Cargo or test execution was performed in this source slice.

Before integration, independently validate canonical vectors for both kinds,
every audience/source/review axis, relationship mismatch, cross-community
refusal, unknown/duplicate/null fields, UTC precision and byte bounds. Real
store/API/runtime/retention/native gates remain separate: edited review → one
explicit channel publication → same employee/current human reuse → omission in
other channels/humans → Stop and exact owned withdrawal. New durable records
must join deletion and backup inventories before deployment.
