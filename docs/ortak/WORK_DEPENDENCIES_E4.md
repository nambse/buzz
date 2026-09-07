# Retained Work dependency editing (E4)

E4 completes the existing same-project `blocked_by` relation with signed reads,
add/remove/re-add commands and a desktop editor. Parent/child decomposition remains
the separate E5 design in [the relation plan](WORK_RELATIONS_E4_PLAN.md).

| Endpoint | Request |
| --- | --- |
| `GET /api/v1/work-items/{item}/dependencies` | Current signed reader; no body |
| `POST /api/v1/work-items/{item}/dependencies` | `operation_id`, `expected_version`, `depends_on` |
| `POST /api/v1/work-items/{item}/dependencies/{edge}/remove` | `operation_id`, `expected_version`, `reason` |

Bodies reject unknown fields. Mutations require current Office/channel/project
scope, a configured human Operator, and project Contributor or Owner permission.
Addition requires a currently visible target in the same project. Self-edges,
active duplicates and cycles fail closed. Removal operates on an opaque edge ID
belonging to the source item and requires a nonempty reason up to 1,024 bytes.

Reads return the source work ID/version and at most 32 active edges. An authorized
target receives the existing safe Work summary; after canonical source removal
only the opaque edge ID remains. Its title, work ID, message ID and state are
withheld. The human can still remove that blocker. Losing source-item or project
authority refuses the entire read instead of returning an empty result.

Immutable migration 67 retains edge identity and original creation provenance.
`released_at IS NULL` defines an active edge; removal sets the timestamp and
re-addition clears it after a fresh cycle check. DELETE/TRUNCATE remain forbidden.
Neither release nor re-add can rewrite endpoint identities. Company-owned edges
reference durable Work/project rows and survive community binding purge.

Every graph mutation takes current Office shared authority, an EXCLUSIVE project
lock **before** any item/shared project lock, then the source item lock. This
avoids concurrent SHARE→EXCLUSIVE upgrade deadlocks. Cross-project targets are
refused before another project's row is locked. The same transaction performs
the version CAS, edge update, one history append and one immutable operation
receipt. Exact retries reauthorize current scope without repeating the mutation;
an add replay also rechecks its target. Terminal items and archived projects
cannot receive a new graph edit.

Cycle checks fetch at most 4,097 active edges and refuse additions at the
4,096-edge project ceiling. Source aggregates and runtime admission ignore released
edges. Existing item-version and Work generation witnesses invalidate queued,
held or active old execution after any edit; restoring the same graph cannot
revive an old witness. Removal does not start a process, change manual status,
satisfy acceptance criteria, or approve a review.

The desktop editor uses current visible targets from the selected work-list page.
It requires a dependency read matching the displayed work ID/version before
offering changes. Failure clears relation data; authorization loss revokes the
Work surface; transport retries stop after five attempts with explicit recovery.
Writes reuse the shared Work hook's frozen operation bytes and explicit retry.

Validation completed before deployment:

- Six production PostgreSQL cases passed against a fresh migration-66 database
  with the exact proposed SQL67 body. They cover remove/re-add identity, actual runtime
  start after removal and cancellation after re-add, hidden-target removal,
  current replay permission, opposite-edge concurrent cycle attempts, atomic
  storage-failure rollback/retry, terminal refusal, a real 4,096-edge graph, and
  held preparation after remove/re-add restores the same final graph.
- Four actual client/component/hook tests pass; the complete Ortak desktop
  matrix passes 72/72, TypeScript checking and scoped Biome pass.
- Central compilation, Rust formatting and genuine PostgreSQL test discovery
  passed. PostgreSQL evidence is retained under
  `/private/tmp/ortak-v0-evidence/e4-provisional-ce0afaf174d64a21bc3b650a075de91a`.
- Immutable migrations 1–67 and a separate fresh real pgschema1.7.4 installation
  produced matching selected catalogs after two idempotent reconciliation passes.
  Evidence is retained under
  `/private/tmp/ortak-v0-evidence/schema-parity-e94ace78d0b740998c59c9095e9b8bec`.

Source requires migration67; do not run its new queries against schema66. The
private deployment still runs schema66 and its separately retained backend/native
artifacts. The E4 source and database checks do not claim native acceptance or
deployment.
