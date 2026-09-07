import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";
import { createOrtakClient, OrtakApiError } from "../client.ts";
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
before(() =>
  Object.assign(globalThis, {
    document: dom.window.document,
    window: dom.window,
    HTMLElement: dom.window.HTMLElement,
    FormData: dom.window.FormData,
    IS_REACT_ACT_ENVIRONMENT: true,
  }),
);
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());
const page = (id = "work", version = 7) => ({
  work_item_id: id,
  work_version: version,
  dependencies: [{ id: "opaque-edge", target: null }],
});
const item = { id: "work", project_id: "project", version: 7, state: "ready" };
const project = { status: "active", can_contribute: true };
const noop = () => {};

test("dependency client signs the exact scoped GET and never sends a target grant", async () => {
  const signed = [],
    calls = [];
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async (event) => {
      signed.push(event);
      return {};
    },
    async (url, init) => {
      calls.push({ url, init });
      return Response.json(page());
    },
  );
  await client.workDependencies("work/+", new AbortController().signal);
  assert.equal(
    calls[0].url,
    "http://127.0.0.1:3010/api/v1/work-items/work%2F%2B/dependencies",
  );
  assert.equal(calls[0].init.method, "GET");
  assert.deepEqual(
    signed[0].tags.find((tag) => tag[0] === "u"),
    ["u", calls[0].url],
  );
  assert.equal(calls[0].init.body, undefined);
});

test("actual panel removes an opaque hidden target and offers only current project-page additions", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, within, act } = await import(
    "@testing-library/react"
  );
  const { DependencyPanel } = await import("./DependencyPanel.tsx");
  const writes = [];
  const client = { workDependencies: async () => page() };
  const props = {
    client,
    item,
    project,
    disabled: false,
    revoke: noop,
    submit: (...args) => writes.push(args),
    targets: [
      item,
      { id: "target", project_id: "project", title: "Current blocker" },
      { id: "foreign", project_id: "foreign-project", title: "Hidden foreign" },
    ],
  };
  const view = render(createElement(DependencyPanel, props));
  await act(async () => {});
  assert.match(
    view.getByText(/Target unavailable/).textContent,
    /can still be removed/,
  );
  const removal = view.getByRole("form", { name: "Remove dependency 1" });
  fireEvent.change(
    within(removal).getByLabelText("Reason to remove dependency 1"),
    { target: { value: "Not required" } },
  );
  fireEvent.submit(removal);
  assert.deepEqual(writes[0], [
    "/api/v1/work-items/work/dependencies/opaque-edge/remove",
    "Remove dependency",
    { expected_version: 7, reason: "Not required" },
  ]);
  const addition = view.getByRole("form", { name: "Add dependency" });
  const select = within(addition).getByLabelText(
    "Blocker from the current work list page",
  );
  assert.deepEqual(
    [...select.options].map((option) => option.value),
    ["", "target"],
  );
  fireEvent.change(select, { target: { value: "target" } });
  fireEvent.submit(addition);
  assert.deepEqual(writes[1], [
    "/api/v1/work-items/work/dependencies",
    "Add dependency",
    { expected_version: 7, depends_on: "target" },
  ]);
  view.rerender(createElement(DependencyPanel, { ...props, disabled: true }));
  fireEvent.submit(removal);
  assert.equal(writes.length, 2);
  view.rerender(
    createElement(DependencyPanel, {
      ...props,
      project: { ...project, status: "archived" },
    }),
  );
  assert.equal(view.queryByRole("form"), null);
});

test("dependency hook rejects old item/client results and stops on current authority revocation", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useDependencies } = await import("./useDependencies.ts");
  let resolveOld,
    oldSignal,
    revoked = 0,
    calls = 0;
  const oldClient = {
    workDependencies: async (_, signal) => {
      oldSignal = signal;
      return new Promise((resolve) => {
        resolveOld = resolve;
      });
    },
  };
  const client = {
    workDependencies: async () => {
      if (++calls > 1) throw new OrtakApiError(403, "revoked");
      return page("next");
    },
  };
  const revoke = () => revoked++;
  const view = renderHook(
    ({ client, id }) => useDependencies(client, id, 7, revoke),
    { initialProps: { client: oldClient, id: "work" } },
  );
  view.rerender({ client, id: "next" });
  await act(async () => {});
  assert.equal(oldSignal.aborted, true);
  assert.equal(view.result.current.data.work_item_id, "next");
  await act(async () => resolveOld(page()));
  assert.equal(view.result.current.data.work_item_id, "next");
  await act(async () => context.mock.timers.tick(5000));
  assert.equal(view.result.current.data, null);
  assert.equal(revoked, 1);
  await act(async () => context.mock.timers.tick(300000));
  assert.equal(calls, 2);
});

test("mismatched version is unavailable and dependency retries stop after five with explicit recovery", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useDependencies } = await import("./useDependencies.ts");
  let calls = 0;
  const client = {
    workDependencies: async () => {
      calls++;
      return page("work", 8);
    },
  };
  const view = renderHook(() => useDependencies(client, "work", 7, noop));
  await act(async () => {});
  assert.equal(view.result.current.data, null);
  for (const delay of [3000, 6000, 12000, 24000, 300000])
    await act(async () => context.mock.timers.tick(delay));
  assert.equal(calls, 5);
  await act(async () => view.result.current.refresh());
  assert.equal(calls, 6);
  assert.match(view.result.current.error, /could not be read/);
});
