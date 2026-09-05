# Private activation composition gaps

Source review and isolated regression validation: 2026-09-05. This note identifies
remaining composition work before a real employee can activate through the
production provisioning saga. The validation below uses disposable PostgreSQL
fixtures with fake external adapters. It is not a production activation receipt
or a command to change a deployment; no provider-backed employee or preserved
external resource was activated.

The current adapters do not yet form an executable production activation path.
There are real Hermes execution, Honcho memory, and Office delivery transports,
but the provisioning Office identity and credential ports have only fake
implementations. The saga also applies one acquisition mode to every resource,
while the currently supported Hermes and Honcho activation paths require
different modes.

## Verified source seams

| Seam | Current behavior and source evidence |
| --- | --- |
| Provisioning Office identity | [`OfficeIdentityAdapter`](../../crates/ortak-control/src/office_identity.rs) requires signer proof, membership create/adopt, membership health, profile publication and owned-membership compensation. The only implementation found in repository Rust source is [`FakeOfficeIdentityAdapter`](../../crates/ortak-control/src/fakes.rs). |
| Office delivery transport | [`EnvOfficeSigner` and `HttpOfficePublisher`](../../crates/ortak-office/src/transport.rs) implement delivery-time `OfficeSigner`/`OfficePublisher`. They load an explicit company/employee/key/reference allowlist and send authorized frozen events. They do **not** implement provisioning membership or profile-publication semantics. Their existence cannot satisfy the missing Office identity port. |
| Provisioning credential resolver | [`CredentialResolver`](../../crates/ortak-control/src/credentials.rs) exposes authorized reference existence without a secret value. Only `FakeCredentialResolver` implements that port. Individual production adapters already resolve selected environment secrets, but that is not a production implementation of the saga resolver. |
| One acquisition mode | [`ProvisioningOperation::resource_mode`, `execute`, `check_ownership`, and `activate`](../../crates/ortak-control/src/provisioning.rs) send the same create/adopt mode to runtime, memory and Office. Update uses the manifest's single mode. Ownership contradicting that mode is rejected. [`activate_revision`](../../crates/ortak-control/src/postgres/provisioning.rs) writes the same mode to the revision and all three resource bindings. |
| Hermes provisioning | [`HermesAdapter::ensure_profile`](../../crates/ortak-runtime/src/hermes.rs) supports **Adopt only**, verifies the configured profile, and returns adopted ownership. Create returns unsupported `ProfileCreate`; profile deletion is unsupported. An isolated prepared profile can be adopted without touching an old profile. |
| Honcho provisioning and health | [`HonchoMemoryAdapter::ensure_resources`, `health`, `probe_capabilities`, and witness checks](../../crates/ortak-memory/src/lib.rs) require the request mode to equal the configured binding mode. Adopt performs native list-only inspection, returns adopted resources, and cannot obtain the required Recall/Remember witness. Create can establish extension ownership; healthy executable memory additionally requires explicit roundtrip validation. |
| Honcho ownership and validation | [`resources.rs`](../../crates/ortak-memory/src/resources.rs) pins the original create request and native resource identities. [`validate_memory_roundtrip`](../../crates/ortak-memory/src/validation.rs) requires a configured Create binding, retained creation identity and an explicit stable diagnostic run/time. A workspace name or arbitrary metadata cannot substitute for ownership. |
| Worker memory is not provisioning | [`WorkerMemory`](../../crates/ortak-server/src/worker_memory.rs) configures already-created bundles as Create, restores their original receipt through read-only `resume_created_resources`, and explicitly refreshes their diagnostic I/O witness. Its `ensure_resources` refuses creation. It neither activates employees nor supplies a saga entry point. |
| Saga composition and tests | Repository uses of `ProvisioningSaga::new` are in [`provisioning_saga.rs`](../../crates/ortak-control/tests/provisioning_saga.rs) and [`postgres_provisioning.rs`](../../crates/ortak-control/tests/postgres_provisioning.rs). The PostgreSQL fixture uses the real repository with fake runtime, memory, Office identity and credential adapters. No production executable currently composes these provisioning ports. |

