import assert from "node:assert/strict";
import { register } from "node:module";
import test, { after, afterEach } from "node:test";
import { JSDOM } from "jsdom";

// Keep production provider, signed client, directory, Badge and message-owner
// branch and the real profile interaction; replace native I/O and navigation.
register(
  `data:text/javascript,${encodeURIComponent(`
export async function load(url,context,nextLoad) {
 if(url.endsWith('/shared/api/tauri.ts')) return {format:'module',shortCircuit:true,source:
  "export * from "+JSON.stringify(url+'?actual')+"; export const signRelayEvent = async (event) => { globalThis.__identityFixture.signed.push(event); return {...event,id:'fixture-signature'}; }; export const getRelayHttpUrl=async()=>globalThis.__identityFixture.relay;"};
 if(url.endsWith('/app/navigation/useAppNavigation.ts')) return {format:'module',shortCircuit:true,source:'export const useAppNavigation=()=>({goAgents:()=>globalThis.__identityFixture.opened++});'};
 const result=await nextLoad(url,context);
 if(url.endsWith('/features/ortak/privateMode.ts')) result.source=String(result.source).replace('import.meta.env?.VITE_ORTAK_PRIVATE_MODE','"true"');
 if(url.endsWith('/features/ortak/useOrtakOrigin.ts')) result.source=String(result.source).replace('import.meta.env.VITE_ORTAK_API_BINDINGS_JSON',JSON.stringify(JSON.stringify({'http://localhost:3038':'http://127.0.0.1:8787','http://localhost:3039':'http://127.0.0.1:8788'})));
 return result;
}
`)}`,
  import.meta.url,
);
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
Object.assign(globalThis, {
  document: dom.window.document,
  window: dom.window,
  HTMLElement: dom.window.HTMLElement,
  MutationObserver: dom.window.MutationObserver,
  Node: dom.window.Node,
  Element: dom.window.Element,
  CustomEvent: dom.window.CustomEvent,
  getComputedStyle: dom.window.getComputedStyle,
  IS_REACT_ACT_ENVIRONMENT: true,
});
const { createElement: h } = await import("react");
const { render, cleanup, waitFor, fireEvent } = await import(
  "@testing-library/react"
);
const { QueryClient, QueryClientProvider } = await import(
  "@tanstack/react-query"
);
const { EmployeeDirectoryProvider, useEmployeeDirectoryRefresh } = await import(
  "./EmployeeDirectoryProvider.tsx"
);
const { EmployeeIdentityBadge } = await import("./EmployeeIdentityBadge.tsx");
const { MessageAgentOwner } = await import(
  "../../messages/ui/MessageAgentOwner.tsx"
);
const { UserProfilePopover } = await import(
  "../../profile/ui/UserProfilePopover.tsx"
);
const originalFetch = globalThis.fetch;
const key = "ab".repeat(32);
const employee = {
  employee_id: "ada",
  name: "Ada",
  title: "Planner",
  status: "active",
  office_public_keys: [key],
};
const clients = [];
afterEach(() => {
  cleanup();
  for (const client of clients.splice(0)) client.clear();
  globalThis.fetch = originalFetch;
});
after(() => dom.window.close());
function Refresh() {
  const refresh = useEmployeeDirectoryRefresh();
  return h("button", { onClick: refresh }, "Refresh identities");
}
function setup() {
  const state = {
    signed: [],
    opened: 0,
    relay: "http://localhost:3038",
    denied: false,
    employees: [employee],
    runs: [],
  };
  globalThis.__identityFixture = state;
  globalThis.fetch = async (url) =>
    state.denied
      ? Response.json({}, { status: 403 })
      : Response.json(
          String(url).includes("/employees")
            ? { employees: state.employees, has_more: false, next_after: null }
            : { runs: state.runs, has_more: false, next_cursor: null },
        );
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  clients.push(client);
  const tree = (generation = "first") =>
    h(
      QueryClientProvider,
      { client },
      h(EmployeeDirectoryProvider, { key: generation }, [
        h(MessageAgentOwner, {
          key: "owner",
          pubkey: key,
          ownerLabel: "Fake inherited owner",
          ownerPubkey: "cd".repeat(32),
        }),
        h(EmployeeIdentityBadge, {
          key: "status",
          pubkey: key,
          showState: true,
        }),
        h(Refresh, { key: "refresh" }),
        h(
          UserProfilePopover,
          { key: "profile", pubkey: key, triggerAriaLabel: "Open Ada" },
          "Ada profile",
        ),
      ]),
    );
  return { state, client, tree };
}

test("Employee profile hover and pointer/keyboard actions use the real identity branch", async () => {
  const { state, tree } = setup();
  const view = render(tree());
  await waitFor(() => assert.ok(view.queryByText("Active")));
  const trigger = view.getByRole("button", { name: "Open Ada" });
  fireEvent.mouseEnter(trigger);
  await waitFor(() => assert.ok(view.queryByTestId("ortak-employee-profile")), {
    timeout: 2000,
  });
  const profile = view.getByTestId("ortak-employee-profile");
  assert.match(profile.textContent, /Ada/);
  assert.match(profile.textContent, /Planner/);
  assert.doesNotMatch(profile.textContent, /owner unavailable|offline|online/i);
  fireEvent.click(
    view.getByRole("button", { name: "View employee and activity" }),
  );
  assert.equal(state.opened, 1);
  fireEvent.click(trigger);
  fireEvent.keyDown(trigger, { key: "Enter" });
  fireEvent.keyDown(trigger, { key: " " });
  assert.equal(state.opened, 4);
});
