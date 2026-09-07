# Protected admission source boundary

Unnumbered and unactivated. Assemble `encrypted_dm_jobs.sql` first, then
`encrypted_dm_admission.sql`; neither file is a deployed migration. Existing
immutable migrations and ordinary snapshot formats remain untouched.

`PgConfidentialRuns::prepare` uses the private verified claim and actual NIP59
decoder result, derives the source/identity from database rows, and retains a
zeroizing protected inner projection. `protect` generates one per-run master,
uses only the explicit Office wrapping provider, and encrypts the exact input.
Retain that object for `commit` retries. The transaction creates one deterministic
decision, one unique visit/counter advance, one confidential run, snapshot,
wrapped key, dedicated dispatch row and immutable outer receipt. A second valid
wrapper of the same employee/human/rumor creates only its duplicate receipt and
finalizes that wrapper as dropped. It retains the original run and outer source.

The ordinary 1059 normalizer still refuses. No existing decision is reset and no
decrypted content enters events, inbox, run_context_snapshots, run_events,
ordinary outputs, Work artifacts, workspace tools or memory tables. The new
dispatch table has no consumer. Its metadata leases have three attempts,
1s/5s retry separation, at most 30s per claim and a ten-minute execution ceiling.
The protected runtime-event sequence is 1..512; snapshot/reply ordinals are zero.

The current predicate fixes the original Office generation as version1's
authority epoch. Selection, active revision/lifecycle, exact pair and both
members, source partition/hash, cohort and binding/TTL remain current. No epoch
renewal exists; remove/restore cannot revive old input, and unrelated Office
mutations may conservatively retire it. A model-only revision does not change
employee identity, but this first confidential run remains pinned to its original
revision and must stop on its replacement. No broader scoped-epoch mechanism is
claimed. Receipt replay and metadata cancellation can settle after revocation.

`load_current_on` acquires Office→selection→job→inbox before selecting ciphertext.
Keep its caller-owned READ COMMITTED transaction for the local authorization
interval and recheck before later effects. It loads no keys. `authority` supplies
the future signed native facade with a five-second public observation, exact
human/channel, decimal-string epochs and no credential reference. The facade must
bind the human argument to the authenticated signer. This source adds no route.

Root owns activation, worker confidential dispatch/pump/reply wiring, participant
projection, migration allocation and deletion/recovery inventory integration.
Those are required before any enabled live encrypted pair. The fragment rejects
direct writes to ordinary content sinks but does not claim to inspect ciphertext
plaintext in PostgreSQL; the private preparation API owns that validation.

Prepared gates (not executed by this source task): two pure protected-inner
tests under `postgres::confidential::tests`; three ignored PG cases under
`postgres_run_supervision::confidential`. Their database must be disposable
localhost:55432 with both candidate fragments installed. The actual environment
key provider fixture requires the explicitly synthetic environment leaf
`ORTAK_TEST_CONFIDENTIAL_SYNTHETIC_KEY` containing 32 bytes of `0x31` as lowercase
hex; no real signer or credential is part of these tests.
