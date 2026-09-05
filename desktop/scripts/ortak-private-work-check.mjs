import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import {
  closeSync,
  constants,
  fstatSync,
  lstatSync,
  openSync,
  readSync,
  realpathSync,
} from "node:fs";
import { request as httpRequest } from "node:http";
import { pathToFileURL } from "node:url";

// Explicit manual verification creates permanent, clearly labeled test records.
// It neither executes employees nor repairs/replaces records changed by others.
const ROOT = "/private/tmp/ortak-private-20260905";
const STACK = "ortak-private-20260905";
const ORIGIN = "http://127.0.0.1:8787";
const SLUG = "private-manual-work-check-v1";
const NAME = "Private manual Work verification v1";
const DESCRIPTION =
  "Permanent private API verification record; manual only, no employee execution.";
const TITLE = "Manual acceptance and human approval verification v1";
const CRITERION =
  "Verify the manual API preserves acceptance and approval gates.";
const GATE = "private_manual_human_review";
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

function validUuid(value) {
  assert(
    typeof value === "string" &&
      UUID.test(value) &&
      !/^0{8}-0{4}-0{4}-0{4}-0{12}$/.test(value),
  );
  return value;
}

function privateJson(name) {
  const fd = openSync(
    `${ROOT}/${name}`,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
  );
  const buffer = Buffer.alloc(65537);
  try {
    const info = fstatSync(fd);
    assert(
      info.isFile() &&
        info.uid === process.getuid() &&
        !(info.mode & 0o077) &&
        info.size <= 65536,
    );
    let size = 0;
    while (size < buffer.length) {
      const read = readSync(fd, buffer, size, buffer.length - size, null);
      if (!read) break;
      size += read;
    }
    assert(size <= 65536);
    return JSON.parse(buffer.subarray(0, size).toString("utf8"));
  } finally {
    buffer.fill(0);
    closeSync(fd);
  }
}

/** Deterministic operation UUIDs never change to get around a conflicting receipt. */
export function operationId(scope, label) {
  const digest = createHash("sha256")
    .update(JSON.stringify([STACK, SLUG, scope, label]))
    .digest();
  digest[6] = (digest[6] & 0x0f) | 0x80;
  digest[8] = (digest[8] & 0x3f) | 0x80;
  const hex = digest.subarray(0, 16).toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** Fixed direct loopback transport; no proxies, redirects, cookies or retries. */
export function requester(
  secret,
  finalizeEvent,
  requestImpl = httpRequest,
  now = () => performance.now(),
) {
  const deadline = now() + 120000;
  let count = 0;
  return async (method, path, body) => {
    assert(++count <= 48 && now() < deadline);
    assert(
      ["GET", "POST"].includes(method) &&
        /^\/api\/v1\/[a-z0-9/?=_-]+$/.test(path),
    );
    const payload = body === undefined ? "" : JSON.stringify(body);
    assert(Buffer.byteLength(payload) <= 16384);
    const tags = [
      ["u", ORIGIN + path],
      ["method", method],
      ["nonce", randomUUID()],
    ];
    if (method === "POST")
      tags.push([
        "payload",
        createHash("sha256").update(payload).digest("hex"),
      ]);
    const event = finalizeEvent(
      {
        kind: 27235,
        content: "",
        created_at: Math.floor(Date.now() / 1000),
        tags,
      },
      secret,
    );
    const authorization = `Nostr ${Buffer.from(JSON.stringify(event)).toString("base64")}`;
    const requestDeadline = Math.min(deadline, now() + 5000);
    return new Promise((resolve, reject) => {
      let response;
      let timer;
      let settled = false;
      const fail = () => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        response?.destroy();
        req.destroy();
        reject(new Error("private_work_transport_failed"));
      };
      const req = requestImpl(
        {
          hostname: "127.0.0.1",
          port: 8787,
          path,
          method,
          agent: false,
          maxHeaderSize: 16384,
          headers: {
            Authorization: authorization,
            "Content-Type": "application/json",
            "Content-Length": Buffer.byteLength(payload),
          },
        },
        (incoming) => {
          response = incoming;
          const chunks = [];
          let bytes = 0;
          incoming.on("data", (chunk) => {
            if (settled) return;
            bytes += chunk.length;
            if (bytes > 262144) {
              fail();
              return;
            }
            chunks.push(chunk);
          });
          incoming.on("error", fail);
          incoming.on("aborted", fail);
          incoming.on("end", () => {
            if (settled) return;
            clearTimeout(timer);
            try {
              assert(
                now() < requestDeadline &&
                  incoming.headers["cache-control"] === "no-store",
              );
              const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
              settled = true;
              resolve({ status: incoming.statusCode, body });
            } catch {
              fail();
            }
          });
        },
      );
      req.on("error", fail);
      timer = setTimeout(fail, Math.max(1, requestDeadline - now()));
      req.end(payload);
    });
  };
}

