# Private MVP Employees and Activity

The `/agents` route becomes Employees only for an explicitly configured Office
origin. Ordinary builds retain the existing screen for unbound communities.
For the bounded private shell, also set `VITE_ORTAK_PRIVATE_MODE=true`: unbound
communities show a configuration error and never fall back to legacy Agents.
Package the native application through `scripts/ortak-private-native.mjs`, which
also fixes `ORTAK_PRIVATE_DESKTOP=1` and the private build identity. The Rust
build refuses a frontend-only private configuration. Its command admission and
startup isolation are documented in
[`NATIVE_PRIVATE_ISOLATION_2026-09-05.md`](../../../../docs/ortak/NATIVE_PRIVATE_ISOLATION_2026-09-05.md).
The app uses the existing native `sign_event` identity; no private key is entered
or stored by this feature.

Set the build-time environment variable (public configuration, no credentials):

```sh
VITE_ORTAK_API_BINDINGS_JSON='{"http://localhost:3000":"http://127.0.0.1:3010"}'
```

Both keys and values are canonical HTTP origins. HTTPS is required except for
loopback development. The API's canonical `origin` must match this value exactly;
the native NIP-98 signature binds the complete request URL including its query.
Configure the API's `allowed_web_origins` with each exact development/Tauri
frontend origin, such as `http://127.0.0.1:4177`, `tauri://localhost`, or
`http://tauri.localhost`. The API handles OPTIONS before authentication and permits
only GET/POST with Authorization and Content-Type. No wildcard or cookie access.
See `docs/ortak/MVP_API_CONTRACT.md` for server configuration and audience rules.

Employees displays saved identity/status and explicitly distinguishes runtime
health. Activity uses real ordered persisted events. Cancellation is a request;
the screen shows pending until the worker records acknowledgement or failure.
Run completion and Office publication are separate: pending, failed, and delivered
replies stay explicit on the same live Activity stream.
Memory shows the admitted pre-start notes and the durable post-delivery write
state. It distinguishes unprepared context from an empty prepared snapshot, and
receives a pushed memory receipt or terminal failure after Office delivery. Source and redaction indicators remain visible;
notes are explicitly limited to the current run. The native package and API need
migration0060 for live Activity (0052 introduced memory).
Only the server-provided `can_request_cancel` capability enables the action.
No provisioning, retry-run, or approval actions are exposed.

Lists page at 25 records. Run Activity uses a native-signed fetch SSE connection,
with 25 persisted events per frame and the latest 500 retained in the display.
PostgreSQL notifications trigger fresh authorized reads. Every connection first
subscribes and then backfills from its confirmed dense sequence, so writes during
reconnect cannot fall between history and live delivery. A reconnect retains its
cursor and re-reads current detail, including late Office/memory changes after
completion. Reload/remount deliberately replays durable history from the start;
private content is never saved to browser storage.

Each stream renews its signature after 45 seconds. Transient failures retry with
backoff for at most five attempts, including repeated disconnects immediately
after initial replay. Disconnection and reconnecting stay visible; Reload remains
available. Company/run switches abort old results. Authorization failures clear
previously displayed private content. Employees and Work lists still use bounded
polling; the run timeline itself uses push.

Focused validation:

```sh
cd desktop
node --import ./test-loader.mjs --experimental-strip-types --test 'src/features/ortak/*.test.mjs'
pnpm typecheck
VITE_ORTAK_PRIVATE_MODE=true VITE_ORTAK_API_BINDINGS_JSON='{"http://localhost:3000":"http://127.0.0.1:3010"}' pnpm build:e2e
pnpm exec playwright test --config src/features/ortak/smoke/playwright.config.mjs
```

The isolated smoke config uses port 4177 and the existing native mock bridge.
HTTP fixtures live only in tests; this smoke proves rendering and request
construction, not live PostgreSQL authorization or browser-to-server CORS.
Those require the API PostgreSQL route test plus a configured live transport
smoke. The browser workflow exercises keyboard and pointer cancellation, ordered
activity, pushed updates after run completion while the Office reply remains
pending, visible delivery failure, and manual reload recovery. Its test-only
ReadableStream transport exercises the actual signed client, SSE parser and hook;
PostgreSQL route tests separately prove real transactional notification delivery. It captures
distinct cancellation, pending, failed, and delivered screenshots under
`desktop/test-results/` after waiting for animations.


