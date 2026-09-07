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

test("reviewed project context keeps provenance while revoked text is withheld", async () => {
  const React = await import("react");
  const { render } = await import("@testing-library/react");
  const { RunMemoryPanel } = await import("./RunMemoryPanel.tsx");
  const memory = {
    scope: "run_scratch_and_reviewed_project",
    run_id: "run-1",
    recall: { status: "prepared", records: [], truncated: false },
    write: null,
    reviewed: [
      {
        fact_id: "reviewed-fact-1",
        approval_id: "approval-1",
        approved_by: "human",
        expires_at: "2026-09-07T12:00:00Z",
        current: false,
        content: { text: "Withheld reviewed content" },
      },
    ],
  };
  const view = render(React.createElement(RunMemoryPanel, { memory }));
  assert.ok(view.getByRole("region", { name: "Reviewed project memory used" }));
  assert.ok(view.getByText(/original use receipt remains/));
  assert.ok(view.getByText(/Reviewed fact: reviewed-fact-1/));
  assert.ok(view.getByText(/Stop using is available/));
  assert.equal(view.queryByText("Withheld reviewed content"), null);
  view.rerender(
    React.createElement(RunMemoryPanel, {
      memory: {
        ...memory,
        reviewed: [{ ...memory.reviewed[0], current: true }],
      },
    }),
  );
  assert.ok(view.getByText("Withheld reviewed content"));
});

test("v4 mixed memory preserves order and withholds derived text without losing the write receipt", async () => {
  const React = await import("react");
  const { render, within } = await import("@testing-library/react");
  const { RunMemoryPanel } = await import("./RunMemoryPanel.tsx");
  const memory = {
    scope: "run_scratch_and_reviewed_conversation",
    run_id: "run-conversation",
    recall: {
      status: "prepared",
      prepared_at: "2026-09-06T12:00:00Z",
      records: [
        {
          record_ref: "scratch-1",
          content: { text: "Earlier scratch note" },
          source: "run:conversation",
          recorded_at: "2026-09-06T12:00:00Z",
        },
      ],
      truncated: false,
    },
    reviewed: ["conversation-first", "project-second"].map((id) => ({
      fact_id: id,
      approval_id: `approval-${id}`,
      approved_by: "human",
      expires_at: "2026-09-07T12:00:00Z",
      current: true,
      content: { text: `Approved ${id} text` },
      audience_kind: id === "conversation-first" ? "conversation" : "project",
      audience:
        id === "conversation-first"
          ? {
              kind: "thread",
              channel_id: "current-audience-channel",
              thread_root_event_id: "current-audience-thread",
            }
          : null,
    })),
    write: {
      status: "acknowledged",
      content: { text: "Derived Office note", redacted: false },
      source: "office:retained-source",
      recorded_at: "2026-09-06T12:00:01Z",
      receipt: { reference: "retained-receipt", written: 1 },
      acknowledged_at: "2026-09-06T12:00:02Z",
    },
  };
  const view = render(React.createElement(RunMemoryPanel, { memory }));
  const region = view.getByRole("region", {
    name: "Reviewed memory with conversation facts",
  });
  assert.deepEqual(
    within(region)
      .getAllByRole("listitem")
      .map((item) => within(item).getByText(/Reviewed fact:/).textContent),
    ["Reviewed fact: conversation-first", "Reviewed fact: project-second"],
  );
  assert.ok(view.getByText("Earlier scratch note"));
  assert.ok(view.getByText("Derived Office note"));
  assert.ok(view.getByText("Audience: this thread"));
  assert.ok(view.getByText("Audience: project"));
  assert.ok(view.getByText("Channel: current-audience-channel"));
  assert.ok(view.getByText("Thread: current-audience-thread"));
  assert.ok(view.getByText("Approval: approval-conversation-first"));
  assert.equal(
    view.queryByRole("region", { name: "Reviewed project memory used" }),
    null,
  );
  // Flags win even if an old render retained content. Server v4 sends empty
  // scratch/write text and current=false reviewed entries on authority loss.
  view.rerender(
    React.createElement(RunMemoryPanel, {
      memory: {
        ...memory,
        recall: { ...memory.recall, withheld: true },
        write: { ...memory.write, withheld: true },
      },
    }),
  );
  assert.ok(view.getByText(/Previously included notes are withheld/));
  assert.equal(view.queryByText("No earlier notes were included."), null);
  assert.equal(view.queryByText("Audience: this thread"), null);
  assert.equal(view.queryByText("Channel: current-audience-channel"), null);
  assert.equal(view.queryByText("Thread: current-audience-thread"), null);
  assert.ok(view.getByText("Approval: approval-conversation-first"));
  for (const text of [
    "Earlier scratch note",
    "Derived Office note",
    "Approved conversation-first text",
    "Approved project-second text",
  ])
    assert.equal(view.queryByText(text), null);
  assert.ok(view.getByText("Reply saved to memory"));
  assert.ok(view.getByText("View write receipt and source"));
  assert.ok(view.getByText("1 note(s) confirmed"));
  assert.ok(view.getByText("Source: office:retained-source"));
  assert.equal(view.getAllByText(/original use receipt remains/).length, 2);
  assert.ok(view.getByText(/Reviewed fact: conversation-first/));
  assert.ok(view.getByText(/Reviewed fact: project-second/));
});

