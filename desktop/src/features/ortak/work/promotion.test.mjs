import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";
import { createOrtakClient, OrtakApiError } from "../client.ts";
import { workSelection } from "./selection.ts";

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
const channel = "22222222-2222-4222-8222-222222222222";
const projectId = "33333333-3333-4333-8333-333333333333";
const itemId = "44444444-4444-4444-8444-444444444444";
const message = "a".repeat(64);
const project = {
  id: projectId,
  channel_id: channel,
  name: "Release plan",
  status: "active",
  can_contribute: true,
  can_review: true,
  role: "owner",
  slug: "release",
  version: 1,
};
const item = {
  id: itemId,
  project_id: projectId,
  source_message_id: message,
  title: "Review release",
  description: "Explicit work",
  priority: "normal",
  state: "proposed",
  version: 1,
  criteria: [],
  approvals: [],
  assignments: [],
  history: [],
  history_omitted: false,
  history_truncated: false,
};

async function setup(overrides = {}) {
  const { createElement } = await import("react");
  const testing = await import("@testing-library/react");
  const { useMessagePromotion } = await import("./useMessagePromotion.ts");
  const { MessagePromotionPanel } = await import("./MessagePromotion.tsx");
  const state = {
    writes: [],
    signatures: [],
    reads: [],
    status: 201,
    sourceStatus: 200,
    projectStatus: 200,
    selected: project,
    source: {
      message_id: message,
      channel_id: channel,
      decision: { mode: "silent" },
    },
    page: {
      projects: [project],
      next_cursor: null,
      create_channels: [],
      can_create_projects: false,
    },
    ...overrides,
  };
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async (event) => {
      state.signatures.push(event);
      return event;
    },
    async (url, init) => {
      state.reads.push({ url, signal: init.signal });
      if (init.method === "POST") {
        state.writes.push({ url, body: init.body, signal: init.signal });
        if (state.holdWrite)
          await new Promise((resolve) => {
            state.finishWrite = resolve;
          });
        return Response.json(
          { work_item: state.saved ?? item, created: true },
          { status: state.status },
        );
      }
      if (url.includes("/routing"))
        return Response.json(state.source, { status: state.sourceStatus });
      if (url.includes("/projects?")) {
        if (state.holdList)
          await new Promise((resolve) => {
            state.finishList = resolve;
          });
        return Response.json(state.page, { status: state.listStatus ?? 200 });
      }
      if (state.holdProject)
        await new Promise((resolve) => {
          state.finishProject = resolve;
        });
      return Response.json(
        { project: state.selected },
        { status: state.projectStatus },
      );
    },
  );
  function Harness({
    currentClient = client,
    currentMessage = message,
    currentChannel = channel,
    open = true,
  }) {
    const promotion = useMessagePromotion(
      currentClient,
      currentChannel,
      currentMessage,
      open,
    );
    state.promotion = promotion;
    return open
      ? createElement(MessagePromotionPanel, {
          key: `${currentMessage}:${currentChannel}:${promotion.cursor}:${promotion.formGeneration}`,
          state: promotion,
          message: currentMessage,
          openWork: (project, work) =>
            createElement(
              "a",
              { href: `/agents?workProject=${project}&workItem=${work}` },
              "Open saved Work",
            ),
        })
      : null;
  }
  const view = testing.render(createElement(Harness, {}));
  await testing.act(async () => {});
  function fill() {
    testing.fireEvent.change(view.getByLabelText("Work project"), {
      target: { value: projectId },
    });
    testing.fireEvent.change(view.getByLabelText("Work title"), {
      target: { value: "Review release" },
    });
    testing.fireEvent.change(view.getByLabelText("Work description"), {
      target: { value: "Explicit work" },
    });
    testing.fireEvent.change(
      view.getByLabelText("Acceptance criteria (one per line)"),
      { target: { value: "Check the receipt" } },
    );
  }
  async function submit() {
    await testing.act(async () =>
      testing.fireEvent.submit(
        view.getByRole("form", { name: "Create work item" }),
      ),
    );
    if (!state.holdProject && !state.holdWrite)
      await testing.waitFor(() => assert.equal(state.promotion.busy, false));
  }
  async function refresh() {
    await testing.act(async () =>
      testing.fireEvent.click(
        view.getByRole("button", { name: "Refresh promotion access" }),
      ),
    );
  }
  return {
    ...testing,
    createElement,
    Harness,
    state,
    client,
    view,
    fill,
    submit,
    refresh,
  };
}