## Private native launch

The browser smoke build is a test artifact using the existing mock IPC bridge.
An operator uses the real Tauri shell and native `sign_event`; do not ship the
E2E bridge, seed its known test identities into a private deployment, or enter a
private key into browser code. Rebuild the frontend when changing either Ortak
build variable.

The checked-in recipe fixes this private stack's native identity and endpoint
composition. The first launch creates a fresh human identity; later launches
reuse that same isolated identity. It never imports the preserved Cem/Zeynep
resources or resets an existing identity. Ordinary worktree dev builds can
import shared managed-agent data when `BUZZ_SHARE_IDENTITY` and a valid key are
inherited. This recipe filters those variables and uses named-demo mode, which
skips legacy/shared app-data import regardless of those variables.

From the repository root, activate the pinned toolchain and inspect the plan:

```sh
. ./bin/activate-hermit
node desktop/scripts/ortak-private-native.mjs plan
node --test desktop/scripts/ortak-private-native.test.mjs
node desktop/scripts/ortak-private-native.mjs verify-identity
```

`plan` is read-only and prints public configuration only. `verify-identity`
compiles a small temporary Rust harness around the actual `build_identity.rs`
and `app_state_keyring.rs` modules, runs their identity tests, and removes the
harness binary. It substitutes only the OS config-directory resolver; it does
not start Tauri, use Cargo, migrate data, or access the keyring. Eight tests pass;
one existing full-crate compiled-flags test is ignored. Separately, native source
inspection confirms that `run_boot_migrations_inner` skips legacy app-data and
shared-agent import for named-demo builds; `lib.rs` skips legacy nest migration;
and the fixed identifier is outside the ordinary dev identifier prefix, so its
dev repo/key migration paths do not run. The harness does not execute those
Tauri boot paths.

When ready to run the native desktop, select exactly one action:

```sh
node desktop/scripts/ortak-private-native.mjs dev
# Or build a local unsigned debug .app without launching it:
node desktop/scripts/ortak-private-native.mjs build
```

The recipe uses `desktop/src-tauri/tauri.ortak-private.conf.json` and the existing
Tauri packaging wrapper. Native default features, including `system-keyring`,
remain enabled. Build produces
`desktop/src-tauri/target/ortak-private-native/debug/bundle/macos/Ortak Private.app`.
It builds the production frontend, including the existing protected-artifact
checks, into the packaging wrapper's per-invocation directory. It does not
package the E2E mock bridge. Dev serves the current frontend on fixed loopback
port 1427 and starts the native app. Neither action starts or migrates servers.
No inherited legacy gateway/CLI sidecars are bundled in this private variant;
Office messaging and NIP-98 signing use native in-process code.

Tauri/Xcode and the pinned Rust/Node dependencies must already be available.
A cold native build can require several GiB beyond the existing cache; check
free disk before building. The recipe's production frontend step (TypeScript plus both protected-artifact
variants), three invocation tests, and the small identity harness have passed.
Emitted OSS assets contain the exact relay/API origins and no mock bridge or
mock-message hooks. A full native build and launch remain separate gates. Cargo
output is forced into the private native directory above, never an inherited
shared/root target. To use a pinned compiler directly while preserving a shared
Cargo download cache, pass an absolute `ORTAK_NATIVE_CARGO` and `RUSTC` in the
parent environment, along with `CARGO_HOME`. The script forwards the Cargo path
through Tauri's `--runner` option. It never sources `scripts/instance-env.sh` or
reads app credentials. It retains only an allowlist of tool/OS environment
variables before adding the explicit private configuration.

The fixed macOS namespaces are:

