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
