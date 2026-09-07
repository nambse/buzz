import assert from "node:assert/strict";
import { register } from "node:module";
import { after, afterEach, test } from "node:test";
import { JSDOM } from "jsdom";

// Substitute build selection and decorative theme context; use the production native origin
// lookup, message action bar, Radix menu/dialog and signed HTTP client unchanged.
register(
  `data:text/javascript,${encodeURIComponent(`
export async function load(url, context, nextLoad) {
  if (url.endsWith('/shared/theme/ThemeProvider.tsx')) return {
    format: 'module', shortCircuit: true,
    source: 'export const useTheme = () => ({isDark:false}); export const ThemeProvider = ({children}) => children;'
  };
  const result = await nextLoad(url, context);
  if (url.endsWith('/features/ortak/useOrtakOrigin.ts')) result.source = String(result.source)
    .replace('import.meta.env.VITE_ORTAK_API_BINDINGS_JSON', 'globalThis.__PROMOTION_TEST_BINDINGS');
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
    url.includes("/routing")
      ? { message_id: "a".repeat(64), channel_id: "channel", decision: null }
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

async function menu(configured, override = {}) {
  globalThis.__PROMOTION_TEST_BINDINGS = configured
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
            body: "Office source",
            ...override,
          },
          reactions: [],
          onRemindLater: () => {},
        }),
      ),
    ),
  );
  return { ...testing, view };
}

test("actual message More menu opens promotion by keyboard after menu focus release", async () => {
  const x = await menu(true);
  assert.equal(requests.length, 0);
  await x.act(async () =>
    x.fireEvent.keyDown(x.view.getByRole("button", { name: "More actions" }), {
      key: "ArrowDown",
    }),
  );
  const action = await x.findByRole(document.body, "menuitem", {
    name: "Promote to Work",
  });
  assert.equal(
    requests.length,
    0,
    "the menu must not fetch every message's Work projects",
  );
  await x.act(async () => x.fireEvent.keyDown(action, { key: "Enter" }));
  await x.waitFor(() =>
    assert.ok(x.view.getByRole("dialog", { name: "Promote message to Work" })),
  );
  await x.waitFor(() => assert.equal(requests.length, 2));
  assert.ok(requests.some((url) => url.endsWith("/projects?limit=25")));
  assert.ok(native.includes("sign_event"));
});

test("actual message menu pointer activation reaches the dialog, while legacy and pending messages have no promotion action", async () => {
  const x = await menu(true);
  await x.act(async () =>
    x.fireEvent.keyDown(x.view.getByRole("button", { name: "More actions" }), {
      key: "ArrowDown",
    }),
  );
  const action = await x.findByRole(document.body, "menuitem", {
    name: "Promote to Work",
  });
  await x.act(async () => x.fireEvent.click(action));
  await x.waitFor(() =>
    assert.ok(x.view.getByRole("dialog", { name: "Promote message to Work" })),
  );
  x.cleanup();
  for (const [configured, message] of [
    [false, {}],
    [true, { pending: true }],
    [true, { kind: 1059 }],
  ]) {
    const y = await menu(configured, message);
    await y.act(async () =>
      y.fireEvent.keyDown(
        y.view.getByRole("button", { name: "More actions" }),
        { key: "ArrowDown" },
      ),
    );
    assert.equal(
      y.queryByRole(document.body, "menuitem", { name: "Promote to Work" }),
      null,
    );
    y.cleanup();
  }
});
