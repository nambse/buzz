# Private native execution isolation — 2026-09-05

Current G3-resume owner: the same immutable6ff native binary is PID9935,
session55115, start `Sun Sep 6 02:32:16 2026`. Exact loaded inode/hash/cwd/start
was verified twice alongside source health and Work/fact persistence; no new
UI mutation was performed by this validator. The receipt and G3 failure/resume
boundary are in the [G3 recovery checkpoint](G3_VOLUME_READER_RECOVERY_2026-09-06.md).
Earlier native PIDs below are historical; source73/voice/C2 changes do not alter
this deployed artifact.

## Latest checkpoint — G3 preparation pins verified G2 resume (2026-09-06)

The same compiled private artifact
`6ff3a935892066429308ec720b3cd3b8c80031b2a53094316be0185f4dd77a21`
resumed after G2 as PID3290/session22975, with schema69/backend69 and the same
selected runtime/storage containers and images. This is an observed process
snapshot, not authority to signal that PID later. Receipt:
`/private/tmp/ortak-private-20260905/recovery-operations/e3535f007c194ebe992a2c610884b73a/source-resume-f52c6403a004472db1ba31153332b0f0/validation-acd1600fe8fb4d10b6706c79a326ceb6/receipt.json`.
The read-only check recorded health200/200/401 and unchanged row hashes/counts
for 16 scoped tables; it explicitly did not perform a new signed API read or
native UI acceptance action.

Root separately navigated the real native app after G1 resume and verified Work
COMPLETED v10, a 39-word artifact, two satisfied criteria, the required approval
entry and live Activity. This was operator navigation evidence, not a personal
review by the user. Ada remains ACTIVE, epoch1, Sol/high; the reviewed fact remains
withdrawn with zero text and retained header/tombstone/publish/withdraw receipts.

Both real G pauses passed, but captures failed: G1
`d6b4737afd1145ff8f2917584230c883` on exact Honcho CHECK catalog round-trip, and
G2 `dbd527bf32db44b78e45b2de5a4074b1` on the repeated database
`recovery-obligations.sql` log path. The G2 main127/Honcho19 table component
archives verified; no full capture or complete offline restoration succeeded.
See the [current G checkpoint](PRIVATE_FULL_STACK_RECOVERY_PLAN_2026-09-05.md).
The G2 owner registry and earlier native receipts are historical after resume;
G3 read-only preparation `5e4af2cc2c7543c38a467231559e9ac8` and registry
`15cc30dc9d3147979876a83b4056acb4` now bind this native owner and 20 frozen operators.
Owners SHA256 is `6f4ca742ad69acc15f7dd060597d123aed8fc2783fab0aaadd184d46c358a695`.
Root has started the G3 pause/capture attempt; its outcome is pending. No new
native build, successful full capture or offline restore is claimed. Neither old PIDs nor old exact pause
commands are current executable authority.

## Historical artifact rollout observations

Status updated 2026-09-06: the integration owner built, probed and deployed the
actual replacement Tauri artifact
`6ff3a935892066429308ec720b3cd3b8c80031b2a53094316be0185f4dd77a21`.
The exact probe returned the private policy evidence below before application
initialization. Public receipt directory:
`/private/tmp/ortak-v0-evidence/native-isolation-build-ab42d7d3cfae4fadb6c8308545686144`.
`native-resumed.json` records PID61964/session58919, start
`Sat Sep 5 23:59:03 2026`, selected private cwd and the same binary hash.
Root verified Office, Employees and Projects in the actual UI. After the
subsequent schema69 rollout, the same6ff binary resumed as PID72102/session16306,
start `Sun Sep 6 00:40:11 2026`, recorded in
`/private/tmp/ortak-private-20260905/rollouts/schema69-605742d230054d619a9561a4444529c9/native-resumed.json`.
The earlier Work dependency endpoint gap belonged to API66; root was then
performing actual F2/Work acceptance against API69; later native verification is
recorded above. The former `0f2ce6b3…` process45301 was stopped
by root with an exact verified SIGKILL after saved UI/lsof evidence; normal
shutdown or SIGTERM would have entered that old artifact's global agent reaper.
G's previous native owner registry is historical and must be rebuilt before capture.

## Retained boundary

`BUZZ_BASELINE.md` replaces the managed-agent/persona model and removes mesh and
huddles from v0. The private native desktop therefore retains human Office,
identity/signing, media, local history and owned cancellation. Employee execution
and recovery belong to the Ortak server, worker and management service.

The frontend flag previously hid some surfaces and refused selected JS IPC
calls. It did not prevent native boot-time restoration, workspace apply's
provider/profile reconciliation, persona/team event flushing, voice downloads,
mesh coordinator startup, or global process reaping at startup and shutdown.
No old external resources were inspected, stopped, migrated or changed to
implement this correction.

`desktop/scripts/ortak-private-native.mjs` now supplies all three build inputs:

