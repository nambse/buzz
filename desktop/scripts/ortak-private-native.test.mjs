import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";
import {
  privateNativePlan,
  runPrivateNative,
} from "./ortak-private-native.mjs";

const contaminated = {
  PATH: "/build/tools",
  HOME: "/isolated-home",
  CARGO_HOME: "/cargo-cache",
  RUSTC: "/pinned/rustc",
  CARGO_TARGET_DIR: "/root/shared-target",
  BUZZ_PRIVATE_KEY: "never-export-this",
  NOSTR_PRIVATE_KEY: "never-export-this",
  BUZZ_SHARE_IDENTITY: "1",
  BUZZ_DEV_KEYRING_SERVICE: "buzz-desktop-dev.old",
  BUZZ_BUILD_DEMO_SLUG: "old-demo",
  BUZZ_BUILD_AGENT_ENV: "TOKEN=never-export-this",
  BUZZ_BUILD_RELAY_RECONNECT_CMD: "unwanted-hook",
  BUZZ_RELAY_URL: "wss://old.example",
  BUZZ_TAURI_CLI_ENTRYPOINT: "/unwanted/launcher",
  OPENAI_API_KEY: "never-export-this",
  TAURI_CONFIG: '{"identifier":"xyz.block.buzz.app"}',
  TAURI_DEV_HOST: "0.0.0.0",
  VITE_BUZZ_BESTIE: "1",
  VITE_ORTAK_PRIVATE_MODE: "false",
  ORTAK_PRIVATE_DESKTOP: "0",
  VITE_ORTAK_ENCRYPTED_DM_CHANNELS_JSON:
    '{"http://unwanted.example":["unwanted"]}',
  BUZZ_PROTECTED_BUILD_OUTPUT: "/unwanted/output",
  APPLE_SIGNING_IDENTITY: "old-signer",
  ORTAK_NATIVE_CARGO: "/pinned/cargo",
};

test("actual native invocation pins private identity, origin and isolated target before any child", () => {
  const calls = [];
  const status = runPrivateNative(["build"], {
    env: contaminated,
    spawn: (...args) => {
      calls.push(args);
      return { status: 0 };
    },
  });
  assert.equal(status, 0);
  assert.equal(calls.length, 1);
  const [executable, args, options] = calls[0];
  assert.equal(executable, process.execPath);
  assert.equal(path.basename(args[0]), "tauri-command.mjs");
  assert.deepEqual(args.slice(1, 7), [
    "build",
    "--debug",
    "--bundles",
    "app",
    "--no-sign",
    "--ci",
  ]);
  assert.equal(path.basename(args[8]), "tauri.ortak-private.conf.json");
  assert.deepEqual(args.slice(9), [
    "--features",
    "system-keyring",
    "--runner",
    "/pinned/cargo",
    "--",
    "--no-default-features",
  ]);
  assert.equal(options.env.BUZZ_BUILD_DEMO_SLUG, "ortak-private-20260905");
  assert.equal(options.env.BUZZ_RELAY_URL, "ws://localhost:3038");
  assert.equal(options.env.BUZZ_BUILD_AUTO_CONNECT_DEFAULT_RELAY, "1");
  assert.equal(options.env.VITE_ORTAK_PRIVATE_MODE, "true");
  assert.equal(options.env.ORTAK_PRIVATE_DESKTOP, "1");
  assert.equal(options.env.VITE_BUZZ_BESTIE, "0");
  assert.deepEqual(JSON.parse(options.env.VITE_ORTAK_API_BINDINGS_JSON), {
    "http://localhost:3038": "http://127.0.0.1:8787",
  });
  assert.deepEqual(
    JSON.parse(options.env.VITE_ORTAK_ENCRYPTED_DM_CHANNELS_JSON),
    {
      "http://localhost:3038": ["be203245-5ca3-4a47-9d88-2c20fc65622a"],
    },
  );
  assert.equal(options.env.CARGO_HOME, "/cargo-cache");
  assert.equal(options.env.RUSTC, "/pinned/rustc");
  assert.ok(options.env.CARGO_TARGET_DIR.startsWith(options.cwd + path.sep));
  assert.match(options.env.CARGO_TARGET_DIR, /target\/ortak-private-native$/);
  assert.doesNotMatch(
    JSON.stringify(options.env),
    /never-export-this|unwanted|old-demo|old-signer|shared-target/,
  );
  for (const name of [
    "BUZZ_SHARE_IDENTITY",
    "TAURI_CONFIG",
    "TAURI_DEV_HOST",
    "BUZZ_DEV_KEYRING_SERVICE",
  ])
    assert.equal(options.env[name], undefined);
});

test("dry plan launches nothing, discloses no inherited secrets, and agrees with the real Tauri config", () => {
  let printed = "";
  assert.equal(
    runPrivateNative([], {
      env: contaminated,
      write: (text) => {
        printed = text;
      },
      spawn: () => {
        assert.fail("plan must never launch a child");
      },
    }),
    0,
  );
  const plan = JSON.parse(printed);
  const actual = privateNativePlan("dev", contaminated);
  assert.equal(plan.identifier, actual.config.identifier);
  assert.equal(plan.keyringService, "buzz-desktop-demo.ortak-private-20260905");
  assert.equal(plan.nestName, ".buzz-demo-ortak-private-20260905");
  assert.deepEqual(actual.config.bundle.externalBin, []);
  assert.deepEqual(actual.config.plugins["deep-link"].desktop.schemes, [
    "buzz-demo-ortak-private-20260905",
  ]);
  assert.match(
    actual.config.build.beforeDevCommand.script,
    /--host localhost --port 1427 --strictPort$/,
  );
  assert.equal(
    actual.config.build.beforeBuildCommand.script,
    "node ./scripts/ortak-private-native.mjs frontend",
  );
  assert.equal(actual.args[1], "dev");
  assert.deepEqual(actual.args.slice(4), [
    "--features",
    "system-keyring",
    "--runner",
    "/pinned/cargo",
    "--",
    "--no-default-features",
  ]);
  assert.doesNotMatch(printed, /never-export-this/);
});

test("packaging uses production frontend matrix and propagates failure before starting the next stage", () => {
  const calls = [];
  const env = {
    ...contaminated,
    BUZZ_PROTECTED_BUILD_OUTPUT: "/fresh-invocation/dist",
  };
  const status = runPrivateNative(["frontend"], {
    env,
    spawn: (...args) => {
      calls.push(args);
      return { status: 0 };
    },
  });
  assert.equal(status, 0);
  assert.equal(path.basename(calls[0][1][0]), "tsc");
  assert.equal(
    path.basename(calls[1][1][0]),
    "build-protected-feature-artifacts.mjs",
  );
  assert.equal(
    calls[1][2].env.BUZZ_PROTECTED_BUILD_OUTPUT,
    "/fresh-invocation/dist",
  );
  assert.equal(calls[1][2].env.VITE_ORTAK_PRIVATE_MODE, "true");
  let count = 0;
  assert.equal(
    runPrivateNative(["frontend"], {
      env,
      spawn: () => {
        count++;
        return { status: 23 };
      },
    }),
    23,
  );
  assert.equal(count, 1);
  assert.equal(
    runPrivateNative(["dev"], { env, spawn: () => ({ status: null }) }),
    1,
  );
  assert.throws(
    () => runPrivateNative(["dev", "--config", "unsafe.json"]),
    /one action/,
  );
});
