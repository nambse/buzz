import assert from "node:assert/strict";
import { register } from "node:module";
import test from "node:test";

// Substitute only Vite's build-time environment. Every assertion below invokes
// production modules, including the actual shared native-command boundary.
register(
  `data:text/javascript,${encodeURIComponent(`
export async function load(url, context, nextLoad) {
  const result = await nextLoad(url, context);
  if (url.endsWith('/features/ortak/privateMode.ts')) return {
    ...result,
    source: String(result.source).replace('import.meta.env?.VITE_ORTAK_PRIVATE_MODE', '"true"'),
  };
  return result;
}`)}`,
  import.meta.url,
);

const calls = [];
globalThis.window = {
  __TAURI_INTERNALS__: {
    invoke: async (command) => {
      calls.push(command);
      return "signed";
    },
  },
};
const { invokeTauri } = await import("@/shared/api/tauri.ts");
const { privateFeatureBlocked, privateSettingsAllowed, privateRouteAllowed } =
  await import("./privateMode.ts");

test("private mode blocks legacy gateway commands at the production IPC boundary and keeps native signing", async () => {
  for (const command of [
    "create_managed_agent",
    "start_managed_agent",
    "start_managed_agent_runtime",
    "restart_managed_agent_runtime",
    "reconcile_managed_agent_runtimes",
    "put_managed_agent_runtime_lifecycle",
    "create_persona",
    "confirm_agent_snapshot_import",
    "create_team",
    "confirm_team_snapshot_import",
    "connect_acp_runtime",
  ])
    await assert.rejects(invokeTauri(command), /Ortak control plane/);
  assert.deepEqual(calls, []);
  assert.equal(await invokeTauri("sign_event"), "signed");
  assert.deepEqual(calls, ["sign_event"]);
});

test("private routes and saved preview flags cannot expose unbuilt flows", () => {
  for (const feature of ["projects", "workflows", "pulse", "forum"])
    assert.equal(privateFeatureBlocked(feature), true);
  for (const path of [
    "/projects",
    "/projects/test",
    "/workflows/test",
    "/pulse",
  ])
    assert.equal(privateRouteAllowed(path), false);
  for (const path of ["/", "/agents", "/channels/test", "/settings"])
    assert.equal(privateRouteAllowed(path), true);
  for (const section of [
    "agents",
    "compute",
    "experimental",
    "channel-templates",
    "hosted-communities",
  ])
    assert.equal(privateSettingsAllowed(section), false);
  for (const section of ["profile", "appearance", "notifications"])
    assert.equal(privateSettingsAllowed(section), true);
});