test("v5 labels employee audiences and hides all retained text on requester or permission loss", async () => {
  const React = await import("react");
  const { render, within } = await import("@testing-library/react");
  const { RunMemoryPanel } = await import("./RunMemoryPanel.tsx");
  const memory = {
    scope: "run_scratch_and_reviewed_employee",
    run_id: "employee-run",
    recall: { status: "prepared", records: [], truncated: false },
    write: null,
    reviewed: ["relationship", "experience", "project"].map((kind) => ({
      fact_id: `${kind}-fact`,
      approval_id: `${kind}-approval`,
      approved_by: "human-key",
      expires_at: "2026-09-07T12:00:00Z",
      current: true,
      content: { text: `Selected ${kind} content` },
      audience_kind: kind === "project" ? "project" : "employee",
      audience:
        kind === "project"
          ? null
          : {
              format: "ortak-reviewed-employee-audience/1",
              kind,
              employee_id: "reviewed-employee",
              destination_channel_id: "explicit-destination",
              human_public_key: kind === "relationship" ? "exact-human" : null,
            },
    })),
  };
  const view = render(React.createElement(RunMemoryPanel, { memory }));
  const region = view.getByRole("region", {
    name: "Reviewed employee and mixed memory used",
  });
  assert.deepEqual(
    within(region)
      .getAllByRole("listitem")
      .map((item) => within(item).getByText(/Reviewed fact:/).textContent),
    [
      "Reviewed fact: relationship-fact",
      "Reviewed fact: experience-fact",
      "Reviewed fact: project-fact",
    ],
  );
  assert.ok(view.getByText("Audience: this human and employee relationship"));
  assert.ok(view.getByText("Audience: employee experience in this channel"));
  assert.ok(view.getByText("Audience: project"));
  assert.ok(view.getByText("Human: exact-human"));
  assert.ok(view.getByText(/employee’s reviewed memory controls/));
  assert.equal(
    view.queryByRole("region", { name: "Reviewed project memory used" }),
    null,
  );
  // Withheld is authoritative even while the previous render's records remain
  // in memory. Do not leak either canonical audience or selected text.
  view.rerender(
    React.createElement(RunMemoryPanel, {
      memory: { ...memory, recall: { ...memory.recall, withheld: true } },
    }),
  );
  assert.ok(view.getByText(/requester or current memory permissions/));
  for (const kind of ["relationship", "experience", "project"]) {
    assert.equal(view.queryByText(`Selected ${kind} content`), null);
    assert.ok(view.getByText(`Approval: ${kind}-approval`));
  }
  assert.equal(view.queryByText("Human: exact-human"), null);
  assert.equal(view.queryByText("Destination: explicit-destination"), null);
});

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

// The fixture drives the production hook at its live transport boundary.
function liveClient(source) {
  let receive;
  let reject;
  let streamSignal;
  return {
    ...source,
    activityStream: async (id, cursor, signal, callback) => {
      receive = callback;
      streamSignal = signal;
      callback({
        detail: await source.detail(id, signal),
        page: await source.events(id, cursor, signal),
      });
      return new Promise((resolve, fail) => {
        reject = fail;
        signal.addEventListener("abort", resolve, { once: true });
      });
    },
    push: async () => {
      receive({
        detail: await source.detail("run-1", streamSignal),
        page: { ...page, entries: [] },
      });
    },
    disconnect: (error) => reject(error),
  };
}

