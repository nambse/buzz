import assert from "node:assert/strict";
import { register } from "node:module";
import { after, afterEach, test } from "node:test";
import { JSDOM } from "jsdom";

// Query results are controlled inputs; candidate building, ranking, selection,
// key extraction and native prepare/publish revalidation remain production code.
const queryModules = {
  "/features/agents/hooks.ts": `
      const state = () => globalThis.__ORTAK_MENTION_TEST;
      export const useManagedAgentsQuery = () => state().managed;
      export const useRelayAgentsQuery = () => state().relayAgents;
      export const usePersonasQuery = () => state().personas;
      export const useTeamsQuery = () => ({data:[]});`,
  "/features/channels/hooks.ts": `
      export const useChannelMembersQuery = id => globalThis.__ORTAK_MENTION_TEST.rosters[id] ?? {data:undefined};
      export const useChannelsQuery = () => ({data:[]});`,
  "/features/identity-archive/hooks.ts": `
      export const useIsArchivedPredicate = () => globalThis.__ORTAK_MENTION_TEST.isArchived;`,
  "/features/profile/hooks.ts": `
      export const USERS_BATCH_ENTRY_FRESH_MS = 30000;
      export const usersBatchEntryKey = key => ['profile',key];
      export const useUsersBatchQuery = () => ({data:{profiles:{}}});
      export const useInfiniteUserSearchQuery = (text, options) => {
        globalThis.__ORTAK_MENTION_TEST.searchEnabled = options.enabled;
        return globalThis.__ORTAK_MENTION_TEST.search;
      };`,
  "/shared/api/hooks.ts": `
      export const useIdentityQuery = () => ({data:{pubkey:globalThis.__ORTAK_MENTION_TEST.human}});`,
};
register(
  `data:text/javascript,${encodeURIComponent(`
export async function load(url, context, nextLoad) {
  const modules = ${JSON.stringify(queryModules)};
  for (const [suffix, source] of Object.entries(modules))
    if (url.endsWith(suffix)) return {format:'module',shortCircuit:true,source};
  const result = await nextLoad(url, context);
  if (url.endsWith('/features/ortak/privateMode.ts')) result.source = String(result.source)
    .replace('export const privateOrtakMode =', 'export let privateOrtakMode =')
    + '\\nexport function selectTestPrivateMode(value) { privateOrtakMode = value; }';
  if (url.endsWith('/messages/lib/ortakEmployeeMentions.ts')) result.source = String(result.source)
    .replace('import.meta.env?.VITE_ORTAK_API_BINDINGS_JSON', JSON.stringify('{"http://127.0.0.1:3038":"http://127.0.0.1:3039"}'));
  return result;
}`)}`,
  import.meta.url,
);

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
Object.assign(globalThis, {
  window: dom.window,
  document: dom.window.document,
  HTMLElement: dom.window.HTMLElement,
  localStorage: dom.window.localStorage,
  IS_REACT_ACT_ENVIRONMENT: true,
});
const HUMAN = "a".repeat(64),
  ADA = "b".repeat(64),
  OUTSIDE = "c".repeat(64);
const CHANNEL = "11111111-2222-3333-4444-555555555555";
const clients = [],
  calls = [];
let freshMembers;
window.__TAURI_INTERNALS__ = {
  invoke: async (command) => {
    calls.push(command);
    if (command === "get_relay_http_url") return "http://127.0.0.1:3038";
    if (command === "get_channel_details")
      return { id: CHANNEL, channel_type: "stream", archived_at: null };
    if (command === "get_channel_members")
      return { members: freshMembers, next_cursor: null };
    assert.fail(`unexpected legacy or write command ${command}`);
  },
};
afterEach(async () => {
  (await import("@testing-library/react")).cleanup();
  for (const client of clients.splice(0)) client.clear();
  calls.length = 0;
});
after(() => dom.window.close());

