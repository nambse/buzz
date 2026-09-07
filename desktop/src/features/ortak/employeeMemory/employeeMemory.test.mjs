import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import {
  actor,
  destination,
  edited,
  employee,
  exactText,
  factId,
  message,
  path,
  preview,
  saved,
  setup,
} from "./fixture.mjs";
import { createOrtakClient } from "../client.ts";

test("signed native review starts empty, requires explicit sharing and binds relationship to the author", async () => {
  const x = await setup();
  await x.choose();
  assert.equal(x.view.getByLabelText("Edited memory text").value, "");
  assert.equal(x.state.previews[0].body.human_public_key, null);
  await x.fill(false);
  await x.submit();
  assert.equal(x.state.writes.length, 0);
  await x.act(async () =>
    x.fireEvent.change(x.view.getByLabelText("Employee memory kind"), {
      target: { value: "relationship" },
    }),
  );
  await x.waitFor(() =>
    assert.equal(x.state.previews.at(-1).body.human_public_key, actor),
  );
  await x.waitFor(() =>
    assert.equal(x.view.getByLabelText("Edited memory text").value, ""),
  );
  await x.fill();
  await x.submit();
  await x.waitFor(() => assert.equal(x.state.writes.length, 1));
  const write = x.state.writes[0];
  const body = JSON.parse(write.body);
  assert.equal(new URL(write.url).pathname, path);
  assert.deepEqual(Object.keys(body).sort(), ["fact", "operation_id"]);
  assert.deepEqual(Object.keys(body.fact).sort(), [
    "content",
    "destination_channel_id",
    "expected_audience_hash",
    "expires_at",
    "human_public_key",
    "kind",
    "reviewed",
    "source_event_created_at",
    "source_event_id",
  ]);
  assert.equal(body.fact.human_public_key, actor);
  assert.equal(body.fact.content, edited);
  assert.equal(body.fact.source_event_id, message);
  assert.equal(
    body.fact.source_event_created_at,
    "2026-09-01T00:00:00.123456Z",
  );
  assert.equal(body.fact.expected_audience_hash, "f".repeat(64));
  const signed = x.state.signatures.find(
    (event) =>
      event.tags.some(([name, value]) => name === "u" && value === write.url) &&
      event.tags.some(([name, value]) => name === "method" && value === "POST"),
  );
  assert.deepEqual(
    signed.tags.find(([name]) => name === "payload"),
    ["payload", createHash("sha256").update(write.body).digest("hex")],
  );
  // Approval does not automatically publish or authorize runtime recall.
  assert.equal(x.state.exportWrites.length, 0);
  assert.equal(
    x.view.queryByRole("button", { name: /Recall|Enable runtime/ }),
    null,
  );
});

test("late preview cannot restore a prior destination or its edited approval", async () => {
  const x = await setup();
  await x.choose();
  await x.fill();
  x.state.holdPreview = true;
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Refresh sharing preview" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.heldPreviews.length, 1));
  x.state.holdPreview = false;
  await x.act(async () =>
    x.fireEvent.change(x.view.getByLabelText("Destination channel"), {
      target: { value: destination },
    }),
  );
  await x.waitFor(() =>
    assert.equal(
      x.state.previews.at(-1).body.destination_channel_id,
      destination,
    ),
  );
  await x.waitFor(() =>
    assert.equal(x.view.getByLabelText("Edited memory text").value, ""),
  );
  await x.act(async () => x.state.heldPreviews.shift()());
  assert.equal(x.state.previews[1].signal.aborted, true);
  assert.equal(
    x.view.getByRole("checkbox").getAttribute("aria-checked"),
    "false",
  );
  await x.fill();
  await x.submit();
  await x.waitFor(() => assert.equal(x.state.writes.length, 1));
  assert.equal(
    JSON.parse(x.state.writes[0].body).fact.destination_channel_id,
    destination,
  );
});

test("uncertain request retains exact bytes on close/reopen and replays after capability and source loss", async () => {
  const x = await setup({ status: 503 });
  await x.choose();
  await x.fill();
  await x.submit();
  await x.waitFor(() =>
    assert.equal(
      x.view.getByRole("button", { name: "Retry exact request" }).disabled,
      false,
    ),
  );
  const original = x.state.writes[0];
  await x.rerender({ open: false });
  assert.equal(
    x.view.queryByRole("button", { name: "Retry exact request" }),
    null,
  );
  x.state.status = 200;
  x.state.canApprove = false;
  x.state.receiptFact = saved(true);
  await x.rerender({ open: true });
  await x.waitFor(() =>
    assert.ok(x.view.getByText(/New approvals are unavailable/)),
  );
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Retry exact request" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.writes.length, 2));
  assert.equal(x.state.writes[1].body, original.body);
  const signed = x.state.signatures.filter(
    (event) =>
      event.tags.some(
        ([name, value]) => name === "u" && value === original.url,
      ) &&
      event.tags.some(([name, value]) => name === "method" && value === "POST"),
  );
  assert.equal(signed.length, 2);
  assert.notDeepEqual(
    signed[0].tags.find(([name]) => name === "nonce"),
    signed[1].tags.find(([name]) => name === "nonce"),
  );
  await x.waitFor(() =>
    assert.equal(
      x.view.queryByRole("button", { name: "Retry exact request" }),
      null,
    ),
  );
  assert.equal(x.view.queryByText(exactText(edited)), null);
});

