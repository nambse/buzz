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
    IS_REACT_ACT_ENVIRONMENT: true,
  }),
);
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());
const message = "a".repeat(64),
  channel = "channel-one";
function page(id = message, mode = "silent") {
  return {
    message_id: id,
    channel_id: channel,
    decision: {
      decision_id: "decision-one",
      mode,
      summary_reason:
        mode === "silent" ? "no_relevant_employee" : "semantic_match",
      policy_version: "routing-v1",
      decided_at: "2026-09-06T00:00:00Z",
      scorer: {
        adapter: "hermes-codex",
        model: "gpt-5.6-sol",
        reasoning_effort: "high",
        version: "semantic-v1",
        prompt_version: "relevance-v1",
        latency_ms: 120,
        cache_hit: false,
        failure_code: null,
        input_tokens: 20,
        output_tokens: 10,
        total_tokens: 30,
      },
      recipients: [
        {
          employee_id: "Visible employee",
          action: mode === "silent" ? "drop" : "wake",
          reason: "below_semantic_threshold",
          score: 0.1,
          evidence: ["domain_match"],
        },
      ],
      recipients_truncated: false,
    },
  };
}

function liveClient(value) {
  const calls = [];
  return {
    calls,
    routingDecisionStream: async (_channel, _message, signal, receive) => {
      receive(value());
      await new Promise((resolve, reject) => {
        calls.push({ signal, receive, resolve, reject });
        signal.addEventListener("abort", resolve, { once: true });
      });
    },
  };
}

test("routing client signs exact channel/message read with no payload or provider mutation", async () => {
  const calls = [],
    signatures = [];
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async (event) => {
      signatures.push(event);
      return {};
    },
    async (url, init) => {
      calls.push({ url, init });
      return Response.json(page());
    },
  );
  await client.routingDecision(
    "channel/+",
    message,
    new AbortController().signal,
  );
  assert.equal(
    calls[0].url,
    `http://127.0.0.1:3010/api/v1/channels/channel%2F%2B/messages/${message}/routing`,
  );
  assert.equal(calls[0].init.method, "GET");
  assert.equal(calls[0].init.body, undefined);
  assert.deepEqual(
    signatures[0].tags.find((tag) => tag[0] === "u"),
    ["u", calls[0].url],
  );
  assert.equal(calls[0].init.cache, "no-store");
});

test("message menu gate permits delivered canonical channel text and refuses pending, DM and unknown scope", async () => {
  const { canReadMessageRouting } = await import("./MessageRoutingAction.tsx");
  for (const kind of [9, 40002])
    assert.equal(canReadMessageRouting({ id: message, kind }, channel), true);
  for (const item of [
    { id: message, kind: 1059 },
    { id: message, kind: 9, pending: true },
    { id: message },
    { id: "pending", kind: 9 },
  ])
    assert.equal(canReadMessageRouting(item, channel), false);
  assert.equal(canReadMessageRouting({ id: message, kind: 9 }, null), false);
});

test("actual panel distinguishes an absent decision from recorded zero-wake evidence", async () => {
  const { createElement } = await import("react");
  const { render, act, fireEvent } = await import("@testing-library/react");
  const { RoutingDecisionPanel } = await import("./RoutingDecisionDialog.tsx");
  let value = { message_id: message, channel_id: channel, decision: null };
  const client = liveClient(() => value);
  const view = render(
    createElement(RoutingDecisionPanel, { client, channel, message }),
  );
  await act(async () => {});
  assert.ok(view.getByText("No routing decision is recorded"));
  assert.equal(
    view.queryByText("No employee was dispatched by this decision."),
    null,
  );
  value = page();
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Refresh routing" })),
  );
  assert.ok(view.getByText("No employee was dispatched by this decision."));
  assert.ok(view.getByText("gpt-5.6-sol / high"));
  assert.ok(view.getByText(/Score 0.10/));
  assert.ok(view.getByText(/Live routing connected/));
});

test("a waking decision with no visible candidates never claims zero dispatches", async () => {
  const { createElement } = await import("react");
  const { render, act } = await import("@testing-library/react");
  const { RoutingDecisionPanel } = await import("./RoutingDecisionDialog.tsx");
  const value = page(message, "semantic");
  value.decision.recipients = [];
  const view = render(
    createElement(RoutingDecisionPanel, {
      client: liveClient(() => value),
      channel,
      message,
    }),
  );
  await act(async () => {});
  assert.ok(view.getByText("This decision selected employees for dispatch."));
  assert.ok(
    view.getByText("No candidate details are available to this account."),
  );
  assert.equal(view.queryByText(/No employee was dispatched/), null);
});