The memory activation gate requires HealthProbe, ResourceInspect, Recall and
Remember, plus healthy workspace and both peers; see
[`ACTIVATION_REQUIRED_MEMORY_CAPABILITIES`](../../crates/ortak-control/src/memory.rs)
and [`evaluate_activation_gates`](../../crates/ortak-control/src/provisioning.rs).
Consequently this is a real refusal, not merely an inaccurate mode label:

| Attempt | Where it stops |
| --- | --- |
| Saga Create | Hermes refuses `ProfileCreate` before the saga reaches memory. |
| Saga Adopt with a Honcho Create binding | Honcho rejects the request/configuration mode mismatch, even if the bundle was independently created and validated. |
| Saga Adopt with a Honcho Adopt binding | Existing resource inspection can succeed, but health is degraded and Recall/Remember remain unavailable. |
| Saga Update | The single manifest mode repeats one of the same constraints. |

Changing a mode string, inserting successful step receipts, stamping
`validated_at`, or returning fake healthy reports would bypass these contracts.
Such fixture construction is not a production solution.

## Smallest coherent future slice

Prefer an **adopt-only private activation path for explicitly prepared fresh
resources**, rather than adding runtime profile creation and mixed per-resource
saga modes together. This is a recommendation, not implemented behavior.

1. Prepare a new isolated Hermes profile, fresh Office identity/membership and
   an extension-created Honcho bundle through explicit, journaled bootstrap
   steps. Preserve their exact public bindings, resource IDs and receipt keys.
   The activation operation adopts them because that operation did not create
   them. Never select the preserved external Cem/Zeynep resources implicitly.
2. Add production Office identity and credential-reference implementations,
   bound at construction to one company/community, a finite employee and
   channel cohort, canonical relay origin, and opaque credential references.
   Prove the exact signer key and current membership, publish the actual
   secret-free Nostr profile with stable retry bytes/receipt, and fail closed
   for unsupported creation/deletion. Delivery-time key-resolution code may
   be factored for reuse; the completed-run delivery authority must not be
   repurposed as a provisioning authorization bypass.
3. Add explicit, read-only recovery of an **already extension-owned** Honcho
   bundle for an Adopt acquisition. Preserve the original create request hash,
   receipt, deployment identity and immutable native IDs, then perform a
   separately authorized, journaled validation roundtrip. Keep saga acquisition
   ownership `Adopted`, so compensation never deletes it. Separate that fact
   from extension ownership that permits scoped I/O. Ordinary Adopt health,
   probing and resource inspection must neither write nor grant a witness.
   Arbitrary preexisting native workspaces remain inspect-only.
4. Make activation and the subsequent worker agree on exactly the same
   binding, original create identity and native resource IDs. The worker's
   current Create selection denotes extension-created ownership; it must not
   silently rewrite the saga's Adopt acquisition history. Reuse the receipt
   and explicit validation request across restart, and enforce the witness
   again at actual I/O. A document or historical bootstrap receipt alone is
   not a current execution witness.
5. Add an explicit, default-off production saga runner with durable operation
   and step keys, using the real PostgreSQL repository and those real adapters.
   Do not silently attach activation to a worker health probe or API read.

An alternative is an immutable per-resource acquisition plan, carrying distinct
runtime/memory/Office modes through requests, ownership receipts, compensation,
revision activation and persisted bindings. That is a broader contract change;
changing only `execute` would leave persistence and compensation inconsistent.
The adopt-only path avoids creating a new runtime profile API and retains the
existing no-delete guarantee for resources prepared before the saga.

## Final admission and evidence

The stale cached-evidence gap is now closed in source. Every new or resumed
activation attempt obtains a sealed
[`ActivationTarget`](../../crates/ortak-control/src/provisioning/activation.rs)
from the repository, then freshly checks runtime and memory capabilities and
health, Office membership, the actual signer public key, and credential-reference
availability. A succeeded historical `ProbeHealth` receipt remains audit history;
it cannot authorize activation. Capability reports must name the prepared
runtime and memory adapters, and the produced signer key must match the exact
prepared Office identity.

