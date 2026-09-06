import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";
import { createOrtakClient, OrtakApiError } from "./client.ts";
import { isConfiguredOrtakRelay, resolveOrtakOrigin } from "./config.ts";

test("the production client signs the exact fetched URL and cancellation payload with a fresh nonce", async () => {
  const signed = [];
  const fetched = [];
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async (input) => {
      signed.push(input);
      return { ...input, id: "test-signature-seam" };
    },
    async (url, init) => {
      fetched.push({ url, init });
      return Response.json({});
    },
  );
  const signal = new AbortController().signal;
  await client.runs(signal, "next+/=");
  await client.cancel("run-id", signal);
  await client.cancel("run-id", signal);
  assert.equal(
    fetched[0].url,
    "http://127.0.0.1:3010/api/v1/runs?limit=25&cursor=next%2B%2F%3D",
  );
  for (let index = 0; index < fetched.length; index++) {
    const { url, init } = fetched[index];
    const event = signed[index];
    const tags = Object.fromEntries(event.tags);
    assert.equal(event.kind, 27235);
    assert.equal(tags.u, url);
    assert.equal(tags.method, init.method);
    assert.equal(init.credentials, "omit");
    assert.equal(init.redirect, "error");
    assert.equal(init.cache, "no-store");
    assert.equal(
      JSON.parse(atob(init.headers.Authorization.slice(6))).id,
      "test-signature-seam",
    );
    if (init.method === "POST") {
      assert.equal(init.body, "{}");
      assert.equal(
        tags.payload,
        createHash("sha256").update(init.body).digest("hex"),
      );
      assert.equal(init.headers["Content-Type"], "application/json");
      assert.deepEqual(JSON.parse(init.body), {});
    } else assert.equal(tags.payload, undefined);
  }
  assert.notEqual(
    Object.fromEntries(signed[1].tags).nonce,
    Object.fromEntries(signed[2].tags).nonce,
  );
});

test("a late native signature cannot send an old-company request after abort", async () => {
  let finishSigning;
  let calls = 0;
  const controller = new AbortController();
  const client = createOrtakClient(
    "https://api.example.test",
    () =>
      new Promise((resolve) => {
        finishSigning = resolve;
      }),
    async () => {
      calls++;
      return Response.json({});
    },
  );
  const pending = client.detail("run-id", controller.signal);
  controller.abort();
  finishSigning({});
  await assert.rejects(pending, { name: "AbortError" });
  assert.equal(calls, 0);
});

test("authorization failures remain typed even with an intermediary HTML body", async () => {
  const client = createOrtakClient(
    "https://api.example.test",
    async () => ({}),
    async () => new Response("<html>Denied</html>", { status: 403 }),
  );
  await assert.rejects(
    client.detail("run-id", new AbortController().signal),
    (error) => error instanceof OrtakApiError && error.status === 403,
  );
});

test("HTTP authorization rejection survives response cancellation failure", async () => {
  for (const status of [401, 403, 404]) {
    let cancelled = false;
    const client = createOrtakClient(
      "https://api.example.test",
      async () => ({}),
      async () =>
        new Response(
          new ReadableStream({
            cancel() {
              cancelled = true;
              throw new Error("Transport cleanup failed");
            },
          }),
          { status },
        ),
    );
    await assert.rejects(
      client.activityStream("run-id", null, new AbortController().signal, () =>
        assert.fail("denied activity"),
      ),
      (error) => error instanceof OrtakApiError && error.status === status,
    );
    assert.equal(cancelled, true);
  }
});

test("responses are bounded before JSON decoding", async () => {
  const client = createOrtakClient(
    "https://api.example.test",
    async () => ({}),
    async () => new Response(new Uint8Array(8 * 1024 * 1024 + 1)),
  );
  await assert.rejects(
    client.detail("run-id", new AbortController().signal),
    /display limit/,
  );
});

test("private relay auto-connect requires the exact explicit origin without URL credentials or routing overrides", () => {
  const bindings = JSON.stringify({
    "http://localhost:3038": "http://127.0.0.1:8787",
  });
  assert.equal(isConfiguredOrtakRelay("ws://localhost:3038", bindings), true);
  assert.equal(isConfiguredOrtakRelay("ws://localhost:3038/", bindings), true);
  for (const relay of [
    "ws://localhost:3000",
    "ws://127.0.0.1:3038",
    "ws://user:password@localhost:3038",
    "ws://localhost:3038/other",
    "ws://localhost:3038?company=other",
    "ws://localhost:3038#other",
    "http://localhost:3038",
    "not a relay",
  ])
    assert.equal(isConfiguredOrtakRelay(relay, bindings), false, relay);
  for (const invalid of [undefined, "{}", "null", "[]", "malformed"])
    assert.equal(isConfiguredOrtakRelay("ws://localhost:3038", invalid), false);
});

