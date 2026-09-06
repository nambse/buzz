import assert from "node:assert/strict";
import { register } from "node:module";
import { after, afterEach, test } from "node:test";
import { JSDOM } from "jsdom";
import { selectedDmScreen } from "./selection.ts";

const channel = "11111111-1111-4111-8111-111111111111";
const other = "22222222-2222-4222-8222-222222222222";
const relay = "ws://localhost:3038";
const raw = JSON.stringify({ "http://localhost:3038": [channel] });

test("operator selection is exact and malformed configuration cannot downgrade", () => {
  assert.equal(selectedDmScreen(raw, relay, channel), "encrypted");
  assert.equal(selectedDmScreen(raw, relay, other), "ordinary");
  assert.equal(
    selectedDmScreen(raw, "ws://localhost:9999", channel),
    "ordinary",
  );
  for (const value of [
    "",
    "[]",
    "null",
    "{",
    JSON.stringify({ "http://localhost:3038": [channel, channel] }),
    JSON.stringify({ "http://localhost:3038/path": [channel] }),
  ])
    assert.equal(selectedDmScreen(value, relay, channel), "unavailable");
  assert.equal(
    selectedDmScreen(raw, `${relay}/unbound`, channel),
    "unavailable",
  );
});

register(
  `data:text/javascript,${encodeURIComponent(`
  export async function load(url, context, nextLoad) {
    if (url.endsWith('/shared/api/hooks.ts')) return {format:'module',shortCircuit:true,source:
      "export const useIdentityQuery=()=>({data:{pubkey:'a'.repeat(64)}});"};
    if (url.endsWith('/shared/api/tauri.ts')) return {format:'module',shortCircuit:true,source:
      "export const getRelayWsUrl=()=>globalThis.__selectedDmRelay();"};
    if (url.endsWith('/confidentialDm/ConfidentialDm.tsx')) return {format:'module',shortCircuit:true,source:
      "import {createElement} from 'react'; export const ConfidentialDm=()=>createElement('p',null,'protected screen');"};
    return nextLoad(url,context);
  }
`)}`,
  import.meta.url,
);
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
for (const key of ["document", "HTMLElement", "Element", "Node", "Event"])
  Object.defineProperty(globalThis, key, {
    value: dom.window[key],
    configurable: true,
  });
globalThis.window = dom.window;
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());

test("actual screen boundary never mounts ordinary hooks while loading, encrypted or unavailable", async () => {
  const React = await import("react");
  const { render, act, waitFor } = await import("@testing-library/react");
  const { SelectedDmScreen } = await import("./SelectedDmScreen.tsx");
  let release;
  globalThis.__selectedDmRelay = () =>
    new Promise((resolve) => {
      release = resolve;
    });
  let ordinaryMounts = 0;
  function Ordinary() {
    ordinaryMounts++;
    return React.createElement("p", null, "ordinary screen");
  }
  const component = (selection, id = channel) =>
    React.createElement(
      SelectedDmScreen,
      { selection, channelId: id },
      React.createElement(Ordinary),
    );
  const view = render(component(raw));
  assert.equal(ordinaryMounts, 0);
  await act(async () => release(relay));
  await waitFor(() => assert.ok(view.getByText("protected screen")));
  assert.equal(ordinaryMounts, 0);
  view.rerender(component("{"));
  assert.ok(view.getByRole("alert"));
  assert.equal(ordinaryMounts, 0);
  view.rerender(component(raw, other));
  assert.ok(view.getByText("ordinary screen"));
  assert.equal(ordinaryMounts, 1);
});