The saga starts one monotonic prepare/probe/commit budget before preparation,
clamped to 1 ms–15 seconds. PostgreSQL supplies `observed_at` and `valid_before`,
with the deadline additionally bounded by Office authority validity. The recorded
validation times use that original issuance time preceding the fresh probes;
activation does not restamp old evidence as current. Preparation releases its
transaction before any external call. The
[final transaction](../../crates/ortak-control/src/postgres/provisioning/activation.rs)
rechecks the running operation, exact activation attempt, completed prerequisites,
original and effective manifests, employee baseline and Office generation before
its own writes. It then atomically persists the revision, bindings and correlated
activation receipt.

[Migration 0056](../../migrations/0056_ortak_activation_admission.sql) validates the
finite admission deadline and exact operation/step/revision correlation at commit,
and preserves succeeded activation audit rows against later mutation. The
supported repository transaction explicitly defers its named guard immediately
before the final success write and commits next. This does not claim protection
against unrestricted SQL that changes constraint mode after that final write.
Dispatch-time memory witnesses and Office fences remain required after activation.

Central validation on 2026-09-05 passed all 25 saga tests (including five new
[freshness cases](../../crates/ortak-control/tests/provisioning_saga/freshness.rs)),
25 control unit tests, and 14 distinct PostgreSQL provisioning tests: the prior
13 passed together, followed by one focused
[same-key reuse case](../../crates/ortak-control/tests/postgres_provisioning/reuse.rs).
That case drives two actual saga activations over the same fake external resources
and proves the second revision retains the Office binding ID and original revision
provenance while refreshing verification to the exact new admission time. The four
[freshness cases](../../crates/ortak-control/tests/postgres_provisioning/freshness.rs)
exercise actual fresh commit/replay and audit immutability, changed authority,
expiry while waiting for an operation row, and expiry at the final success write
with complete rollback. Migration and desired-schema parity also passed for 0056.
These are production repository tests using synthetic adapter facts, not proof
that the missing production provisioning ports are composed or deployed.

The remaining end-to-end acceptance proof must drive the real saga over a fresh
disposable employee with no fake adapters or hand-inserted active revision. It
must cover all of the following:

- Missing/wrong signer, changed membership, foreign company/employee binding,
  replaced Honcho native resource and expired memory witness all refuse
  activation without a partial active revision.
- A lost acknowledgement and process restart preserve the same operation,
  resource identities and profile/diagnostic receipts; adopted compensation
  never removes those resources.
- Actual configured Hermes profile execution produces a bounded result with
  the selected provider, followed by canonical Office delivery and scoped
  memory behavior. Containment or protocol tests using a synthetic runner
  remain separate evidence from a provider-backed employee loop.
- The final database transaction creates one active revision, exact bindings
  and a succeeded operation only after fresh authority passes. Late expiry,
  revocation and concurrent activation races are tested at that production
  transaction seam.

The [`draft bootstrap`](../../scripts/ortak/bootstrap_private_control.py)
intentionally stops before activation. The
[`memory bootstrap receipt`](../../runtime/private-stack/MEMORY_BOOTSTRAP.md)
proves historical owned memory I/O, not employee activation or provider use.
Real PostgreSQL saga tests prove persistence and retry behavior with synthetic
adapter facts; Office HTTP tests prove signing/publishing; Hermes containment
and Honcho roundtrip checks prove their stated component contracts. None alone
proves this missing production composition or the complete private employee
loop. Keep deployed, observed and fixture-only evidence separate as required by
[`DEPLOYMENT_STRATEGY_V0.md`](DEPLOYMENT_STRATEGY_V0.md) and
[`UPSTREAM_MAINTENANCE.md`](UPSTREAM_MAINTENANCE.md).
