import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import {
  closeSync,
  constants,
  fstatSync,
  lstatSync,
  openSync,
  readFileSync,
} from "node:fs";
import { finalizeEvent, getPublicKey } from "nostr-tools";

// Explicit first-run check against the disposable private stack. No existing
// desktop identity is read and no signed authorization is printed or saved.
const root = "/private/tmp/ortak-private-20260905";
const origin = "http://127.0.0.1:8787";
const metadata = lstatSync(root);
assert(
  metadata.isDirectory() &&
    metadata.uid === process.getuid() &&
    !(metadata.mode & 0o077),
);
function privateJson(name) {
  const descriptor = openSync(
    `${root}/${name}`,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
  );
  try {
    const info = fstatSync(descriptor);
    assert(
      info.isFile() &&
        info.uid === process.getuid() &&
        !(info.mode & 0o077) &&
        info.size < 65536,
    );
    return JSON.parse(readFileSync(descriptor, "utf8"));
  } finally {
    closeSync(descriptor);
  }
}
assert.deepEqual(privateJson(".ortak-private-stack.json"), {
  project: "ortak-private-20260905",
  state_directory: root,
});
const identity = privateJson("identities.json");
assert.equal(identity.project, "ortak-private-20260905");
assert.match(identity.owner.secret_key, /^[0-9a-f]{64}$/);
const secret = Uint8Array.from(Buffer.from(identity.owner.secret_key, "hex"));
assert.equal(getPublicKey(secret), identity.owner.public_key);
function authorization(path) {
  const event = finalizeEvent(
    {
      kind: 27235,
      content: "",
      created_at: Math.floor(Date.now() / 1000),
      tags: [
        ["u", origin + path],
        ["method", "GET"],
        ["nonce", randomUUID()],
      ],
    },
    secret,
  );
  return `Nostr ${Buffer.from(JSON.stringify(event)).toString("base64")}`;
}
async function request(path, headers = {}, method = "GET") {
  const response = await fetch(origin + path, {
    method,
    headers,
    redirect: "error",
    signal: AbortSignal.timeout(5000),
  });
  const chunks = [];
  let bytes = 0;
  for await (const chunk of response.body) {
    bytes += chunk.length;
    assert(bytes <= 65536, "API response must remain bounded");
    chunks.push(chunk);
  }
  const body = Buffer.concat(chunks).toString("utf8");
  return {
    status: response.status,
    headers: response.headers,
    body: body ? JSON.parse(body) : null,
  };
}
try {
  const path = "/api/v1/employees";
  assert.equal((await request(path)).status, 401);
  const auth = authorization(path);
  const list = await request(path, { Authorization: auth });
  assert.equal(list.status, 200);
  assert.equal(list.headers.get("cache-control"), "no-store");
  assert.equal(list.body.employees.length, 1);
  assert.equal(list.body.employees[0].employee_id, "ada-private");
  assert.equal(list.body.employees[0].status, "draft");
  assert.equal(list.body.employees[0].active_revision_id, null);
  const replay = await request(path, { Authorization: auth });
  assert.equal(replay.status, 401);
  assert.equal(replay.body.error.code, "authentication_replayed");
  const detailPath = `${path}/ada-private`;
  const detail = await request(detailPath, {
    Authorization: authorization(detailPath),
  });
  assert.equal(detail.status, 200);
  assert.equal(detail.body.runtime_health, "not_probed");
  assert.equal(detail.body.current_run, null);
  const queuePath = `${detailPath}/work-items?limit=25`;
  const queue = await request(queuePath, {
    Authorization: authorization(queuePath),
  });
  assert.equal(queue.status, 200);
  assert.equal(queue.headers.get("cache-control"), "no-store");
  assert.deepEqual(queue.body, {
    employee_id: "ada-private",
    work_items: [],
    next_cursor: null,
    execution_available: false,
  });
  const forbiddenPath = `${path}/ungranted-employee`;
  assert.equal(
    (
      await request(forbiddenPath, {
        Authorization: authorization(forbiddenPath),
      })
    ).status,
    404,
  );
  const forbiddenQueuePath = `${forbiddenPath}/work-items`;
  assert.equal(
    (
      await request(forbiddenQueuePath, {
        Authorization: authorization(forbiddenQueuePath),
      })
    ).status,
    404,
  );
  assert.equal(
    (await request(path, { Authorization: authorization(detailPath) })).status,
    401,
  );
  const preflight = await request(
    path,
    {
      Origin: "http://localhost:1427",
      "Access-Control-Request-Method": "GET",
      "Access-Control-Request-Headers": "authorization",
    },
    "OPTIONS",
  );
  assert(preflight.status >= 200 && preflight.status < 300);
  assert.equal(
    preflight.headers.get("access-control-allow-origin"),
    "http://localhost:1427",
  );
  console.log(
    JSON.stringify({
      result: "passed",
      checks: [
        "signed_company_directory",
        "draft_not_active",
        "runtime_health_honest",
        "manual_employee_queue",
        "queue_employee_audience",
        "nip98_replay",
        "exact_signed_url",
        "employee_audience",
        "private_cors",
      ],
      provider_calls: 0,
    }),
  );
} finally {
  secret.fill(0);
}