test("pushed revocation clears private evidence without polling and keeps manual recovery", async (t) => {
  const { createElement } = await import("react");
  const { render, act, fireEvent } = await import("@testing-library/react");
  const { RoutingDecisionPanel } = await import("./RoutingDecisionDialog.tsx");
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const client = liveClient(() => page());
  const view = render(
    createElement(RoutingDecisionPanel, { client, channel, message }),
  );
  await act(async () => {});
  assert.ok(view.getByText(/Visible employee/));
  await act(async () =>
    client.calls[0].reject(new OrtakApiError(403, "forbidden")),
  );
  assert.equal(view.queryByText(/Visible employee/), null);
  assert.ok(view.getByRole("button", { name: "Refresh routing" }));
  await act(async () => t.mock.timers.tick(100000));
  assert.equal(client.calls.length, 1);
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Refresh routing" })),
  );
  assert.ok(view.getByText(/Visible employee/));
  assert.equal(client.calls.length, 2);
});

test("five failed subscriptions stop automatic retry while manual refresh remains available", async (t) => {
  const { createElement } = await import("react");
  const { render, act, fireEvent } = await import("@testing-library/react");
  const { RoutingDecisionPanel } = await import("./RoutingDecisionDialog.tsx");
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let calls = 0;
  const client = {
    routingDecisionStream: async () => {
      calls++;
      throw new OrtakApiError(503, "unavailable");
    },
  };
  const view = render(
    createElement(RoutingDecisionPanel, { client, channel, message }),
  );
  await act(async () => {});
  for (const ms of [3000, 6000, 12000, 24000])
    await act(async () => t.mock.timers.tick(ms));
  assert.equal(calls, 5);
  await act(async () => t.mock.timers.tick(100000));
  assert.equal(calls, 5);
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Refresh routing" })),
  );
  assert.equal(calls, 6);
});

test("switching message or client aborts old ownership and ignores late private response", async () => {
  const { createElement } = await import("react");
  const { render, act } = await import("@testing-library/react");
  const { RoutingDecisionPanel } = await import("./RoutingDecisionDialog.tsx");
  let complete, signal;
  const client = {
    routingDecisionStream: async (_channel, _message, s, receive) => {
      signal = s;
      complete = receive;
      return new Promise((resolve) =>
        s.addEventListener("abort", resolve, { once: true }),
      );
    },
  };
  const view = render(
    createElement(RoutingDecisionPanel, { client, channel, message }),
  );
  await act(async () => {});
  const next = "b".repeat(64);
  const nextClient = liveClient(() => ({ ...page(next), decision: null }));
  view.rerender(
    createElement(RoutingDecisionPanel, {
      client: nextClient,
      channel,
      message: next,
    }),
  );
  await act(async () => {});
  assert.equal(signal.aborted, true);
  await act(async () => complete(page()));
  assert.equal(view.queryByText(/Visible employee/), null);
  assert.ok(view.getByText("No routing decision is recorded"));
});

test("one subscription pushes undecided to silence without a timer or second read", async () => {
  const { createElement } = await import("react");
  const { render, act } = await import("@testing-library/react");
  const { RoutingDecisionPanel } = await import("./RoutingDecisionDialog.tsx");
  const client = liveClient(() => ({
    message_id: message,
    channel_id: channel,
    decision: null,
  }));
  const view = render(
    createElement(RoutingDecisionPanel, { client, channel, message }),
  );
  await act(async () => {});
  assert.ok(view.getByText("No routing decision is recorded"));
  await act(async () => client.calls[0].receive(page()));
  assert.ok(view.getByText("No employee was dispatched by this decision."));
  assert.equal(client.calls.length, 1);
});

test("partial frames never reset the bounded failed-connection budget", async (t) => {
  const { createElement } = await import("react");
  const { render, act } = await import("@testing-library/react");
  const { RoutingDecisionPanel } = await import("./RoutingDecisionDialog.tsx");
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let calls = 0;
  const client = {
    routingDecisionStream: async (_c, _m, _s, receive) => {
      calls++;
      receive(page());
      throw new Error("lost transport");
    },
  };
  const view = render(
    createElement(RoutingDecisionPanel, { client, channel, message }),
  );
  await act(async () => {});
  for (const ms of [3000, 6000, 12000, 24000])
    await act(async () => t.mock.timers.tick(ms));
  await act(async () => t.mock.timers.tick(100000));
  assert.equal(calls, 5);
  assert.equal(view.queryByText(/Visible employee/), null);
});

test("normal renewal reconnects once with a fresh snapshot and fences late old callbacks", async (t) => {
  const { createElement } = await import("react");
  const { render, act } = await import("@testing-library/react");
  const { RoutingDecisionPanel } = await import("./RoutingDecisionDialog.tsx");
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let value = page();
  const client = liveClient(() => value);
  const view = render(
    createElement(RoutingDecisionPanel, { client, channel, message }),
  );
  await act(async () => {});
  const first = client.calls[0];
  await act(async () => first.resolve());
  assert.equal(first.signal.aborted, true);
  value = { ...page(), decision: null };
  await act(async () => t.mock.timers.tick(1000));
  assert.equal(client.calls.length, 2);
  await act(async () => first.receive(page()));
  assert.ok(view.getByText("No routing decision is recorded"));
  assert.equal(view.queryByText(/Visible employee/), null);
});
