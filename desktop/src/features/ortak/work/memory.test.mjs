import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";
import { createOrtakClient, OrtakApiError } from "../client.ts";
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
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());
const project = {
  id: "p1",
  name: "Project one",
  slug: "one",
  status: "active",
  role: "owner",
  version: 1,
  can_contribute: true,
  can_review: true,
  channel_id: "channel1",
  description: "Private plan",
};
const employee = {
  employee_id: "cem",
  name: "Cem",
  status: "active",
  role: "Developer",
  skills: [],
};
const fact = {
  id: "fact1",
  project_id: "p1",
  employee_id: "cem",
  source: { kind: "conversation", message_id: "message1" },
  source_visible: true,
  content: "Approved deployment fact",
  version: 1,
  status: "active",
  approved_by: "human1",
  approved_at: "2026-09-01T00:00:00Z",
  expires_at: "2026-11-01T00:00:00Z",
  revoked_by: null,
  revoked_at: null,
  revoke_reason: null,
};
const item = {
  id: "w1",
  project_id: "p1",
  source_message_id: "message1",
  title: "Ship the review",
  description: "Unreviewed raw output",
  state: "review",
  priority: "normal",
  version: 1,
  criteria: [],
  approvals: [],
  assignments: [],
  history: [],
  history_omitted: false,
  history_truncated: false,
  execution_available: false,
};
const emptyPage = { facts: [], next_after: null };
const exportJob = {
  state: "pending",
  retry_version: 0,
  attempt_count: 0,
  next_attempt_at: fact.expires_at,
  error_code: null,
};
async function setup(overrides = {}) {
  const { createElement } = await import("react");
  const testing = await import("@testing-library/react");
  const { ReviewedMemoryPanel } = await import("./ReviewedMemoryPanel.tsx");
  const props = {
    project,
    employees: [employee],
    item,
    executions: [],
    disabled: false,
    refresh: 0,
    submit: () => {},
    revoke: () => {},
    client: {
      reviewedMemory: async () => ({ facts: [fact], next_after: null }),
    },
    ...overrides,
  };
  const view = testing.render(createElement(ReviewedMemoryPanel, props));
  await testing.act(async () =>
    testing.fireEvent.change(view.getByLabelText("Memory audience: employee"), {
      target: { value: "cem" },
    }),
  );
  return { ...testing, ...view, props, createElement, ReviewedMemoryPanel };
}

test("reviewed-memory client signs exact scoped GET and recall POST without query text in the URL", async () => {
  const signed = [],
    fetched = [];
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async (input) => {
      signed.push(input);
      return {};
    },
    async (url, init) => {
      fetched.push({ url, init });
      return Response.json(emptyPage);
    },
  );
  const signal = new AbortController().signal;
  await client.reviewedMemory("project/a", "cem+b", signal, "next/+");
  await client.recallReviewedMemory(
    "project/a",
    "cem+b",
    "private search text",
    signal,
  );
  assert.equal(
    fetched[0].url,
    "http://127.0.0.1:3010/api/v1/projects/project%2Fa/reviewed-memory?employee_id=cem%2Bb&after=next%2F%2B",
  );
  assert.equal(fetched[0].init.method, "GET");
  assert.equal(
    fetched[1].url,
    "http://127.0.0.1:3010/api/v1/projects/project%2Fa/reviewed-memory/recall",
  );
  assert.equal(fetched[1].init.method, "POST");
  assert.deepEqual(JSON.parse(fetched[1].init.body), {
    employee_id: "cem+b",
    query: "private search text",
  });
  for (const [index, request] of fetched.entries()) {
    const tags = Object.fromEntries(signed[index].tags);
    assert.equal(tags.u, request.url);
    assert.equal(tags.method, request.init.method);
    if (index)
      assert.equal(
        tags.payload,
        createHash("sha256").update(request.init.body).digest("hex"),
      );
  }
});

