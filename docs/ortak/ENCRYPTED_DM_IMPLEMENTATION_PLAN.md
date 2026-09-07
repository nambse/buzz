# Encrypted human–employee DM: bounded implementation plan

Status: source review only, 2026-09-06. No code, dependency, SQL, secret lookup,
provider call or activation is included. Migration numbers and rollout gates
remain with the integrating task. Existing plaintext private DMs remain intact.

The later isolated codec source addendum below records a separately authorized
implementation slice; it does not activate any of this plan's runtime paths.

This defines the encrypted-DM boundary identified in
[Remaining D](REMAINING_WORK_V1.md) and the
[memory/DM plan](MEMORY_AND_DM_NEXT_D_PLAN.md#encrypted-dm-boundary-after-the-explicit-crypto-contract).
It preserves [Architecture invariants 2–10](ARCHITECTURE_V0.md): central routing,
one dispatch, canonical participants, atomic ingress, durable retries and opaque
credential references. It deliberately narrows inherited
[VISION.md](../../VISION.md)'s server-readable/eDiscovery model for this new
encrypted mode; it does not relabel ordinary DMs as encrypted.

## Smallest complete behavior

One current human sends one encrypted text message to one current employee in
an explicitly selected, canonical private two-person DM. The central service
verifies/decrypts it, deterministically selects that employee, executes one run,
and returns one encrypted reply. The human's native client decrypts the reply.
No group DM, employee delegation, attachments, encrypted thread tree, Work
promotion, automatic memory writes, reviewed-memory recall or tool execution is
part of this first mode. A normal direct reply reference may name an already
verified rumor in the same pair; it never imports plaintext thread metadata.

The employee's authorized service and selected model necessarily receive the
input. The promise is encrypted transport, protected durable content and
participant-only product reads, not secrecy from that service, its host
administrator or the selected provider. Provider retention is a separate
boundary and must not be described as erased by local Stop or deletion.

## Confirmed reusable seams and gaps

| Existing source | Reuse / required change |
| --- | --- |
| `ortak-control/src/postgres/direct_channel.rs`, migration 73 | Reuse the exact retained two-key fingerprint, current human checks, employee identity and channel/member/TTL checks. Resolve the unique pair within the host-derived community; no client channel or outer tag supplies membership. |
| `ortak-control/src/postgres/inbox.rs`, relay `handlers/office_ingress.rs` | Ciphertext event and inbox already commit together. Current kind 1059 capture is company-cohort-wide unsupported audit, **not** permission to decrypt every recipient. Add explicit employee/pair decrypt selection before claiming crypto work. |
| `ortak-office/src/normalizer/mod.rs` | Currently refuses 1059 before selecting content, with an untrusted outer transport origin. Preserve that branch unless the distinct encrypted admission contract succeeds; do not replace the ordinary canonical event's author/channel with decrypted claims. |
| `ortak-office/src/transport.rs` | `EnvOfficeSigner` loads only explicit company/employee/ref/public-key/environment mappings, checks public-key equality and sanitizes errors. Reuse this selection pattern, not implicit access to every signer's keys. Signing health is not decrypt permission. |
| `ortak-control/src/runtime.rs`, runtime `memory_context.rs` and `postgres/memory_context.rs` | Existing `RunSpec.input` is a normal String; formats 1–4 store the full plaintext spec in `run_context_snapshots.spec_bytes`. A separate confidential persistence/transport branch is required. Existing SQL snapshot validators cannot validate ciphertext as legacy JSON. |
| Runtime output persistence, server `store/visibility.rs`, `memory.rs`, `activity_stream.rs` | Existing paths normalize/store/project plaintext run events, output drafts and memory. Current DM participant filtering is reusable but insufficient for encrypted bytes. Generic reads must never deserialize a confidential payload. |
| Hermes bridge `journal.py`, `docker_executor.py` | The journal stores assistant text in `events.payload`. `DockerEngine.launch` stages the entire request in an anonymous `TemporaryFile`; anonymity does not establish volatile storage. Both are explicit confidential-mode prerequisites. `Journal.reserve` itself retains a spec hash, not full input; keep that distinction. |
| Relay `handlers/ingest.rs` | Kind 1059 is WebSocket-only and has no canonical `channel_id`. Existing HTTP Office publishing cannot deliver it. Preserve this rule and add bounded NIP-42 WebSocket publication via `buzz-ws-client::publish_event`. |
| Native `commands/messages.rs`, `relay.rs`, `commands/identity.rs` | Current send creates ordinary channel events; HTTP `submit_signed_event_with_keys` also requires event author = auth key. Neither accepts an ephemeral gift-wrap signer. Existing NIP-44 commands are self-encryption only. Add a purpose-specific native DM command and recipient subscription/decryption, not a generic remote decrypt command. |

All paths above are under `crates/` unless named native (`desktop/src-tauri/src/`)
or Hermes (`runtime/hermes-bridge/ortak_hermes_bridge/`).

## Exact local crypto contract

The retained local crate is `nostr` **0.44.7** (Cargo.lock). Its `nip59` source
exists, but workspace `nostr` features currently select `nip44,nip98`, and the
desktop selects `nip44,nip49`; `nip59` is not selected by these declarations.
Enable the feature explicitly without changing the tested version.

Relevant local primary APIs are `nips/nip59.rs::UnwrappedGift::from_gift_wrap`,
`nip44::decrypt_to_bytes`, `Event::verify`, `UnsignedEvent::verify_id` and
`EventBuilder::{private_msg_rumor,seal,gift_wrap_from_seal}`. The unwrap helper
verifies the seal and checks rumor author = seal signer. It does **not** verify
the outer event, require a kind-13 seal, check exact recipients, or require and
validate the rumor ID. `UnsignedEvent::verify_id` accepts an absent ID;
`ensure_id` does not repair an incorrect supplied ID. Therefore use the retained
primitives in a small audited two-stage decoder rather than treating unwrap as
authorization. This also allows bounds and zeroizing buffers between stages.

1. Load the exact stored outer event by community, ID and partition timestamp.
   Recheck inbox agreement, kind 1059, ID/signature and nondeleted state. Require
   exactly one canonical `p` recipient equal to the explicitly selected employee
   key. The outer ephemeral signer and delivery connection identity establish
   no human authorship. Reject outer channel/dispatch/mention metadata.
2. Resolve only the exact selected key reference/version; decrypt NIP-44 v2 to
   bounded bytes. Parse a strict signed seal object, require kind 13 and no tags,
   verify its ID/signature. No stage logs parser input or error source strings.
3. Decrypt the seal content, parse a strict unsigned rumor, require kind 14,
   a present ID and successful `verify_id`, and seal signer = rumor author.
   Require its single `p` to equal the employee and author to equal the current
   nonautomated human in the exact pair. Reject unknown JSON fields, duplicate
   keys and ambiguous/extra participant tags. A signed rumor is not expected:
   authenticity is the verified seal over the exact encrypted rumor bytes.
4. Derive the pair hash and canonical DM from server rows. If a reply `e` exists,
   it must name a retained verified rumor of this pair, with matching provenance;
   arbitrary message/root IDs never reset a chain or authorize another employee.
   The employee's own sender-copy of a known output is correlated and nondispatching.

Proposed initial limits are input text 8 KiB, output text 8 KiB, rumor JSON
16 KiB, decrypted seal JSON 32 KiB and complete outer JSON 64 KiB. Apply the
encoded JSON limit as well as the text limit: heavily escaped text may reach
the former first. At most 16 rumor tags / 2 KiB aggregate / 256 bytes per value;
only the one `p` and optional single same-pair reply `e` are admitted initially.
Outer tags contain only its one `p`. Empty/invalid UTF-8 and NUL input are refused
before model I/O. Bounds are checked before JSON/base64 allocation and again
after each decrypt; library maximums are not the application budget. Use relay
accepted time for the job lease, and retain exact outer and inner timestamps.
Do not demand outer/seal time equality with the rumor: the local NIP-59 builder
randomizes those timestamps by up to two days.

## Authorized decrypt and one-dispatch transaction

Proposed isolated port: `OfficeEnvelopeDecryptor::decrypt(AuthorizedDmEnvelope)`
returns a non-Debug `VerifiedDmRumor` plus bounded verification metadata.
Only `dm/authorization.rs` may construct the request. Its selection pins company,
community, employee, Office binding ID, current key version/public key, opaque
decrypt reference, source outer tuple, claim generation and deadline. The
verified result is not itself final concurrency authority.

An explicit configuration capability may name the **existing** employee Office
signer reference for `dm_decrypt`, `dm_seal` and confidential key wrapping, only
after matching its public identity. Reusing material is optional; purpose
authorization is mandatory. No ambient keyring, old profile scan, OAuth store,
human desktop secret extraction or try-every-key fallback. The current
`CredentialResolver` only verifies reference presence and must not be widened
into an unrestricted secret-returning API.

Use a leased durable crypto job before decryption, bounded to two local workers,
one envelope per claim and a 5-second operation budget. A 30-second claim allows
the final transaction; no DB transaction spans crypto or provider I/O. Three
total transient attempts with delayed retry (for example 1s then 5s) end in a
retained failure. Crypto/shape/recipient mismatch is terminal and nondispatching.
Missing explicit material is a closed unavailable state with bounded retry,
visible recovery metadata and no raw error. A late result cannot change a
retired claim. Cancellation/deadline is checked before and after each local
step; any blocking execution must itself have a bounded lifetime/containment.

The final short transaction takes the shared Office fence first, then the
canonical encrypted-pair/key authority and dedupe row, then inbox/chain/run
rows. It rechecks current company/community, pair, both members, user status,
binding/key version, employee lifecycle, selected cohort and claim/deadline.
Only then persist verified provenance, the protected input and deterministic
decision/reservation/outbox together.

Deduplicate at two levels: existing `(company, outer_event_id)`, plus retained
`(company, employee, human_public_key, rumor_id)` independent of key version and
delivery wrapper. Different valid wraps of the same rumor link to the original
decision/run and get a nondispatching duplicate outcome; they never allocate a
fresh root, run or wake budget. Keep the first outer event as the existing run's
Office reference and a separate verified inner source tuple. Do not insert a
fake plaintext `events` row, change `office_inbox` author/channel facts, or use a
rumor ID where current SQL expects a stored outer event. Durable provenance
records retain the original outer ID/time/hash, seal ID/signer, inner ID/time,
human/employee/pair, selected key/binding version, and verification format/hash.
Store sensitive seal/rumor bytes only inside the protected payload.
Already finalized unsupported wrappers remain finalized; activating a pair does
not reset their inbox decisions or replay historical ciphertext automatically.

## Confidential persistence and runtime boundary

This is a required implementation slice, not a claim supplied by directory modes
or Docker volume selection. Introduce a distinct confidential origin and payload
envelope; preserve formats 1–4 byte-for-byte. The following version-1 contract is
a proposed implementation target, not an implemented capability.

### Algorithm and verified dependency boundary

Use **AES-256-GCM**, a 32-byte key, a fresh 12-byte nonce and the full 16-byte
authentication tag. Derive purpose keys with **HKDF-SHA256**. These are library
operations, not application implementations of AES, GCM, HKDF or NIP-44.

| Implementation | Locally verified source pin and intended API |
| --- | --- |
| Rust AEAD | `Cargo.lock`: `aes-gcm 0.10.3`, already used by `buzz-push-gateway`. Its cached source exposes `Aes256Gcm` with the default 16-byte tag and `AeadInPlace::{encrypt_in_place_detached,decrypt_in_place_detached}`. Add a direct dependency to the isolated confidential module's crate without upgrading the lockfile version. |
| Rust KDF/randomness | `hkdf 0.13.0` and `getrandom 0.4.3` are locked and locally cached; use `Hkdf::<sha2::Sha256>::new/expand` with existing `sha2 0.11.0`, and `getrandom::fill`. Do not mix the older AEAD crate's `rand_core` traits with workspace `rand 0.10`. |
| Python AEAD/KDF | Pinned Hermes `29112bef099274229cadff79cdff7bf7b99c4b77` declares **`cryptography==50.0.0` in its main dependencies**, with the same version and artifact hashes in `uv.lock`. Use PyCA `AESGCM` and `HKDF(algorithm=SHA256(), length=32, ...)`; `AESGCM.encrypt` returns ciphertext followed by the tag. |
| Image dependency composition | `runtime/hermes-bridge/Dockerfile` runs `uv sync --frozen --no-dev --no-install-project` against that selected source; the controller inherits the worker dependency closure. This establishes the intended default dependency path, not an installed-image crypto result. |

This source review ran no crypto import or image command. The actual selected
Linux wheel/backend and cross-language vectors still require the integrating
task's one installed-artifact gate before advertising capability. PyNaCl's
presence in the upstream lockfile alone is not a default-install witness and
is not used. No new Python dependency or provider/model change is proposed.

### Exact version-1 type and bytes

Introduce `ConfidentialPayloadIdentity`, `ConfidentialPayloadEnvelope` and a
non-Debug, non-Clone, zeroizing `OpenedConfidentialPayload`. Construction of an
identity requires the authorized canonical source observation, not a supplied
source hash. The closed identity object has exactly these keys:

```text
authority_epoch, community_id, company_id, conversation_id, employee_id,
employee_lifecycle_epoch, employee_public_key, employee_revision_id,
human_public_key, key_id, key_version, office_binding_id, rumor_id, run_id,
source_evidence_hash, source_outer_created_at, source_outer_id
```

UUIDs are canonical lowercase non-nil strings; keys, event IDs and the source
evidence SHA256 are exactly 64 lowercase hex characters. Employee ID uses the
existing bounded ASCII grammar. Epochs/key version are canonical decimal
**strings** in `0..=i64::MAX`, without signs or leading zeroes except `"0"`.
`source_outer_created_at` is the exact stored partition time as UTC
`YYYY-MM-DDTHH:MM:SS.000000Z`, with the codec's year/whole-second restrictions.
`conversation_id` is the server-resolved private channel; `office_binding_id`
and all epochs are pinned observations, not reusable current authority.
`source_evidence_hash` is SHA256 of a canonical source object with exactly:
`format` = `"ortak-confidential-dm-source/1"`, `community_id`, `company_id`,
`conversation_id`, `employee_id`, `employee_public_key`, `human_public_key`,
`office_binding_id`, `key_version`, `outer_event_id`, `outer_event_created_at`,
`outer_json_sha256`, `seal_event_id`, `seal_event_created_at`, `rumor_event_id`,
`rumor_event_created_at`, `rumor_json_sha256`, and `reply_rumor_id`. The last
field is null or a verified same-pair lowercase event ID; all other fields use
the identity grammars above. Times retain each layer's own verified timestamp.
Outer JSON means the exact bounded stored-source representation passed to the
decoder; rumor JSON means its exact verified decrypted bytes, not a second
serialization. This canonical object uses the same sorted compact encoding
defined below and binds server membership/source facts to the crypto result.
It must not reuse a legacy plaintext message hash. Keep it internal; an unkeyed
content hash is not a safe public substitute for content.

The header and envelope have exactly these fields:

```text
header = {
  "algorithm":"A256GCM",
  "format":"ortak-confidential-payload/1",
  "identity":<ConfidentialPayloadIdentity>,
  "ordinal":<integer>,
  "plaintext_bytes":<integer>,
  "purpose":"snapshot" | "runtime_event" | "reply_draft"
}
envelope = {"ciphertext":<base64(C || tag)>,"header":<header>,"nonce":<base64(nonce)>}
```

Canonical JSON is compact UTF-8, keys lexicographically ordered at every level,
with no whitespace. Header strings are restricted to the above ASCII grammars;
there are no floating numbers, optional/null fields or arbitrary text. Both
implementations reject arrays, unknown/duplicate fields, noncanonical numbers
and any input not byte-equal to its validated canonical re-encoding. Base64 is
standard RFC4648, padded where required, without whitespace; decode/re-encode
must agree. AAD is the **exact canonical header bytes**. AES-GCM ciphertext/tag
length is `plaintext_bytes + 16`; nonce length is exactly 12, never truncated.
Validate the expected full identity, purpose and ordinal before opening, then
validate the authenticated plaintext's inner identity/spec/event schema before
use. A valid tag does not grant a run, read, publication or lease.

Identity/header limits are 2 KiB each, complete envelope 96 KiB. Snapshot
plaintext is at most 48 KiB; a runtime-event object at most 32 KiB; assembled
reply-draft JSON at most 16 KiB. The existing 8 KiB input/final-text limits still
apply, independently of JSON escaping. There is one snapshot and one frozen
reply draft at ordinal 0; runtime events use the existing ordered sequence
1..512. Initial execution retains the current bounded final-response behavior;
it does not add unbounded streaming. Check encoded limits before allocation,
decoded length before crypto and inner limits after authentication.

### Key purpose, nonce ownership and recovery

Generate a fresh random 32-byte per-run master key and random UUID `key_id` only
after the current pre-admission check, in zeroizing central buffers. Wrap the
master with pinned NIP-44 v2 self-encryption to the exact selected employee Office key. The
strict wrapped plaintext is canonical JSON with exactly `format` =
`"ortak-confidential-key/1"`, `identity` = the full canonical identity JSON string,
`identity_hash` = SHA256(canonical identity), `key_id`, `master_key` = padded
base64 of those 32 bytes, `purpose` = `"confidential_master"`, and `signer_ref`.
Validate every field after unwrap; NIP-44 itself has no separate AAD argument.
The outer canonical envelope contains only `ciphertext`, `format` =
`"ortak-confidential-key-envelope/1"`, the identical `identity` string,
`purpose` = `"confidential_master"`, and `signer_ref`. Exact expected identity
and reference must match before key resolution; authenticated inner copies
prevent retagging an outer envelope to another key reference or identity. Store
only that envelope and the exact opaque key reference/version/public-key mapping. Never
persist the master, derived keys or their Debug/exception representation.

For each closed purpose derive 32 bytes using HKDF-SHA256 with IKM = master,
salt = SHA256(canonical identity), and info = ASCII
`"ortak-confidential-dm-aead/1"`, one zero byte, then the exact purpose string.
Only the central owner receives the master and `reply_draft` key. A current
authorized runtime start may receive the `snapshot` and `runtime_event` derived
keys for **that exact identity only**; the employee Office key, key resolver,
wrapped master and any other run's key never enter the bridge/child.

Every fresh encrypted record draws a random nonce from the OS. The single
writer for `snapshot`/`reply_draft` is PostgreSQL admission/materialization;
the single writer for `runtime_event` is the child's SQLite journal. PostgreSQL
copies the journal event envelope byte-for-byte instead of encrypting it again.
Each store atomically enforces unique `(key_id,purpose,nonce)` and immutable
`(run_id,purpose,ordinal)` before exposing bytes; a collision/refused persist
propagates without publishing. All retries reuse committed envelopes. A changed
body for an existing ordinal is a conflict; do not silently re-encrypt it.
Commit the wrapped key and snapshot in the same admission transaction, and
retain them through output/stop/ACK settlement. Never recreate a run master
because its ref is temporarily unavailable.

The isolated key port now exposes operation-specific `wrap_master/unwrap_master`;
it provides neither a generic callback nor a general key map. The later decrypt
and seal purpose ports must preserve the same exact-selection boundary.
Configuration must explicitly enumerate `(company, employee, office_binding_id,
key_version, public_key, credential_ref, allowed_purposes)` with distinct
`dm_decrypt`, `dm_seal`, `confidential_wrap`, `confidential_unwrap` purposes.
Current admission/read/output code, under the existing Office fence, constructs
the operation-specific selection with claim generation/deadline. Ref presence
or signer health alone cannot construct it. A retained old version permits
only explicitly authorized participant history opening or receipt recovery;
it cannot admit/start/deliver an old run. Missing selected material is a retained
unavailable result, never a reason to scan credentials or fall back to plaintext.

The first confidential spec is capped at 48 KiB, contains no recall, and requires
the explicitly selected revision's empty tool policy. No silent downgrade from
Files permissions. Protect the frozen spec, runtime assistant events, assembled
reply, any draft and runtime journal payload before a durable write. Generic
tables may retain status/ciphertext metadata only, or reference the new protected
store; their existing plaintext JSON validators/readers must explicitly reject
this origin. SQL verifies structural pins/epochs and ciphertext identity; the
authorized codec verifies the decrypted spec and its hash at freeze/retry/use.
Neither check replaces the other.

The bridge must advertise an installed, tested confidential capability bound to
the selected image digest. Use a separate authenticated
`POST /v1/confidential/runs` with strict body `{company_id, snapshot, keys}`:
`snapshot` is the above envelope, and `keys` has exactly the two base64 derived
keys `snapshot` and `runtime_event`. The body is capped at 112 KiB; keys stay in
bounded memory and are excluded from fingerprints, logs and journals. Bind the
start key/fingerprint to the canonical encrypted snapshot bytes, including its
run identity. Lookup/cancel and exact replay must not require decrypt keys or
renew authority; a fresh start/resume still needs current central authorization.
Do not accept this body at ordinary `/v1/runs`, or route a confidential run to
the ordinary endpoint when capability/key validation fails. Existing transport
authentication and loopback-or-TLS requirements remain mandatory.

The Linux controller must replace `TemporaryFile` for this branch with a bounded
`memfd_create` descriptor, write and seek it once, apply write/grow/shrink/seal
seals, and pass it as child stdin under existing launch/deadline containment.
Failure or unsupported memfd is a closed missing capability, not a disk-file
fallback. No keys/input in argv, environment or mounted configuration. Child
checks the outer identity and authenticates/decrypts the snapshot, then validates
its inner identity, empty tool policy and absent memory before supplying the
volatile spec to the selected provider. It encrypts events before journal commit,
and returns envelopes through a distinct confidential event endpoint. Journal
restart/lookup returns stored bytes without keys. Central materialization opens
them only after current authority and never passes plaintext into ordinary event
append/projector APIs. Authentication failure is a retained corruption/refusal,
not an empty successful event list or a plaintext fallback.

Keep content-bearing envelopes separate from an allowlisted metadata-only
status/stop receipt schema. A restarted controller without keys must still
record exact cancellation/containment and closed failure codes; it must not
invent a decryptable assistant event, discard pending encrypted events or
need a recovered key to settle an already known stop.

Existing no-log, read-only child root and tmpfs Hermes HOME are useful; the
installed gate must bind actual SDK session/output paths and disable core dumps
for controller and child. memfd/tmpfs do not promise protection from host memory
inspection or swap. Provider calls keep the existing selected identity/model;
no employee Office/key subscription is added to the child.

Native retains only ciphertext, frozen send copies and metadata in durable
cache/drafts; decrypted text stays in the participant-scoped view. Clear it on
scope/account change or authorization loss. Do not reuse normal plaintext
optimistic caches, persisted composer drafts, previews, notifications, FTS,
scorer payloads/caches, Work promotion or memory writes for this mode. Generic
Activity/run/memory APIs and Operator role do not grant a decrypt path. A
participant-only Activity branch can show status and, if needed, authenticated
protected details; global operational drain reports get counts/closed codes,
not private content or source metadata. All error and tracing paths must avoid
`RunSpec`'s current Debug representation and provider exception bodies.

### Exact implementation files and additive migration boundary

The next slice can implement the Rust/Python envelope codec in isolation first.
It must not enable ingress until all following persistence and exposure seams
compose. Proposed new paths are marked **new**; this document changes none.

| Boundary | Files and required change |
| --- | --- |
| Rust protected values | **New** `crates/ortak-control/src/confidential/{mod.rs,wire.rs}` for bounded opaque envelopes; **new** `crates/ortak-runtime/src/confidential/{mod.rs,crypto.rs}` for AEAD/KDF and non-Debug opened values, with narrowly scoped direct Cargo dependencies above. No widening of ordinary `RunSpec` serialization. |
| Authorized Office key/source | Existing `crates/ortak-office/src/encrypted/` decoder stays pure; **new** `encrypted/{authorization.rs,key_provider.rs}` owns purpose selection and wrapped-key codec. `normalizer/mod.rs` keeps its deny gate until the distinct encrypted ingress handler is mounted. |
| Run snapshot/events | **New** runtime `postgres/confidential.rs`; branch before `postgres/memory_context.rs` loads/inserts `spec_bytes`, `postgres.rs` appends `event.payload_json()`, and `supervisor.rs` projects raw events. `memory_context.rs` formats 1–4 remain unchanged. Only protected rows plus bounded status metadata may represent this mode. |
| Current run authority | `crates/ortak-runtime/src/postgres/authority.rs` and the new confidential PG module derive current pair/key/source authority from canonical retained metadata **before** opening content. Do not call the ordinary plaintext-message derivation on a 1059 event or require decryption to discover which authorization check to perform. Recheck at final effects and after any I/O. |
| Reply/memory | **New** runtime `confidential/output.rs` and Office encrypted outbox branch. `office_output.rs` must not insert decrypted `draft_content`; `ortak-office/src/postgres.rs` must not put it in a normal publish payload. The first mode creates no `runtime_memory_writes`, recalled memory, Work item or workspace tool row. |
| Bridge | **New** `runtime/hermes-bridge/ortak_hermes_bridge/confidential/{codec.py,wire.py,journal.py,worker.py}`; narrow routing in `service.py`, `docker_executor.py`, `hermes_candidate.py` and Rust `hermes.rs`. Existing `journal.py` plaintext `events.payload` remains exclusive to ordinary runs. New journal rows store ciphertext and sequence/status metadata; no key column. Both journals retain stop/retry accounting. |
| Server reads | `crates/ortak-server/src/{memory.rs,activity_stream.rs,store/visibility.rs}` plus a **new** participant-only confidential read module. Reject confidential rows before generic snapshot decode/event projection. Current human recipient access must not be inferred from Operator/project-review grants. |
| Native send/read/drafts | `desktop/src-tauri/src/commands/{messages.rs,identity.rs}`, `relay.rs`, `private_native.rs` and a **new** purpose-specific encrypted-DM module. Retain encrypted outer events in `channel_head_cache.rs`/`archive/store.rs`; never insert synthetic decrypted events. Branch `desktop/src/features/messages/lib/useDrafts.ts` and `ui/useDraftPersistSnapshot.ts` before the existing `buzz-drafts.v2` localStorage save/restore: native self-NIP44 encryption under the current human identity protects durable drafts, with fresh scope checks before reopening. Clear volatile display/optimistic text on account/pair/authority change; notifications/search/telemetry stay content-free. |

The integrating task allocates **one new additive migration** after the then
current immutable ledger; this plan reserves no number. It introduces an
immutable run `payload_mode` discriminator (ordinary default, explicit
`confidential_dm_v1`) and separate encrypted source/job/dedupe, scoped authority,
wrapped-key, protected-payload and frozen encrypted-delivery storage. Source
outer FK/partition facts remain real ciphertext events. Retained unique rumor
dedupe is separate from outer-wrapper uniqueness. Protected payload constraints
enforce sizes, closed purposes, immutable identity/ordinal, nonce uniqueness and
current admission epochs; SQL never attempts to decrypt.

The same migration guards against confidential rows entering ordinary snapshot,
plaintext assistant-event, output-draft or memory-write tables. It adds current
pair/key/lifecycle and final-use checks while preserving receipt-only lease,
lookup, cancellation and ACK accounting after revocation. Existing plaintext
rows/validators and migrations are not rewritten or backfilled. The SQLite
journal gets a separate versioned schema change with explicit mode guards; a
PostgreSQL migration alone cannot protect its payloads. G capture/restore and
canonical deletion inventories must include new retained rows, scoped epochs,
wrapped-key references, encrypted journal data and pending delivery obligations
before any live opt-in. Backup success must not imply key availability or
participant authorization; restoration cannot silently generate replacement keys.

## Revocation, reply and recovery

Retain the existing Office mutation fence and migration-65 lifecycle barrier;
add only the missing encrypted pair/key authority. It needs a monotonic epoch
for member removal/rejoin, key rotation, decrypt-capability revocation,
channel/source removal and restore. A value restored later cannot revive old
uses. Migration 73 freezes pair identity and fences channel/TTL mutations;
time-only expiry still needs the valid-before witness and final commit check.
Model-only revision changes with the same current Office identity do not rotate
the encryption key. Canonical key rotation selects a new version; an explicitly
retained old ref is for history/receipt cleanup only, never old-run admission.

Recheck current authority before decrypt, after decrypt, freeze, fresh/held
runtime start, output materialization, seal/wrap creation and **every actual
publication attempt**, including a frozen retry. Read permission is separately
current-participant-gated; historical key availability is not permission.
Known run lookup, stop and exact receipt settlement remain possible without
decrypting content or regaining access. Revocation stops new delivery and queues
the existing bounded cancellation lane, retaining unconfirmed containment.

Build one kind-14 rumor from the exact final text, then its signed kind-13 seals
and two kind-1059 copies (human receiver and employee sender history). Freeze
both exact signed outer byte strings, IDs and recipient ordinals atomically
before any send. Human native send likewise freezes recipient and sender copies
from the same rumor. Publish by bounded authenticated WebSocket and check the
OK event ID. Lost ACK retries reuse bytes; one successful recipient copy does
not erase the remaining copy's retry obligation. Known employee sender copies
and copied wraps never wake another run. Do not pass ephemeral outer signatures
through ordinary OfficeSigner assumptions or relax its plaintext output guard.

## Implementation order and smallest proof

1. The isolated NIP-59 decoder is now source-complete with the integrating
   task's nine-test gate below. Next implement the separate confidential
   envelope codecs and purpose-specific key ports above. One common fixed
   Rust/Python vector must assert exact identity/header, HKDF output and
   ciphertext/tag bytes in both directions, plus wrong identity/purpose/ordinal,
   nonce/tag mutation and duplicate/noncanonical object rejection. Bind the
   actual selected Python image to those APIs; do not change the unsupported gate.
2. Add the centrally allocated additive persistence/fences and confidential
   runtime/bridge lane, plus explicit participant-only reads and G deletion,
   drain, ciphertext backup and historical-key restoration inventories. All
   source/destination and codec gates must compose before any decrypt opt-in.
3. Add native encrypted send/receive for the selected two-person channel,
   private-mode command allowlist, exact send recovery and WebSocket publisher.
   Then switch only that configured encrypted pair from unsupported audit.

One focused automated gate should bind the real crypto adapter and real
transaction/transport paths: valid two-layer send/reply; wrong outer/seal/rumor
identity, recipient or missing/incorrect inner ID; truncation and limits; two
different wraps causing exactly one dispatch; held decrypt followed by member
removal/key rotation; lost publish ACK and restart using identical ciphertext;
revocation followed by rejoin refusing the old frozen start/output. Include one
distinctive synthetic canary absent from raw DB/journal/temp/cache bytes and
generic/scorer/operator responses, while the actual participant view decrypts
it. A removed crypto/provenance/at-rest guard must make that gate fail. No fake
`"encrypted-content"` fixture can establish decryption or confidential storage.

The minimal live gate is one fresh native encrypted question and employee reply,
one lost-ACK recovery observation without a second provider run, then one
member/key-authority loss proving old-content withholding and late-output
refusal. Use the already selected model/provider. Record operator review as
operator review, and retain only bounded metadata outside the protected store.

There is no demonstrated user blocker today: implementation is missing. Existing
human native identity and explicitly selected employee signer refs may suffice;
their purpose authorization/availability must be checked through the finished
interface, without discovering keys. If an exact ref is then missing or cannot
decrypt, report that concrete selection only. Do not request a new OAuth login,
credentials in chat, broad crypto approval, or group-DM setup. The earliest next
step is the isolated confidential envelope codec, not a live credential probe.

## Isolated codec source addendum — 2026-09-06

`crates/ortak-office/src/encrypted/{mod.rs,wire.rs,codec.rs}` implements the pure
two-stage decoder behind explicit crate feature `encrypted-dm = ["nostr/nip59"]`.
The only crate wiring is a cfg-gated module export; no dependency version or
normalizer/DB/runtime/native path changes. Kind 1059 remains refused by normal
routing. Confidential storage, key-purpose authorization, dedupe, actual
membership/ancestry/currentness and deadline scheduling remain unimplemented.

The API is `decode(&DmDecryptKey, &ExpectedEnvelope, outer_bytes)`. The borrowed
key must already be purpose-authorized by its caller. `ExpectedEnvelope::new`
checks distinct identities and a whole-second 1970–9999 source partition time;
it does not claim that the caller actually read canonical storage. A successful
`VerifiedDmRumor` retains exact outer expectations/hash, seal ID/time, verified
rumor ID/time/hash, optional reply claim and zeroizing text/raw-rumor buffers.
It has neither Debug, Clone nor serialization, and grants no effect authority.
Pinned Nostr internals can make transient allocations; the wrapper does not
promise process-memory, swap or crash-dump confidentiality.

The codec checks total JSON limits before each parse, separate 48 KiB/24 KiB
ciphertext ceilings before base64/crypto, NIP-44 v2, strict JSON object fields,
canonical lowercase identifiers, signed outer/seal ID/signature, exact outer
source facts, kind 1059/13/14, selected human agreement, single exact employee
recipient, present/correct rumor ID and text/tag bounds. The optional e grammar
is exactly `["e",id]` or `["e",id,"","reply"]`; same-pair ancestry remains a
future caller check. Seal tags must be empty. No plaintext input is truncated.

Nine prepared tests use fresh synthetic keys and real pinned Nostr encryption
and signing. They exercise official NIP-59 round trips, two wraps sharing an
inner ID, exact source/key checks, wrong signatures/kinds/senders/recipients/IDs,
unknown and duplicate fields at all layers, bounds/ciphertext/text failures,
and the narrow reply grammar. A shared pre-parse object-token check rejects
otherwise valid positional arrays at outer, seal and rumor layers; the ninth
regression also preserves valid leading JSON whitespace. Forged JSON is test
input to the production parser, not a parallel parser. Focused selector:
`cargo test -p ortak-office --features encrypted-dm encrypted::tests`.
No Cargo, tests, provider or live action was executed while preparing this slice.
The integrating task subsequently reported all nine tests passing (0.06 seconds,
3.27-second compile) and formatted the two affected files. That result validates
the isolated codec only; none of the confidential persistence/runtime proposal
above has been implemented or exercised.

## Isolated confidential codec source addendum — 2026-09-06

The separately authorized first envelope slice is now source-complete in
`crates/ortak-control/src/confidential/{mod.rs,wire.rs}`,
`crates/ortak-runtime/src/confidential/mod.rs` and
`runtime/hermes-bridge/ortak_hermes_bridge/confidential/{wire.py,codec.py}`.
Only the control/runtime module exports and their direct dependency declarations
were added. No database, snapshot, normalizer, runtime HTTP/worker/journal,
provider, native or server route calls these modules. Source/key authorization,
key wrapping, atomic uniqueness, persistence and delivery were outside this
codec slice; the following key-provider addendum records the next source boundary.

`ValidatedIdentity` means canonical structural claims, explicitly not a canonical
source observation or an authorized constructor. Rust `seal/open` and Python
`seal/open_payload` take an explicit caller-owned master and full expected
identity/purpose/ordinal. The public seal functions always obtain an OS nonce;
deterministic nonce injection is a private vector seam. Envelopes are immutable,
canonical and bounded before JSON/base64 decoding. Opened bytes are opaque:
successful AEAD verification does not validate a RunSpec or relax the existing
DM NUL/text policy. Rust owns zeroizing master/derived/plaintext buffers; Python
provides explicit owned-buffer close, redacted repr, blocked secret pickling and
closed errors without retained parser/backend exception context. Neither language
claims complete erasure of internal library/caller/host memory copies.

The shared literal `crates/ortak-control/src/confidential/vector.json` was
calculated by the integrating task with independent raw host PyCA 45.0.4
HKDF/AESGCM APIs, not either codec. SHA256:
`9cd7d4adff44c9daa6faf8eb4ac9a83518d8aadca624b36d8d51e582e2bf87cd`.
It fixes identity/AAD bytes, identity hash, derived key, ciphertext/tag and the
whole envelope. It is supplemented by literal RFC5869 case 1 and NIST CAVS
`gcmEncryptExtIV256.rsp` anchors. This is independent expected-data preparation,
not evidence that the new codecs or installed PyCA 50 image have passed.

Three control tests, four runtime tests and seven Python tests are prepared.
They cover the literal bytes, crypto anchors, exact binary/Unicode preservation,
identity/purpose/ordinal/key/nonce/tag substitutions, all object layers,
duplicates/unknown fields/noncanonical forms, padding bits, bounds before
decode/entropy, closed errors and owned-buffer retirement. Intended focused
selectors are `cargo test -p ortak-control confidential::tests`,
`cargo test -p ortak-runtime confidential::tests` and Python unittest discovery
of `runtime/hermes-bridge/tests/test_confidential.py` with the bridge package on
PYTHONPATH. No Cargo, Python/test, image, provider or live operation was run by
the implementing lane. The integrating task subsequently reported all three
control, four runtime and seven Python tests passing, with an independent source
review finding no issue. This remains an isolated codec result.

After this codec gate, the next implementation boundary is an explicit
purpose-authorized existing Office-key provider and wrapped-key codec, followed
by the additive encrypted admission/payload persistence transaction. That work
must supply the canonical source observation and durable nonce/ordinal/dedupe
constraints before any runtime propagation can be authorized. It must not
promote these parsed claims into an authority witness or write decrypted data
through the ordinary snapshot/event/output paths.

## Isolated Office-key provider source addendum — 2026-09-06

`crates/ortak-office/src/encrypted/key_provider/{mod.rs,wire.rs}` now implements
the next bounded source slice behind the existing opt-in `encrypted-dm` feature.
It reuses `transport::OfficeSignerBinding` as an explicitly selected public
owner/reference/public-key/environment mapping, with an Office binding UUID,
key version and closed `confidential_wrap`/`confidential_unwrap` purpose list.
The complete 1..64-entry registry is validated without resolving any secret.
Duplicate owner/version, public-key and environment aliases are refused. Each
operation resolves only its exact allowed environment entry, after expected
identity/reference and purpose checks, then verifies the loaded key's public
identity. There is no ambient discovery, fallback key trial, raw Office-key
getter, public callback with key access, generic signing or runtime key transfer.

`DmKeySelection::from_expected_claims` and control's borrowed
`ValidatedIdentity::key_claims` are explicitly structural, not current authority.
The wrapping format above authenticates all identity fields, including the
pinned revision/lifecycle/source/run/master-key identity. A new model revision
can keep the same durable Office owner/key mapping, but it cannot open an old
envelope under changed expected claims. Actual current source, pair membership,
revision/lifecycle, binding, key-purpose/epoch and deadline witnesses must be
supplied and rechecked by the future central authority/persistence caller.

Outer JSON is capped at 12 KiB before parsing, NIP-44 v2 ciphertext at 8 KiB
before base64/decryption, and authenticated plaintext at 4 KiB before inner JSON
parsing. These fit the existing bounded identity and opaque reference without
truncation. Objects must be canonical compact lexical JSON with no duplicates,
unknown keys, positional arrays or alternate encodings; base64 must be canonical
and padded. NIP-44 supplies a fresh nonce on wrapping, with no public deterministic
nonce API. Exact retained envelope bytes must be reused after uncertain persistence.
Caller-owned masters, parsed master strings and decrypted/canonical inner buffers
are zeroizing Rust values. The pinned library may retain transient internal copies;
the module makes no full process-memory erasure claim. Errors carry only closed
codes, and private key/master/envelope types expose no Debug or implicit Serde.

Six focused test sources bind actual pinned NIP-44 encryption/decryption and the
production provider's private exact-reader seam. They cover round-trip bytes and
fresh ciphertext, all identity fields, purpose-before-read, complete configuration
validation and independent duplicate constraints, retagged outer metadata,
authenticated inner purpose/reference/identity, object/encoding/size guards, and
missing/malformed/foreign selected material without fallback. No environment is
read by those tests. Selector:
`cargo test -p ortak-office --features encrypted-dm encrypted::key_provider::tests`.
Tests/builds/live operations have not been executed by this source lane. The
integrating task subsequently reported all six focused tests passing (0.04s,
5.16s compile) and an independent review without findings. It formatted the
key-provider/control accessor files. This is an isolated source capability.

The next integration boundary remains canonical encrypted source authorization
and one atomic admission persist for wrapped master plus protected snapshot,
with durable dedupe/nonce/ordinal uniqueness and retained recovery accounting.
This slice adds no admission, SQL, journal, worker, server, runtime transport or
native entrypoint and changes no existing plaintext path.

## Unactivated selection and decrypt-job source — 2026-09-06

The next source candidate is
`crates/ortak-office/src/encrypted/jobs/{mod.rs,selection.rs,repository.rs,enqueue.sql}`
and the unnumbered `docs/ortak/sql/encrypted_dm_jobs.sql`. It is not an applied
migration. Only its opt-in module export is wired; no normalizer, worker, API,
runtime or key resolver invokes these ports.

The two retained tables contain metadata only. `encrypted_dm_selections` uses an
immutable selection ID and immutable company/community/pair/employee/Office
binding/key-version/reference tuple. Explicit disable/re-enable changes only
its monotonic activation generation and server timestamps. Disabled history
cannot be deleted or overwritten. A partial unique index permits **one enabled
encrypted human pair per employee in this initial slice**, with at most 128
retained selections per company. This is an explicit activation limit: the
outer gift wrap names an employee recipient, not its human or channel. No
outer author/tag is used to guess a pair. A future multipair extension need not
rewrite historical selection rows but will require a reviewed decrypt boundary.

`ConfiguredDmPair` is configuration, not current authority. Registration checks
the shared production `direct_channel_on` resolver and an independent SQL
predicate for the same exact retained two-key fingerprint, current participants,
human automation/deactivation markers, employee lifecycle, active manifest,
owned verified Office identity and validity/TTL. Model-only revisions do not
change the selected Office key. Every job separately freezes its current
revision/lifecycle and Office generation. A later authority loss/restoration or
selection re-enable cannot silently renew that job's witness.

`encrypted_dm_decrypt_jobs` is unique by company and original outer ID. Admission
loads the exact original event partition plus canonical inbox facts, one exact
lowercase `p` recipient and bounded signed ciphertext. It derives compact sorted
outer JSON through the existing 75 canonical formatter and stores only its
hash/provenance metadata. A job may be queued only for an untouched pending1059
received after this explicit pair activation, while its selected channel and
employee belong to the enabled company cohort. Old finalized unsupported rows
and old pending receipts are never reset, scanned or replayed by this module.

Claims use a shared Office fence, immutable selection row, job and inbox locks
in that order. Per-company claim serialization and a SQL guard allow at most
two live claims. One call examines at most one due job; callers must schedule
themselves finitely. Each job has three total attempts, 1s/5s backoff after
transient missing material or abandoned lease, a 5-second crypto deadline, a
30-second nonrenewable claim, and an original 120-second receipt deadline.
Binding, channel and Office time boundaries can shorten these deadlines.
Expired/exhausted/changed-authority work becomes retained failure. Exact
generation/token/worker checks fence results; closed failure accounting can
settle a still-owned claim after ordinary authority revocation without reading
a key or content. Community quiescence still uses the existing universal fence;
this adds no deletion bypass.

`record_verified` accepts only the isolated production decoder's
`VerifiedDmRumor`. It compares original outer/pair/hash facts and persists seal
and rumor IDs/times/hash plus optional reply reference, never text or decrypted
JSON. A reply must name previously verified retained provenance from this same
pair. Exact in-budget receipt replay does not rewrite metadata. The `verified`
state retains its claim; it is **not** completed dispatch, a plaintext cache or
final-run authority. After a crash, expiration permits only a fresh bounded
decrypt attempt whose verification metadata must equal the original. The new
`lock_verified_on` port rechecks that exact current verified claim on a future
caller-owned transaction without extending its lease or creating a run.

The integrating slice must still add the atomic protected snapshot/wrapped-key
persist, independent cross-wrapper rumor dedupe, deterministic reservation/run,
deferred final lease/authority guards, confidential runtime/output transport and
receipt completion state. It must include these retained tables in G/deletion
inventory and encrypted runtime recovery before any consumer activation. The
ordinary normalizer remains unchanged: a concurrent ordinary unsupported
decision makes this candidate refuse, rather than consume or reset its history.
No SQL, test, Cargo, provider, secret or live operation was executed while
preparing this source candidate; no execution or correctness gate is claimed.