```text
ORTAK_PRIVATE_DESKTOP=1
VITE_ORTAK_PRIVATE_MODE=true
BUZZ_BUILD_DEMO_SLUG=ortak-private-20260905
```

`build.rs` validates their agreement and emits `cfg(ortak_private_desktop)`.
Missing/mismatched private inputs fail the build. Cargo rerun markers cover all
inputs. The Rust policy reads no runtime environment setting: changing those
variables after compilation cannot widen admission. Ordinary builds and other
named demo builds retain their existing behavior.

`src/private_native.rs` is the exact allowlist for application commands. The
production Tauri generated handler is wrapped by `native_command_handler`,
which calls its `dispatch` function before any selected command handler. Unknown
commands are rejected, including future additions to the generated registry.
Direct IPC bypassing the shared JS helper receives the same refusal.

The allowlist preserves the actual Ortak client's `sign_event`, Office relay
authentication/messages/queries, human identity import/backup and local media
cancel/release. It excludes terminal attach/input, ACP install/connect/start,
managed-agent reconciliation, mesh starts, huddle starts/joins, voice downloads,
legacy persona/team writes, workflow execution, provider probes and reconnect
shell hooks. Legacy agent listing/stopping is excluded because it may reconcile
persisted records or operate on older/remote processes. `get_model_status` is
also excluded: constructing its lazy model manager can repair interrupted
installs. `get_huddle_state` and owned media/terminal cancellation remain
available. This is a boundary around the retained native application command
registry and startup paths; it does not remove the inherited dependency graph.

The same constant gates boot migration/backfill/harness/nest work, pending agent
restore, the periodic global sweep, persona/team flush and voice downloads.
Mesh runtime/coordinator startup is compiled out of a private build even when
the mesh feature is selected. `apply_workspace` retains its serialized
relay/identity transaction and skips legacy reconciliation/publishing/restoring.
The central runtime spawn and restore functions refuse before reading agent
configuration or spawning. Shutdown and the global reaper entry points skip
legacy PID lookup and process scanning in the compiled private desktop.

The private HuddleProvider uses inert context and mounts no voice hooks. Huddle
presence, channel header/menu indicators, wave-message start buttons and the
drawer are absent; Ctrl+Shift+Space does not consume the key or emit a huddle
event. Office message content remains visible.

## Bounded verification performed

- `node desktop/scripts/ortak-private-native.mjs verify-identity`: a small
  `rustc --test` harness includes the actual identity, build validation and
  admission modules with the private cfg. It tests the dispatch callback seam,
  unknown/legacy denial, retained signing/cancellation and exact probe CLI
  routing. This is not a compiled Tauri application test.
  Result: 13 passed, one existing opt-in demo test ignored.
- Focused Node tests use the production private recipe, native source wiring,
  HuddleProvider/indicator/wave components, keyboard hook and the real Ortak
  client with the native signing bridge. Fixture HTTP has no real provider or
  server calls. The client's employee, management and run queries generate the
  admitted signer request and NIP-98 header.
  Result: all 12 focused Node tests passed.
- Full desktop `tsc --noEmit` passed. Rust files were formatted. No Cargo or
  Docker build, native launch/quit, provider call or private-state mutation was
  performed in this slice.

## Integration owner's artifact gate

Build only through the existing private native recipe after serial build
coordination. Freeze its source/binary hashes in a fresh receipt first. Use the
following flag only on that verified new artifact; the old binary does not
implement it and unknown CLI arguments can follow normal application startup.

```text
<verified-new-native-binary> --ortak-private-policy-probe
```

The exact standalone argument returns this public evidence before application,
keyring, WebKit or network initialization:

```json
{"probe":"ortak-private-native-policy-v1","compiled_private":true,"legacy_startup_enabled":false,"sign_event_admitted":true,"legacy_start_admitted":false,"unknown_command_admitted":false}
```

Extra arguments, equals forms and unrelated arguments do not take the probe
exit path. The probe does not enable private mode or alter policy. Record the
new binary hash plus this result, then verify the actual native Office/API
workflow and absence of huddle affordances. Rebuild G's native artifact/owner
closure for that exact artifact and process before any coordinated capture.

## Voice-note source follow-up

The live Office composer still exposes `Record voice note`. Read-only source
review traces it to browser `getUserMedia`, `MediaRecorder` and `AudioContext`,
then a WAV attachment through ordinary media upload. It does not invoke native
STT/TTS, model downloads, agent execution, mesh or huddle admission. Therefore
it is not an execution-policy bypass. Voice remains outside v0's product scope.
After root accepted the bounded correction, the source toolbar now omits the
button in private mode and `useVoiceNoteRecorder.start` refuses before microphone
permission or recorder creation. Existing attachment playback and file upload
remain available. A rendered production-toolbar test and direct recorder-hook
test verify omission and zero microphone/native recording requests; the ordinary
recorder's three existing lifecycle tests also pass. All 16 focused JS tests and
full desktop TypeScript checking passed. No real microphone request or native
rebuild occurred. These changes are **not in the running6ff artifact** and await
the next coherent native/UI batch.