test("the desktop enables only the exact configured relay origin and a canonical secure API origin", () => {
  const config = JSON.stringify({
    "https://office.example.test": "https://api.example.test",
    "http://localhost:3000": "http://127.0.0.1:3010",
  });
  assert.equal(
    resolveOrtakOrigin(config, "https://office.example.test/channels"),
    "https://api.example.test",
  );
  assert.equal(resolveOrtakOrigin(config, "https://other.example.test"), null);
  assert.equal(
    resolveOrtakOrigin(config, "http://localhost:3000"),
    "http://127.0.0.1:3010",
  );
  for (const invalid of [
    "http://public.example.test",
    "https://api.example.test/",
    "https://user:secret@api.example.test",
    "https://api.example.test/path",
    "https://api.example.test?company=other",
  ]) {
    assert.equal(
      resolveOrtakOrigin(
        JSON.stringify({ "https://office.example.test": invalid }),
        "https://office.example.test",
      ),
      null,
    );
  }
  for (const invalid of [undefined, "{", "null", "[]", "0"])
    assert.equal(
      resolveOrtakOrigin(invalid, "https://office.example.test"),
      null,
    );
});

function sseFrame(sequence = 0, run = "run-id") {
  return `event: activity\nid: ${sequence}\ndata: ${JSON.stringify({ detail: { detail: { run: { run_id: run } } }, page: { entries: [{ sequence }], next_after_sequence: sequence, has_more: false, gap: null } })}\n\n`;
}
const sseResponse = (stream) =>
  new Response(stream, { headers: { "content-type": "text/event-stream" } });

test("production stream signs its resume cursor and decodes split frames without polling", async () => {
  const frames = [];
  let requests = 0;
  let cancelled = false;
  const encoder = new TextEncoder();
  const bytes = encoder.encode(
    sseFrame(3).replaceAll("\n", "\r\n") +
      'event: heartbeat\ndata: {}\n\nevent: control\ndata: {"code":"renew"}\n\n',
  );
  const signed = [];
  const client = createOrtakClient(
    "https://api.example.test",
    async (event) => {
      signed.push(event);
      return event;
    },
    async (url, init) => {
      requests++;
      assert.equal(
        url,
        "https://api.example.test/api/v1/runs/run-id/stream?after_sequence=2",
      );
      assert.equal(Object.fromEntries(signed.at(-1).tags).u, url);
      assert.equal(init.method, "GET");
      let offset = 0;
      return sseResponse(
        new ReadableStream({
          pull(controller) {
            controller.enqueue(bytes.slice(offset, offset + 7));
            offset += 7;
          },
          cancel() {
            cancelled = true;
          },
        }),
      );
    },
  );
  await client.activityStream(
    "run-id",
    2,
    new AbortController().signal,
    (frame) => frames.push(frame),
  );
  assert.equal(requests, 1);
  assert.equal(frames.length, 1);
  assert.equal(frames[0].page.next_after_sequence, 3);
  assert.equal(cancelled, true);
});

test("live authorization control remains typed and cannot deliver a later queued frame", async () => {
  let received = 0;
  const client = createOrtakClient(
    "https://api.example.test",
    async () => ({}),
    async () =>
      sseResponse(`event: control\ndata: {"code":"revoked"}\n\n${sseFrame()}`),
  );
  await assert.rejects(
    client.activityStream(
      "run-id",
      null,
      new AbortController().signal,
      () => received++,
    ),
    (error) => error instanceof OrtakApiError && error.status === 403,
  );
  assert.equal(received, 0);
});

test("authoritative stream control survives cancellation failure and releases its reader", async () => {
  for (const [code, status] of [
    ["revoked", 403],
    ["resync", 409],
  ]) {
    let received = 0;
    let cancelled = false;
    const response = sseResponse(
      new ReadableStream({
        start(controller) {
          controller.enqueue(
            new TextEncoder().encode(
              `event: control\ndata: {"code":"${code}"}\n\n${sseFrame()}`,
            ),
          );
        },
        cancel() {
          cancelled = true;
          return Promise.reject(new Error("Transport cleanup failed"));
        },
      }),
    );
    const client = createOrtakClient(
      "https://api.example.test",
      async () => ({}),
      async () => response,
    );
    await assert.rejects(
      client.activityStream(
        "run-id",
        null,
        new AbortController().signal,
        () => received++,
      ),
      (error) => error instanceof OrtakApiError && error.status === status,
    );
    assert.equal(received, 0);
    assert.equal(cancelled, true);
    assert.equal(response.body.locked, false);
  }
});

