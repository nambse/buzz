import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const desktopRoot = path.resolve(import.meta.dirname, "..");
const configPath = path.join(
  desktopRoot,
  "src-tauri/tauri.ortak-private.conf.json",
);
export const privateNativeIdentity = Object.freeze({
  slug: "ortak-private-20260905",
  identifier: "dev.ortak.private20260905",
  relayUrl: "ws://localhost:3038",
  relayHttp: "http://localhost:3038",
  apiOrigin: "http://127.0.0.1:8787",
  devOrigin: "http://localhost:1427",
  encryptedDmChannels: Object.freeze(["be203245-5ca3-4a47-9d88-2c20fc65622a"]),
});

// Carry build tools and OS paths, never ambient application identities, provider
// credentials, reconnect hooks, signing credentials or preview overrides.
const buildEnvironmentKeys = new Set([
  "PATH",
  "HOME",
  "USER",
  "LOGNAME",
  "SHELL",
  "TMPDIR",
  "TMP",
  "TEMP",
  "TERM",
  "LANG",
  "SYSTEMROOT",
  "COMSPEC",
  "APPDATA",
  "LOCALAPPDATA",
  "CARGO_HOME",
  "CARGO_NET_OFFLINE",
  "RUSTUP_HOME",
  "RUSTC",
  "CC",
  "CXX",
  "SDKROOT",
  "MACOSX_DEPLOYMENT_TARGET",
  "PKG_CONFIG_PATH",
  "CPATH",
  "LIBRARY_PATH",
  "DEVELOPER_DIR",
  "HTTP_PROXY",
  "HTTPS_PROXY",
  "NO_PROXY",
]);

/** Compose the actual child invocation without reading keyrings or app data. */
export function privateNativePlan(mode, sourceEnv = process.env) {
  if (!["plan", "dev", "build", "frontend", "verify-identity"].includes(mode))
    throw new Error(
      "Usage: node scripts/ortak-private-native.mjs [plan|dev|build|verify-identity]",
    );
  const config = JSON.parse(readFileSync(configPath, "utf8"));
  const identity = privateNativeIdentity;
  if (
    config.identifier !== identity.identifier ||
    config.build.devUrl !== identity.devOrigin ||
    JSON.stringify(config.plugins["deep-link"].desktop.schemes) !==
      JSON.stringify([`buzz-demo-${identity.slug}`]) ||
    config.bundle.externalBin.length !== 0
  )
    throw new Error(
      "Private native configuration and compiled identity disagree.",
    );

  const env = Object.fromEntries(
    Object.entries(sourceEnv).filter(
      ([key]) => buildEnvironmentKeys.has(key) || key.startsWith("LC_"),
    ),
  );
  const target = path.join(
    desktopRoot,
    "src-tauri/target/ortak-private-native",
  );
  Object.assign(env, {
    CARGO_TARGET_DIR: target,
    CARGO_INCREMENTAL: "0",
    CARGO_BUILD_JOBS: "2",
    CARGO_PROFILE_DEV_DEBUG: "0",
    ORTAK_PRIVATE_DESKTOP: "1",
    BUZZ_BUILD_DEMO_SLUG: identity.slug,
    BUZZ_BUILD_AUTO_CONNECT_DEFAULT_RELAY: "1",
    BUZZ_RELAY_URL: identity.relayUrl,
    BUZZ_RELAY_HTTP: identity.relayHttp,
    VITE_ORTAK_PRIVATE_MODE: "true",
    VITE_ORTAK_API_BINDINGS_JSON: JSON.stringify({
      [identity.relayHttp]: identity.apiOrigin,
    }),
    VITE_ORTAK_ENCRYPTED_DM_CHANNELS_JSON: JSON.stringify({
      [identity.relayHttp]: identity.encryptedDmChannels,
    }),
    VITE_BUZZ_BESTIE: "0",
    VITE_PORT: "1427",
  });
  // Only the existing packaging wrapper supplies this to its frontend child.
  // Top-level build/dev invocations discard any inherited output override.
  if (mode === "frontend" && sourceEnv.BUZZ_PROTECTED_BUILD_OUTPUT)
    env.BUZZ_PROTECTED_BUILD_OUTPUT = sourceEnv.BUZZ_PROTECTED_BUILD_OUTPUT;

  const args = [path.join(desktopRoot, "scripts/tauri-command.mjs")];
  if (mode === "build")
    args.push("build", "--debug", "--bundles", "app", "--no-sign", "--ci");
  else args.push("dev");
  args.push("--config", configPath);
  args.push("--features", "system-keyring");
  if (sourceEnv.ORTAK_NATIVE_CARGO) {
    if (!path.isAbsolute(sourceEnv.ORTAK_NATIVE_CARGO))
      throw new Error(
        "ORTAK_NATIVE_CARGO must be an absolute pinned Cargo path.",
      );
    args.push("--runner", sourceEnv.ORTAK_NATIVE_CARGO);
  }
  // Tauri forwards Cargo-only switches after its argument separator. Private
  // builds omit the default-on legacy-terminal and legacy-voice graphs.
  args.push("--", "--no-default-features");
  return { identity, config, env, args, cwd: desktopRoot, target };
}