test("archived project and hidden evidence retain Stop using for the paused employee", async () => {
  const writes = [];
  const view = await setup({
    project: { ...project, status: "archived" },
    employees: [{ ...employee, status: "paused" }],
    item: null,
    client: {
      reviewedMemory: async () => ({
        facts: [
          { ...fact, content: null, source: null, source_visible: false },
        ],
        next_after: null,
      }),
    },
    submit: (...args) => writes.push(args),
  });
  assert.equal(view.queryByText(fact.content), null);
  assert.ok(view.getByText(/Source evidence is no longer visible/));
  assert.equal(view.queryByRole("button", { name: "Approve fact" }), null);
  const stop = view.getByRole("button", { name: "Stop using fact fact1" });
  assert.equal(stop.disabled, false);
  view.fireEvent.click(stop);
  assert.deepEqual(writes, [
    [
      "/api/v1/projects/p1/reviewed-memory/fact1/stop",
      "Fact use stopped",
      { expected_version: 1, reason: "Human selected Stop using" },
    ],
  ]);
  assert.doesNotMatch(view.container.textContent, /\b(?:deleted|forgotten)\b/i);
});

test("real Work surface approves only edited reviewed text and retries the frozen operation", async () => {
  const { createElement } = await import("react");
  const { render, act, fireEvent, within } = await import(
    "@testing-library/react"
  );
  const { WorkScreen } = await import("./WorkScreen.tsx");
  const writes = [];
  const client = {
    projects: async () => ({
      projects: [project],
      next_cursor: null,
      can_create_projects: false,
      create_channels: [],
    }),
    project: async () => ({ project }),
    workItems: async () => ({ work_items: [item], next_cursor: null }),
    workItem: async () => ({ work_item: item }),
    workExecutions: async () => ({ executions: [] }),
    reviewedMemory: async () => emptyPage,
    workMutation: async (path, body) => {
      writes.push({ path, body });
      throw new OrtakApiError(503, "unavailable");
    },
  };
  const view = render(
    createElement(WorkScreen, { client, employees: [employee] }),
  );
  await act(async () => {});
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: /Project one/ })),
  );
  assert.ok(view.getByRole("region", { name: "Reviewed project memory" }));
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: /Ship the review/ })),
  );
  await act(async () =>
    fireEvent.change(view.getByLabelText("Memory audience: employee"), {
      target: { value: "cem" },
    }),
  );
  const form = view.getByRole("form", { name: "Approve project memory" });
  const editor = within(form);
  assert.equal(editor.getByLabelText("Edited fact").value, "");
  fireEvent.change(editor.getByLabelText("Edited fact"), {
    target: { value: "Human edited fact" },
  });
  fireEvent.change(editor.getByLabelText("Use until (local time)"), {
    target: {
      value: new Date(Date.now() + 86400000).toISOString().slice(0, 16),
    },
  });
  fireEvent.click(editor.getByLabelText(/I reviewed this fact/));
  // Programmatic submit bypasses native required: no selection must still refuse.
  await act(async () => fireEvent.submit(form));
  assert.equal(writes.length, 0);
  fireEvent.change(editor.getByLabelText("Evidence reviewed"), {
    target: { value: JSON.stringify(fact.source) },
  });
  fireEvent.click(editor.getByLabelText(/I reviewed this fact/));
  await act(async () => fireEvent.submit(form));
  assert.equal(writes.length, 0);
  fireEvent.click(editor.getByLabelText(/I reviewed this fact/));
  await act(async () => fireEvent.submit(form));
  assert.equal(writes.length, 1);
  assert.equal(writes[0].path, "/api/v1/projects/p1/reviewed-memory");
  const saved = JSON.parse(writes[0].body);
  assert.match(saved.operation_id, /^[0-9a-f-]{36}$/);
  assert.deepEqual(saved.fact, {
    employee_id: "cem",
    source: fact.source,
    content: "Human edited fact",
    expires_at: saved.fact.expires_at,
    reviewed: true,
  });
  assert.equal(writes[0].body.includes(item.description), false);
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Retry same operation" })),
  );
  assert.deepEqual(writes[1], writes[0]);
});

