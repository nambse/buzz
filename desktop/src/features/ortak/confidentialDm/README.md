# Native encrypted pair view — source only

`SelectedDmScreen` now replaces the ordinary route subtree **and composer** for
an explicitly selected encrypted channel. The immutable private-build setting
`VITE_ORTAK_ENCRYPTED_DM_CHANNELS_JSON` maps exact relay HTTP origins to channel
UUID arrays. An absent setting preserves ordinary routes; a malformed configured
setting refuses instead of falling back. Native still verifies current central
authority independently. Never mount this alongside `useDrafts`, an ordinary
optimistic timeline, previews, notifications, search or promotion components.
Its `selected` value is routing context only; native independently fetches
current authority with the current native human's NIP98 signature.

The native endpoint is `GET /api/v1/channels/{channel}/encrypted-dm/authority`.
The server `encrypted-dm` feature mounts it behind existing fresh NIP98 and both
channel/employee deployment grants. It derives company/community and the exact current
human/employee pair under the canonical Office fence. The response is the strict
`Pair` DTO in native `authority.rs`; 8 KiB maximum. Counter fields are canonical
decimal strings, selection generation is positive, key version and Office
generation may be zero. `authority_epoch` equals `office_generation`. Native
accepts at most 15 seconds between observation and expiry; the server's selected
contract is 5 seconds. No private reference or credential enters this DTO.

Native selects the API origin from operator-only
`ORTAK_ENCRYPTED_DM_API_BINDINGS` (the existing exact relay-HTTP-origin → API-origin
JSON shape), then the existing operator `VITE_ORTAK_API_BINDINGS_JSON` environment
or compiled value. No IPC argument can supply
an origin or authority assertion. Missing selection/endpoint refuses. Ordinary
DM behavior is unchanged. These commands refuse in a non-private build.

The dedicated `app_data/ortak-encrypted-dm-v1/ciphertext.sqlite` contains only
self-NIP44 drafts, frozen signed outer ciphertext and metadata. Native creates
its directory 0700 and file 0600 on Unix; other hosts refuse this initial storage
adapter. It rejects symlink/foreign-owner/hardlink database leaves and uses
SQLite NOFOLLOW and FULL-sync transactions. It does not claim protection from
a hostile same-UID process, host memory inspection, swap or complete heap erasure.
The selected backup inventory must include this directory and any SQLite journal
before live acceptance; no current recovery selection was changed here.

One Send first seals the exact draft, freezes both wrappers from one rumor in
one transaction, then publishes the exact recipient copy and sender-history
copy via bounded NIP42 WebSockets. Unknown ACK retains that copy. Retry takes
only the stored operation ID, never fresh plaintext or new randomness. Completing
both ACKs atomically consumes the matching protected draft. One pending send
blocks new sends in that account/channel, including after an authority epoch
change. “Keep old send and start new draft” explicitly retires that exact send;
it preserves its scope, frozen bytes and ACKs, never retries it, and does not
claim it was undelivered. A fresh draft starts empty under current authority.
The matching old draft is consumed atomically; replaying retirement cannot
clear a later draft. The view shows the latest 16 retained retirement receipts.
The same SQLite path upgrades atomically from its original `user_version=0`
to version2 with monotonic retirement metadata and a pending index excluding
retired rows. Unknown versions refuse. Cold G capture remains an opaque byte
copy, preserving old and retired rows without migration or replay. Limits are
64 retained sends, 64 draft scopes and a 12 MiB store ceiling; no silent eviction.

The view has no generic decrypt API. It opens a maximum 32 human-addressed outer
events and retains only valid selected-pair messages in volatile state. Sender
history and recipient copies dedupe by verified rumor ID. Unknown reply claims
do not become thread links. This bounded recent snapshot is not complete history.
At most 12 automatic snapshots run per explicit refresh. Each socket is dropped
on return/error/25-second command timeout; frames, message sizes and auth/ACK
loops have separate limits. No frames or crypto errors are logged by this lane.

Native owns a view ID that close/switch invalidates before pending work can
return plaintext or admit another send. React generation checks also reject late
results. A 2-second read-only authority heartbeat and absolute response-expiry
timer clear the view on failure/change; blur, hidden document and unmount clear
it immediately. Protected draft saves are debounced 400 ms; edits not yet sealed
are lost when the view locks. This is shown as “Saving encrypted draft…”.

Root gates use the same private native recipe, including its Tauri override and
a canonical temporary directory (SQLite NOFOLLOW rejects macOS `/var` aliases):

```
node --input-type=module - <<'JS'
import {privateNativePlan} from './desktop/scripts/ortak-private-native.mjs';
import {spawnSync} from 'node:child_process';
import {realpathSync} from 'node:fs';
import {tmpdir} from 'node:os';
const plan = privateNativePlan('plan');
plan.env.TAURI_CONFIG = JSON.stringify(plan.config);
plan.env.TMPDIR = realpathSync(tmpdir());
const result = spawnSync(process.env.ORTAK_NATIVE_CARGO ?? 'cargo', [
  'test', '--locked', '--manifest-path', 'src-tauri/Cargo.toml',
  '--no-default-features', '--features', 'system-keyring',
  '--lib', 'commands::encrypted_dm',
], {cwd: plan.cwd, env: plan.env, stdio: 'inherit'});
process.exit(result.status ?? 1);
JS
cd desktop
node --import ./test-loader.mjs --experimental-strip-types --test src/features/ortak/confidentialDm/confidentialDm.test.mjs src/features/ortak/confidentialDm/selection.test.mjs
```

Eight unique native cases and seven component/selection cases passed. Tests bind the actual pinned crypto, physical SQLite writes/reopen, immutable
outbox/ACK gates, React component and purpose-command boundary. Root still owns
final installed artifact, signed endpoint execution, current storage inventory,
explicit live pair selection and one actual encrypted question/reply acceptance.
