import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";
import { createOrtakClient, OrtakApiError } from "./client.ts";
import { provisioningSteps } from "./provisioning/types.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
  pretendToBeVisual: true,
});
before(() =>
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
    getComputedStyle: dom.window.getComputedStyle.bind(dom.window),
    requestAnimationFrame: dom.window.requestAnimationFrame.bind(dom.window),
    cancelAnimationFrame: dom.window.cancelAnimationFrame.bind(dom.window),
  }),
);
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());
const employee = { employee_id: "employee-1", name: "Ada", status: "draft" };
const operation = (id = "operation-1", employeeId = employee.employee_id) => ({
  operation_id: id,
  employee_id: employeeId,
  mode: "adopt",
  dry_run: false,
  status: "failed",
  current_step: "ensure_runtime_profile",
  result_revision_id: null,
  created_at: "2026-09-05T12:00:00Z",
  updated_at: "2026-09-05T12:00:02Z",
  finished_at: "2026-09-05T12:00:02Z",
});
const page = (op = operation()) => ({
  employee_id: op.employee_id,
  operations: [op],
  has_more: false,
  next_cursor: null,
  read_only: true,
});
const detail = (op = operation()) => ({
  operation: op,
  read_only: true,
  steps: Object.keys(provisioningSteps).map((name) => ({
    name,
    state: name === "ensure_runtime_profile" ? "failed" : "pending",
    attempt_count: name === "ensure_runtime_profile" ? 3 : 0,
    adopted_existing: name === "ensure_runtime_profile",
    started_at: null,
    finished_at: null,
  })),
});

test("actual provisioning panel distinguishes pending cleanup from recorded readiness", async () => {
  const { createElement } = await import("react");
  const { render, act, fireEvent } = await import("@testing-library/react");
  const { ProvisioningPanel } = await import(
    "./provisioning/ProvisioningPanel.tsx"
  );
  for (const [state, message] of [
    ["running", /connection check or its cleanup is pending/],
    ["succeeded", /Activation still requires fresh current health checks/],
    ["failed", /Use the available command recovery action/],
  ]) {
    const op = {
      ...operation(),
      status: "running",
      current_step: "validate_runtime_profile",
    };
    const client = {
      provisioning: async () => page(op),
      provisioningOperation: async () => ({
        ...detail(op),
        runtime_probe: {
          state,
          generation: 2,
          created_at: op.created_at,
          deadline: op.updated_at,
          contained_at: state === "running" ? null : op.updated_at,
          error_code: state === "failed" ? "probe_transport" : null,
        },
      }),
    };
    const view = render(
      createElement(ProvisioningPanel, { client, employee, onClose() {} }),
    );
    await act(async () => {});
    await act(async () =>
      fireEvent.click(
        view.getByRole("button", { name: /View provisioning steps/ }),
      ),
    );
    assert.ok(view.getByText(/Runtime connection check · Attempt 2/));
    assert.ok(view.getByText(message));
    assert.equal(
      view.queryByRole("button", { name: /Start probe|Activate/ }),
      null,
    );
    view.unmount();
  }
});

test("real provisioning panel shows persisted steps and retention without offering unsupported mutations", async () => {
  const { createElement } = await import("react");
  const { render, act, fireEvent, within } = await import(
    "@testing-library/react"
  );
  const { ProvisioningPanel } = await import(
    "./provisioning/ProvisioningPanel.tsx"
  );
  let current = operation();
  const calls = [];
  const client = {
    provisioning: async (id) => {
      calls.push(["read", id]);
      return page(current);
    },
    provisioningOperation: async (id, op) => {
      calls.push(["steps", id, op]);
      return detail(current);
    },
  };
  const view = render(
    createElement(ProvisioningPanel, { client, employee, onClose() {} }),
  );
  await act(async () => {});
  assert.ok(view.getByText(/Last saved progress/));
  assert.ok(view.getByText(/do not confirm that a runner/));
  await act(async () =>
    fireEvent.click(
      view.getByRole("button", { name: /View provisioning steps/ }),
    ),
  );
  assert.equal(
    within(
      view.getByRole("list", { name: "Recorded provisioning steps" }),
    ).getAllByRole("listitem").length,
    10,
  );
  assert.ok(view.getByText("Provisioning step failed"));
  assert.ok(view.getByText("Attempts: 3"));
  assert.ok(view.getByText("Existing resource retained"));
  assert.ok(
    view.getByText(
      /Existing resources are retained when this Adopt operation is compensated/,
    ),
  );
  for (const name of [
    /Activate employee/,
    /^Retry operation$/,
    /^Compensate$/,
    /^Create employee$/,
  ])
    assert.equal(view.queryByRole("button", { name }), null);
  current = {
    ...current,
    dry_run: true,
    status: "succeeded",
    current_step: null,
  };
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Refresh progress" })),
  );
  assert.ok(
    view.getByText(
      /This is a dry run. It did not publish a profile or activate/,
    ),
  );
  assert.equal(view.queryByText("Provisioning step failed"), null);
  assert.ok(calls.every(([kind]) => ["read", "steps"].includes(kind)));
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Close steps" })),
  );
  assert.equal(
    document.activeElement ===
      view.getByRole("button", { name: /View provisioning steps/ }),
    true,
  );
});