| Purpose | Value |
| --- | --- |
| Compiled slug | `ortak-private-20260905` |
| App identifier/data directory | `dev.ortak.private20260905` / `~/Library/Application Support/dev.ortak.private20260905` |
| Human/agent keyring service | `buzz-desktop-demo.ortak-private-20260905` |
| Agent workspace | `~/.buzz-demo-ortak-private-20260905` |
| Agent credential cache | `~/Library/Application Support/buzz-demo-ortak-private-20260905` |
| Deep-link scheme | `buzz-demo-ortak-private-20260905` |

The fixed endpoint composition is:

| Surface | Address |
| --- | --- |
| Canonical Office relay | `ws://localhost:3038` / `http://localhost:3038` |
| Relay listener | `127.0.0.1:3038` |
| Relay diagnostics (not UI/API endpoints) | health `127.0.0.1:8089`, metrics `127.0.0.1:9198` |
| Ortak product API | `http://127.0.0.1:8787` |
| Native dev frontend | `http://localhost:1427` |

The native relay HTTP URL and Vite binding key both use `localhost` because the
fresh relay community is bound to that canonical host. The product API's signed
URL uses `127.0.0.1:8787`; the API config must declare that exact canonical
`origin`, `ORTAK_API_BIND=127.0.0.1:8787`, and
`allowed_web_origins:["http://localhost:1427"]` for dev. Add `tauri://localhost`
only when using the packaged macOS app. Never substitute the relay diagnostic
ports for the product API. API and relay use the same fresh Office/control-plane
PostgreSQL stack; configure Redis replay protection too. The UI never calls the
Hermes controller or a per-employee gateway.

On first launch complete the native human identity/backup step, then choose the
existing-community member flow for `ws://localhost:3038`. An operator must admit
that public key to the relay and cohort channel, register it as human, and put
its public key plus the exact channel/employee IDs in the API's server-owned
`humans` grant. No browser field selects company, actor, or role; no private key
is entered into browser code. Changing this stack's canonical URLs or identity
requires updating the recipe and matching Tauri config, rerunning the focused
tests, and rebuilding. Do not point the fixed identity at the preserved stack.

Private mode hides legacy create/import and agent-add actions, gateway lifecycle
services, channel templates, and Projects/Workflows/Pulse/Forum previews even
when saved preview preferences enable them. Direct unavailable routes redirect
to Employees; Agents/Compute/Experiments/Templates/Hosted settings are omitted.
First-run setup keeps human identity/backup and community admission, with no
runtime/provider installation or automatic Welcome-agent provisioning. The
shared UI-to-native command seam also rejects legacy provisioning/start/restart
calls. This UI boundary does not replace server-side authorization or native
process isolation. Office chat remains the existing channel UI; Employee
provisioning and runtime control remain the Ortak operator/worker responsibility.

The remaining live integration check must exercise a fresh human's Office message
through central routing, the pinned runtime, persisted Activity and a real Office
reply while this native UI is connected, including cancellation and reconnect.
The fixture browser smoke does not prove that complete loop or native CORS.


Private Office employee mentions use fresh native channel metadata and membership
reads at both prepare and publish. They require the exact configured relay, an
existing stream channel, and current sender/recipient membership. Unknown members,
DM destinations, retired channels and failed reads refuse publication. The central
Ortak router alone decides whether a message should wake an employee; this path
never provisions a persona, attaches a legacy agent, enrolls it in a huddle or
starts its runtime. Generic Buzz builds retain their existing agent policy flow.
The actual hook regressions are `agentMentionRevalidation.ortak.test.mjs` and
`useMentionSendFlow.ortak.test.mjs` under `features/messages`.

## Message routing

Delivered channel text in a configured private Office exposes **More actions →
View routing decision**. The signed, scoped read includes recorded silence even
without a run. It refreshes authorized snapshots every five seconds while open;
it is not a decision stream and cannot trigger scoring or dispatch. Missing
records remain distinct from zero-wake decisions. See
[`ROUTING_DECISION_READ_D3.md`](../../../../docs/ortak/ROUTING_DECISION_READ_D3.md).