test("cleanup failure without authorization control remains a disconnect and releases its reader", async () => {
  const response = sseResponse(
    new ReadableStream({
      start(controller) {
        controller.enqueue(
          new TextEncoder().encode(
            'event: control\ndata: {"code":"renew"}\n\n',
          ),
        );
      },
      cancel() {
        throw new Error("Transport cleanup failed");
      },
    }),
  );
  const client = createOrtakClient(
    "https://api.example.test",
    async () => ({}),
    async () => response,
  );
  await assert.rejects(
    client.activityStream("run-id", null, new AbortController().signal, () =>
      assert.fail("unexpected activity"),
    ),
    /Transport cleanup failed/,
  );
  assert.equal(response.body.locked, false);
});

test("invalid stream frames preserve their failure when cleanup also fails", async () => {
  const response = sseResponse(
    new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(sseFrame(0, "foreign")));
      },
      cancel() {
        throw new Error("Transport cleanup failed");
      },
    }),
  );
  const client = createOrtakClient(
    "https://api.example.test",
    async () => ({}),
    async () => response,
  );
  await assert.rejects(
    client.activityStream("run-id", null, new AbortController().signal, () =>
      assert.fail("foreign activity must not render"),
    ),
    /inconsistent/,
  );
  assert.equal(response.body.locked, false);
});

test("stream frames reject a foreign run, inconsistent cursor, oversized payload and unexpected EOF", async () => {
  for (const [body, expected] of [
    [sseFrame(0, "foreign"), /inconsistent/],
    [sseFrame().replace("id: 0", "id: 2"), /inconsistent/],
    [`data: ${"x".repeat(4 * 1024 * 1024 + 65_537)}`, /display limit/],
    ["", /closed/],
  ]) {
    const client = createOrtakClient(
      "https://api.example.test",
      async () => ({}),
      async () => sseResponse(body),
    );
    await assert.rejects(
      client.activityStream("run-id", null, new AbortController().signal, () =>
        assert.fail("invalid frame must not render"),
      ),
      expected,
    );
  }
});

test("abort cancels a pending body reader and forbids a late frame", async () => {
  let cancelled = false;
  let received = false;
  let opened;
  const ready = new Promise((resolve) => {
    opened = resolve;
  });
  const client = createOrtakClient(
    "https://api.example.test",
    async () => ({}),
    async () =>
      sseResponse(
        new ReadableStream({
          start() {
            opened();
          },
          cancel() {
            cancelled = true;
          },
        }),
      ),
  );
  const controller = new AbortController();
  const pending = client.activityStream(
    "run-id",
    null,
    controller.signal,
    () => {
      received = true;
    },
  );
  await ready;
  await new Promise((resolve) => setTimeout(resolve, 0));
  controller.abort();
  await assert.rejects(pending, { name: "AbortError" });
  assert.equal(cancelled, true);
  assert.equal(received, false);
});

test("Work artifact client signs the scoped path, bounds UTF-8 bytes, and releases readers when cleanup fails", async () => {
  let fetched;
  const client = createOrtakClient(
    "https://api.example.test",
    async () => ({}),
    async (url) => {
      fetched = url;
      return new Response("Complete text");
    },
  );
  assert.equal(
    await client.textArtifact(
      "work/a",
      "artifact/b",
      new AbortController().signal,
    ),
    "Complete text",
  );
  assert.equal(
    fetched,
    "https://api.example.test/api/v1/work-items/work%2Fa/artifacts/artifact%2Fb",
  );
  for (const text of ["x".repeat(32769), "é".repeat(16385)]) {
    const oversized = createOrtakClient(
      "https://api.example.test",
      async () => ({}),
      async () => new Response(text),
    );
    await assert.rejects(
      oversized.textArtifact("w", "a", new AbortController().signal),
      /display limit/,
    );
  }
  for (const readFails of [false, true]) {
    let released = false;
    const operation = new Error("original read failure");
    const cleanup = new Error("reader cancellation failed");
    const response = {
      ok: true,
      body: {
        getReader: () => ({
          read: async () => {
            if (readFails) throw operation;
            return { done: true };
          },
          cancel: async () => {
            throw cleanup;
          },
          releaseLock: () => {
            released = true;
          },
        }),
      },
    };
    const broken = createOrtakClient(
      "https://api.example.test",
      async () => ({}),
      async () => response,
    );
    await assert.rejects(
      broken.textArtifact("w", "a", new AbortController().signal),
      (error) => error === (readFails ? operation : cleanup),
    );
    assert.equal(released, true);
  }
});