test("actual promotion form signs one canonical source body and retries identical bytes after uncertain HTTP response", async () => {
  const x = await setup({ status: 503 });
  x.fill();
  await x.submit();
  assert.equal(x.state.writes.length, 1);
  const first = x.state.writes[0];
  assert.equal(
    first.url,
    `http://127.0.0.1:3010/api/v1/projects/${projectId}/promotions`,
  );
  const body = JSON.parse(first.body);
  assert.equal(body.source_message_id, message);
  assert.equal(body.title, "Review release");
  assert.deepEqual(body.criteria, ["Check the receipt"]);
  assert.deepEqual(Object.keys(body).sort(), [
    "approvals",
    "criteria",
    "description",
    "operation_id",
    "priority",
    "source_message_id",
    "title",
  ]);
  assert.ok(body.operation_id);
  assert.equal(
    x.view
      .getByRole("button", { name: "Promote message to Work" })
      .matches(":disabled"),
    true,
  );
  // Direct submissions cannot bypass the disabled fieldset or replace the pending request.
  x.fireEvent.change(x.view.getByLabelText("Work title"), {
    target: { value: "Changed while uncertain" },
  });
  await x.submit();
  assert.equal(x.state.writes.length, 1);
  x.state.status = 200;
  await x.refresh();
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Retry same promotion" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.promotion.busy, false));
  assert.equal(x.state.writes.length, 2);
  assert.equal(x.state.writes[1].body, first.body);
  const signed = x.state.signatures.filter((event) =>
    event.tags.some((tag) => tag[0] === "method" && tag[1] === "POST"),
  );
  assert.equal(signed.length, 2);
  for (const event of signed)
    assert.deepEqual(
      event.tags.find((tag) => tag[0] === "payload"),
      ["payload", createHash("sha256").update(first.body).digest("hex")],
    );
  assert.notDeepEqual(
    signed[0].tags.find((t) => t[0] === "nonce"),
    signed[1].tags.find((t) => t[0] === "nonce"),
  );
  assert.equal(
    x.view.getByRole("link", { name: "Open saved Work" }).getAttribute("href"),
    `/agents?workProject=${projectId}&workItem=${itemId}`,
  );
});

test("project selection uses only the current channel and contribution grant, with bounded signed pagination", async () => {
  const x = await setup({
    page: {
      projects: [
        project,
        {
          ...project,
          id: "foreign",
          name: "Foreign channel",
          channel_id: "other",
        },
        { ...project, id: "viewer", name: "Read only", can_contribute: false },
        { ...project, id: "archived", name: "Archived", status: "archived" },
      ],
      next_cursor: "next/+",
      create_channels: [],
      can_create_projects: false,
    },
  });
  assert.deepEqual(
    Array.from(
      x.view.getByLabelText("Work project").options,
      (option) => option.text,
    ),
    ["Choose a project", "Release plan"],
  );
  x.state.page = { ...x.state.page, projects: [], next_cursor: null };
  await x.act(async () =>
    x.fireEvent.click(x.view.getByRole("button", { name: "Next projects" })),
  );
  assert.ok(
    x.state.reads.some(({ url }) =>
      url.endsWith("/projects?limit=25&cursor=next%2F%2B"),
    ),
  );
  assert.ok(x.view.getByText(/No active project/));
  assert.equal(x.state.writes.length, 0);
});