test("failed current access check clears fact text and stops retrying until manual recovery", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  let calls = 0,
    revoked = 0;
  const view = await setup({
    client: {
      reviewedMemory: async () => {
        calls++;
        if (calls > 1) throw new OrtakApiError(403, "Access revoked");
        return { facts: [fact], next_after: null };
      },
    },
    revoke: () => revoked++,
  });
  assert.ok(view.getByText(fact.content));
  await view.act(async () =>
    view.fireEvent.click(
      view.getByRole("button", { name: "Refresh reviewed memory" }),
    ),
  );
  assert.equal(view.queryByText(fact.content), null);
  assert.equal(
    view.queryByRole("button", { name: "Stop using fact fact1" }),
    null,
  );
  assert.equal(revoked, 1);
  assert.equal(calls, 2);
  assert.ok(view.getByRole("button", { name: "Refresh reviewed memory" }));
  await view.act(async () => context.mock.timers.tick(300000));
  assert.equal(calls, 2);
});

test("employee changes abort in-flight inspection and refuse stale audience results", async () => {
  let finish, firstSignal;
  const view = await setup({
    employees: [employee, { ...employee, employee_id: "ada", name: "Ada" }],
    client: {
      reviewedMemory: async (_project, audience, signal) => {
        if (audience === "cem") {
          firstSignal = signal;
          return new Promise((resolve) => {
            finish = resolve;
          });
        }
        return {
          facts: [
            {
              ...fact,
              id: "fact2",
              employee_id: "ada",
              content: "Ada scoped fact",
            },
          ],
          next_after: null,
        };
      },
    },
  });
  await view.act(async () =>
    view.fireEvent.change(view.getByLabelText("Memory audience: employee"), {
      target: { value: "ada" },
    }),
  );
  assert.equal(firstSignal.aborted, true);
  await view.act(async () => finish({ facts: [fact], next_after: null }));
  assert.equal(view.queryByText(fact.content), null);
  assert.ok(view.getByText("Ada scoped fact"));
});

test("recall preview clears on current-authority refresh and aborts the prior query", async () => {
  let resolveQuery, querySignal;
  const view = await setup({
    client: {
      reviewedMemory: async () => emptyPage,
      recallReviewedMemory: async (_project, _employee, _query, signal) => {
        querySignal = signal;
        return new Promise((resolve) => {
          resolveQuery = resolve;
        });
      },
    },
  });
  view.fireEvent.change(view.getByLabelText("Search approved facts"), {
    target: { value: "deployment" },
  });
  await view.act(async () =>
    view.fireEvent.submit(
      view.getByRole("form", { name: "Search reviewed context" }),
    ),
  );
  assert.equal(querySignal.aborted, false);
  await view.act(async () =>
    view.fireEvent.click(
      view.getByRole("button", { name: "Refresh reviewed memory" }),
    ),
  );
  assert.equal(querySignal.aborted, true);
  await view.act(async () => resolveQuery({ facts: [fact], truncated: false }));
  assert.equal(view.queryByText(fact.content), null);
  assert.equal(
    view.getByRole("button", { name: "Preview recall" }).disabled,
    false,
  );
});

test("evidence selection remains pinned when deliverables reorder and clears when removed", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent } = await import("@testing-library/react");
  const { ReviewedFactForm } = await import("./ReviewedFactForm.tsx");
  const writes = [];
  const first = {
    employee_id: "cem",
    artifact_id: "artifact1",
    execution_version: 1,
  };
  const second = { ...first, artifact_id: "artifact2", execution_version: 2 };
  const props = {
    project,
    employee,
    item,
    executions: [first, second],
    disabled: false,
    submit: (...args) => writes.push(args),
  };
  const view = render(createElement(ReviewedFactForm, props));
  const source = { kind: "artifact", artifact_id: "artifact1" };
  fireEvent.change(view.getByLabelText("Evidence reviewed"), {
    target: { value: JSON.stringify(source) },
  });
  fireEvent.change(view.getByLabelText("Edited fact"), {
    target: { value: "Edited artifact fact" },
  });
  fireEvent.change(view.getByLabelText("Use until (local time)"), {
    target: {
      value: new Date(Date.now() + 86400000).toISOString().slice(0, 16),
    },
  });
  fireEvent.click(view.getByLabelText(/I reviewed this fact/));
  view.rerender(
    createElement(ReviewedFactForm, { ...props, executions: [second, first] }),
  );
  fireEvent.submit(view.getByRole("form", { name: "Approve project memory" }));
  assert.deepEqual(writes[0][2].fact.source, source);
  view.rerender(
    createElement(ReviewedFactForm, { ...props, executions: [second] }),
  );
  assert.equal(view.getByLabelText("Evidence reviewed").value, "");
  fireEvent.submit(view.getByRole("form", { name: "Approve project memory" }));
  assert.equal(writes.length, 1);
});

