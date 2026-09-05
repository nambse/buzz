import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { EventEmitter } from "node:events";
import test from "node:test";
import {
  operationId,
  requester,
  verifyWork,
} from "./ortak-private-work-check.mjs";

const scope = {
  company: "6e3fb290-6d9d-4f24-baf2-fc595272465c",
  community: "f0624af3-c640-4e88-9248-bb1ff767ce73",
  channel: "a5f0f1c5-361f-4139-82c6-7c3d283bf2bb",
  owner: "a".repeat(64),
};
const projectId = "be59ea87-159c-4916-936a-bf006e3ef560";
const itemId = "d566ddfb-01ea-429c-8aaf-43d3da4acde0";
const criterionId = "d246fe37-fe23-4ca1-8540-8e393eb3d614";
const approvalId = "f1a36405-1985-44b8-9f4e-3901cfe098d4";
const actor = { type: "human", public_key: scope.owner };
const clone = (value) => structuredClone(value);

// Models immutable operation receipts and advanced current-state replay, with
// failures injected AFTER a response's mutation has committed in this fixture.
function fixture() {
  const receipts = new Map();
  const seen = [];
  let project, item;
  async function request(method, path, body) {
    seen.push([method, path, clone(body)]);
    if (method === "GET") {
      if (path.endsWith("employees/ada-private"))
        return {
          status: 200,
          body: {
            employee: {
              employee_id: "ada-private",
              status: "draft",
              active_revision_id: null,
            },
            current_run: null,
          },
        };
      if (path === "/api/v1/projects")
        return {
          status: 200,
          body: {
            can_create_projects: true,
            create_channels: [{ id: scope.channel }],
          },
        };
      if (path === `/api/v1/work-items/${itemId}`)
        return { status: 200, body: { work_item: clone(item) } };
      assert.equal(path, `/api/v1/projects/${projectId}/work-items`);
      return {
        status: 200,
        body: {
          work_items: [{ id: item.id, state: item.state }],
          next_cursor: null,
        },
      };
    }
    const encoded = JSON.stringify([path, body]),
      receipt = receipts.get(body.operation_id);
    if (receipt && receipt !== encoded) return { status: 409 };
    const response = (created) => ({
      status: created ? 201 : 200,
      body:
        path === "/api/v1/projects"
          ? { project: clone(project), created }
          : { work_item: clone(item), created },
    });
    if (receipt) return response(false);
    if (path === "/api/v1/projects") {
      assert(!project, "never create a replacement project");
      project = {
        id: projectId,
        ...body.project,
        channel_id: scope.channel,
        role: "owner",
        version: 1,
        can_contribute: true,
        can_review: true,
      };
      receipts.set(body.operation_id, encoded);
      return response(true);
    }
    if (path === `/api/v1/projects/${projectId}/work-items`) {
      assert(!item, "never create a replacement task");
      item = {
        id: itemId,
        project_id: projectId,
        title: body.title,
        description: body.description,
        source_message_id: null,
        execution_available: false,
        history_omitted: false,
        history_truncated: false,
        assignments: [],
        created_by: actor,
        state: "proposed",
        version: 1,
        criteria: [
          {
            id: criterionId,
            position: 0,
            text: body.criteria[0],
            status: "pending",
          },
        ],
        approvals: [
          { id: approvalId, ...body.approvals[0], status: "pending" },
        ],
        history: [
          { sequence: 0, version: 1, event_type: "work.created", actor },
        ],
      };
      receipts.set(body.operation_id, encoded);
      return response(true);
    }
    if (path.endsWith("/assignments")) return { status: 409 };
    if (body.expected_version !== item.version) return { status: 409 };
    let event;
    if (path.endsWith("/transitions")) {
      if (
        body.target === "completed" &&
        (item.criteria[0].status !== "satisfied" ||
          item.approvals[0].status !== "approved")
      )
        return { status: 409 };
      event = {
        event_type: "work.state_changed",
        from: item.state,
        to: body.target,
      };
      item.state = body.target;
    } else if (path.endsWith(`/criteria/${criterionId}/satisfy`)) {
      item.criteria[0].status = "satisfied";
      event = { event_type: "work.criterion_satisfied" };
    } else {
      assert(path.endsWith(`/approvals/${approvalId}/resolve`));
      assert.equal(body.decision, "approve");
      item.approvals[0].status = "approved";
      event = { event_type: "work.approval_resolved" };
    }
    item.version += 1;
    item.history.push({
      sequence: item.version - 1,
      version: item.version,
      ...event,
      actor,
    });
    receipts.set(body.operation_id, encoded);
    return response(false);
  }
  return { request, receipts, seen, item: () => item };
}

