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
const noop = () => {};
const item = {
  id: "work",
  project_id: "project",
  version: 7,
  state: "ready",
  description: "Parent private context",
  title: "Parent definition",
};
const project = { status: "active", can_contribute: true };
const summary = (id) => ({
  id,
  project_id: "project",
  title: id,
  state: "proposed",
  version: 1,
});
const page = (id = "work", version = 7) => ({
  work_item_id: id,
  work_version: version,
  parent: summary("ancestor"),
  children: [summary("child")],
});

test("decomposition client signs exact scoped read without a parent-derived grant", async () => {
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
  await client.workDecomposition("work/+", new AbortController().signal);
  assert.equal(
    calls[0].url,
    "http://127.0.0.1:3010/api/v1/work-items/work%2F%2B/decomposition",
  );
  assert.equal(calls[0].init.method, "GET");
  assert.equal(calls[0].init.body, undefined);
  assert.deepEqual(
    signed[0].tags.find((tag) => tag[0] === "u"),
    ["u", calls[0].url],
  );
});

test("actual decomposition panel navigates visible links and submits an independent child definition", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, act, within } = await import(
    "@testing-library/react"
  );
  const { DecompositionPanel } = await import("./DecompositionPanel.tsx");
  const writes = [],
    selected = [];
  const props = {
    client: { workDecomposition: async () => page() },
    item,
    project,
    disabled: false,
    submit: (...args) => writes.push(args),
    selectItem: (id) => selected.push(id),
    revoke: noop,
  };
  const view = render(createElement(DecompositionPanel, props));
  await act(async () => {});
  fireEvent.click(view.getByRole("button", { name: /Parent: ancestor/ }));
  fireEvent.click(view.getByRole("button", { name: /Child: child/ }));
  assert.deepEqual(selected, ["ancestor", "child"]);
  fireEvent.click(view.getByText("Create child work"));
  const form = view.getByRole("form", { name: "Create child work" });
  const fields = within(form);
  assert.equal(fields.getByLabelText("Child title").value, "");
  assert.equal(fields.getByLabelText("Child description").value, "");
  fireEvent.change(fields.getByLabelText("Child title"), {
    target: { value: "Independent task" },
  });
  fireEvent.change(
    fields.getByLabelText("Child acceptance criteria (one per line)"),
    { target: { value: "New human acceptance\nSecond criterion" } },
  );
  fireEvent.submit(form);
  assert.deepEqual(writes[0], [
    "/api/v1/work-items/work/children",
    "Create child work",
    {
      expected_version: 7,
      child: {
        title: "Independent task",
        description: "",
        priority: "normal",
        criteria: ["New human acceptance", "Second criterion"],
        approvals: [{ gate: "review", required: true }],
      },
    },
  ]);
  assert.equal(JSON.stringify(writes).includes(item.description), false);
  view.rerender(
    createElement(DecompositionPanel, { ...props, disabled: true }),
  );
  fireEvent.submit(form);
  assert.equal(writes.length, 1);
  for (const override of [
    { project: { ...project, status: "archived" } },
    { project: { ...project, can_contribute: false } },
    { item: { ...item, state: "completed" } },
  ]) {
    view.rerender(createElement(DecompositionPanel, { ...props, ...override }));
    assert.equal(view.queryByRole("form"), null);
  }
});

test("child form enforces byte bounds before any mutation", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, act, within } = await import(
    "@testing-library/react"
  );
  const { DecompositionPanel } = await import("./DecompositionPanel.tsx");
  let writes = 0;
  const view = render(
    createElement(DecompositionPanel, {
      client: { workDecomposition: async () => page() },
      item,
      project,
      disabled: false,
      submit: () => writes++,
      revoke: noop,
      selectItem: noop,
    }),
  );
  await act(async () => {});
  fireEvent.click(view.getByText("Create child work"));
  const form = view.getByRole("form", { name: "Create child work" });
  fireEvent.change(within(form).getByLabelText("Child title"), {
    target: { value: "é".repeat(101) },
  });
  fireEvent.submit(form);
  assert.equal(writes, 0);
  assert.match(view.getByRole("alert").textContent, /200 bytes/);
});