const STATES = [
  "proposed",
  "ready",
  "in_progress",
  "review",
  "review",
  "review",
  "completed",
];
const EVENTS = [
  "work.created",
  "work.state_changed",
  "work.state_changed",
  "work.state_changed",
  "work.criterion_satisfied",
  "work.approval_resolved",
  "work.state_changed",
];

function validateItem(item, project, owner, expectedId) {
  assert(
    item &&
      Number.isSafeInteger(item.version) &&
      item.version >= 1 &&
      item.version <= 7,
  );
  validUuid(item.id);
  if (expectedId) assert.equal(item.id, expectedId);
  assert.equal(item.project_id, project);
  assert.equal(item.title, TITLE);
  assert.equal(item.description, DESCRIPTION);
  assert.equal(item.source_message_id, null);
  assert.equal(item.execution_available, false);
  assert.equal(item.history_omitted, false);
  assert.equal(item.history_truncated, false);
  assert.deepEqual(item.assignments, []);
  assert.deepEqual(item.created_by, { type: "human", public_key: owner });
  for (const key of [
    "attachments",
    "dependencies",
    "run_id",
    "runtime_run_ref",
    "source_decision_id",
  ])
    assert(!(key in item));
  assert.equal(item.state, STATES[item.version - 1]);
  assert.equal(item.criteria.length, 1);
  assert.equal(item.approvals.length, 1);
  const criterion = item.criteria[0],
    approval = item.approvals[0];
  validUuid(criterion.id);
  validUuid(approval.id);
  assert.equal(criterion.position, 0);
  assert.equal(criterion.text, CRITERION);
  assert.equal(criterion.status, item.version >= 5 ? "satisfied" : "pending");
  assert.equal(approval.gate, GATE);
  assert.equal(approval.required, true);
  assert.equal(approval.status, item.version >= 6 ? "approved" : "pending");
  assert.equal(item.history.length, item.version);
  item.history.forEach((entry, index) => {
    assert.equal(entry.sequence, index);
    assert.equal(entry.version, index + 1);
    assert.equal(entry.event_type, EVENTS[index]);
    assert.deepEqual(entry.actor, { type: "human", public_key: owner });
    assert(!("payload" in entry));
    if (entry.event_type === "work.state_changed") {
      assert.equal(entry.from, STATES[index - 1]);
      assert.equal(entry.to, STATES[index]);
    }
  });
  return item;
}

