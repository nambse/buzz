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
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  }),
);
afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});
after(() => dom.window.close());
const queue = (employee = "ada", cursor = null, title = "Review the plan") => ({
  employee_id: employee,
  work_items: title
    ? [
        {
          id: title,
          project_id: "project",
          title,
          priority: "normal",
          state: "review",
          version: 2,
          assignment_role: "reviewer",
        },
      ]
    : [],
  next_cursor: cursor,
  execution_available: false,
});
const employee = {
  employee_id: "ada",
  name: "Ada",
  status: "paused",
  title: "Reviewer",
  active_revision_id: null,
};

test("actual queue client signs employee-scoped encoded pagination as GET without a mutation payload", async () => {
  const signed = [];
  const fetched = [];
  const client = createOrtakClient(
    "https://api.example.test",
    async (event) => {
      signed.push(event);
      return event;
    },
    async (url, init) => {
      fetched.push({ url, init });
      return Response.json(queue());
    },
  );
  await client.employeeWork(
    "employee/a",
    new AbortController().signal,
    "queue+/=cursor",
  );
  assert.equal(
    fetched[0].url,
    "https://api.example.test/api/v1/employees/employee%2Fa/work-items?limit=25&cursor=queue%2B%2F%3Dcursor",
  );
  const tags = Object.fromEntries(signed[0].tags);
  assert.equal(tags.u, fetched[0].url);
  assert.equal(tags.method, "GET");
  assert.equal(tags.payload, undefined);
  assert.equal(fetched[0].init.body, undefined);
  assert.equal(fetched[0].init.cache, "no-store");
});

test("real employee panel paginates one page, keeps inactive assignments visible, and distinguishes unavailable from empty", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, act } = await import("@testing-library/react");
  const { EmployeeWorkQueue } = await import("./EmployeeWorkQueue.tsx");
  const calls = [];
  let fail = false;
  let empty = false;
  const client = {
    employeeWork: async (id, _signal, cursor) => {
      calls.push({ id, cursor });
      if (fail) throw new OrtakApiError(503, "unavailable");
      return queue(
        id,
        cursor ? null : "page-two",
        empty ? null : cursor ? "Follow up" : "Review the plan",
      );
    },
  };
  const view = render(
    createElement(EmployeeWorkQueue, { client, employee, onClose: () => {} }),
  );
  await act(async () => {});
  assert.ok(view.getByRole("heading", { name: "Ada’s assigned work" }));
  assert.ok(
    view.getByText(/Outstanding assignments remain visible while inactive/),
  );
  assert.ok(view.getByText(/do not start or confirm employee execution/));
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "More assignments" })),
  );
  assert.deepEqual(calls[1], { id: "ada", cursor: "page-two" });
  assert.ok(view.getByRole("heading", { name: "Follow up" }));
  assert.equal(view.queryByRole("heading", { name: "Review the plan" }), null);
  fail = true;
  await act(async () =>
    fireEvent.click(
      view.getByRole("button", { name: "Refresh assigned work" }),
    ),
  );
  assert.ok(view.getByRole("alert"));
  assert.equal(view.queryByRole("heading", { name: "Follow up" }), null);
  assert.equal(
    view.queryByText("No visible outstanding assignments in this page."),
    null,
  );
  fail = false;
  empty = true;
  await act(async () =>
    fireEvent.click(
      view.getByRole("button", { name: "Refresh assigned work" }),
    ),
  );
  assert.ok(view.getByText("No visible outstanding assignments in this page."));
  assert.equal(calls.at(-1).cursor, undefined);
});

test("queue read independently fences employee and client changes against late results", async () => {
  const { renderHook, act } = await import("@testing-library/react");
  const { useEmployeeWork } = await import("./useEmployeeWork.ts");
  for (const boundary of ["employee", "client"]) {
    let finish;
    let oldSignal;
    const oldClient = {
      employeeWork: async (id, signal) => {
        if (id !== "ada") return queue(id, null, "Current assignment");
        oldSignal = signal;
        return await new Promise((resolve) => {
          finish = resolve;
        });
      },
    };
    const newClient = {
      employeeWork: async (id) => queue(id, null, "Current assignment"),
    };
    const view = renderHook(
      ({ client, id }) => useEmployeeWork(client, id, undefined, 0),
      { initialProps: { client: oldClient, id: "ada" } },
    );
    const id = boundary === "employee" ? "bea" : "ada";
    view.rerender({
      client: boundary === "client" ? newClient : oldClient,
      id,
    });
    await act(async () => {});
    assert.equal(oldSignal.aborted, true, boundary);
    assert.equal(view.result.current.page.employee_id, id);
    await act(async () => finish(queue("ada", null, "Old private assignment")));
    assert.equal(
      view.result.current.page.work_items[0].title,
      "Current assignment",
      boundary,
    );
    view.unmount();
  }
});

test("polling authorization loss clears queued work and stops retries until explicit refresh", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useEmployeeWork } = await import("./useEmployeeWork.ts");
  let calls = 0;
  const client = {
    employeeWork: async () => {
      if (++calls > 1) throw new OrtakApiError(403, "revoked");
      return queue();
    },
  };
  const view = renderHook(
    ({ refresh }) => useEmployeeWork(client, "ada", undefined, refresh),
    { initialProps: { refresh: 0 } },
  );
  await act(async () => {});
  assert.equal(view.result.current.page.work_items.length, 1);
  await act(async () => context.mock.timers.tick(5000));
  assert.equal(view.result.current.page, null);
  assert.match(view.result.current.error, /permission/);
  await act(async () => context.mock.timers.tick(300000));
  assert.equal(calls, 2);
  view.rerender({ refresh: 1 });
  await act(async () => {});
  assert.equal(calls, 3);
});

test("mismatched employee response is refused and transport retries stop at five attempts", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useEmployeeWork } = await import("./useEmployeeWork.ts");
  let calls = 0;
  const client = {
    employeeWork: async () => {
      calls++;
      return queue("other");
    },
  };
  const view = renderHook(() => useEmployeeWork(client, "ada", undefined, 0));
  await act(async () => {});
  assert.equal(view.result.current.page, null);
  assert.match(view.result.current.error, /did not match/);
  for (const delay of [3000, 6000, 12000, 24000, 300000])
    await act(async () => context.mock.timers.tick(delay));
  assert.equal(calls, 5);
});
