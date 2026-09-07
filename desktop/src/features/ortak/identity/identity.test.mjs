import assert from "node:assert/strict";
import { register } from "node:module";
import test, { after, afterEach } from "node:test";
import { JSDOM } from "jsdom";

// Keep production provider, signed client, directory, Badge and message-owner
// branch; replace only native I/O and the unrelated legacy profile child.
register(
  `data:text/javascript,${encodeURIComponent(`
export async function load(url,context,nextLoad) {
 if(url.endsWith('/shared/api/tauri.ts')) return {format:'module',shortCircuit:true,source:
  "export const signRelayEvent = async (event) => { globalThis.__identityFixture.signed.push(event); return {...event,id:'fixture-signature'}; }; export const getRelayHttpUrl=async()=>globalThis.__identityFixture.relay;"};
 if(url.endsWith('/features/profile/ui/UserProfilePopover.tsx')) return {format:'module',shortCircuit:true,source:'export const UserProfilePopover=({children})=>children;'};
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
  IS_REACT_ACT_ENVIRONMENT: true,
});
const { createElement: h } = await import("react");
const { render, cleanup, act, waitFor } = await import(
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
      ]),
    );
  return { state, client, tree };
}

test("real message owner branch shows Employee and recorded status without inventing an owner or online state", async () => {
  const { state, tree, client } = setup();
  const view = render(tree());
  await waitFor(() => assert.ok(view.queryByText("Employee")));
  assert.ok(view.queryByText("Active"));
  assert.doesNotMatch(view.container.textContent, /owner|online|offline/i);
  assert.equal(state.signed.length, 2);
  state.runs = [{ employee_id: "ada", status: "running" }];
  await act(() =>
    client.invalidateQueries({ queryKey: ["ortak-employee-identities"] }),
  );
  await waitFor(() => assert.ok(view.queryByText("Working")));
  assert.equal(view.getAllByTestId("ortak-employee-identity").length, 2);
});

test("forbidden refresh clears identity and the visible refresh action recovers it", async () => {
  const { state, tree, client } = setup();
  const view = render(tree());
  await waitFor(() => assert.ok(view.queryByText("Employee")));
  state.denied = true;
  await act(() =>
    client.invalidateQueries({ queryKey: ["ortak-employee-identities"] }),
  );
  await waitFor(() =>
    assert.equal(Boolean(view.queryByText("Employee")), false),
  );
  assert.doesNotMatch(
    view.container.textContent,
    /owner unavailable|Active|Working/,
  );
  const count = state.signed.length;
  await act(async () => {});
  assert.equal(state.signed.length, count);
  state.denied = false;
  await act(async () => view.getByText("Refresh identities").click());
  await waitFor(() => assert.ok(view.queryByText("Employee")));
  state.employees = [];
  await act(() =>
    client.invalidateQueries({ queryKey: ["ortak-employee-identities"] }),
  );
  await waitFor(() =>
    assert.equal(Boolean(view.queryByText("Employee")), false),
  );
});

test("a community remount does not retain employee labels from the prior signed origin", async () => {
  const { state, tree } = setup();
  const view = render(tree());
  await waitFor(() => assert.ok(view.queryByText("Employee")));
  state.relay = "http://localhost:3039";
  state.employees = [];
  view.rerender(tree("second"));
  assert.equal(Boolean(view.queryByText("Employee")), false);
  await waitFor(() =>
    assert.ok(
      state.signed.some((event) =>
        event.tags.some(
          (t) => t[0] === "u" && t[1].startsWith("http://127.0.0.1:8788/"),
        ),
      ),
    ),
  );
  assert.equal(Boolean(view.queryByText("Employee")), false);
});