test("manual production sequence observes both acceptance guards and completed retry adds nothing", async () => {
  const f = fixture();
  const first = await verifyWork(f.request, scope);
  assert.equal(first.initial_version, 1);
  assert.deepEqual(Object.values(first.negative_checks), [
    "observed_now",
    "observed_now",
    "observed_now",
  ]);
  assert.equal(f.receipts.size, 8);
  const before = clone(f.item());
  const second = await verifyWork(f.request, scope);
  assert.equal(second.initial_version, 7);
  assert.deepEqual(
    Object.values(second.negative_checks),
    Array(3).fill("not_retested_already_advanced"),
  );
  assert.equal(f.receipts.size, 8);
  assert.deepEqual(f.item(), before);
  assert.equal(second.runtime_database_counts, "not_checked");
});

test("lost responses at every request boundary converge without new operation IDs or histories", async () => {
  const completed = fixture();
  await verifyWork(completed.request, scope);
  for (let boundary = 1; boundary <= completed.seen.length; boundary++) {
    const f = fixture();
    let count = 0;
    await assert.rejects(
      verifyWork(async (...args) => {
        const result = await f.request(...args);
        if (++count === boundary) throw new Error("fixture lost response");
        return result;
      }, scope),
    );
    assert.equal((await verifyWork(f.request, scope)).final_version, 7);
    assert.deepEqual(
      [...f.receipts.keys()].sort(),
      [...completed.receipts.keys()].sort(),
    );
    assert.deepEqual(f.item(), completed.item());
  }
});

test("altered existing records and receipt conflicts stop the sequence without replacements", async () => {
  const f = fixture();
  await verifyWork(f.request, scope);
  f.item().title = "User changed this record";
  const count = f.receipts.size;
  await assert.rejects(verifyWork(f.request, scope));
  assert.equal(f.receipts.size, count);
  assert.equal(f.item().title, "User changed this record");
  const g = fixture();
  await verifyWork(g.request, scope);
  const op = operationId(
    [scope.company, scope.community, scope.channel, scope.owner],
    "ready",
  );
  g.receipts.set(op, "conflicting original receipt");
  await assert.rejects(verifyWork(g.request, scope));
  assert.equal(g.receipts.size, 8);
});

function fakeHttp(deliver) {
  const calls = [];
  const impl = (options, onResponse) => {
    const req = new EventEmitter();
    req.destroy = () => {
      req.destroyed = true;
    };
    req.end = (payload) => {
      calls.push({ options, payload, req });
      queueMicrotask(() => {
        const incoming = new EventEmitter();
        incoming.destroy = () => {
          incoming.destroyed = true;
        };
        incoming.headers = { "cache-control": "no-store" };
        incoming.statusCode = 200;
        onResponse(incoming);
        deliver(incoming);
      });
    };
    return req;
  };
  return { impl, calls };
}

test("transport pins direct loopback and signs exact POST bytes with fresh auth on retry", async () => {
  const signatures = [];
  const http = fakeHttp((response) => {
    response.emit("data", Buffer.from("{}"));
    response.emit("end");
  });
  const request = requester(
    new Uint8Array(32),
    (event) => {
      signatures.push(event);
      return event;
    },
    http.impl,
  );
  const body = { operation_id: operationId([], "fixture"), title: "Unicode é" };
  for (let i = 0; i < 2; i++) await request("POST", "/api/v1/projects", body);
  const [{ options, payload }] = http.calls;
  assert.equal(options.hostname, "127.0.0.1");
  assert.equal(options.port, 8787);
  assert.equal(options.agent, false);
  assert.equal(options.maxHeaderSize, 16384);
  assert.equal(payload, JSON.stringify(body));
  assert.equal(options.headers["Content-Length"], Buffer.byteLength(payload));
  assert.deepEqual(signatures[0].tags.slice(0, 2), [
    ["u", "http://127.0.0.1:8787/api/v1/projects"],
    ["method", "POST"],
  ]);
  assert.deepEqual(signatures[0].tags[3], [
    "payload",
    createHash("sha256").update(payload).digest("hex"),
  ]);
  assert.notDeepEqual(signatures[0].tags[2], signatures[1].tags[2]);
  for (let i = 2; i < 48; i++) await request("GET", "/api/v1/projects");
  await assert.rejects(request("GET", "/api/v1/projects"));
  assert.equal(http.calls.length, 48);
});

test("oversized output and late synchronous completion fail and close the request", async () => {
  for (const late of [false, true]) {
    let now = 0;
    const http = fakeHttp((response) => {
      if (late) now = 5001;
      response.emit("data", late ? Buffer.from("{}") : Buffer.alloc(262145));
      response.emit("end");
    });
    const request = requester(
      new Uint8Array(32),
      (event) => event,
      http.impl,
      () => now,
    );
    await assert.rejects(
      request("GET", "/api/v1/projects"),
      /private_work_transport_failed/,
    );
    assert.equal(http.calls[0].req.destroyed, true);
  }
});