/** Run the real request sequence; injecting transport permits provider-free recovery tests. */
export async function verifyWork(
  request,
  { company, community, channel, owner },
) {
  [company, community, channel].forEach(validUuid);
  assert(typeof owner === "string" && /^[0-9a-f]{64}$/.test(owner));
  const scope = [company, community, channel, owner];
  const op = (label) => operationId(scope, label);
  const checks = [];
  const employee = await request("GET", "/api/v1/employees/ada-private");
  assert.equal(employee.status, 200);
  assert.equal(employee.body.employee.employee_id, "ada-private");
  assert.equal(employee.body.employee.status, "draft");
  assert.equal(employee.body.employee.active_revision_id, null);
  assert.equal(employee.body.current_run, null);
  checks.push("draft_employee_precondition");
  const capabilities = await request("GET", "/api/v1/projects");
  assert.equal(capabilities.status, 200);
  assert.equal(capabilities.body.can_create_projects, true);
  assert(
    capabilities.body.create_channels.some((entry) => entry.id === channel),
  );
  const projectBody = {
    operation_id: op("project"),
    channel_id: channel,
    project: { slug: SLUG, name: NAME, description: DESCRIPTION },
  };
  const created = await request("POST", "/api/v1/projects", projectBody);
  assert([200, 201].includes(created.status));
  const project = validUuid(created.body.project.id);
  assert.equal(created.body.project.slug, SLUG);
  assert.equal(created.body.project.name, NAME);
  assert.equal(created.body.project.description, DESCRIPTION);
  assert.equal(created.body.project.channel_id, channel);
  assert.equal(created.body.project.role, "owner");
  assert.equal(created.body.project.version, 1);
  assert.equal(created.body.project.can_contribute, true);
  assert.equal(created.body.project.can_review, true);
  const replay = await request("POST", "/api/v1/projects", projectBody);
  assert.equal(replay.status, 200);
  assert.equal(replay.body.created, false);
  assert.deepEqual(replay.body.project, created.body.project);
  assert.equal(
    (
      await request("POST", "/api/v1/projects", {
        ...projectBody,
        project: {
          ...projectBody.project,
          name: "Rejected conflicting manual check",
        },
      })
    ).status,
    409,
  );
  checks.push("project_create_replay_conflict");
  const listPath = `/api/v1/projects/${project}/work-items`;
  const itemBody = {
    operation_id: op("work"),
    title: TITLE,
    description: DESCRIPTION,
    criteria: [CRITERION],
    approvals: [{ gate: GATE, required: true }],
  };
  const creation = await request("POST", listPath, itemBody);
  assert([200, 201].includes(creation.status));
  let item = validateItem(creation.body.work_item, project, owner);
  const initialVersion = item.version;
  const id = item.id,
    criterion = item.criteria[0].id,
    approval = item.approvals[0].id;
  const path = `/api/v1/work-items/${id}`;
  const check = (result) => {
    assert.equal(result.status, 200);
    const next = validateItem(result.body.work_item, project, owner, id);
    assert.equal(next.criteria[0].id, criterion);
    assert.equal(next.approvals[0].id, approval);
    return next;
  };
  assert.deepEqual(check(await request("POST", listPath, itemBody)), item);
  assert.equal(
    (
      await request("POST", listPath, {
        ...itemBody,
        title: "Rejected conflicting work",
      })
    ).status,
    409,
  );
  checks.push("work_create_replay_conflict");
  const negative = {
    draft_assignment: "not_retested_already_advanced",
    completion_pending_criteria: "not_retested_already_advanced",
    completion_pending_approval: "not_retested_already_advanced",
  };
  if (item.version === 1) {
    assert.equal(
      (
        await request("POST", `${path}/assignments`, {
          operation_id: op("draft-assignment-refused"),
          expected_version: 1,
          employee_id: "ada-private",
          role: "owner",
        })
      ).status,
      409,
    );
    assert.deepEqual(check(await request("GET", path)), item);
    negative.draft_assignment = "observed_now";
  }
  const steps = [
    ["ready", "transitions", { target: "ready" }],
    ["in-progress", "transitions", { target: "in_progress" }],
    ["review", "transitions", { target: "review" }],
    ["criterion", `criteria/${criterion}/satisfy`, {}],
    ["approval", `approvals/${approval}/resolve`, { decision: "approve" }],
    ["completed", "transitions", { target: "completed" }],
  ];
  for (const [index, [label, suffix, value]] of steps.entries()) {
    const expected = index + 1;
    if (item.version === expected && [4, 5].includes(expected)) {
      const field =
        expected === 4
          ? "completion_pending_criteria"
          : "completion_pending_approval";
      assert.equal(
        (
          await request("POST", `${path}/transitions`, {
            operation_id: op(field),
            expected_version: expected,
            target: "completed",
          })
        ).status,
        409,
      );
      assert.deepEqual(check(await request("GET", path)), item);
      negative[field] = "observed_now";
    }
    assert(item.version >= expected);
    const command = {
      operation_id: op(label),
      expected_version: expected,
      ...value,
    };
    const previous = item;
    item = check(await request("POST", `${path}/${suffix}`, command));
    if (previous.version > expected) assert.deepEqual(item, previous);
    else assert.equal(item.version, expected + 1);
    assert.deepEqual(
      check(await request("POST", `${path}/${suffix}`, command)),
      item,
    );
  }
  assert.equal(item.version, 7);
  assert.equal(item.state, "completed");
  assert.deepEqual(check(await request("GET", path)), item);
  const list = await request("GET", listPath);
  assert.equal(list.status, 200);
  assert.equal(list.body.next_cursor, null);
  assert.equal(list.body.work_items.length, 1);
  assert.equal(list.body.work_items[0].id, id);
  assert.equal(list.body.work_items[0].state, "completed");
  checks.push(
    "manual_lifecycle",
    "mutation_receipt_replays",
    "dense_seven_row_history",
    "single_project_work_item",
    "manual_execution_unavailable",
    "no_raw_runtime_projection",
  );
  return {
    result: "passed",
    project_id: project,
    work_item_id: id,
    project_slug: SLUG,
    initial_version: initialVersion,
    final_version: 7,
    checks,
    negative_checks: negative,
    permanent_test_records: true,
    provider_calls: 0,
    runtime_database_counts: "not_checked",
  };
}