test("provisioning hook clears revoked records and does not retry an authorization denial", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useProvisioning } = await import("./provisioning/useProvisioning.ts");
  let calls = 0;
  const client = {
    provisioning: async () => {
      if (++calls > 1) throw new OrtakApiError(403, "forbidden");
      return page();
    },
    provisioningOperation: async () => detail(),
  };
  const view = renderHook(() =>
    useProvisioning(client, employee.employee_id, undefined, "operation-1", 0),
  );
  await act(async () => {});
  assert.equal(
    view.result.current.detail.operation.operation_id,
    "operation-1",
  );
  await act(async () => context.mock.timers.tick(5000));
  assert.equal(view.result.current.page, null);
  assert.equal(view.result.current.detail, null);
  assert.equal(view.result.current.retrying, false);
  await act(async () => context.mock.timers.tick(300_000));
  assert.equal(calls, 2);
});

test("late operation results cannot cross an employee switch", async () => {
  const { renderHook, act } = await import("@testing-library/react");
  const { useProvisioning } = await import("./provisioning/useProvisioning.ts");
  let finishOld;
  let oldSignal;
  const client = {
    provisioning: async (id, signal) => {
      if (id === "old") {
        oldSignal = signal;
        return new Promise((resolve) => {
          finishOld = resolve;
        });
      }
      return page(operation("new-operation", id));
    },
  };
  const view = renderHook(
    ({ id }) => useProvisioning(client, id, undefined, null, 0),
    { initialProps: { id: "old" } },
  );
  view.rerender({ id: "new" });
  assert.equal(view.result.current.page, null);
  await act(async () => {});
  assert.equal(oldSignal.aborted, true);
  await act(async () => finishOld(page(operation("old-operation", "old"))));
  assert.equal(view.result.current.page.employee_id, "new");
});

test("progress read failures stop after five attempts and refresh starts a new bounded generation", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useProvisioning } = await import("./provisioning/useProvisioning.ts");
  let calls = 0;
  const client = {
    provisioning: async () => {
      calls++;
      throw new OrtakApiError(503, "unavailable");
    },
  };
  const view = renderHook(
    ({ refresh }) =>
      useProvisioning(client, employee.employee_id, undefined, null, refresh),
    { initialProps: { refresh: 0 } },
  );
  await act(async () => {});
  for (const delay of [3000, 6000, 12_000, 24_000, 300_000])
    await act(async () => context.mock.timers.tick(delay));
  assert.equal(calls, 5);
  assert.equal(view.result.current.retrying, false);
  view.rerender({ refresh: 1 });
  await act(async () => {});
  assert.equal(calls, 6);
});

test("production provisioning client binds employee, operation and opaque cursor to signed GET only", async () => {
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
      return Response.json({});
    },
  );
  const signal = new AbortController().signal;
  await client.provisioning("employee-1", signal, "cursor+/=");
  await client.provisioningOperation("employee-1", "operation-1", signal);
  assert.match(
    fetched[0].url,
    /employee-1\/provisioning\?limit=25&cursor=cursor%2B%2F%3D$/,
  );
  assert.match(fetched[1].url, /employee-1\/provisioning\/operation-1$/);
  fetched.forEach(({ url, init }, index) => {
    assert.equal(Object.fromEntries(signed[index].tags).u, url);
    assert.equal(Object.fromEntries(signed[index].tags).method, "GET");
    assert.equal(init.body, undefined);
  });
});

test("actual Employees screen exposes provisioning only for its server-provided capability", async () => {
  const { createElement } = await import("react");
  const { render, act, fireEvent } = await import("@testing-library/react");
  const { OrtakScreen } = await import("./OrtakScreen.tsx");
  let enabled = false;
  let managementReads = 0;
  const originalFetch = globalThis.fetch;
  window.__TAURI_INTERNALS__ = {
    invoke: async (command, input) => {
      assert.equal(command, "sign_event");
      return JSON.stringify({ ...input, id: "test-native-signature" });
    },
  };
  globalThis.fetch = async (url) => {
    const path = new URL(url).pathname;
    if (path.endsWith("/employees"))
      return Response.json({
        employees: [employee],
        has_more: false,
        next_after: null,
        can_view_provisioning: enabled,
      });
    if (path.endsWith("/runs"))
      return Response.json({ runs: [], has_more: false, next_cursor: null });
    if (path.endsWith("/provisioning")) {
      managementReads++;
      return Response.json({ ...page(), operations: [] });
    }
    throw new Error("Unexpected test endpoint");
  };
  try {
    const view = render(
      createElement(OrtakScreen, { origin: "https://api.example.test" }),
    );
    await act(async () => {});
    assert.equal(
      view.queryByRole("button", { name: "View provisioning for Ada" }),
      null,
    );
    assert.equal(managementReads, 0);
    enabled = true;
    await act(async () =>
      fireEvent.click(
        view.getByRole("button", { name: "Refresh", exact: true }),
      ),
    );
    await act(async () =>
      fireEvent.click(
        view.getByRole("button", { name: "View provisioning for Ada" }),
      ),
    );
    assert.ok(
      view.getByText(
        "No provisioning operations have been recorded for this employee.",
      ),
    );
    assert.equal(managementReads, 1);
    enabled = false;
    await act(async () =>
      fireEvent.click(
        view.getByRole("button", { name: "Refresh", exact: true }),
      ),
    );
    assert.equal(
      view.queryByRole("button", { name: "View provisioning for Ada" }),
      null,
    );
    assert.equal(
      view.queryByRole("region", { name: "Provisioning for Ada" }),
      null,
    );
    view.unmount();
  } finally {
    globalThis.fetch = originalFetch;
    delete window.__TAURI_INTERNALS__;
  }
});
