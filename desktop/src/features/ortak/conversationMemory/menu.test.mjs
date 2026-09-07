import assert from "node:assert/strict";
import { register } from "node:module";
import { after, afterEach, test } from "node:test";
import { JSDOM } from "jsdom";

// Only build selection and decorative theme context are substituted. The actual
// message action bar, native origin/signing seam and Radix focus handoff execute.
register(
  `data:text/javascript,${encodeURIComponent(`
export async function load(url, context, nextLoad) {
  if (url.endsWith('/shared/theme/ThemeProvider.tsx')) return {
    format:'module', shortCircuit:true,
    source:'export const useTheme=()=>({isDark:false}); export const ThemeProvider=({children})=>children;'
  };
  const result = await nextLoad(url, context);
  if (url.endsWith('/features/ortak/useOrtakOrigin.ts')) result.source = String(result.source)
    .replace('import.meta.env.VITE_ORTAK_API_BINDINGS_JSON', 'globalThis.__CONVERSATION_TEST_BINDINGS');
  return result;
}`)}`,
  import.meta.url,
);
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
  pretendToBeVisual: true,
});
for (const name of [
  "window",
  "document",
  "HTMLElement",
  "HTMLInputElement",
  "Element",
  "Node",
  "NodeFilter",
  "CustomEvent",
  "MutationObserver",
  "Event",
  "MouseEvent",
  "KeyboardEvent",
])
  Object.defineProperty(globalThis, name, {
    value: name === "window" ? dom.window : dom.window[name],
    configurable: true,
  });
globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const originalFetch = globalThis.fetch;
const requests = [],
  native = [],
  clients = [];
window.__TAURI_INTERNALS__ = {
  invoke: async (command, payload) => {
    native.push(command);
    if (command === "get_relay_http_url") return "http://localhost:3038";
    if (command === "sign_event")
      return JSON.stringify({
        ...payload,
        id: "fixture",
        pubkey: "fixture",
        sig: "fixture",
      });
    if (command === "list_custom_emoji") return [];
    throw new Error(`Unexpected native command: ${command}`);
  },
};
globalThis.fetch = async (url) => {
  requests.push(url);
  return Response.json(
    url.includes("/employees")
      ? { employees: [], has_more: false, next_after: null }
      : {
          projects: [],
          next_cursor: null,
          can_create_projects: false,
          create_channels: [],
        },
  );
};
afterEach(async () => {
  (await import("@testing-library/react")).cleanup();
  for (const client of clients.splice(0)) client.clear();
  requests.length = 0;
  native.length = 0;
});
after(() => {
  globalThis.fetch = originalFetch;
  dom.window.close();
});

async function menu(configured = true, override = {}) {
  globalThis.__CONVERSATION_TEST_BINDINGS = configured
    ? '{"http://localhost:3038":"http://127.0.0.1:3010"}'
    : undefined;
  const { createElement } = await import("react");
  const testing = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { TooltipProvider } = await import("@/shared/ui/tooltip.tsx");
  const { MessageActionBar } = await import(
    "@/features/messages/ui/MessageActionBar.tsx"
  );
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  clients.push(client);
  client.setQueryData(["custom-emoji"], []);
  const view = testing.render(
    createElement(
      QueryClientProvider,
      { client },
      createElement(
        TooltipProvider,
        null,
        createElement(MessageActionBar, {
          channelId: "channel",
          message: {
            id: "a".repeat(64),
            kind: 9,
            body: "Never prefill this source body",
            ...override,
          },
          reactions: [],
          onRemindLater: () => {},
        }),
      ),
    ),
  );
  await testing.act(async () =>
    testing.fireEvent.keyDown(
      view.getByRole("button", { name: "More actions" }),
      { key: "ArrowDown" },
    ),
  );
  return { ...testing, view };
}

test("actual message menu keyboard opens independent review dialog and restores More focus on close", async () => {
  const x = await menu();
  const action = await x.findByRole(document.body, "menuitem", {
    name: "Review conversation memory",
  });
  assert.equal(
    requests.length,
    0,
    "opening a menu must not fetch every message's audiences",
  );
  assert.ok(
    x.getByRole(document.body, "menuitem", { name: "Promote to Work" }),
    "existing Work promotion stays separate",
  );
  await x.act(async () => x.fireEvent.keyDown(action, { key: "Enter" }));
  const dialog = await x.findByRole(document.body, "dialog", {
    name: "Review conversation memory",
  });
  await x.waitFor(() => assert.equal(requests.length, 2));
  assert.ok(native.includes("sign_event"));
  assert.equal(x.queryByText(dialog, "Never prefill this source body"), null);
  assert.equal(
    x.queryByRole(document.body, "dialog", { name: "Promote message to Work" }),
    null,
  );
  await x.act(async () => x.fireEvent.keyDown(dialog, { key: "Escape" }));
  await x.waitFor(() =>
    assert.equal(
      x.queryByRole(document.body, "dialog", {
        name: "Review conversation memory",
      }),
      null,
    ),
  );
  await x.waitFor(() =>
    assert.equal(
      document.activeElement,
      x.view.getByRole("button", { name: "More actions" }),
    ),
  );
});

test("pointer activation reaches review while pending, encrypted and unconfigured messages expose no action", async () => {
  const x = await menu();
  const action = await x.findByRole(document.body, "menuitem", {
    name: "Review conversation memory",
  });
  await x.act(async () => x.fireEvent.click(action));
  await x.findByRole(document.body, "dialog", {
    name: "Review conversation memory",
  });
  x.cleanup();
  for (const [configured, override] of [
    [false, {}],
    [true, { pending: true }],
    [true, { kind: 1059 }],
    [true, { kind: 14 }],
  ]) {
    const y = await menu(configured, override);
    assert.equal(
      y.queryByRole(document.body, "menuitem", {
        name: "Review conversation memory",
      }),
      null,
    );
    y.cleanup();
  }
});
