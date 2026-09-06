import assert from "node:assert/strict";
import { register } from "node:module";
import { after, test } from "node:test";
import { JSDOM } from "jsdom";

// Only the build flag is replaced; render the actual onboarding and native
// identity API to prove that continuing does not import, persist or reveal a key.
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
window.matchMedia = () => ({
  matches: true,
  addListener() {},
  removeListener() {},
  addEventListener() {},
  removeEventListener() {},
});
after(() => dom.window.close());

test("private onboarding continues with the configured identity without key mutation or export", async () => {
  const { createElement } = await import("react");
  const { render, act, fireEvent, cleanup } = await import(
    "@testing-library/react"
  );
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { MachineOnboardingFlow } = await import("./MachineOnboardingFlow.tsx");
  const pubkey = "11".repeat(32);
  const calls = [];
  window.__TAURI_INTERNALS__ = {
    invoke: async (command) => {
      calls.push(command);
      if (command === "get_relay_http_url") return "http://localhost:3038";
      if (command === "get_media_proxy_port") return 0;
      assert.equal(command, "get_identity");
      return { pubkey, display_name: "Private owner", storage: "environment" };
    },
  };
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  queryClient.setQueryData(["identity"], {
    pubkey,
    displayName: "Private owner",
    storage: "environment",
  });
  const completed = [];
  const view = render(
    createElement(
      QueryClientProvider,
      { client: queryClient },
      createElement(MachineOnboardingFlow, {
        queryClient,
        complete: (key) => completed.push(key),
        continueWithIdentity: () => assert.fail("must not import"),
        continueWithRecoveredIdentity: () => assert.fail("must not recover"),
        identityLost: false,
      }),
    ),
  );
  try {
    assert.ok(view.getByRole("heading", { name: "Ortak" }));
    assert.equal(
      view.queryByRole("button", { name: "Create a new identity key" }),
      null,
    );
    await act(async () =>
      fireEvent.click(
        view.getByRole("button", { name: "Continue with configured identity" }),
      ),
    );
    assert.ok(
      view.getByRole("heading", { name: "Connect to your private Office" }),
    );
    assert.equal(
      view.queryByRole("button", { name: "Reveal private key" }),
      null,
    );
    await act(async () =>
      fireEvent.click(view.getByRole("button", { name: "Back" })),
    );
    assert.ok(
      view.getByRole("button", { name: "Continue with configured identity" }),
    );
    await act(async () =>
      fireEvent.click(
        view.getByRole("button", { name: "Continue with configured identity" }),
      ),
    );
    await act(async () =>
      fireEvent.click(view.getByRole("button", { name: "Continue to Office" })),
    );
    assert.deepEqual(completed, [pubkey]);
    assert.equal(
      calls.filter((command) => command === "get_identity").length,
      2,
    );
    assert.ok(
      calls.every((command) =>
        ["get_identity", "get_relay_http_url", "get_media_proxy_port"].includes(
          command,
        ),
      ),
    );
  } finally {
    cleanup();
    queryClient.clear();
  }
});
