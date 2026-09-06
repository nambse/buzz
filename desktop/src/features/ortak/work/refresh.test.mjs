import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";
import { OrtakApiError } from "../client.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
before(() =>
  Object.assign(globalThis, {
    window: dom.window,
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    FormData: dom.window.FormData,
    IS_REACT_ACT_ENVIRONMENT: true,
  }),
);
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());

async function setup() {
  const { createElement } = await import("react");
  const testing = await import("@testing-library/react");
  const { WorkScreen } = await import("./WorkScreen.tsx");
  const state = {
    error: null,
    hold: false,
    memoryError: null,
    memoryHold: false,
    reads: 0,
    version: 1,
    writes: [],
  };
  const project = (id) => ({
    id,
    name: `Project ${id}`,
    slug: id,
    status: "active",
    role: "owner",
    version: 1,
    can_contribute: true,
    can_review: true,
    channel_id: "channel1",
    description: "Private project",
  });
  const item = (id) => ({
    id,
    project_id: "p1",
    title: `Work ${id}`,
    description: "Private work",
    source_message_id: "message1",
    state: "review",
    priority: "normal",
    version: state.version,
    criteria: [
      { id: "c1", text: "Verify receipt", status: "pending", position: 0 },
    ],
    approvals: [
      { id: "a1", gate: "release", status: "pending", required: true },
    ],
    assignments: [],
    history: [],
    history_omitted: false,
    history_truncated: false,
  });
  const page = {
    projects: [project("p1"), project("p2")],
    next_cursor: "next",
    can_create_projects: true,
    create_channels: [{ id: "channel1", name: "Planning" }],
  };
  const fact = {
    id: "fact1",
    project_id: "p1",
    employee_id: "ada",
    version: 1,
    status: "active",
    source_visible: true,
    content: "Private approved fact",
    approved_by: "human1",
    approved_at: "2026-09-01T00:00:00Z",
    expires_at: "2026-11-01T00:00:00Z",
  };
  const client = {
    projects: async () => {
      state.reads++;
      if (state.error) throw state.error;
      if (state.hold)
        await new Promise((resolve) => {
          state.finish = resolve;
        });
      return structuredClone(page);
    },
    project: async (id) => ({ project: project(id) }),
    workItems: async () => ({
      work_items: [item("w1"), item("w2")],
      next_cursor: "next",
    }),
    workItem: async (id) => ({ work_item: item(id) }),
    workExecutions: async () => ({ executions: [] }),
    workDependencies: async (id) => ({
      work_item_id: id,
      work_version: state.version,
      dependencies: [],
    }),
    workDecomposition: async (id) => ({
      work_item_id: id,
      work_version: state.version,
      parent: null,
      children: [],
    }),
    reviewedMemory: async () => {
      if (state.memoryError) throw state.memoryError;
      if (state.memoryHold)
        await new Promise((resolve) => {
          state.finishMemory = resolve;
        });
      return { facts: [fact], next_after: null };
    },
    workMutation: async (path, body) => {
      state.writes.push({ path, body: JSON.parse(body) });
      if (state.mutationError) throw state.mutationError;
      state.version++;
      return {};
    },
  };
  const props = {
    client,
    employees: [{ employee_id: "ada", name: "Ada", status: "active" }],
  };
  const view = testing.render(createElement(WorkScreen, props));
  const { act, fireEvent } = testing;
  await act(async () => {});
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: /Project p1/ })),
  );
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: /Work w1/ })),
  );
  await act(async () =>
    fireEvent.change(view.getByLabelText("Memory audience: employee"), {
      target: { value: "ada" },
    }),
  );
  const drafts = new Map([
    ["Project name", "Unsaved project"],
    ["Decision for release", "approve"],
    ["Reason for release (optional)", "Unsaved review reason"],
    ["Memory audience: employee", "ada"],
    ["Edited fact", "Unsaved reviewed fact"],
  ]);
  const nodes = new Map();
  for (const [label, value] of drafts) {
    const node = view.getByLabelText(label);
    fireEvent.change(node, { target: { value } });
    nodes.set(label, node);
  }
  function unchanged() {
    for (const [label, value] of drafts) {
      assert.equal(
        view.getByLabelText(label),
        nodes.get(label),
        `${label} must stay mounted`,
      );
      assert.equal(
        view.getByLabelText(label).value,
        value,
        `${label} draft must survive`,
      );
    }
  }
  async function attemptStaleWrite() {
    const count = state.writes.length;
    assert.equal(
      view.getByRole("button", { name: "Save approval" }).matches(":disabled"),
      true,
    );
    // Direct form submission bypasses the native disabled button. The actual
    // WorkScreen callback must still refuse the write, including keyboard paths.
    await act(async () =>
      fireEvent.submit(
        view.getByRole("form", { name: "Resolve release approval" }),
      ),
    );
    assert.equal(state.writes.length, count);
  }
  return {
    ...testing,
    ...view,
    state,
    props,
    createElement,
    WorkScreen,
    nodes,
    unchanged,
    attemptStaleWrite,
  };
}

