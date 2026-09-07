# Persistent private setup after the reboot

The user permits replacing the disposable OAuth, signer identities, conversation
history and Work records. Source code and Git history survived. The retained old
Docker volumes and unrelated Cem/Zeynep resources have not been removed or adopted.

The new state root is `/Users/nambse/.local/share/ortak/private-v0`, mode0700.
The Compose project is `ortak-private-v0`. Credentials, controller selections,
receipts and new evidence now live in persistent user storage, outside Git.
Do not run the historical `/private/tmp` launch or restore instructions.

Current verified facts:

- Fresh PostgreSQL and Redis pass their configured health checks. Schema79 was
  installed by `buzz-admin migrate`; the authenticated object-store bucket check
  passed. The native relay and API have started against the fresh database.
- Company `1ba4daf8-bdea-46cc-979d-2924ed1e6c38` is bound to Office community
  `e18e2c4f-4390-4d56-975c-707a2d78e75e`. Its private channel is
  `3b07150a-19f6-426d-8f00-e402892ba0f1` (`ortak-private`).
- The user completed one fresh Hermes device login. Ada owns that selected
  connection; Bora and Deniz have explicit same-company grants. Three distinct
  real profile probes completed using `gpt-5.6-sol` with `high` reasoning.
  Enrollment and token values were not printed or copied into Git.
- Worker image remains
  `sha256:ce23f9f95b9573cacc4eaf855e9826161bc725ac9ee8ccdf44f69df823b1e9f3`;
  controller image remains
  `sha256:2fca87f25abd15b573cec7c2c3e40803cb0e270b4ac5a273e27a81ede744f1e9`.
  The new controller is `ortak-private-v0-hermes`, with an explicitly labeled
  fresh named journal volume. Its service listens only at host127.0.0.1:8650.
  Profile probes are historical execution evidence, not permanent health grants.
- Fresh Honcho storage and API configuration were prepared with full-text recall,
  embeddings disabled and no deriver. All three employee bootstraps passed real
  write/recall checks and production activation gates. Docker Desktop requires
  the additional selected API bridge for the host-published Honcho endpoint.
- Ada and Deniz remain Sol/high. Bora was changed through the native prepared
  employee update flow to Luna/high, revision
  `33515e90-46ad-4d53-8d6a-78adc62e3597`. Controller registry V2 retains the old
  Sol profile and explicitly adds this immutable Luna choice. Original enrollment
  ownership and the one shared refresh lock remain unchanged.
- Real native Office passed Ada→Bora translation without repasting, a shorter
  revision, a separate slogan thread, and a return to the first thread after
  controller/worker restart and model change. Six runs completed; six dispatch
  and six Office publication rows were delivered once each. Frozen input and
  outbox evidence is `evidence/native-model-restart-and-threads.json`.

Operational selections are under `hermes/controller`, `honcho`, and `memory`.
Initial model receipts are `evidence/hermes-initial-probes.jsonl`. The temporary
operator preparation scripts there record this bootstrap; they are not yet the
finished repeatable install/start/stop/restart interface. Four `dev.ortak.private-v0`
user LaunchAgents currently manage the relay, API, worker and management service
with immutable copied artifacts. Full Mac reboot, backup/recovery, final rebuilt
artifact provenance and the remaining product goal are pending. Current Hermes
launch selection is `hermes/controller/launch-argv-v2.json`, not V1.

Validation: eight production-bound control-bootstrap tests passed, including the
selected Compose container name. Repository formatting, Clippy, desktop/web and
mobile checks passed in the first fresh CI attempt. Its file-size gate required
an explicit base because this fork has `origin/ortak/main`, not `origin/main`.
The gate passed with `CHECK_FILE_SIZES_BASE` set to that branch's merge base.
Attempt04 passed the desktop's6,652 tests and build, then failed three protected
store tests and the native identity-command inventory. The macOS parent-path
fix, retained child link rejection and inventory update pass focused tests.
Earlier migration/reconnect assertion fixes also pass. Full CI remains pending;
logs are in the persistent `evidence` directory.
