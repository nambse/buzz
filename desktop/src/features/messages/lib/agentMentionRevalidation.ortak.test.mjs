import assert from "node:assert/strict";
import { register } from "node:module";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";

register(
  `data:text/javascript,${encodeURIComponent(`
export async function load(url, context, nextLoad) {
  const result = await nextLoad(url, context);
  if (url.endsWith('/features/ortak/privateMode.ts')) result.source = String(result.source).replace('import.meta.env?.VITE_ORTAK_PRIVATE_MODE', '"true"');
  if (url.endsWith('/messages/lib/ortakEmployeeMentions.ts')) result.source = String(result.source).replace('import.meta.env?.VITE_ORTAK_API_BINDINGS_JSON', JSON.stringify('{"http://127.0.0.1:3038":"http://127.0.0.1:3039"}'));
  return result;
}`)}`,
  import.meta.url,
);

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
const HUMAN = "a".repeat(64),
  ADA = "b".repeat(64),
  CHANNEL = "11111111-2222-3333-4444-555555555555";
const calls = [];
let relay, channel, members, readFailure, switchRelay;
before(() => {
  Object.assign(globalThis, {
    window: dom.window,
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  dom.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      calls.push([command, args]);
      if (command === "get_relay_http_url") return relay;
      if (command === "get_channel_details") return channel;
      if (command === "get_channel_members") {
        if (readFailure) throw new Error("disconnected");
        if (switchRelay) relay = "http://127.0.0.1:4040";
        return {
          members: members.map((pubkey) => ({
            pubkey,
            role: pubkey === ADA ? "bot" : "member",
          })),
          next_cursor: null,
        };
      }
      assert.fail(`unexpected legacy command ${command}`);
    },
  };
});
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());

async function setup(scope = { type: "channel", channelId: CHANNEL }) {
  calls.length = 0;
  relay = "http://127.0.0.1:3038";
  channel = { id: CHANNEL, channel_type: "stream", archived_at: null };
  members = [HUMAN, ADA];
  readFailure = false;
  switchRelay = false;
  const { renderHook } = await import("@testing-library/react");
  const { useAgentMentionRevalidation } = await import(
    "./agentMentionRevalidation.ts"
  );
  return renderHook(() =>
    useAgentMentionRevalidation({
      agentPubkeys: new Set([ADA]),
      getSelectedAgentPubkeys: () => new Set([ADA]),
      currentPubkey: HUMAN,
      eligibilityScope: scope,
      sharedChannelIds: new Set(),
      refetchManagedAgents: async () => assert.fail("no local agent inventory"),
    }),
  );
}

test("real private hook reads current native channel membership at prepare and publish, without legacy wake policy", async () => {
  const hook = await setup();
  for (const phase of ["prepare", "publish"])
    assert.deepEqual(await hook.result.current([ADA], CHANNEL, { phase }), [
      ADA,
    ]);
  assert.equal(
    calls.filter(([command]) => command === "get_channel_members").length,
    2,
  );
  assert.equal(
    calls.filter(([command]) => command === "get_channel_details").length,
    2,
  );
  members = [HUMAN];
  await assert.rejects(
    hook.result.current([ADA], CHANNEL, { phase: "publish" }),
    /Could not verify/,
  );
});

for (const scenario of [
  "unbound",
  "switched",
  "dm",
  "archived",
  "wrong-channel",
  "human-removed",
  "unknown",
  "disconnected",
  "missing",
])
  test(`private native mention gate refuses ${scenario} without legacy fallback`, async () => {
    const hook = await setup(
      scenario === "missing" ? { type: "owned", channelId: null } : undefined,
    );
    if (scenario === "unbound") relay = "http://127.0.0.1:4040";
    if (scenario === "switched") switchRelay = true;
    if (scenario === "dm") channel.channel_type = "dm";
    if (scenario === "archived") channel.archived_at = "2026-01-01";
    if (scenario === "wrong-channel") channel.id = "another";
    if (scenario === "human-removed") members = [ADA];
    if (scenario === "disconnected") readFailure = true;
    const key = scenario === "unknown" ? "c".repeat(64) : ADA;
    await assert.rejects(
      hook.result.current([key], scenario === "missing" ? null : CHANNEL),
      /Could not verify/,
    );
  });

test("real private readiness hook never reads local personas or queues a wake for a cached managed employee", async () => {
  const { renderHook } = await import("@testing-library/react");
  const { useEnsureAgentMentionsReady } = await import(
    "../ui/useEnsureAgentMentionsReady.ts"
  );
  const forbidden = async () => assert.fail("legacy preparation must not run");
  const hook = renderHook(() =>
    useEnsureAgentMentionsReady({
      attachAgentToChannel: forbidden,
      getManagedAgentsByPubkey: forbidden,
      getPersonas: forbidden,
      memberPubkeys: new Set(),
    }),
  );
  assert.deepEqual(
    await hook.result.current(
      [ADA],
      CHANNEL,
      [],
      [{ pubkey: ADA, status: "stopped" }],
    ),
    { errors: [], pubkeys: [], wroteRelayState: false, agentsToWake: [] },
  );
});