test("successful mutation keeps unrelated drafts mounted while its authority refresh is held", async () => {
  const v = await setup();
  v.state.hold = true;
  await v.act(async () =>
    v.fireEvent.click(
      v.getByRole("button", { name: "Accept criterion: Verify receipt" }),
    ),
  );
  assert.equal(v.state.writes.length, 1);
  v.unchanged();
  await v.attemptStaleWrite();
  v.state.hold = false;
  await v.act(async () => v.state.finish());
  v.unchanged();
  await v.act(async () =>
    v.fireEvent.submit(
      v.getByRole("form", { name: "Resolve release approval" }),
    ),
  );
  assert.equal(v.state.writes.length, 2);
  assert.equal(v.state.writes[1].body.expected_version, 2);
  assert.equal(v.state.writes[1].body.decision, "approve");
});

test("idle polling preserves drafts through bounded 503 failure and manual recovery without stale writes", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const v = await setup();
  await v.act(async () => context.mock.timers.tick(5000));
  v.unchanged();
  v.state.error = new OrtakApiError(503, "Temporarily unavailable");
  for (const delay of [5000, 3000, 6000, 12000, 24000]) {
    await v.act(async () => context.mock.timers.tick(delay));
    v.unchanged();
    await v.attemptStaleWrite();
  }
  const stopped = v.state.reads;
  await v.act(async () => context.mock.timers.tick(300000));
  assert.equal(v.state.reads, stopped);
  v.state.error = null;
  await v.act(async () =>
    v.fireEvent.click(v.getByRole("button", { name: "Refresh work" })),
  );
  v.unchanged();
  assert.equal(
    v.getByRole("button", { name: "Save approval" }).matches(":disabled"),
    false,
  );
});

test("routine polling does not disable a focused native selection while the next read is pending", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const v = await setup();
  const decision = v.getByLabelText("Decision for release");
  decision.focus();
  v.state.hold = true;
  v.state.memoryHold = true;
  await v.act(async () => context.mock.timers.tick(5000));
  v.unchanged();
  assert.equal(decision.matches(":disabled"), false);
  assert.equal(v.getByLabelText("Edited fact").matches(":disabled"), false);
  assert.equal(document.activeElement, decision);
  v.state.hold = false;
  v.state.memoryHold = false;
  await v.act(async () => {
    v.state.finish();
    v.state.finishMemory();
  });
  v.unchanged();
  assert.equal(document.activeElement, decision);
});