test("fresh source and project authorization happen before any POST; revoked drafts disappear", async () => {
  for (const change of [
    { selected: { ...project, can_contribute: false } },
    { selected: { ...project, channel_id: "other" } },
    { sourceStatus: 403 },
  ]) {
    const x = await setup();
    x.fill();
    Object.assign(x.state, change);
    await x.submit();
    assert.equal(x.state.writes.length, 0);
    assert.equal(x.view.queryByLabelText("Work title"), null);
    assert.equal(
      x.view.queryByRole("button", { name: "Retry same promotion" }),
      null,
    );
    x.view.unmount();
  }
});

test("an unprocessed canonical message cannot be promoted, and mismatched projection never authorizes a write", async () => {
  for (const source of [
    { message_id: message, channel_id: channel, decision: null },
    {
      message_id: "b".repeat(64),
      channel_id: channel,
      decision: { mode: "silent" },
    },
  ]) {
    const x = await setup({ source });
    await x.act(async () =>
      x.state.promotion.submit(
        `/api/v1/projects/${projectId}/promotions`,
        "Promotion",
        { title: "Forbidden" },
      ),
    );
    assert.equal(x.state.writes.length, 0);
    assert.equal(x.view.queryByRole("form"), null);
    x.view.unmount();
  }
});

test("held refresh and transient failure retain the same form but disable direct writes until fresh access returns", async () => {
  const x = await setup();
  x.fill();
  const title = x.view.getByLabelText("Work title");
  x.state.holdList = true;
  await x.refresh();
  assert.equal(x.view.getByLabelText("Work title"), title);
  assert.equal(title.value, "Review release");
  await x.submit();
  assert.equal(x.state.writes.length, 0);
  x.state.listStatus = 503;
  await x.act(async () => x.state.finishList());
  assert.equal(x.view.getByLabelText("Work title"), title);
  await x.submit();
  assert.equal(x.state.writes.length, 0);
  x.state.holdList = false;
  x.state.listStatus = 200;
  await x.refresh();
  await x.submit();
  assert.equal(x.state.writes.length, 1);
});

test("close and reopen preserves exact uncertain recovery even after archival; scope changes clear it", async () => {
  const x = await setup({ status: 503 });
  x.fill();
  await x.submit();
  const first = x.state.writes[0].body;
  x.state.selected = { ...project, status: "archived" };
  x.state.page = { ...x.state.page, projects: [x.state.selected] };
  await x.act(async () =>
    x.view.rerender(x.createElement(x.Harness, { open: false })),
  );
  await x.act(async () => x.view.rerender(x.createElement(x.Harness, {})));
  assert.ok(x.view.getByRole("button", { name: "Retry same promotion" }));
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Retry same promotion" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.promotion.busy, false));
  assert.equal(x.state.writes[1].body, first);
  await x.act(async () =>
    x.view.rerender(
      x.createElement(x.Harness, { currentMessage: "b".repeat(64) }),
    ),
  );
  assert.equal(
    x.view.queryByRole("button", { name: "Retry same promotion" }),
    null,
  );
  assert.equal(x.view.queryByLabelText("Work title"), null);
});

test("held project recheck cannot dispatch after the selected message changes", async () => {
  const x = await setup({ holdProject: true });
  x.fill();
  await x.submit();
  await x.act(async () =>
    x.view.rerender(
      x.createElement(x.Harness, { currentChannel: "another-channel" }),
    ),
  );
  await x.act(async () => x.state.finishProject());
  assert.equal(x.state.writes.length, 0);
  assert.equal(x.view.queryByRole("link", { name: "Open saved Work" }), null);
});

test("401/403/404 read failures clear all private form and result state", async () => {
  for (const status of [401, 403, 404]) {
    const x = await setup();
    x.fill();
    x.state.sourceStatus = status;
    await x.refresh();
    assert.equal(x.view.queryByLabelText("Work title"), null);
    assert.equal(x.view.queryByRole("option", { name: "Release plan" }), null);
    assert.equal(x.state.promotion.pending, null);
    assert.equal(x.state.writes.length, 0);
    x.view.unmount();
  }
});

