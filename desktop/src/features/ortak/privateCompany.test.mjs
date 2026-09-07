import assert from "node:assert/strict";
import { register } from "node:module";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";

// Only substitute compiled Vite inputs; exercise the real React providers,
// persistence helpers and mutation callbacks, without native or network I/O.
register(
  `data:text/javascript,${encodeURIComponent(`
export async function load(url, context, nextLoad) {
  const result = await nextLoad(url, context);
  if (url.endsWith('/features/ortak/privateMode.ts')) return {
    ...result, source: String(result.source).replace('import.meta.env?.VITE_ORTAK_PRIVATE_MODE', '"true"'),
  };
  if (url.endsWith('/features/ortak/privateCompany.ts')) return {
    ...result, source: String(result.source).replace('import.meta.env?.VITE_ORTAK_API_BINDINGS_JSON', JSON.stringify('{"http://localhost:3038":"http://127.0.0.1:8787"}')),
  };
  return result;
}`)}`,
  import.meta.url,
);

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
before(() =>
  Object.assign(globalThis, {
    window: dom.window,
    document: dom.window.document,
    localStorage: dom.window.localStorage,
    HTMLElement: dom.window.HTMLElement,
    Element: dom.window.Element,
    IS_REACT_ACT_ENVIRONMENT: true,
  }),
);
afterEach(async () => {
  (await import("@testing-library/react")).cleanup();
  dom.window.localStorage.clear();
});
after(() => dom.window.close());

const company = {
  id: "selected",
  name: "Example company",
  relayUrl: "ws://localhost:3038",
  pubkey: "a".repeat(64),
  addedAt: "2026-09-06",
};
const other = {
  ...company,
  id: "other",
  name: "Retained other",
  relayUrl: "wss://other.example",
};

test("private company provider refuses add/switch/onboarding while reconnect and identity metadata remain usable", async () => {
  localStorage.setItem("buzz-communities", JSON.stringify([other, company]));
  localStorage.setItem("buzz-active-community-id", other.id);
  const pending = '{"id":"retained-onboarding"}';
  localStorage.setItem("buzz-community-onboarding-transaction.v1", pending);
  const React = await import("react");
  const { render, act } = await import("@testing-library/react");
  const { CommunitiesProvider, useCommunities } = await import(
    "@/features/communities/useCommunities.tsx"
  );
  const { CommunityOnboardingProvider, useCommunityOnboarding } = await import(
    "@/features/onboarding/communityOnboarding.tsx"
  );
  let state;
  let onboarding;
  function Probe() {
    state = useCommunities();
    onboarding = useCommunityOnboarding();
    return null;
  }
  render(
    React.createElement(
      CommunitiesProvider,
      null,
      React.createElement(
        CommunityOnboardingProvider,
        { enabled: true },
        React.createElement(Probe),
      ),
    ),
  );
  assert.deepEqual(state.communities, [company]);
  assert.equal(state.activeCommunity.id, company.id);
  const before = localStorage.getItem("buzz-communities");
  for (const mutation of [
    () => state.addCommunity({ ...other, id: "new" }),
    () => state.switchCommunity(other.id),
    () => state.removeCommunity(company.id),
    () => state.clearCommunities(),
    () => state.reorderCommunities([company.id, other.id]),
    () => state.updateCommunity(company.id, { relayUrl: other.relayUrl }),
    () => state.updateCommunity(company.id, { token: "replacement" }),
  ])
    assert.throws(mutation, /one company/);
  assert.equal(localStorage.getItem("buzz-communities"), before);
  assert.equal(onboarding.transaction, null);
  assert.equal(
    onboarding.start({ relayUrl: other.relayUrl, source: "deep-link-join" }),
    false,
  );
  assert.equal(
    localStorage.getItem("buzz-community-onboarding-transaction.v1"),
    pending,
  );
  await act(async () => state.switchCommunity(company.id));
  await act(async () => state.reconnectCommunity());
  assert.equal(state.reinitKey, 1);
  await act(async () =>
    state.updateCommunity(company.id, { pubkey: "b".repeat(64) }),
  );
  assert.equal(state.activeCommunity.pubkey, "b".repeat(64));
  assert.deepEqual(
    JSON.parse(localStorage.getItem("buzz-communities"))[0],
    other,
  );
});

test("fixed first-company bootstrap preserves older saved entries", async () => {
  localStorage.setItem("buzz-communities", JSON.stringify([other]));
  const { initFirstCommunity } = await import(
    "@/features/communities/communityStorage.ts"
  );
  const created = initFirstCommunity(
    company.relayUrl,
    company.pubkey,
    company.name,
  );
  assert(created);
  assert.equal(localStorage.getItem("buzz-active-community-id"), created.id);
  assert.deepEqual(JSON.parse(localStorage.getItem("buzz-communities")), [
    other,
    created,
  ]);
});

test("private membership recovery keeps retry and key import without community or invite actions", async () => {
  const React = await import("react");
  const { render, fireEvent, act } = await import("@testing-library/react");
  const { nsecEncode } = await import("nostr-tools/nip19");
  const { CommunityOnboardingProvider } = await import(
    "@/features/onboarding/communityOnboarding.tsx"
  );
  const { MembershipDenied } = await import(
    "@/features/onboarding/ui/MembershipDenied.tsx"
  );
  let retries = 0;
  let imported = null;
  const view = render(
    React.createElement(
      CommunityOnboardingProvider,
      null,
      React.createElement(MembershipDenied, {
        activeRelayUrl: company.relayUrl,
        pubkey: company.pubkey,
        onBack: () => {},
        onChangeCommunity: () => assert.fail("private community action"),
        onRetry: () => {
          retries += 1;
        },
        onImportKey: async (value) => {
          imported = value;
        },
      }),
    ),
  );
  assert.equal(view.queryByRole("button", { name: "Change community" }), null);
  assert.equal(view.queryByRole("button", { name: "Have an invite?" }), null);
  fireEvent.click(view.getByRole("button", { name: "Try again" }));
  assert.equal(retries, 1);
  fireEvent.click(view.getByRole("button", { name: "Use a different key" }));
  const synthetic = nsecEncode(new Uint8Array(32).fill(7));
  fireEvent.change(view.getByLabelText("Private key"), {
    target: { value: synthetic },
  });
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Import key" })),
  );
  assert.equal(imported, synthetic);
});
