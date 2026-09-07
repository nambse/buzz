import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = (path) =>
  readFileSync(new URL(`../src-tauri/${path}`, import.meta.url), "utf8");
const code = (text) =>
  text.replace(/"(?:\\.|[^"\\])*"|\/\/[^\n]*|\/\*[\s\S]*?\*\//g, (token) =>
    " ".repeat(token.length),
  );
function closingBrace(text, open) {
  let depth = 1;
  for (let index = open + 1; index < text.length; index++) {
    if (text[index] === "{") depth++;
    if (text[index] === "}" && --depth === 0) return index;
  }
  assert.fail("unbalanced production block");
}
function body(path, name) {
  const text = code(source(path));
  const start = text.indexOf(`fn ${name}(`);
  assert(start >= 0, name);
  const open = text.indexOf("{", start);
  return text.slice(open + 1, closingBrace(text, open));
}

test("voice dependency edges and native mounts require the default-on legacy feature", () => {
  const manifest = source("Cargo.toml");
  assert.match(manifest, /^default = \[[^\n]*"legacy-voice"/m);
  const voiceFeature = manifest.match(/^legacy-voice = \[([^\n]+)\]$/m)?.[1];
  assert(voiceFeature, "explicit voice dependency feature");
  for (const dependency of [
    "buzz_voice_pkg",
    "opus",
    "neteq",
    "sherpa-onnx",
    "rodio",
    "earshot",
    "rubato",
    "audioadapter-buffers",
    "tauri-plugin-global-shortcut",
  ]) {
    assert(voiceFeature.includes(`"dep:${dependency}"`), dependency);
    assert.match(
      manifest,
      new RegExp(`^${dependency} = \\{[^\\n]*optional = true[^\\n]*\\}$`, "m"),
      dependency,
    );
  }
  const native = source("src/lib.rs");
  for (const module of ["huddle", "ptt_shortcut"]) {
    assert.match(
      native,
      new RegExp(`#\\[cfg\\(feature = "legacy-voice"\\)\\]\\s*mod ${module};`),
    );
  }
  assert.match(
    native,
    /#\[cfg\(all\(ortak_private_desktop, feature = "legacy-voice"\)\)\]\s*compile_error!\(/,
  );
  const common = JSON.parse(source("capabilities/default.json"));
  const voice = JSON.parse(source("capabilities/legacy-voice.json"));
  assert(
    common.permissions.every(
      (permission) => !permission.startsWith("global-shortcut:"),
    ),
  );
  assert.deepEqual(voice.windows, common.windows);
  assert.deepEqual(voice.permissions, [
    "global-shortcut:allow-register",
    "global-shortcut:allow-unregister",
    "global-shortcut:allow-is-registered",
  ]);
  assert.match(
    source("build.rs"),
    /if cfg!\(feature = "legacy-voice"\)\s*\{\s*attributes\s*\}\s*else\s*\{[^}]*attributes\.capabilities_path_pattern\("\.\/capabilities\/default\.json"\)/,
  );
});

test("actual Tauri handler admits every request through the compiled policy first", () => {
  const text = source("src/lib.rs");
  assert.match(
    text,
    /\.invoke_handler\(native_command_handler\(tauri::generate_handler!/,
  );
  const handler = body("src/lib.rs", "native_command_handler");
  assert.match(handler, /private_native::dispatch\(&command, invoke, &handler/);
  assert.match(handler, /invoke\.resolver\.reject\(reason\)/);
  assert.match(source("build.rs"), /validate_private_build_flags\(/);
  assert.match(source("build.rs"), /cargo:rustc-cfg=ortak_private_desktop/);
});

test("artifact policy probe precedes every native initialization path", () => {
  const main = body("src/main.rs", "main");
  assert.match(
    main,
    /^\s*if buzz_lib::print_private_policy_probe_if_requested\(\)\s*\{\s*return;/,
  );
  assert(
    main.indexOf("print_private_policy_probe_if_requested") <
      main.indexOf("webkit_rendering::apply"),
  );
  assert(
    main.indexOf("print_private_policy_probe_if_requested") <
      main.indexOf("buzz_lib::run()"),
  );
});

test("every legacy startup effect remains inside the private compile-time guard", () => {
  const text = code(source("src/lib.rs"));
  const guards = [
    ...text.matchAll(/if private_native::legacy_enabled\(\)[^{]*\{/g),
  ].map((match) => {
    const open = text.indexOf("{", match.index);
    return [open, closingBrace(text, open)];
  });
  for (const call of [
    "backfill_persona_snapshots(",
    "warm_harness_registry_from_dir(",
    "ensure_nest()",
    "try_regenerate_nest(",
    ".start_stt_download(",
    ".start_tts_download(",
    "sweep_system_agent_processes_with_grace(",
    "reap_dead_instance_agents(",
    "flush_active_pending_events(",
    ".managed_agent_restore_pending",
  ]) {
    let found = 0;
    for (
      let index = text.indexOf(call);
      index >= 0;
      index = text.indexOf(call, index + 1)
    ) {
      found++;
      assert(
        guards.some(([start, end]) => start < index && end > index),
        `${call} outside native policy`,
      );
    }
    assert(found > 0, `missing inspected production seam ${call}`);
  }
  assert.match(
    text,
    /#\[cfg\(all\(feature =\s*, not\(ortak_private_desktop\)\)\)\]\s*\{\s*crate::mesh_llm::install_progress_sink/,
  );
});

test("workspace apply, shutdown and central spawn cannot bypass the native boundary", () => {
  const workspace = body("src/commands/workspace.rs", "apply_workspace");
  assert.match(
    workspace,
    /^\s*crate::private_native::require_workspace_apply\(&relay_url, nsec\.as_deref\(\)\)\?;/,
  );
  const relay = body("src/relay.rs", "relay_ws_url");
  assert(
    relay.indexOf("private_native::selected_company_relay()") <
      relay.indexOf("configured_env_var("),
  );
  const gate = workspace.indexOf("if !crate::private_native::legacy_enabled()");
  assert(gate > workspace.indexOf("*override_guard = Some(relay_url)"));
  assert(
    gate < workspace.indexOf("provider_access::reconcile_on_workspace_apply"),
  );
  assert.match(
    workspace.slice(gate),
    /^if !crate::private_native::legacy_enabled\(\)\s*\{[^}]*return Ok\(\(\)\)/,
  );
  for (const [path, name] of [
    ["src/managed_agents/runtime.rs", "spawn_agent_child"],
    ["src/managed_agents/runtime.rs", "start_managed_agent_process"],
    ["src/managed_agents/restore.rs", "restore_managed_agents_on_launch"],
  ]) {
    assert.match(
      body(path, name),
      /^\s*crate::private_native::require_legacy\(\)\?;/,
    );
  }
  assert.match(
    body("src/shutdown.rs", "shutdown_managed_agents"),
    /^\s*if !crate::private_native::legacy_enabled\(\)\s*\{\s*return Ok\(\(\)\);/,
  );
  for (const [path, name] of [
    [
      "src/managed_agents/runtime/orphan_sweep.rs",
      "sweep_system_agent_processes",
    ],
    [
      "src/managed_agents/runtime/instance_reaper.rs",
      "reap_dead_instance_agents",
    ],
  ]) {
    assert.match(
      body(path, name),
      /^\s*if !crate::private_native::legacy_enabled\(\)\s*\{\s*return;/,
    );
  }
  const migrations = body("src/migration.rs", "run_boot_migrations_inner");
  assert(
    migrations.indexOf("if !crate::private_native::legacy_enabled()") <
      migrations.indexOf("maybe_migrate_dev_repos_dir("),
  );
});