test("publication requires a separate confirmation and sends only the opaque fact version", async () => {
  const writes = [];
  const view = await setup({
    client: {
      reviewedMemory: async () => ({
        facts: [{ ...fact, publication_available: true }],
        next_after: null,
      }),
    },
    submit: (...args) => writes.push(args),
  });
  const form = view.getByRole("form", { name: "Publish reviewed fact fact1" });
  view.fireEvent.submit(form);
  assert.equal(
    writes.length,
    0,
    "programmatic/keyboard submit cannot bypass consent",
  );
  view.fireEvent.click(view.getByLabelText(/I approve sending this fact/));
  view.fireEvent.submit(form);
  assert.deepEqual(writes, [
    [
      "/api/v1/projects/p1/reviewed-memory/fact1/publish",
      "Reviewed fact publication requested",
      { expected_version: 1, confirmed: true },
    ],
  ]);
  assert.match(
    view.container.textContent,
    /saving alone never enables use in runs/,
  );
});

test("failed cleanup remains recoverable after source loss and never asserts unproved removal", async () => {
  const writes = [];
  const retained = {
    ...fact,
    status: "revoked",
    version: 2,
    source_visible: false,
    source: null,
    content: null,
    export: {
      fact_id: fact.id,
      publication: { ...exportJob, state: "acknowledged" },
      cleanup: {
        ...exportJob,
        state: "failed",
        retry_version: 2,
        attempt_count: 20,
        error_code: "target_unavailable",
      },
      erased_from_reviewed_store: false,
      runtime_consumption_enabled: false,
    },
  };
  const view = await setup({
    project: { ...project, status: "archived" },
    employees: [{ ...employee, status: "disabled" }],
    client: {
      reviewedMemory: async () => ({ facts: [retained], next_after: null }),
    },
    submit: (...args) => writes.push(args),
  });
  assert.equal(view.queryByText(fact.content), null);
  assert.ok(view.getByText(/cleanup failed; removal is not confirmed/));
  assert.equal(view.queryByText(/Reviewed-store text removed/), null);
  const retry = view.getByRole("button", {
    name: "Retry reviewed-store cleanup for fact fact1",
  });
  assert.equal(retry.disabled, false);
  view.fireEvent.click(retry);
  assert.deepEqual(writes, [
    [
      "/api/v1/projects/p1/reviewed-memory/fact1/exports/withdraw/retry",
      "Same reviewed memory operation queued",
      { retry_version: 2 },
    ],
  ]);
  assert.equal(
    view.queryByRole("button", { name: "Publish reviewed fact" }),
    null,
  );
  assert.doesNotMatch(view.container.textContent, /\b(?:deleted|forgotten)\b/i);
});

test("only the explicit removal receipt renders reviewed-store text removed", async () => {
  const view = await setup({
    client: {
      reviewedMemory: async () => ({
        facts: [
          {
            ...fact,
            status: "expired",
            export: {
              fact_id: fact.id,
              publication: { ...exportJob, state: "failed" },
              cleanup: { ...exportJob, state: "acknowledged" },
              erased_from_reviewed_store: true,
              runtime_consumption_enabled: false,
            },
          },
        ],
        next_after: null,
      }),
    },
  });
  assert.ok(
    view.getByText(
      /Reviewed-store text removed. Approval and tombstone records remain/,
    ),
  );
  assert.ok(view.getByText(/Runtime use is not enabled for this fact/));
  assert.equal(view.queryByRole("button", { name: /Retry publication/ }), null);
});