test("capability-off hidden-source recovery lists metadata and signs exact Employee Stop without preview", async () => {
  const x = await setup(
    { canApprove: false, facts: [saved(true)], receiptFact: saved(true) },
    true,
  );
  assert.equal(x.state.previews.length, 0);
  assert.equal(x.view.queryByText(exactText(edited)), null);
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Stop using experience memory" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.writes.length, 1));
  assert.equal(
    new URL(x.state.writes[0].url).pathname,
    `${path}/${factId}/stop`,
  );
  assert.deepEqual(Object.keys(JSON.parse(x.state.writes[0].body)).sort(), [
    "expected_version",
    "operation_id",
  ]);
  assert.equal(JSON.parse(x.state.writes[0].body).expected_version, 1);
  await x.waitFor(() =>
    assert.equal(
      x.view.queryByRole("button", { name: "Stop using experience memory" }),
      null,
    ),
  );
});

test("source preview refusal leaves Stop recovery reachable", async () => {
  const x = await setup({
    previewStatus: 403,
    facts: [saved(true)],
    receiptFact: saved(true),
  });
  await x.choose();
  await x.waitFor(() =>
    assert.ok(x.view.getByText(/source must be your own decided plaintext/)),
  );
  assert.ok(
    x.view.getByRole("button", { name: "Stop using experience memory" }),
  );
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Stop using experience memory" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.writes.length, 1));
});

test("preflight denial aborts a held saved read and cannot resurrect sensitive text", async () => {
  const x = await setup({ facts: [saved()] }, true);
  assert.ok(x.view.getByText(exactText(edited)));
  const previous = x.state.review;
  x.state.holdRead = true;
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Refresh saved approvals" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.heldReads.length, 1));
  x.state.holdRead = false;
  x.state.factsStatus = 403;
  // Exercise the real retained callback; its admission guard must refuse while
  // the newer read is unresolved, without creating a request.
  await x.act(async () => previous.stop(factId));
  assert.equal(x.state.writes.length, 0);
  await x.act(async () => x.state.review.mutation.stop(saved()));
  await x.waitFor(() =>
    assert.ok(x.view.getByText(/does not have permission/)),
  );
  await x.act(async () => x.state.heldReads.shift()());
  assert.equal(x.state.reads[1].signal.aborted, true);
  assert.equal(x.view.queryByText(exactText(edited)), null);
  assert.equal(x.state.writes.length, 0);
});

test("saved pagination stays independent of source preview and preserves the exact cursor", async () => {
  const page = Array.from({ length: 16 }, (_, index) => ({
    ...saved(true),
    id: `33333333-3333-4333-8333-${String(index + 1).padStart(12, "0")}`,
  }));
  const cursor = page.at(-1).id;
  const x = await setup(
    {
      page: (after) => ({
        can_approve: false,
        facts: after
          ? [{ ...saved(true), id: "44444444-4444-4444-8444-444444444444" }]
          : page,
        next_after: after ? null : cursor,
      }),
    },
    true,
  );
  assert.equal(
    x.view.getAllByRole("article", { name: "experience approval" }).length,
    16,
  );
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Next approvals page" }),
    ),
  );
  await x.waitFor(() =>
    assert.equal(
      x.view.getAllByRole("article", { name: "experience approval" }).length,
      1,
    ),
  );
  assert.equal(
    new URL(x.state.reads.at(-1).url).searchParams.get("after"),
    cursor,
  );
  assert.equal(x.state.previews.length, 0);
});

test("cross-employee preview and receipt are refused without displaying their content", async () => {
  const bad = preview();
  bad.employee_id = "other-employee";
  const x = await setup({ previewOverride: bad });
  await x.choose();
  await x.waitFor(() =>
    assert.ok(x.view.getByText(/read could not be confirmed/)),
  );
  assert.equal(x.view.queryByLabelText("Edited memory text"), null);
  x.state.previewOverride = null;
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Refresh sharing preview" }),
    ),
  );
  await x.waitFor(() => assert.ok(x.view.getByLabelText("Edited memory text")));
  x.state.receiptFact = {
    ...saved(),
    employee_id: "other-employee",
    content: "foreign private text",
  };
  await x.fill();
  await x.submit();
  await x.waitFor(() =>
    assert.ok(x.view.getByRole("button", { name: "Retry exact request" })),
  );
  assert.equal(x.view.queryByText("foreign private text"), null);
  assert.equal(JSON.parse(x.state.writes[0].body).fact.human_public_key, null);
});

test("employee-memory client refuses oversized requests before signing and bounds response bytes", async () => {
  let signed = 0,
    calls = 0;
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async (event) => {
      signed++;
      return event;
    },
    async () => {
      calls++;
      return new Response("x".repeat(262145));
    },
  );
  await assert.rejects(
    client.employeeMemoryMutation(
      employee.employee_id,
      null,
      "x".repeat(32769),
      new AbortController().signal,
    ),
    /too long/,
  );
  assert.equal(signed, 0);
  assert.equal(calls, 0);
  await assert.rejects(
    client.employeeMemoryFacts(
      employee.employee_id,
      new AbortController().signal,
    ),
    /display limit/,
  );
  assert.equal(signed, 1);
  assert.equal(calls, 1);
});