test("an authoritative preflight denial retires the held poll so stale success cannot restore private drafts", async (context) => {
  const schedule = globalThis.setTimeout;
  let poll;
  context.mock.method(globalThis, "setTimeout", (callback, delay, ...args) => {
    if (delay === 5000) {
      poll = callback;
      return schedule(() => {}, 0);
    }
    return schedule(callback, delay, ...args);
  });
  const x = await setup();
  x.fill();
  x.state.holdList = true;
  await x.act(async () => poll());
  assert.equal(typeof x.state.finishList, "function");
  assert.equal(x.state.promotion.ready, true);
  const held = x.state.reads
    .filter(({ url }) => url.includes("/projects?"))
    .at(-1);
  x.state.sourceStatus = 403;
  await x.submit();
  assert.equal(x.state.promotion.revoked, true);
  assert.equal(held.signal.aborted, true);
  assert.equal(x.view.queryByLabelText("Work title"), null);
  await x.act(async () => x.state.finishList());
  const reads = x.state.reads.length;
  await x.act(async () => poll());
  assert.equal(x.state.reads.length, reads);
  assert.equal(x.state.promotion.revoked, true);
  assert.equal(x.state.promotion.page, null);
  assert.equal(x.state.promotion.pending, null);
  assert.equal(x.view.queryByRole("option", { name: "Release plan" }), null);
  assert.equal(x.view.queryByLabelText("Work title"), null);
  assert.equal(x.state.writes.length, 0);
  // Only explicit fresh recovery can restore access, with an empty draft.
  x.state.holdList = false;
  x.state.sourceStatus = 200;
  await x.refresh();
  assert.equal(x.state.promotion.revoked, false);
  x.fireEvent.change(x.view.getByLabelText("Work project"), {
    target: { value: projectId },
  });
  assert.equal(x.view.getByLabelText("Work title").value, "");
});

test("Work navigation accepts only a complete UUID pair and fresh selected detail remains authority", async () => {
  assert.deepEqual(workSelection(projectId, itemId), {
    project: projectId,
    item: itemId,
  });
  for (const pair of [
    [projectId, undefined],
    ["../private", itemId],
    [projectId, { id: itemId }],
  ])
    assert.equal(workSelection(...pair), undefined);
  const { createElement } = await import("react");
  const { render, act } = await import("@testing-library/react");
  const { WorkScreen } = await import("./WorkScreen.tsx");
  const reads = [];
  const client = {
    projects: async () => ({
      projects: [project],
      next_cursor: null,
      create_channels: [],
    }),
    project: async (id) => {
      reads.push(["project", id]);
      return { project };
    },
    workItems: async () => ({ work_items: [item], next_cursor: null }),
    workItem: async (id) => {
      reads.push(["item", id]);
      return { work_item: item };
    },
    workExecutions: async () => ({ executions: [] }),
    workDependencies: async () => ({ dependencies: [] }),
    workDecomposition: async () => ({ parent: null, children: [] }),
  };
  const view = render(
    createElement(WorkScreen, {
      client,
      employees: [],
      initialSelection: workSelection(projectId, itemId),
    }),
  );
  await act(async () => {});
  assert.deepEqual(reads, [
    ["project", projectId],
    ["item", itemId],
  ]);
  assert.ok(view.getByRole("heading", { name: "Review release" }));
  view.unmount();
  const hidden = render(
    createElement(WorkScreen, {
      client: {
        ...client,
        workItem: async () => {
          throw new OrtakApiError(403, "revoked");
        },
      },
      employees: [],
      initialSelection: workSelection(projectId, itemId),
    }),
  );
  await act(async () => {});
  assert.equal(hidden.queryByRole("heading", { name: "Review release" }), null);
});