async function main() {
  let secret;
  try {
    assert.equal(process.argv.length, 2);
    const metadata = lstatSync(ROOT);
    assert(
      metadata.isDirectory() &&
        !metadata.isSymbolicLink() &&
        metadata.uid === process.getuid() &&
        !(metadata.mode & 0o077),
    );
    assert.equal(realpathSync(ROOT), ROOT);
    assert.deepEqual(privateJson(".ortak-private-stack.json"), {
      project: STACK,
      state_directory: ROOT,
    });
    const identity = privateJson("identities.json"),
      config = privateJson("api-config.json");
    assert.equal(identity.project, STACK);
    assert.equal(identity.employee_id, "ada-private");
    assert(
      typeof identity.owner.secret_key === "string" &&
        /^[0-9a-f]{64}$/.test(identity.owner.secret_key),
    );
    secret = Uint8Array.from(Buffer.from(identity.owner.secret_key, "hex"));
    delete identity.owner.secret_key;
    const { finalizeEvent, getPublicKey } = await import("nostr-tools");
    assert.equal(getPublicKey(secret), identity.owner.public_key);
    assert.equal(config.origin, ORIGIN);
    assert.equal(config.humans.length, 1);
    const human = config.humans[0];
    assert.equal(human.public_key, identity.owner.public_key);
    assert.equal(human.role, "operator");
    assert.equal(human.can_create_projects, true);
    assert.deepEqual(human.employee_ids, ["ada-private"]);
    assert.equal(human.channel_ids.length, 1);
    const result = await verifyWork(requester(secret, finalizeEvent), {
      company: identity.company_id,
      community: config.community_id,
      channel: human.channel_ids[0],
      owner: human.public_key,
    });
    console.log(JSON.stringify(result));
  } catch {
    // Never print exception messages/objects: they may contain signed headers,
    // response content or key material from failed library assertions.
    console.error(
      JSON.stringify({
        result: "failed",
        code: "private_manual_work_check_failed",
        retry: "same_script_same_operations",
        records_preserved: true,
      }),
    );
    process.exitCode = 1;
  } finally {
    secret?.fill(0);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href)
  await main();
