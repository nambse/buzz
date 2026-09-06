# Selected Honcho D2a rollout helper

Prepared for the integration owner on 2026-09-06; the preparing agent did not
execute this helper, stop a service, initialize a schema or call a provider.
Root reported that the four selected backend writers, native6ff and old Hermes
controller were stopped and the main database reached schema69 before handoff.
Root subsequently executed the exact checkpoint continuation successfully:
`honcho-d2a-2934fc7da116489dab977d5f88f32e88/receipt.json` under the schema69
rollout records `upgraded_verified` at2026-09-05T21:38:34.007656+00:00. The new
API is `ad579c8e6cd7c556cb3155630dc7f1c8db79ccc030dd2d72e5d9160380bc35a4`,
image `sha256:febea5609d74f51026ab5a98ac9ce7b8648989ac7f639893ef4f71846c65dc1b`.
All four D2a tables were present and empty; every old count/logical row hash and
settings/sequence value remained equal. Health and authenticated protocol GETs
returned200, unauthenticated access was denied, and frozen D2a routes matched.
No provider call or API record mutation was part of the helper verification.

Root then resumed the new Hermes controller, all four backend writers and
native. The former pause receipt and commands below are historical; they must
not be replayed as current authority. G source pins now name the successful
new selection. Later G preparation `e81d55418f4045dbbc2a33b50d5213df` and owner
registry `ea8c88f50f2e40e2beb5a016dd386a08` passed; no live full capture has run.

Root's first run stopped the exact old API but retained a failure before creating
any new container: `old_api_unclean_exit`. The image's shell/init shutdown exited
143 after the explicit SIGTERM, with OOM false. The original helper and its
failure remain immutable evidence. The continuation below was the selected
root-run command for that exact checkpoint; neither command may be replayed
against the now resumed generation.

The standalone owner-private helper is
`/private/tmp/ortak-rollout-honcho-d2a.py`, 46824 bytes, mode0600, SHA256
`8801a82c8e413195e613e09bf3b7d65e6f75e34ba788f4f3cc9e969f2f837ec6`.
It embeds bounded SQL from the reviewed backup/recovery sources and records
their source hashes. It imports no mutable repository helper at execution.
AST parsing and placeholder/selected-route checks passed. This is static
verification, not an executed rollout or a PostgreSQL helper regression.

## Exact selection

- Old API: `13cbff3d670de2030792ac515fe52b7506506ee227ac0df8fa7d54c7ed412182`,
  image `sha256:cc8b4a29c0adda08978886e205ff5c5ff0a13923e4ed15e1626b24194d0c0c21`.
- New tested D2a image:
  `sha256:febea5609d74f51026ab5a98ac9ce7b8648989ac7f639893ef4f71846c65dc1b`.
  Its fixed validation receipt is under
  `/private/tmp/ortak-v0-evidence/honcho-d2a-3d4a69b4d1494fe79132c543be18d6d8`.
  The image passed23 PostgreSQL and11 local tests plus runtime initialization.
- Preserved PostgreSQL:
  `e5d4bd4ff4cabcc6f8e8d4712c3001e83fb8cd89291214dd840f4ea5edfe3d88`,
  database `ortak_honcho_adapter_test`, role `ortak_honcho`, retained volume
  `ortak-honcho-test-data-20260905`.
- Default verified backup:
  `/private/tmp/ortak-private-20260905/honcho-backups/20260905T211813Z_f69491617c6741e883aabd2db600c4a9/manifest.json`.
  Archive SHA256 `194c62bea6c74ac618d169f9b034b8922bd8a8d708011a9862021ccc5592675b`;
  retained verification DB `ortak_honcho_verify_5e6d847e83cd486e85b1402312eaa5b0`.

The public old-container inspection established: `app` user, `/app` cwd,
`sh /app/entrypoint.sh`, init enabled, no mounts, memory1GiB, CPU2 and256 PIDs.
The replacement retains these limits, two exact network IDs and the existing
`honcho-test-api` alias, with only `127.0.0.1:8009→8000` published. Logs gain
`json-file`, maximum10MiB across three files. No image pull or source volume
attachment is allowed by the creation command.

## Root command and pause evidence

Run using the same selected macOS account; “root” here denotes the coordinating
agent, not an instruction to use sudo.

```sh
python3 /private/tmp/ortak-rollout-honcho-d2a.py \
  --rollout-dir /private/tmp/ortak-private-20260905/rollouts/schema69-605742d230054d619a9561a4444529c9 \
  --pause-receipt /private/tmp/ortak-private-20260905/rollouts/schema69-605742d230054d619a9561a4444529c9/paused-drain.json
```