test("decomposition hook drops prior scope and clears all links on current revocation", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useDecomposition } = await import("./useDecomposition.ts");
  let resolveOld,
    oldSignal,
    calls = 0,
    revoked = 0;
  const old = {
    workDecomposition: (_, signal) => {
      oldSignal = signal;
      return new Promise((resolve) => {
        resolveOld = resolve;
      });
    },
  };
  const client = {
    workDecomposition: async () => {
      if (++calls > 1) throw new OrtakApiError(403, "revoked");
      return page("next");
    },
  };
  const revoke = () => revoked++;
  const view = renderHook(
    ({ client, id }) => useDecomposition(client, id, 7, "project", revoke),
    { initialProps: { client: old, id: "work" } },
  );
  view.rerender({ client, id: "next" });
  await act(async () => {});
  assert.equal(oldSignal.aborted, true);
  await act(async () => resolveOld(page()));
  assert.equal(view.result.current.data.work_item_id, "next");
  await act(async () => context.mock.timers.tick(5000));
  assert.equal(view.result.current.data, null);
  assert.equal(revoked, 1);
  await act(async () => context.mock.timers.tick(300000));
  assert.equal(calls, 2);
});

test("foreign project or changed version is unavailable and retries are bounded with explicit recovery", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useDecomposition } = await import("./useDecomposition.ts");
  let calls = 0;
  const client = {
    workDecomposition: async () => {
      calls++;
      return {
        ...page(),
        children: [{ ...summary("foreign"), project_id: "other" }],
      };
    },
  };
  const view = renderHook(() =>
    useDecomposition(client, "work", 7, "project", noop),
  );
  await act(async () => {});
  assert.equal(view.result.current.data, null);
  for (const delay of [3000, 6000, 12000, 24000, 300000])
    await act(async () => context.mock.timers.tick(delay));
  assert.equal(calls, 5);
  await act(async () => view.result.current.refresh());
  assert.equal(calls, 6);
  assert.match(view.result.current.error, /could not be read/);
});

test("actual child form plus mutation hook preserves exact bytes after lost acknowledgment", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, act, within } = await import(
    "@testing-library/react"
  );
  const { DecompositionPanel } = await import("./DecompositionPanel.tsx");
  const { useWorkMutation } = await import("./useWorkMutation.ts");
  const calls = [];
  let refreshed = 0;
  const client = {
    workDecomposition: async () => page(),
    workMutation: async (path, body) => {
      calls.push({ path, body });
      if (calls.length === 1) throw new Error("lost acknowledgment");
    },
  };
  function Harness() {
    const mutation = useWorkMutation(client, () => refreshed++, noop);
    return createElement(
      "div",
      null,
      createElement(DecompositionPanel, {
        client,
        item,
        project,
        disabled: mutation.busy || !!mutation.pending,
        submit: mutation.submit,
        revoke: noop,
        selectItem: noop,
      }),
      mutation.pending
        ? createElement(
            "button",
            { onClick: mutation.retry },
            "Retry same operation",
          )
        : null,
    );
  }
  const view = render(createElement(Harness));
  await act(async () => {});
  fireEvent.click(view.getByText("Create child work"));
  const form = view.getByRole("form", { name: "Create child work" });
  fireEvent.change(within(form).getByLabelText("Child title"), {
    target: { value: "One exact child" },
  });
  await act(async () => fireEvent.submit(form));
  fireEvent.submit(form);
  assert.equal(calls.length, 1);
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Retry same operation" })),
  );
  assert.deepEqual(calls[0], calls[1]);
  assert.ok(JSON.parse(calls[0].body).operation_id);
  assert.equal(refreshed, 1);
});