async function setup(privateMode = true) {
  const members = [
    { pubkey: HUMAN, displayName: "Owner", role: "admin" },
    { pubkey: ADA, displayName: "Ada", role: "bot", isAgent: true },
  ];
  freshMembers = members;
  const state = {
    human: HUMAN,
    rosters: { [CHANNEL]: { data: members, isError: false } },
    isArchived: () => false,
    managed: {
      data: [],
      error: new Error("legacy directory unavailable"),
      isSuccess: false,
      isError: true,
    },
    // An offline legacy record lacks the owner's respondTo grant. Office
    // membership, not this independent runtime directory, names the employee.
    relayAgents: {
      data: [
        {
          pubkey: ADA,
          name: "Ada",
          ownerPubkey: OUTSIDE,
          status: "offline",
          respondTo: "owner-only",
          respondToAllowlist: [],
          channelIds: [CHANNEL],
        },
      ],
      error: null,
      isSuccess: true,
      isError: false,
    },
    personas: {
      data: [
        { id: OUTSIDE, displayName: "Unconfigured persona", isActive: true },
      ],
    },
    search: {
      data: {
        pages: [
          {
            users: [
              { pubkey: OUTSIDE, displayName: "Outside", isAgent: false },
            ],
          },
        ],
      },
    },
  };
  globalThis.__ORTAK_MENTION_TEST = state;
  const { selectTestPrivateMode } = await import(
    "@/features/ortak/privateMode.ts"
  );
  selectTestPrivateMode(privateMode);
  const { createElement } = await import("react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { renderHook, act } = await import("@testing-library/react");
  const { useMentions } = await import("./useMentions.ts");
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  clients.push(client);
  const hook = renderHook(
    ({ channelId, externalMembers }) =>
      useMentions(channelId, externalMembers, undefined, {
        channelType: "stream",
      }),
    {
      initialProps: { channelId: CHANNEL, externalMembers: undefined },
      wrapper: ({ children }) =>
        createElement(QueryClientProvider, { client }, children),
    },
  );
  act(() => hook.result.current.openMentionPicker(0, "first-agent"));
  return { ...hook, state, act };
}

for (const modality of ["pointer", "keyboard"])
  test(`private picker selects an offline roster employee with ${modality} and rechecks membership`, async () => {
    const hook = await setup();
    if (modality === "pointer") {
      hook.state.relayAgents = {
        data: [],
        error: null,
        isSuccess: true,
        isError: false,
      };
      hook.rerender({ channelId: CHANNEL });
    }
    assert.deepEqual(
      new Set(hook.result.current.suggestions.map((s) => s.pubkey)),
      new Set([HUMAN, ADA]),
    );
    assert.equal(hook.state.searchEnabled, false);
    let suggestion = hook.result.current.suggestions.find(
      (s) => s.pubkey === ADA,
    );
    assert.equal(suggestion.agentProvenance, undefined);
    assert.equal(suggestion.notInChannel, false);
    if (modality === "keyboard") {
      await hook.act(async () => {
        hook.result.current.updateMentionQuery("@Ad", 3);
        await new Promise((resolve) => setTimeout(resolve, 150));
      });
      assert.deepEqual(
        hook.result.current.suggestions.map((s) => s.pubkey),
        [ADA],
      );
      const event = {
        key: "Enter",
        nativeEvent: { key: "Enter" },
        preventDefault() {},
      };
      hook.act(() => {
        suggestion = hook.result.current.handleMentionKeyDown(event).suggestion;
      });
      assert.equal(suggestion.pubkey, ADA);
    }
    let edit;
    hook.act(() => {
      edit = hook.result.current.insertMention(
        suggestion,
        modality === "keyboard" ? 3 : 0,
      );
    });
    assert.equal(edit.insertText, "@Ada ");
    const keys = hook.result.current.extractMentionPubkeys(
      `${edit.insertText}hello`,
    );
    assert.deepEqual(keys, [ADA]);
    for (const phase of ["prepare", "publish"])
      assert.deepEqual(
        await hook.result.current.revalidateMentionPubkeys(keys, CHANNEL, {
          phase,
        }),
        [ADA],
      );
    freshMembers = freshMembers.filter((member) => member.pubkey !== ADA);
    await assert.rejects(
      hook.result.current.revalidateMentionPubkeys(keys, CHANNEL, {
        phase: "publish",
      }),
      /Could not verify/,
    );
    assert.equal(
      calls.filter((command) => command === "get_channel_members").length,
      3,
    );
  });

test("private picker drops stale discovery on roster error, removal, archive or channel change", async () => {
  const hook = await setup();
  const original = hook.state.rosters[CHANNEL].data;
  hook.state.search = { ...hook.state.search, isFetching: true };
  hook.state.rosters[CHANNEL] = { data: original, isError: true };
  hook.rerender({ channelId: CHANNEL });
  assert.deepEqual(
    hook.result.current.suggestions,
    [],
    "no cached roster fallback on error",
  );
  hook.state.rosters[CHANNEL] = {
    data: original.filter((m) => m.pubkey !== ADA),
    isError: false,
  };
  hook.rerender({ channelId: CHANNEL });
  assert.deepEqual(
    hook.result.current.suggestions.map((s) => s.pubkey),
    [HUMAN],
  );
  hook.state.rosters[CHANNEL] = {
    data: original.filter((m) => m.pubkey !== HUMAN),
    isError: false,
  };
  hook.rerender({ channelId: CHANNEL });
  assert.deepEqual(
    hook.result.current.suggestions,
    [],
    "viewer must remain a member",
  );
  hook.state.rosters[CHANNEL] = { data: original, isError: false };
  hook.state.isArchived = (key) => key === ADA;
  hook.rerender({ channelId: CHANNEL });
  assert.deepEqual(
    hook.result.current.suggestions.map((s) => s.pubkey),
    [HUMAN],
  );
  hook.rerender({ channelId: "another-channel", externalMembers: original });
  assert.deepEqual(
    hook.result.current.suggestions,
    [],
    "no old-channel candidates from lagging external props",
  );
});

test("normal-mode picker retains the legacy respondTo filter", async () => {
  const hook = await setup(false);
  assert.equal(
    hook.result.current.suggestions.some((s) => s.pubkey === ADA),
    false,
  );
  assert.equal(
    hook.result.current.suggestions.some((s) => s.pubkey === HUMAN),
    true,
  );
  assert.equal(
    hook.result.current.suggestions.some((s) => s.kind === "persona"),
    true,
  );
});