The helper accepts the exact `ortak-schema69-paused-drain/1` shape produced by
root's reviewed `/private/tmp/ortak-pause-and-migrate69.py`: full original owner
records plus `started_at` equal to each receipt's complete `identity` string,
native6ff, the old66 artifact directory, closed admission, contained children,
fenced OAuth enrollment, the old stopped Hermes ID, drain and pause timestamp.
It also requires `main-migrated.json` and a fresh read of live ledger69. A saved
pause alone is insufficient: original PIDs must be absent, selected names must
have no replacement in the private cwd, the old Hermes controller must remain
stopped and no company-owned runtime child may be running. The pause is limited
to30 minutes and revalidated around mutation boundaries.

## Mutation and verification boundaries

The helper checks the backup's verified source/restore metadata and archive
digest, then compares current source table counts and complete logical bytes.
Any drift refuses initialization. Old container environment values are parsed
privately, matched to the exact saved service file and compared with both
immutable images' base defaults. Unknown variables or changed defaults refuse.
The exact validated environment is copied only to a fresh0600 `service.env`
inside the rollout evidence directory; values never enter argv or diagnostics.

After a fresh pause check, it stops only the selected old API, requiring a clean
exit and no remaining Honcho database client. A second consistent snapshot
must match the pre-stop state. It creates a uniquely named replacement, attaches
the second selected network before startup, and validates environment, mounts,
network identity, init, ports and resource/log bounds.

Startup runs the tested image's native migration script and extension
initialization. Success requires exactly four new empty D2a tables, every old
table count and full logical hash unchanged, and an unchanged old-schema
projection. That projection permits only the four new table families and five
exact D2a guard-function names; all prior functions/schema metadata remain
checked. Database settings, role metadata and sequence values must also match.

Readiness runs inside the new container. It generates an admin JWT in memory
and performs bounded GETs for native liveness, unauthenticated denial,
authenticated protocol and route presence. It never prints the JWT or response
bodies. Route checks use frozen D2a `/recall`, publish and withdraw routes.
The newer working-tree-only `/recall-selected` route is deliberately absent
from this tested image's contract. No record creation, withdrawal, provider
request or runtime activation is performed by readiness.

Every phase retains private intent/result evidence, with a five-minute command
budget and bounded discarded stderr. Success prints `new_api_id`, `new_image`
and the final `receipt.json` path. Failure retains the old container and all
database state; it attempts to stop only its newly labeled API, including
rediscovery after an uncertain create response. It never restarts the old API,
drops new tables, deletes a container or restores over the source database.
Backend/native resumption and recovery remain with root.

## Exact retained143 checkpoint continuation

Root's failed operation is
`/private/tmp/ortak-private-20260905/rollouts/schema69-605742d230054d619a9561a4444529c9/honcho-d2a-12ff27a992a248d091e87ebb628a5d01/failure.json`.
The old process was started at `2026-09-05T04:42:59.271767716Z` and finished at
`2026-09-05T21:34:52.344101139Z`, exit143/OOM false. No new-create intent or
container existed at that failure checkpoint.

The separately prepared continuation is
`/private/tmp/ortak-resume-honcho-d2a.py`, 48898 bytes, mode0600, SHA256
`9e3f68a388d29a611e4b6510bc164442355aa330e8bc88056105478452d4f641`.
It was statically parsed but not executed by the preparing agent. Root executed:

```sh
python3 /private/tmp/ortak-resume-honcho-d2a.py \
  --rollout-dir /private/tmp/ortak-private-20260905/rollouts/schema69-605742d230054d619a9561a4444529c9 \
  --pause-receipt /private/tmp/ortak-private-20260905/rollouts/schema69-605742d230054d619a9561a4444529c9/paused-drain.json
```

It recognizes only this exact failure identity, original helper hash, old image,
start/finish timestamps and143 exit. Before accepting the stopped checkpoint it
requires zero current Honcho database clients; all normal source/backup table
counts, complete row hashes, schema projection, settings, sequences and owner
pause checks still run. It rechecks the checkpoint before creation. It does not
stop or restart the old API and does not generally convert nonzero exits into
clean recovery authority. Subsequent failure containment applies only to the
uniquely created replacement, which remains retained.

G's current image/process selectors are not updated by this helper. A successful
rollout receipt must first be reviewed and explicitly selected in the later
full-stack recovery preparation.
