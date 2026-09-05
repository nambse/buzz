# Ortak deployment strategy v0

Decision date: 2026-09-05. This records the owner's clarification during B1 work
and supersedes any assumption that the existing Coolify stack must become
production or be adopted before independent implementation can proceed.

## Clean deployment, existing test environment preserved

The existing Hetzner/Coolify Hermes, Honcho, and Buzz services are test/reference
instances. Ortak may start with a separate clean stack. Their current topology,
gateway settings, credentials, and data are not production compatibility
requirements. Do not delete, replace, or migrate their data implicitly.

Keep Cem and Zeynep as useful employee definitions and test references. Reusing
their existing external resources is optional; migrating their identities,
persona files, memories, or credentials is a separate explicit operation with a
backup and verification plan. A fresh development fixture must be distinguishable
from an adopted existing resource and use the correct create/adopt mode.

The owner confirmed that Cem and Zeynep's gateways were intentionally stopped
and permits starting them if a controlled test needs them. This is not a request
to start them now. Do not accidentally restore independent Office subscriptions
or the previous employee reply loop when preparing a central-dispatch test.

## Delivery sequence

1. Finish the local routing, privacy, permission, runtime, and delivery seams.
   Snapshot-correct normalization alone is not production authorization: the
   commit/admission fence must exist before any live central-routing activation.
2. Define a reproducible, isolated Ortak development stack with separate data
   volumes, explicit secrets references, pinned source/image revisions, and no
   dependency on the existing test stack's private state.
3. Provide profile-scoped runtime execution and durable event capture/replay.
   A successful root Hermes capability probe is not proof of employee-specific
   execution, permission enforcement, or reconnect safety.
4. Provision a clearly disposable test employee and exercise one controlled
   channel loop, cancellation, and restart/retry behavior. Use fresh identities
   and test data unless a specific adoption/migration was chosen.
5. Promote a verified build to a separate production deployment only after
   authentication, privacy, backup/restore, secrets, and rollout checks. Existing
   test instances can remain available; their removal is a separate decision.

Creating a clean stack does not authorize unreviewed public exposure, credential
sharing, a new paid server, or deletion of the old stack. Resolve those concrete
operational choices when the deployment step reaches them.

## What the current probes establish

The old stack remains useful capability evidence, not a mandatory migration
target. Read-only probes confirmed the existing profile directories, company
workspace directory, Honcho workspace/peers, and expected Office public-key
matches. Cem's configured model is `gpt-5.6-sol`, Zeynep's is `gpt-5.6-terra`, both
using `openai-codex`; this does not prove current OAuth validity.

The deployed source documents `/p/<profile>/...` API routing, but authenticated
`GET /p/cem/v1/capabilities` and `GET /p/zeynep/v1/capabilities` each returned 404.
That observation is consistent with stopped/unserved profiles; its exact cause
was not established. It is not evidence that all Hermes versions lack profile
scoping. Neither gateway was started or modified during these probes.

See `RUNTIME_DISCOVERY_2026-09-05.md` for the remaining observations and unknowns,
and `UPSTREAM_MAINTENANCE.md` for version selection and upgrade handling.