test("the real run panel keeps cancellation failure recoverable and never presents pending as stopped", async () => {
  const { createElement } = await import("react");
  const { render, act, fireEvent } = await import("@testing-library/react");
  const { RunPanel } = await import("./RunPanel.tsx");
  let attempts = 0;
  let cancellation = null;
  const client = liveClient({
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
  });
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
  const client = liveClient({
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
  });
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

test("the streaming hook rejects late previous-run frames after a scope switch", async () => {
  const { renderHook, act } = await import("@testing-library/react");
  const { useRunActivity } = await import("./useActivity.ts");
  let oldCallback;
  let oldSignal;
  const client = {
    activityStream: async (id, _cursor, signal, receive) => {
      if (id === "old") {
        oldCallback = receive;
        oldSignal = signal;
      } else receive({ detail: detail(id), page });
      return new Promise(() => {});
    },
  };
  const view = renderHook(({ id }) => useRunActivity(client, id, 0), {
    initialProps: { id: "old" },
  });
  view.rerender({ id: "new" });
  await act(async () => {});
  assert.equal(view.result.current.detail.detail.run.run_id, "new");
  assert.equal(oldSignal.aborted, true);
  await act(async () => oldCallback({ detail: detail("old"), page }));
  assert.equal(view.result.current.detail.detail.run.run_id, "new");
});

test("stream revocation clears rendered private content and stops retries", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useRunActivity } = await import("./useActivity.ts");
  let calls = 0;
  const client = liveClient({
    detail: async () => {
      calls++;
      return detail();
    },
    events: async () => page,
  });
  const view = renderHook(() => useRunActivity(client, "run-1", 0));
  await act(async () => {});
  assert.equal(view.result.current.entries.length, 1);
  await act(async () => client.disconnect(new OrtakApiError(403, "forbidden")));
  assert.equal(view.result.current.entries.length, 0);
  assert.equal(view.result.current.detail, null);
  assert.match(view.result.current.error, /permission/);
  await act(async () => context.mock.timers.tick(300_000));
  assert.equal(calls, 1);
});

test("reconnect resumes the dense cursor; repeated disconnects terminate even after a successful replay", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useRunActivity } = await import("./useActivity.ts");
  const cursors = [];
  const client = {
    activityStream: async (_id, cursor, _signal, receive) => {
      cursors.push(cursor);
      receive({
        detail: detail(),
        page: cursor === null ? page : { ...page, entries: [] },
      });
      throw new Error("Network disconnected");
    },
  };
  const view = renderHook(
    ({ refresh }) => useRunActivity(client, "run-1", refresh),
    { initialProps: { refresh: 0 } },
  );
  await act(async () => {});
  for (const delay of [3000, 6000, 12_000, 24_000, 300_000])
    await act(async () => context.mock.timers.tick(delay));
  assert.deepEqual(cursors, [null, 0, 0, 0, 0]);
  assert.equal(view.result.current.entries.length, 1);
  assert.equal(view.result.current.connected, false);
  assert.equal(view.result.current.reconnecting, false);
  view.rerender({ refresh: 1 });
  await act(async () => {});
  assert.deepEqual(cursors, [null, 0, 0, 0, 0, null]);
});

test("terminal run receives a pushed memory receipt on its existing stream", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { createElement } = await import("react");
  const { render, act } = await import("@testing-library/react");
  const { RunPanel } = await import("./RunPanel.tsx");
  let calls = 0;
  let status = "pending";
  const client = liveClient({
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
  });
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
  await act(async () => client.push());
  assert.ok(view.getByText("Reply saved to memory"));
  assert.ok(view.getByText("1 note(s) confirmed"));
  assert.equal(view.queryByText("Memory write pending"), null);
  await act(async () => context.mock.timers.tick(300_000));
  assert.equal(
    calls,
    2,
    "one initial snapshot and one actual push; no HTTP polling",
  );
});
