# Milestone 8 voice and mesh dependency boundary

The source handoff below is historical. The integration owner built and deployed
this boundary on 2026-09-06, including actual private graphs, native UI and
live relay route checks. See the [current checkpoint](CONTINUATION_PROGRESS_2026-09-05.md)
and [usage notes](PRIVATE_V0_RUNBOOK.md) for the selected artifacts and limits.

Source handoff only. These changes do not establish a rebuilt, installed or
resumed artifact. The integration owner owns compilation, selected dependency
graph evidence and cutover. Other Milestone 8 pruning remains separate.

| Package | Retained default feature | Private selection |
| --- | --- | --- |
| `buzz-desktop` | `legacy-voice`, alongside existing `legacy-terminal` and `system-keyring` | `--no-default-features --features system-keyring` |
| `buzz-relay` | `legacy-mesh` for the coupled relay mesh, huddle audio and tunnel modules | `--no-default-features` |

The native feature owns `buzz-voice`, Opus, NetEq, Sherpa, Rodio, Earshot,
Rubato, audioadapter-buffers and the push-to-talk global-shortcut plugin.
Its modules, state, startup work, commands and huddle window handling are
compiled only with the feature. Private builds reject accidentally enabling
`legacy-voice`, as they already reject `legacy-terminal`. The existing inert
private HuddleProvider does not call the omitted commands. Common native
permissions stay unchanged; the three existing shortcut permissions are in
a separate capability loaded only with voice support.

The relay feature owns the optional `buzz-relay-mesh` dependency and its
coupled audio/mesh/tunnel integration. Disabling an environment switch alone
does not remove a Cargo edge. Private relay artifacts must disable Cargo
defaults. Ordinary developer builds retain their existing default behavior,
including the separate `dev` feature. The retained mesh SDK dev-dependencies
belong to the test graph and are not evidence of a production dependency.
Without `legacy-mesh`, unset or explicit `false`/`off`/`0` transport flags are
accepted. Other values for `BUZZ_MESH`, `BUZZ_MESH_DEMO_ECHO` or
`BUZZ_HUDDLE_AUDIO_AVAILABLE`, and any `BUZZ_MESH_BIND_ADDR`, refuse startup.
The mesh status/demo and huddle audio routes are absent; huddle-only liveness
subscriptions receive an unsupported `CLOSED` response.

The source directories and workspace members remain available for inherited
development. Use selected package roots and normal/build edges to establish
private exclusion; an all-workspace or all-features graph deliberately
includes retained packages.

## Integration owner commands

From the repository root, after activating the pinned toolchain and selecting
the owned build target/cache, build the private relay explicitly:

```sh
. ./bin/activate-hermit
cargo build --locked --offline -p buzz-relay --bin buzz-relay --no-default-features
```

For the native application, keep the existing identity/config/artifact recipe:

```sh
node desktop/scripts/ortak-private-native.mjs build
```

That recipe passes `--features system-keyring` to Tauri and forwards
`--no-default-features` to Cargo after `--`. It retains the selected private
identity, encrypted-DM entrypoint, credential references and isolated target.
Do not replace it with an unconfigured direct native build.

Inspect the exact selected dependency graphs, without selecting workspace
members or test-only edges:

```sh
cargo tree --locked --offline -p buzz-relay --no-default-features --edges normal,build
cargo tree --locked --offline --manifest-path desktop/src-tauri/Cargo.toml -p buzz-desktop --no-default-features --features system-keyring --edges normal,build
```

The relay graph must omit `buzz-relay-mesh` and its mesh SDK transport graph;
the native graph must omit the voice dependencies above and their audio/ONNX
native build dependencies. Existing native `mesh-llm` and terminal exclusions
must remain absent. Cargo.lock may retain packages used by default developer
or test graphs; lockfile presence is not selected dependency reachability.

The focused source/recipe seam is:

```sh
node --test desktop/scripts/ortak-private-native.test.mjs desktop/scripts/ortak-private-native-guards.test.mjs
cargo test --locked --offline -p buzz-relay --lib --no-default-features private_build_
```

It binds actual private invocation arguments, default-on optional dependency
edges, module admission and feature-selected capabilities. Compilation and
the actual graphs above remain the final evidence. The two relay cases exercise
production configuration loading and actual router omission while retaining
health/status routes. No test or command was run as part of this source handoff.
