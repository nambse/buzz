# Reviewed project records — candidate D2a

This additive `reviewed-project/1` record family lives in dedicated Ortak extension
tables in the selected Honcho database. Current frozen native workspace and peer
ownership is checked before operations; these records never enter native messages,
sessions, embeddings, derivation queues or peer representations. That separation
is required: skipping one native enqueue call does not prevent a later peer
backfill from reading a native message. The upstream pin and legacy RunScratch
routes remain unchanged.

The server caller must authorize the human approval and its exact company,
project and employee audience. The extension checks its frozen resource ownership;
it does not replace current Ortak project ACLs. An approval supplies a stable UUID,
edited text, content/source hashes, approving human and approval UUID, and expiry.
No automatic extraction, prompt output or broader memory scope is accepted.

One transaction writes immutable record metadata, its separate text row and a
hash-only operation receipt. Identical operation retry preserves the record ID;
changed content, scope or operation identity conflicts. A withdrawal may arrive
before publication and installs the same irreversible tombstone without creating
text. A later delayed publication cannot resurrect it. Explicit expiry removal
requires the database clock to have reached the immutable expiry; reads stop
returning text at expiry even before that removal runs.

Inspect uses a 25-record UUID keyset page. Recall uses exact workspace/project/
employee scope and SQL full-text search over active text, bounded to eight records
and 8 KiB. No provider I/O is involved. Scope quotas retain at most 1024 record or
pre-publication tombstone identities; each record permits one publish, one human
withdrawal and one expiry operation key. Database and HTTP deadlines remain the
extension's existing finite bounds.

An erasure result means the referenced text row is absent from this extension's
current record store. The immutable hash-only receipt/tombstone remains. It does
not claim removal of D1's approval registry, original Office evidence, Work
artifacts, already delivered contexts, database backups, or legacy Honcho records.
D1 publication outbox, runtime current-fact admission/withdrawal fencing and UI
integration belong to D2b; until then D1 remains explicitly preview-only.

The isolated PostgreSQL tests must exercise actual handlers and storage: rollback
after text insertion, duplicate concurrent publication, withdrawal-before-write,
expiry without a sweeper, retry after process/connection restart, cross-scope
refusal, absent text after erasure and no native message/embedding/queue changes
even when global embedding settings are enabled. New runtime image and selected
store checks remain an integrating-task gate; source tests are not deployed proof.
