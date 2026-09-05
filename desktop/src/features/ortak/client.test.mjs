import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";
import { createOrtakClient, OrtakApiError } from "./client.ts";
import { resolveOrtakOrigin } from "./config.ts";

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
