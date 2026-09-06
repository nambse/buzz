# Current private G74 recovery selection

## Actual volume capture and offline restore — 2026-09-06

Root completed the populated schema74 capture and offline foundation restore.
The operation used the current Docker local journal volume, retained workspace
inputs/run copies and the ordinary Office baseline: revision
`61430887-dcc6-4def-8435-cfd723077f69`, empty tools/workspaces, routing enabled
and a fresh enabled cohort.

All paths below are under `/private/tmp/ortak-private-20260905`:

| Evidence | Result |
| --- | --- |
| `recovery-preparations/0157e313a698413096492514afa3d1e5/preparation.json` | Fresh read-only preparation passed. |
| `recovery-operations/c8b8d10995044751be326de19917cef1/owners.json` | Registered owner digest `94215ed0cf98f6c8bf30694229d0b65c8067a286bac798d58cfad4e1a467ae7d`. |
| `recovery-bundles/214fd4f027a34604aeb7469d9dfb9a60/manifest.json` | Captured at 04:37:36 UTC; manifest SHA `52d7114f3ad89ccfec96065ec21732b9cff26a5b5cdcd214e1148e5bb9953ae2`. |
| `recovery-offline-restores/cea594c6416d42f7a3403aa7509d2c70/manifest.json` | Offline foundation verified at 04:39:45 UTC; manifest SHA `0dc990c562f00007fa065c9dad0ff6cbab5fea03d607f12f1e6ab65fd123bffb`. |

The result covers 135 main and 19 Honcho tables. The current volume's raw journal
was physically restored and its coherent SQLite rows matched: 25 runs,
zero nonterminal runs and zero invalid cursors; two workspace journal runs,
two tool calls, zero pending calls and zero invalid workspace rows. The
workspace component physically restored and verified 16 entries. The frozen
28-file operator closure is
`2d38e3faa196e68063341355ee3fa330e34d2ed64b3af778d541ffccb9878c67`.

Root restarted the same original source services successfully, observed relay
readiness/liveness 200/200, unauthenticated API401 and a connected native client.
The independent post-resume verification passed at
`recovery-operations/c8b8d10995044751be326de19917cef1/resume-verification-1446bb1e1b784068a650c415d0e3dc87/receipt.json`.
It verified all five process owners, original/executed launcher hashes, all six
selected container generations, captured Work/artifact/policy/cohort state and
the six workspace table hashes, without a provider request.

| Source service | Verified post-resume PID/session |
| --- | --- |
| Relay | 74866 / 1650 |
| API | 74882 / 73881 |
| Worker | 75018 / 1381 |
| Management | 74999 / 13829 |
| Native | 75037 / 4639 |

These are observed identities, not future signal authorization. The registry
above records pre-pause owners and must not authorize another signal or capture
after restart. Current-owner and continuation-ledger updates remain root-owned.

`recovery-operations/c8b8d10995044751be326de19917cef1/owned-temporary-cleanup/receipt.json`
records removal of eight exact stopped temporary containers after their evidence
was retained. Volumes and bundles remain; no images were built. The retained
image export is 936,839,680 bytes; free disk was approximately 25 GiB.

This is same-host offline storage recovery. No restored runtime or restored
journal volume was activated, and no separate-host or separate-daemon recovery
was exercised. Redis AOF/expiry and MinIO application semantics are not newly
claimed by this foundation result. See
[the named journal recovery contract](G74_NAMED_JOURNAL_RECOVERY.md).
The previous G69/G73 bundles and failed G74 preparation remain historical.

## Historical first G74 preparation — before Files and the volume migration

The actual74 selection passed read-only preparation at
`/private/tmp/ortak-private-20260905/recovery-preparations/0faf086f87c34b5396be12dea9dddbe1/preparation.json`.
The source/operator hashes and154 focused test results are recorded in
`/private/tmp/ortak-v0-evidence/g74-current-selection-34cc717bb81947b5a53766776fa9e91b/receipt.json`.
The26-file operator closure SHA is
`cb140a2b338e8ef92bbbb8abc4fe05b1422a12e7fc75c978752699197d8a6b59`.
This operation created inventory evidence only: no pause, snapshot, restore,
provider request or new Docker resource.

The selected rollout is
`/private/tmp/ortak-private-20260905/rollouts/schema74-0bfb12ca13194005bde76e8c53b07ea6`.
Its `current-owners74.json` binds relay1170/session27338, API1193/session32536,
management1207/session58750, worker3480/session66115 and native4873/session24328
through exact PID/start, loaded inode, executable hash and launcher hashes.
Native uses `launch-native-source-pinned.py` with the original helper SHA
`1b05403bde7f58ebdc3d27cb58b6973fa042f76ebc05a940442477da92f2e55b`;
a copied helper derives a different bundle path and is not a valid launch.
The immutable eight-binary artifact receipt retains its original staging status;
actual deployment is established by the current owners and migration/health
receipts, without rewriting that historical receipt.

Controller6783ec2e uses image
`sha256:032e09a5a8318f3d22c82edbd9e861150362c3bea0f66cf693d4006a10a54961`
and worker image
`sha256:aebff616e80db46e4e0f22e1aecec2ef5330298f0e0771b69908bc0018cd4f6a`.
All three newly selected profile directories are bound to the same existing
OAuth directory and exact digest-gated workspace capability. Their immutable
0400 public JSON is accepted and hashed; secret file mode rules are unchanged.
Honcho remains the exact33d4e53f D2c container/image9358bd04 with the same database
and named volume. Current selection never inspects deleted historical
DBC/8ee containers or requires those images.

`config/worker74-retained.json` has fixed grant/expiry and
`register_selected_inputs=false`. The explicit selection covers the rollout's
`inputs` and separate `runs` roots and the backend74
`ortak-workspace-reader` SHA
`5edc64b9dad481c31604dc94bb3e67489a76af5c0dbac1c5a9b13b640ab79857`, UID501.
The retained registration receipt records one binding and one265-byte file,
zero uses/actions/readers, OfficeOFF and the prior Employee policy. Those are
historical observations, not a requirement that prevents later Files activation
or capture of terminal uses. Preparation permits dynamic row changes; an actual
capture re-reads current obligations, rejects unresolved work/readers, acquires
fresh schema/Linux barriers and preserves the complete selected filesystem.
Current grant availability is never substituted for historical run evidence.

At that checkpoint, a future full capture needed a newly registered current operation and root's
coordinated pause. This preparation is not reusable proof that applications are
stopped. Neither a registry nor source resume may enable Office automatically.
Earlier frozen G73/G4 operations, captures and offline restores remain historical
and untouched. Old stopped containers/images were removed by root under the
user's explicit cleanup request; backup archives and volumes remain retained.
