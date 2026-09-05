# Private native build cache review — 2026-09-05

Read-only inventory found about 8.2 GiB available on the shared APFS data
volume. A cold native build is unnecessary: the idle task-owned
`/private/tmp/ortak-local-build-target` already contains the native Tauri graph.
This review did not copy, delete, build or launch anything.

## Selected inputs

- Seed: `/private/tmp/ortak-local-build-target`, 14 GiB apparent allocated size.
  Do not seed from the root target while its Cargo invocation is active.
- Rust: 1.95.0, commit `59807616e`, matching the pinned executable at
  `/Users/nambse/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin`.
- Cached registry provenance:
  `/Users/nambse/dev/ortak.dev-worktrees/buzz-import-2026-09-05/.hermit/rust`.
  The relevant source trees remain present. Use this Cargo home for best reuse.
- Desktop Cargo.lock SHA256:
  `69f8bcb287bc1f2687abcefe0702381bc81129311ea482f1bea8d141e7408edb`.
- Desktop package.json SHA256:
  `2793d16bd78133da4b1a7c467798823780ee85a766a0446ddfdbd5a917dd7293`.
- Root pnpm-lock.yaml SHA256:
  `8b805024400252fc1272b7b09675d09ac90d26f2d4756787700a222c4103b9db`.
  These three files agree between the integration root, product-api worktree
  and the prior native source/cache worktrees.
- Frontend seed: `/private/tmp/ortak-product-api/node_modules` (467 MiB) **and**
  `/private/tmp/ortak-product-api/desktop/node_modules` (32 KiB of pnpm links).
  The latter resolves relative paths into the former, so copy both together.

The native seed contains Tauri 2.11.5, Wry 0.55.1, 11 plugin rlibs, an earlier
164 MiB desktop executable and 870 MiB of desktop library artifacts. Its own
features are default/system-keyring, no mesh-llm; rustflags are empty and the
repo dev line-table profile agrees. Cached Tauri includes its test feature,
so the packaged production feature graph may rebuild part of that dependency
slice. Source changes and private build identity must also rebuild the app.

Cached Sherpa build output still names
`/Users/nambse/dev/ortak.dev-worktrees/m6-work-projects-foundation/target/sherpa-onnx-prebuilt`
as a static link-search path. That 105 MiB directory exists and must remain
available; do not delete m6 while using this seed. Preserve the entire seed's
build outputs and fingerprint timestamps; do not hand-edit fingerprints.

## Bounded root build sequence

The private recipe fixes its target to
`desktop/src-tauri/target/ortak-private-native`; external CARGO_TARGET_DIR is
discarded deliberately. Seed that new directory with an APFS clone, keeping it
separate from both the active root target and previous executable identity.
Do not use hard links. Confirm each destination is absent before these commands
and fail if it already exists; never merge caches into an unknown destination.

From `/Users/nambse/.codex/worktrees/a5ed/ortak.dev`:

```sh
mkdir -p desktop/src-tauri/target
/bin/cp -cRp /private/tmp/ortak-local-build-target desktop/src-tauri/target/ortak-private-native
/bin/cp -cRp /private/tmp/ortak-product-api/node_modules node_modules
/bin/cp -cRp /private/tmp/ortak-product-api/desktop/node_modules desktop/node_modules
```

All paths are on the same APFS data volume. Clone copies share unchanged blocks
while later app writes are private. Inspect free disk after seeding before
starting Cargo; apparent `du` totals count shared clone blocks and do not prove
how much a later deletion would reclaim. Never fall back to an ordinary full
14 GiB copy when disk cannot accommodate it.

The recipe currently preserves pinned tool paths but discards ambient Cargo
job/offline controls. Its owner should explicitly set `CARGO_BUILD_JOBS=2` and
`CARGO_NET_OFFLINE=true` in its constructed build environment before this run.
Keep `CARGO_INCREMENTAL=0`, default/system-keyring features and the existing dev
profile; changing debug flags now would discard useful compatibility. This
keeps the build bounded and turns missing dependencies into an explicit failure
instead of an unexpected download. A missing cached dependency can be handled
by the integration owner before retrying.

Once the root's current Cargo command has completed and the recipe controls
are present, use the existing recipe with exact pinned tools:

```sh
. ./bin/activate-hermit
export CARGO_HOME=/Users/nambse/dev/ortak.dev-worktrees/buzz-import-2026-09-05/.hermit/rust
export RUSTC=/Users/nambse/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin/rustc
export ORTAK_NATIVE_CARGO=/Users/nambse/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin/cargo
node desktop/scripts/ortak-private-native.mjs verify-identity
node desktop/scripts/ortak-private-native.mjs build
```

The recipe requests only a debug macOS app bundle, without signing, external
sidecars or DMG creation; it builds the real frontend artifact matrix and then
uses Tauri's private configuration. It does not enable mesh-llm. The build uses
direct resolved Vite JS; the cloned desktop `.bin/vite` shell shim still names
the earlier installation's NODE_PATH, so do not use that shim as the private
native dev path without regenerating it.

Allow several GiB for new app/library objects, link scratch and the app copy;
8 GiB appears sufficient for a warm rebuild but is not a measured guarantee.
Check disk after frontend completion and during compilation. If free space
approaches 3 GiB, stop only this owned build and reassess its newly created
outputs. Do not globally prune caches or remove another lane's target.

Only launch the freshly rebuilt
`desktop/src-tauri/target/ortak-private-native/debug/bundle/macos/Ortak Private.app`
after checking its Info.plist identifier `dev.ortak.private20260905`, private
URL scheme and newly emitted build identity. Never launch the copied seed's
old `buzz-desktop` executable. The intended relay/API remain loopback
localhost:3038 and 127.0.0.1:8787, with the private keyring/store namespace.

## Cleanup scope if later needed

The task-owned temporary local/API/root targets report 14/15/16 GiB, but they
are likely APFS clones and their unique reclaimable bytes are unknown. No
whole-target deletion is recommended for the first native attempt. The root
must retain its running service executables, and m6 must retain the static
link path above. The root target's 843 MiB incremental subtree is also only an
apparent number; CARGO_INCREMENTAL=0 prevents new incremental growth, and no
cleanup was performed. The reviewed Hermes OCI export (474 MiB), source
(183 MiB) and source receipts remain useful reproducibility evidence.