test("memory refresh and transient error preserve its draft but pause fact changes independently", async () => {
  const v = await setup();
  v.state.memoryHold = true;
  await v.act(async () =>
    v.fireEvent.click(
      v.getByRole("button", { name: "Refresh reviewed memory" }),
    ),
  );
  v.unchanged();
  assert.equal(
    v.getByRole("button", { name: "Approve fact" }).matches(":disabled"),
    true,
  );
  const stop = v.getByRole("button", { name: "Stop using fact fact1" });
  assert.equal(stop.matches(":disabled"), true);
  await v.act(async () => v.fireEvent.click(stop));
  assert.equal(v.state.writes.length, 0);
  v.state.memoryHold = false;
  await v.act(async () => v.state.finishMemory());
  v.state.memoryError = new OrtakApiError(503, "Temporarily unavailable");
  await v.act(async () =>
    v.fireEvent.click(
      v.getByRole("button", { name: "Refresh reviewed memory" }),
    ),
  );
  v.unchanged();
  assert.equal(stop.matches(":disabled"), true);
  v.state.memoryError = null;
  await v.act(async () =>
    v.fireEvent.click(
      v.getByRole("button", { name: "Refresh reviewed memory" }),
    ),
  );
  v.unchanged();
  assert.equal(stop.matches(":disabled"), false);
});

test("uncertain write retry waits for fresh authority and then reuses the original operation", async () => {
  const v = await setup();
  v.state.mutationError = new OrtakApiError(503, "Acknowledgment missing");
  await v.act(async () =>
    v.fireEvent.submit(
      v.getByRole("form", { name: "Resolve release approval" }),
    ),
  );
  assert.equal(v.state.writes.length, 1);
  v.state.hold = true;
  await v.act(async () =>
    v.fireEvent.click(v.getByRole("button", { name: "Refresh work" })),
  );
  const retry = v.getByRole("button", { name: "Retry same operation" });
  assert.equal(retry.disabled, true);
  await v.act(async () => v.fireEvent.click(retry));
  await v.attemptStaleWrite();
  v.unchanged();
  v.state.hold = false;
  v.state.mutationError = null;
  await v.act(async () => v.state.finish());
  assert.equal(retry.disabled, false);
  await v.act(async () => v.fireEvent.click(retry));
  assert.equal(v.state.writes.length, 2);
  assert.deepEqual(v.state.writes[1], v.state.writes[0]);
});

for (const status of [401, 403, 404]) {
  test(`Work ${status} clears private data and drafts before explicit recovery`, async () => {
    const v = await setup();
    v.state.error = new OrtakApiError(status, "Access lost");
    await v.act(async () =>
      v.fireEvent.click(v.getByRole("button", { name: "Refresh work" })),
    );
    assert.equal(v.queryByText("Private approved fact"), null);
    assert.equal(v.queryByRole("region", { name: "Project detail" }), null);
    for (const node of v.nodes.values()) assert.equal(node.isConnected, false);
    v.state.error = null;
    await v.act(async () =>
      v.fireEvent.click(v.getByRole("button", { name: "Refresh work" })),
    );
    assert.equal(v.getByLabelText("Project name").value, "");
    assert.equal(v.queryByLabelText("Memory audience: employee"), null);
  });
}

for (const scope of [
  "client",
  "project",
  "item",
  "project cursor",
  "item cursor",
]) {
  test(`${scope} change clears mounted drafts immediately, including a held next read`, async () => {
    const v = await setup();
    v.state.hold = true;
    await v.act(async () => {
      if (scope === "client") {
        v.rerender(
          v.createElement(v.WorkScreen, {
            ...v.props,
            client: { ...v.props.client },
          }),
        );
      } else {
        const name = {
          project: /Project p2/,
          item: /Work w2/,
          "project cursor": "More projects",
          "item cursor": "More work items",
        }[scope];
        v.fireEvent.click(v.getByRole("button", { name }));
      }
    });
    assert.equal(v.queryByLabelText("Memory audience: employee"), null);
    assert.equal(v.queryByText("Private approved fact"), null);
    for (const node of v.nodes.values()) assert.equal(node.isConnected, false);
    v.state.hold = false;
    await v.act(async () => v.state.finish());
    assert.equal(v.getByLabelText("Project name").value, "");
    assert.equal(v.getByLabelText("Memory audience: employee").value, "");
    assert.equal(v.state.writes.length, 0);
  });
}
