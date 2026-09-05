import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { createHash } from "node:crypto";
import { JSDOM } from "jsdom";
import { createOrtakClient, OrtakApiError } from "../client.ts";
import { availableTransitions, workOperation } from "./operations.ts";
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
before(() =>
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    FormData: dom.window.FormData,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  }),
);
afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});
after(() => dom.window.close());
const project = (id = "p1") => ({
  id,
  name: `Project ${id}`,
  slug: id,
  status: "active",
  role: "owner",
  version: 1,
  can_contribute: true,
  can_review: true,
  channel_id: "channel1",
  description: "Private plan",
});
const page = {
  projects: [project(), project("p2")],
  next_cursor: null,
  can_create_projects: true,
  create_channels: [{ id: "channel1", name: "Planning" }],
};
const item = {
  id: "w1",
  project_id: "p1",
  title: "Ship the review",
  description: "Manual work",
  state: "review",
  priority: "normal",
  version: 1,
  criteria: [
    { id: "c1", text: "Verify the receipt", status: "pending", position: 0 },
  ],
  approvals: [],
  assignments: [],
  history: [],
  history_omitted: false,
  history_truncated: false,
  execution_available: false,
};
const clientStub = () => ({
  projects: async () => page,
  project: async (id) => ({ project: project(id) }),
  workItems: async () => ({ work_items: [item], next_cursor: null }),
  workItem: async () => ({ work_item: item }),
});

