import assert from "node:assert/strict";
import { register } from "node:module";
import { after, afterEach, test } from "node:test";
import { JSDOM } from "jsdom";

// Build selection and decorative theme only; native-origin, identity query,
// production message menu and Radix keyboard semantics remain real.
register(
  `data:text/javascript,${encodeURIComponent(`
export async function load(url, context, nextLoad) {
  if (url.endsWith('/shared/theme/ThemeProvider.tsx')) return { format:'module', shortCircuit:true, source:'export const useTheme=()=>({isDark:false});' };
  const result = await nextLoad(url, context);
  if (url.endsWith('/features/ortak/useOrtakOrigin.ts')) result.source = String(result.source).replace('import.meta.env.VITE_ORTAK_API_BINDINGS_JSON', 'globalThis.__EMPLOYEE_MEMORY_BINDINGS');
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
const actor = "a".repeat(64),
  clients = [],
  native = [];
window.__TAURI_INTERNALS__ = {
  invoke: async (command) => {
    native.push(command);
    if (command === "get_relay_http_url") return "http://localhost:3038";
    if (command === "list_custom_emoji") return [];
    throw new Error(`Unexpected native action ${command}`);
  },
};
afterEach(async () => {
  (await import("@testing-library/react")).cleanup();
  for (const client of clients.splice(0)) client.clear();
  native.length = 0;
});
after(() => dom.window.close());

async function menu(override = {}, configured = true) {
  globalThis.__EMPLOYEE_MEMORY_BINDINGS = configured
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
  client.setQueryData(["identity"], { pubkey: actor, displayName: "Owner" });
  client.setQueryData(["custom-emoji"], []);
  clients.push(client);
  const view = testing.render(
    createElement(
      QueryClientProvider,
      { client },
      createElement(
        TooltipProvider,
        null,
        createElement(MessageActionBar, {
          channelId: "11111111-1111-4111-8111-111111111111",
          message: {
            id: "b".repeat(64),
            kind: 9,
            pubkey: actor,
            body: "Private source must never be copied automatically",
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
  await testing.waitFor(() => assert.ok(view.getByRole("menu")));
  // Allow the selected native origin query to settle before testing absence.
  await testing.act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  return { ...testing, view };
}

test("actual private Office menu exposes own-message employee review separately from Work and conversation", async () => {
  const x = await menu();
  assert.ok(
    await x.findByRole(document.body, "menuitem", {
      name: "Review employee memory",
    }),
  );
  assert.ok(
    x.getByRole(document.body, "menuitem", {
      name: "Review conversation memory",
    }),
  );
  assert.ok(
    x.getByRole(document.body, "menuitem", { name: "Promote to Work" }),
  );
  assert.equal(
    native.includes("sign_event"),
    false,
    "opening the menu performs no memory API command",
  );
});

test("other authors, delegated signers, agents, pending/encrypted messages and normal mode have no employee review entry", async () => {
  for (const [override, configured] of [
    [{ pubkey: "c".repeat(64) }, true],
    [{ signerPubkey: "c".repeat(64) }, true],
    [{ isAgent: true }, true],
    [{ pending: true }, true],
    [{ kind: 1059 }, true],
    [{}, false],
  ]) {
    const x = await menu(override, configured);
    assert.equal(
      x.queryByRole(document.body, "menuitem", {
        name: "Review employee memory",
      }),
      null,
    );
    x.view.unmount();
  }
  assert.equal(native.includes("sign_event"), false);
});
