import assert from "node:assert/strict";
import { test } from "node:test";
import { createOrtakClient, OrtakApiError } from "../client.ts";

const message = "a".repeat(64),
  channel = "channel/+";
const page = { message_id: message, channel_id: channel, decision: null };
const frame = (event, value) =>
  `event: ${event}\ndata: ${JSON.stringify(value)}\n\n`;
function response(text, cleanup = () => {}) {
  const bytes = new TextEncoder().encode(text);
  return new Response(
    new ReadableStream({
      start(controller) {
        // Real decoder sees a split UTF8/SSE boundary rather than a parsed fake.
        controller.enqueue(bytes.slice(0, 13));
        controller.enqueue(bytes.slice(13));
      },
      cancel: cleanup,
    }),
    { headers: { "content-type": "text/event-stream" } },
  );
}

test("signed routing SSE has exact URL, no cursor or payload, snapshot before renewal and transport cleanup", async () => {
  const signed = [],
    calls = [],
    seen = [];
  let cancelled = 0;
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async (event) => {
      signed.push(event);
      return {};
    },
    async (url, init) => {
      calls.push({ url, init });
      return response(
        frame("routing", page) +
          frame("heartbeat", {}) +
          frame("control", { code: "renew" }),
        () => cancelled++,
      );
    },
  );
  await client.routingDecisionStream(
    channel,
    message,
    new AbortController().signal,
    (value) => seen.push(value),
  );
  assert.equal(
    calls[0].url,
    `http://127.0.0.1:3010/api/v1/channels/channel%2F%2B/messages/${message}/routing/stream`,
  );
  assert.deepEqual(
    signed[0].tags.find((tag) => tag[0] === "u"),
    ["u", calls[0].url],
  );
  assert.equal(calls[0].init.method, "GET");
  assert.equal(calls[0].init.body, undefined);
  assert.equal(calls[0].init.cache, "no-store");
  assert.deepEqual(seen, [page]);
  assert.equal(calls[0].init.signal.aborted, true);
  assert.equal(cancelled, 1);
});

test("normal renewal cancels an abort-aware fetch body before aborting its transport", async () => {
  const steps = [],
    seen = [];
  let fetchSignal;
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async () => ({}),
    async (_url, init) => {
      fetchSignal = init.signal;
      return new Response(
        new ReadableStream({
          start(controller) {
            // Match fetch: abort makes an open response body errored, so cancel()
            // would reject without invoking the underlying cancel callback.
            init.signal.addEventListener(
              "abort",
              () => {
                steps.push("abort");
                controller.error(init.signal.reason);
              },
              { once: true },
            );
            controller.enqueue(
              new TextEncoder().encode(
                frame("routing", page) + frame("control", { code: "renew" }),
              ),
            );
          },
          async cancel() {
            steps.push("cancel");
            assert.equal(init.signal.aborted, false);
            await Promise.resolve();
            assert.equal(init.signal.aborted, false);
            steps.push("cancelled");
          },
        }),
        { headers: { "content-type": "text/event-stream" } },
      );
    },
  );
  await client.routingDecisionStream(
    channel,
    message,
    new AbortController().signal,
    (value) => seen.push(value),
  );
  assert.deepEqual(seen, [page]);
  assert.deepEqual(steps, ["cancel", "cancelled", "abort"]);
  assert.equal(fetchSignal.aborted, true);
});

test("foreign snapshot, absent decision field and invented history IDs never reach the consumer", async () => {
  for (const wire of [
    frame("routing", { ...page, message_id: "b".repeat(64) }),
    frame("routing", { ...page, channel_id: "foreign" }),
    frame("routing", { channel_id: channel, message_id: message }),
    `event: routing\nid: 1\ndata: ${JSON.stringify(page)}\n\n`,
  ]) {
    let cancelled = 0;
    const client = createOrtakClient(
      "http://127.0.0.1:3010",
      async () => ({}),
      async () => response(wire, () => cancelled++),
    );
    await assert.rejects(
      client.routingDecisionStream(
        channel,
        message,
        new AbortController().signal,
        () => assert.fail("invalid frame rendered"),
      ),
    );
    assert.equal(cancelled, 1);
  }
});

test("revoked control remains authoritative even when body cleanup fails", async () => {
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async () => ({}),
    async () =>
      response(frame("control", { code: "revoked" }), () => {
        throw new Error("cleanup failed");
      }),
  );
  await assert.rejects(
    client.routingDecisionStream(
      channel,
      message,
      new AbortController().signal,
      () => assert.fail("no private payload"),
    ),
    (cause) => cause instanceof OrtakApiError && cause.status === 403,
  );
});

test("routing bounds an incomplete frame and aborts its actual fetch signal", async () => {
  let signal;
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async () => ({}),
    async (_url, init) => {
      signal = init.signal;
      return response(`data: ${"x".repeat(70_000)}`);
    },
  );
  await assert.rejects(
    client.routingDecisionStream(
      channel,
      message,
      new AbortController().signal,
      () => assert.fail("oversized frame rendered"),
    ),
    /display limit/,
  );
  assert.equal(signal.aborted, true);
});

test("abort cancels a pending read and bounded cleanup cannot strand the subscription", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let fetchSignal,
    cancelled = false;
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async () => ({}),
    async (_url, init) => {
      fetchSignal = init.signal;
      return new Response(
        new ReadableStream({
          cancel() {
            cancelled = true;
            return new Promise(() => {});
          },
        }),
        { headers: { "content-type": "text/event-stream" } },
      );
    },
  );
  const owner = new AbortController();
  const pending = client.routingDecisionStream(
    channel,
    message,
    owner.signal,
    () => assert.fail("aborted payload"),
  );
  const rejected = assert.rejects(pending);
  for (let i = 0; i < 8; i++) await Promise.resolve();
  owner.abort();
  for (let i = 0; i < 8; i++) await Promise.resolve();
  t.mock.timers.tick(1000);
  await rejected;
  assert.equal(fetchSignal.aborted, true);
  assert.equal(cancelled, true);
});