test("uncertain production write retries identical bytes and operation ID with fresh authentication", async () => {
  const { renderHook, act, waitFor } = await import("@testing-library/react");
  const { useWorkMutation } = await import("./useWorkMutation.ts");
  const sent = [];
  const signed = [];
  let refreshes = 0;
  const client = createOrtakClient(
    "https://api.example.test",
    async (event) => {
      signed.push(event);
      return event;
    },
    async (_url, init) => {
      sent.push(init.body);
      return sent.length === 1
        ? new Response("", { status: 503 })
        : Response.json({ work_item: item });
    },
  );
  const view = renderHook(() =>
    useWorkMutation(
      client,
      () => refreshes++,
      () => assert.fail("not revoked"),
    ),
  );
  await act(async () =>
    view.result.current.submit(
      "/api/v1/work-items/w1/transitions",
      "Manual status",
      { expected_version: 1, target: "completed" },
    ),
  );
  await waitFor(() => assert.equal(view.result.current.busy, false));
  assert.equal(sent.length, 1);
  assert.ok(view.result.current.pending);
  await act(async () =>
    view.result.current.submit("/api/v1/projects", "Another write", {}),
  );
  assert.equal(sent.length, 1);
  await act(async () => view.result.current.retry());
  await waitFor(() => assert.equal(view.result.current.pending, null));
  assert.equal(sent.length, 2);
  assert.equal(sent[0], sent[1]);
  assert.ok(JSON.parse(sent[0]).operation_id);
  for (let i = 0; i < 2; i++)
    assert.equal(
      Object.fromEntries(signed[i].tags).payload,
      createHash("sha256").update(sent[i]).digest("hex"),
    );
  assert.notEqual(
    Object.fromEntries(signed[0].tags).nonce,
    Object.fromEntries(signed[1].tags).nonce,
  );
  assert.equal(refreshes, 1);
});
test("409 clears the stale attempt and refreshes without an automatic mutating retry", async () => {
  const { renderHook, act } = await import("@testing-library/react");
  const { useWorkMutation } = await import("./useWorkMutation.ts");
  let calls = 0;
  let refreshes = 0;
  const client = {
    workMutation: async () => {
      calls++;
      throw new OrtakApiError(409, "conflict");
    },
  };
  const view = renderHook(() =>
    useWorkMutation(
      client,
      () => refreshes++,
      () => {},
    ),
  );
  await act(async () =>
    view.result.current.submit("/api/v1/work-items/w1/transitions", "Status", {
      expected_version: 1,
      target: "completed",
    }),
  );
  assert.equal(calls, 1);
  assert.equal(refreshes, 1);
  assert.equal(view.result.current.pending, null);
  assert.match(view.result.current.notice, /refreshed item/);
});
test("real Work reads abort old selection, reject late results, and clear on polling revocation", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useWorkData } = await import("./useWorkData.ts");
  let finishOld;
  let oldSignal;
  let deny = false;
  let calls = 0;
  const client = {
    ...clientStub(),
    project: async (id, signal) => {
      calls++;
      if (deny) throw new OrtakApiError(403, "forbidden");
      if (id === "p1") {
        oldSignal = signal;
        return await new Promise((resolve) => {
          finishOld = resolve;
        });
      }
      return { project: project(id) };
    },
  };
  const view = renderHook(
    ({ id }) => useWorkData(client, id, null, undefined, undefined, 0, false),
    { initialProps: { id: "p1" } },
  );
  view.rerender({ id: "p2" });
  await act(async () => {});
  assert.equal(oldSignal.aborted, true);
  assert.equal(view.result.current.data.project.id, "p2");
  await act(async () => finishOld({ project: project("p1") }));
  assert.equal(view.result.current.data.project.id, "p2");
  deny = true;
  await act(async () => context.mock.timers.tick(5000));
  assert.equal(view.result.current.data, null);
  assert.equal(view.result.current.revoked, true);
  const stopped = calls;
  await act(async () => context.mock.timers.tick(300000));
  assert.equal(calls, stopped);
});
test("real Work screen submits named channel and reviewer action with current version", async () => {
  const { createElement } = await import("react");
  const { render, act, fireEvent } = await import("@testing-library/react");
  const { WorkScreen } = await import("./WorkScreen.tsx");
  const writes = [];
  const client = {
    ...clientStub(),
    workMutation: async (path, body) => {
      writes.push({ path, body: JSON.parse(body) });
      return {};
    },
  };
  const view = render(createElement(WorkScreen, { client, employees: [] }));
  await act(async () => {});
  assert.equal(
    view.getByLabelText("Office channel").options[1].text,
    "Planning",
  );
  fireEvent.change(view.getByLabelText("Project name"), {
    target: { value: "Release" },
  });
  fireEvent.change(view.getByLabelText("Project slug"), {
    target: { value: "release" },
  });
  fireEvent.change(view.getByLabelText("Office channel"), {
    target: { value: "channel1" },
  });
  await act(async () =>
    fireEvent.submit(view.getByLabelText("Project name").closest("form")),
  );
  assert.equal(writes[0].body.channel_id, "channel1");
  assert.equal(writes[0].body.project.name, "Release");
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: /Project p1/ })),
  );
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: /Ship the review/ })),
  );
  assert.ok(
    view.getByText(/This does not start or confirm employee execution/),
  );
  await act(async () =>
    fireEvent.click(
      view.getByRole("button", {
        name: "Accept criterion: Verify the receipt",
      }),
    ),
  );
  assert.equal(writes[1].path, "/api/v1/work-items/w1/criteria/c1/satisfy");
  assert.equal(writes[1].body.expected_version, 1);
});
test("closed transition controls separate contribution from review and cap request bytes", () => {
  assert.deepEqual(
    availableTransitions("review", { ...project(), can_review: false }),
    ["cancelled"],
  );
  assert.deepEqual(
    availableTransitions("review", { ...project(), can_contribute: false }),
    ["completed", "in_progress"],
  );
  assert.deepEqual(availableTransitions("completed", project()), []);
  assert.deepEqual(
    availableTransitions("ready", {
      ...project(),
      can_contribute: false,
      can_review: false,
    }),
    [],
  );
  assert.throws(
    () =>
      workOperation("/api/v1/projects", "Project", {
        description: "x".repeat(17000),
      }),
    /too long/,
  );
});

test("recovered mutation lifetime aborts on unmount and late results cannot refresh old scope", async () => {
  const { renderHook, act } = await import("@testing-library/react");
  const { useWorkMutation } = await import("./useWorkMutation.ts");
  let signal;
  let finish;
  let refreshed = 0;
  const client = {
    workMutation: async (_path, _body, nextSignal) => {
      signal = nextSignal;
      return await new Promise((resolve) => {
        finish = resolve;
      });
    },
  };
  const view = renderHook(() =>
    useWorkMutation(
      client,
      () => refreshed++,
      () => {},
    ),
  );
  await act(async () => view.result.current.pause());
  await act(async () =>
    view.result.current.submit("/api/v1/projects", "Project", {}),
  );
  assert.equal(signal.aborted, false);
  view.unmount();
  assert.equal(
    signal.aborted,
    true,
    "cleanup must abort the controller created during recovery",
  );
  await act(async () => finish({}));
  assert.equal(refreshed, 0);
});
