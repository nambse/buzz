# Frozen snapshot JSON compatibility

The immutable71 signed HTTP regression suite exposed a production compatibility
regression: `ortak_reviewed_snapshot_consistent` converted the entire frozen JSON
to PostgreSQL `jsonb`. Valid escaped NUL in legacy RunScratch caused `22P05`,
before the legacy branch could return. Version-three scratch, rendered context
and original byte accounting require the same compatibility.

Proposal `proposals/0072_reviewed_snapshot_json_compat.sql` applies an injective
escape encoding before any PostgreSQL JSON field access: even `json` field
extraction unescapes unrelated NUL strings. NUL becomes SOH+STX, SOH becomes
SOH+SOH. All reviewed count, scope, pin, binding, rendered equality, budget and
current-use guards remain. Actual Unicode escapes are
matched with backslash parity, preserving literal backslash-u strings. Comparison
scratch byte accounting reverses the extra byte per encoded pair. Outer records
are encoded once; serialized inner context receives its own single encoding.
Expected retained fact, binding and pin values use the same encoding for exact
equality, and reviewed byte counts use original retained fact content. No frozen
bytes, runtime inputs or retained use rows are rewritten.

Root owns additive migration, desired-schema and exact-function reconciler
integration; immutable71 must remain unchanged. Fresh desired state needs both
the comparison helper and the replaced snapshot function. Existing installations
use the additive migration.

Validation uses the production signed API and actual PostgreSQL/runtime seams:

- `work::reviewed_exports::runtime::json_compat::` has two PostgreSQL cases.
  The comparison case covers NUL/SOH/STX, literal and repeated backslashes,
  Unicode spelling and object ordering. The freeze case begins with the real
  selected reviewed builder, restores valid control-containing scratch through
  `FrozenRunSnapshot::decode`, and rejects seven forged commits. It then freezes
  a context at exactly 16 KiB, preserves the original bytes, and starts the
  runtime without another recall. Ordinary remote recall strips controls; this
  test exercises valid immutable restored context without changing that policy.
- `activity_stream::terminal_memory_receipt_pushes_without_new_event_and_resumes_current_detail`
  retains its existing valid legacy NUL scratch fixture.
- `activity_stream::terminal_run_pushes_late_office_status` now supplies its
  retained routing pin, as production Office scheduling requires since63.
- `memory::signed_memory_projection_is_bounded_redacted_audience_scoped_and_fail_closed`
  expects the explicit empty `reviewed` collection and keeps all redaction,
  malformed snapshot, audience and stale-binding assertions.

The build owner verified the corrected implementation in the full 104-case
signed PostgreSQL suite; all cases passed, including both new comparison/freeze
regressions and the legacy Activity/memory fixtures. Four DM normalizer PG cases
also passed. Root then integrated immutable72/73 from the reviewed proposals;
deployment and desired-schema parity remain separate gates. The separate root
SQL fuzz receipt covers 1,572 string/escaping variants and original UTF-8 byte
counts; it supplements these admission regressions.