/** Start only the explicitly selected action; the default prints a public plan. */
export function runPrivateNative(args, options = {}) {
  if (args.length > 1)
    throw new Error("Private native recipe accepts one action only.");
  const mode = args[0] ?? "plan";
  const plan = privateNativePlan(mode, options.env ?? process.env);
  const write = options.write ?? console.log;
  if (mode === "plan") {
    write(
      JSON.stringify(
        {
          ...plan.identity,
          keyringService: `buzz-desktop-demo.${plan.identity.slug}`,
          nestName: `.buzz-demo-${plan.identity.slug}`,
          credentialDirectory: `buzz-demo-${plan.identity.slug}`,
          target: plan.target,
          buildArtifact: path.join(
            plan.target,
            "debug/bundle/macos/Ortak Private.app",
          ),
          allowedWebOrigins: [plan.identity.devOrigin, "tauri://localhost"],
          config: configPath,
          action: "No build or application launched.",
          nativeExecutionPolicy: "compiled-office-only",
        },
        null,
        2,
      ),
    );
    return 0;
  }
  const spawn = options.spawn ?? spawnSync;
  const invoke = (childArgs) => {
    const result = spawn(process.execPath, childArgs, {
      cwd: plan.cwd,
      env: plan.env,
      stdio: "inherit",
    });
    if (result.error) throw result.error;
    return result.status ?? 1;
  };
  if (mode === "verify-identity") {
    const scratch = mkdtempSync(path.join(tmpdir(), "ortak-native-identity-"));
    try {
      const binary = path.join(scratch, "identity-tests");
      const result = spawn(
        plan.env.RUSTC ?? "rustc",
        [
          "--edition=2021",
          "--test",
          "--cfg",
          "ortak_private_desktop",
          "-A",
          "dead_code",
          path.join(desktopRoot, "scripts/ortak-private-native-identity.rs"),
          "-o",
          binary,
        ],
        {
          cwd: plan.cwd,
          env: {
            ...plan.env,
            BUZZ_DESKTOP_BUILD_DEMO_SLUG: plan.identity.slug,
            BUZZ_DESKTOP_BUILD_RELAY_URL: plan.env.BUZZ_RELAY_URL,
          },
          stdio: "inherit",
          timeout: 60_000,
        },
      );
      if (result.error) throw result.error;
      if (result.status !== 0) return result.status ?? 1;
      const tests = spawn(binary, [], {
        cwd: plan.cwd,
        env: plan.env,
        stdio: "inherit",
        timeout: 30_000,
      });
      if (tests.error) throw tests.error;
      return tests.status ?? 1;
    } finally {
      rmSync(scratch, { recursive: true, force: true });
    }
  }
  if (mode === "frontend") {
    const status = invoke([
      path.join(desktopRoot, "node_modules/typescript/bin/tsc"),
    ]);
    return status === 0
      ? invoke([
          path.join(
            desktopRoot,
            "scripts/build-protected-feature-artifacts.mjs",
          ),
        ])
      : status;
  }
  return invoke(plan.args);
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    process.exitCode = runPrivateNative(process.argv.slice(2));
  } catch (error) {
    console.error(
      error instanceof Error ? error.message : "Private native recipe failed.",
    );
    process.exitCode = 1;
  }
}
