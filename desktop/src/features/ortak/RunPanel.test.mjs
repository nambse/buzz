import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";
import { OrtakApiError } from "./client.ts";

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

const run = (id = "run-1") => ({
  run_id: id,
  employee_id: "employee-1",
  status: "running",
  outcome: { kind: "pending" },
  timing: { queued_at: "2026-09-05T12:00:00Z", started_at: null },
  last_event: { sequence: 0 },
});
const detail = (id = "run-1") => ({
  detail: { run: run(id), error_message: null, cancel_reason: null },
  cancellation: null,
  can_request_cancel: true,
  office_delivery: null,
});
const page = {
  entries: [
    {
      sequence: 0,
      event_type: "assistant.output",
      occurred_at: "2026-09-05T12:00:01Z",
      activity: { kind: "assistant_output", text: { text: "Private work" } },
    },
  ],
  next_after_sequence: 0,
  has_more: false,
  gap: null,
};

test("the real run panel keeps cancellation failure recoverable and never presents pending as stopped", async () => {
  const { createElement } = await import("react");
  const { render, act, fireEvent } = await import("@testing-library/react");
  const { RunPanel } = await import("./RunPanel.tsx");
  let attempts = 0;
  let cancellation = null;
  const client = {
    detail: async () => ({
      ...detail(),
      cancellation,
      can_request_cancel: cancellation === null,
    }),
    events: async () => page,
    cancel: async () => {
      if (++attempts === 1) throw new OrtakApiError(503, "unavailable");
      cancellation = {
        request_id: "request-1",
        run_id: "run-1",
        status: "pending",
        requested_at: "2026-09-05T12:00:02Z",
      };
      return cancellation;
    },
  };
  const view = render(
    createElement(RunPanel, {
      client,
      runId: "run-1",
      employeeName: "Test Employee",
    }),
  );
  await act(async () => {});
  assert.ok(view.getByText("Private work"));
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Cancel run" })),
  );
  assert.ok(view.getByText("Could not request cancellation"));
  assert.ok(view.getByRole("button", { name: "Cancel run" }));
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Cancel run" })),
  );
  assert.ok(view.getByText("Cancellation requested"));
  assert.ok(
    view.getByText("The worker has not confirmed that execution stopped."),
  );
  assert.ok(view.getByText("running", { exact: true }));
  assert.equal(view.queryByRole("button", { name: "Cancel run" }), null);
  assert.ok(view.getByRole("button", { name: "Reload timeline" }));
});

test("a completed run stays distinct from pending, failed, and confirmed Office delivery", async () => {
  const { createElement } = await import("react");
  const { render, act, fireEvent } = await import("@testing-library/react");
  const { RunPanel } = await import("./RunPanel.tsx");
  let status = "pending";
  const client = {
    detail: async () => ({
      ...detail(),
      detail: {
        ...detail().detail,
        run: {
          ...run(),
          status: "completed",
          outcome: { kind: "completed", delivery_intent: "reply" },
        },
      },
      can_request_cancel: false,
      office_delivery: {
        status,
        error_code: status === "failed" ? "office_output_source_invalid" : null,
        delivered_at: status === "delivered" ? "2026-09-05T12:00:03Z" : null,
      },
    }),
    events: async () => page,
  };
  const view = render(
    createElement(RunPanel, {
      client,
      runId: "run-1",
      employeeName: "Test Employee",
    }),
  );
  await act(async () => {});
  assert.ok(view.getByText("completed", { exact: true }));
  assert.ok(view.getByText("Office reply pending"));
  assert.equal(view.queryByText("Office reply delivered"), null);
  status = "failed";
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Reload timeline" })),
  );
  assert.ok(view.getByText("Office reply failed"));
  assert.ok(view.getByText("completed", { exact: true }));
  assert.equal(view.queryByText("Office reply delivered"), null);
  status = "delivered";
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Reload timeline" })),
  );
  assert.ok(view.getByText("Office reply delivered"));
  assert.ok(view.getByText("Office accepted this run’s reply."));
  assert.equal(view.queryByText("Office reply failed"), null);
});

test("the actual polling hook rejects late previous-run results after a scope switch", async () => {
  const { renderHook, act } = await import("@testing-library/react");
  const { useRunActivity } = await import("./useActivity.ts");
  let finishOld;
  let oldSignal;
  const client = {
    detail: async (id, signal) => {
      if (id === "old") {
        oldSignal = signal;
        return await new Promise((resolve) => {
          finishOld = resolve;
        });
      }
      return detail(id);
    },
    events: async () => page,
  };
  const view = renderHook(({ id }) => useRunActivity(client, id, 0), {
    initialProps: { id: "old" },
  });
  view.rerender({ id: "new" });
  await act(async () => {});
  assert.equal(view.result.current.detail.detail.run.run_id, "new");
  assert.equal(oldSignal.aborted, true);
  await act(async () => finishOld(detail("old")));
  assert.equal(view.result.current.detail.detail.run.run_id, "new");
});

test("polling revocation clears already rendered private content and stops automatic retries", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useRunActivity } = await import("./useActivity.ts");
  let calls = 0;
  const client = {
    detail: async () => {
      if (++calls > 1) throw new OrtakApiError(403, "forbidden");
      return detail();
    },
    events: async () => page,
  };
  const view = renderHook(() => useRunActivity(client, "run-1", 0));
  await act(async () => {});
  assert.equal(view.result.current.entries.length, 1);
  await act(async () => context.mock.timers.tick(2500));
  assert.equal(view.result.current.entries.length, 0);
  assert.equal(view.result.current.detail, null);
  assert.match(view.result.current.error, /permission/);
  await act(async () => context.mock.timers.tick(300_000));
  assert.equal(calls, 2);
});

test("persistent transport failures stop after five attempts and preserve a manual recovery seam", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useRunActivity } = await import("./useActivity.ts");
  let calls = 0;
  const client = {
    detail: async () => {
      calls++;
      throw new OrtakApiError(503, "unavailable");
    },
    events: async () => page,
  };
  const view = renderHook(
    ({ refresh }) => useRunActivity(client, "run-1", refresh),
    { initialProps: { refresh: 0 } },
  );
  await act(async () => {});
  for (const delay of [3000, 6000, 12_000, 24_000, 300_000])
    await act(async () => context.mock.timers.tick(delay));
  assert.equal(calls, 5);
  view.rerender({ refresh: 1 });
  await act(async () => {});
  assert.equal(calls, 6);
});

test("confirmed Office delivery keeps polling pending memory until the actual receipt is visible", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { createElement } = await import("react");
  const { render, act } = await import("@testing-library/react");
  const { RunPanel } = await import("./RunPanel.tsx");
  let calls = 0;
  let status = "pending";
  const client = {
    detail: async () => {
      calls++;
      return {
        ...detail(),
        detail: { ...detail().detail, run: { ...run(), status: "completed" } },
        can_request_cancel: false,
        office_delivery: {
          status: "delivered",
          error_code: null,
          delivered_at: "2026-09-05T12:00:03Z",
        },
        memory: {
          scope: "run_scratch",
          run_id: "run-1",
          recall: {
            status: "prepared",
            prepared_at: "2026-09-05T12:00:00Z",
            records: [],
            truncated: false,
          },
          write: {
            status,
            content: { text: "Confirmed Office answer", redacted: false },
            source: "office:fixture",
            recorded_at: "2026-09-05T12:00:03Z",
            receipt:
              status === "acknowledged"
                ? { reference: "receipt:fixture", written: 1 }
                : null,
            acknowledged_at:
              status === "acknowledged" ? "2026-09-05T12:00:04Z" : null,
          },
        },
      };
    },
    events: async () => page,
  };
  const view = render(
    createElement(RunPanel, {
      client,
      runId: "run-1",
      employeeName: "Test Employee",
    }),
  );
  await act(async () => {});
  assert.ok(view.getByText("Office reply delivered"));
  assert.ok(view.getByText("Memory write pending"));
  assert.ok(view.getByText("No earlier notes were included."));
  assert.equal(view.queryByText("Reply saved to memory"), null);
  status = "acknowledged";
  await act(async () => context.mock.timers.tick(2500));
  assert.ok(view.getByText("Reply saved to memory"));
  assert.ok(view.getByText("1 note(s) confirmed"));
  assert.equal(view.queryByText("Memory write pending"), null);
  await act(async () => context.mock.timers.tick(300_000));
  assert.equal(calls, 2, "receipt and final activity stop polling");
});
