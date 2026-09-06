import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import {
  setup,
  projectId,
  project,
  employee,
  message,
  path,
  factId,
  reviewedText,
  exactText,
  saved,
} from "./fixture.mjs";

test("actual review form starts empty, defaults to thread and signs only the reviewed draft", async () => {
  const x = await setup();
  await x.choose();
  assert.equal(x.view.getByLabelText("Conversation audience").value, "thread");
  assert.equal(x.view.getByLabelText("Edited fact text").value, "");
  assert.equal(
    x.view.getByRole("checkbox").getAttribute("aria-checked"),
    "false",
  );
  assert.ok(x.view.getByText(/only this canonical thread/));
  assert.deepEqual(x.state.previews[0].body, {
    employee_id: employee.employee_id,
    source_message_id: message,
    audience: { kind: "thread" },
  });
  await x.fill(false);
  await x.submit();
  assert.equal(
    x.state.writes.length,
    0,
    "direct submit must require the human checkbox",
  );
  await x.act(async () => x.fireEvent.click(x.view.getByRole("checkbox")));
  await x.submit();
  assert.equal(x.state.writes.length, 1);
  const first = x.state.writes[0];
  assert.equal(new URL(first.url).pathname, path);
  const body = JSON.parse(first.body);
  assert.deepEqual(Object.keys(body).sort(), ["fact", "operation_id"]);
  assert.match(body.operation_id, /^[0-9a-f-]{36}$/);
  assert.deepEqual(Object.keys(body.fact).sort(), [
    "audience",
    "content",
    "employee_id",
    "expected_audience_hash",
    "expires_at",
    "reviewed",
    "source_message_id",
  ]);
  assert.equal(body.fact.content, reviewedText);
  assert.equal(body.fact.expected_audience_hash, "c".repeat(64));
  assert.equal(body.fact.reviewed, true);
  const signed = x.state.signatures.filter(
    (event) =>
      event.tags.some((tag) => tag[0] === "u" && tag[1] === first.url) &&
      event.tags.some((tag) => tag[0] === "method" && tag[1] === "POST"),
  );
  assert.equal(signed.length, 1);
  assert.deepEqual(
    signed[0].tags.find((tag) => tag[0] === "payload"),
    ["payload", createHash("sha256").update(first.body).digest("hex")],
  );
  assert.equal(x.state.review.mutation.receipt.fact.fact.content, reviewedText);
  assert.equal(
    x.view.queryByRole("button", { name: /Publish|Enable runtime|Recall/ }),
    null,
  );
});

test("changed audience clears text/review and rejects both late preview and stale approval callbacks", async () => {
  const x = await setup();
  await x.choose();
  await x.fill();
  const old = x.state.review;
  const draft = {
    employee_id: employee.employee_id,
    source_message_id: message,
    audience: { kind: "thread" },
    expected_audience_hash: old.preview.value.audience_hash,
    content: reviewedText,
    expires_at: new Date(Date.now() + 3600000).toISOString(),
    reviewed: true,
  };
  x.state.holdPreview = true;
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Refresh audience preview" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.heldPreviews.length, 1));
  x.state.holdPreview = false;
  await x.act(async () =>
    x.fireEvent.change(x.view.getByLabelText("Conversation audience"), {
      target: { value: "channel" },
    }),
  );
  await x.waitFor(() =>
    assert.equal(x.state.review.preview.value?.audience.kind, "channel"),
  );
  assert.equal(x.view.getByLabelText("Edited fact text").value, "");
  assert.equal(
    x.view.getByRole("checkbox").getAttribute("aria-checked"),
    "false",
  );
  await x.act(async () => {
    old.approve(old.preview.value, draft);
    x.state.heldPreviews.shift()();
  });
  assert.equal(x.state.writes.length, 0);
  assert.equal(x.state.review.preview.value.audience.kind, "channel");
  assert.equal(x.state.previews[1].signal.aborted, true);
  await x.fill();
  await x.submit();
  assert.equal(
    JSON.parse(x.state.writes[0].body).fact.expected_audience_hash,
    "d".repeat(64),
  );
});

test("an uncertain approval retains exact bytes across close/reopen and permits source-hidden archived recovery", async () => {
  const x = await setup({ status: 503 });
  await x.choose();
  await x.fill();
  await x.submit();
  const first = x.state.writes[0].body;
  assert.ok(x.state.review.mutation.pending);
  assert.equal(
    x.view.getByLabelText("Conversation audience").matches(":disabled"),
    true,
  );
  await x.submit();
  assert.equal(
    x.state.writes.length,
    1,
    "no new operation replaces an uncertain request",
  );
  await x.act(async () =>
    x.view.rerender(x.createElement(x.Harness, { open: false })),
  );
  x.state.selected = { ...project, status: "archived" };
  x.state.projects = [x.state.selected];
  x.state.employees = [{ ...employee, status: "disabled" }];
  x.state.previewStatus = 403;
  x.state.facts = [saved(true)];
  x.state.receipt = saved(true);
  x.state.status = 200;
  await x.act(async () =>
    x.view.rerender(x.createElement(x.Harness, { open: true })),
  );
  await x.waitFor(() => assert.equal(x.state.review.directory.ready, true));
  await x.waitFor(() =>
    assert.ok(x.view.getByText(/Source evidence has changed/)),
  );
  assert.equal(x.view.queryByLabelText("Edited fact text"), null);
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Retry same memory operation" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.review.mutation.pending, null));
  assert.equal(x.state.writes.length, 2);
  assert.equal(x.state.writes[1].body, first);
  assert.equal(x.state.review.mutation.receipt.fact.fact.source_visible, false);
});

test("a definite conflict clears approval and requires a new preview before another operation", async () => {
  const x = await setup({ status: 409 });
  await x.choose();
  await x.fill();
  await x.submit();
  assert.equal(x.state.writes.length, 1);
  assert.equal(x.state.review.mutation.pending, null);
  assert.equal(x.view.queryByLabelText("Edited fact text"), null);
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Refresh audience preview" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.review.preview.ready, true));
  assert.equal(
    x.view.getByRole("checkbox").getAttribute("aria-checked"),
    "false",
  );
  await x.submit();
  assert.equal(x.state.writes.length, 1);
});

test("source refusal leaves Stop using accessible in message review and project-only recovery", async () => {
  const x = await setup({
    previewStatus: 403,
    facts: [saved(true)],
    receipt: saved(true),
  });
  await x.choose();
  assert.equal(x.view.queryByLabelText("Edited fact text"), null);
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", {
        name: `Stop using conversation fact ${factId}`,
      }),
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
    "reason",
  ]);
  x.cleanup();
  const y = await setup(
    {
      selected: { ...project, status: "archived" },
      employees: [{ ...employee, status: "disabled" }],
      facts: [saved(true)],
      receipt: saved(true),
    },
    true,
  );
  await y.choose();
  await y.act(async () =>
    y.fireEvent.click(
      y.view.getByRole("button", {
        name: `Stop using conversation fact ${factId}`,
      }),
    ),
  );
  await y.waitFor(() => assert.equal(y.state.writes.length, 1));
  assert.equal(
    y.state.previews.length,
    0,
    "project recovery must not fetch the missing source",
  );
  assert.equal(JSON.parse(y.state.writes[0].body).expected_version, 1);
});

test("write preflight revocation aborts held reads and cannot resurrect private preview or saved text", async () => {
  const x = await setup({ facts: [saved()] });
  await x.choose();
  x.state.holdPreview = true;
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Refresh audience preview" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.heldPreviews.length, 1));
  x.state.projectStatus = 403;
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", {
        name: `Stop using conversation fact ${factId}`,
      }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.review.directory.ready, false));
  assert.equal(x.state.writes.length, 0);
  await x.act(async () => x.state.heldPreviews.shift()());
  assert.equal(x.state.review.preview.value, null);
  assert.equal(x.view.queryByText(exactText(reviewedText)), null);
  assert.equal(x.view.queryByLabelText("Edited fact text"), null);
  x.state.holdPreview = false;
  x.state.projectStatus = 200;
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Refresh conversation access" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.review.preview.ready, true));
  assert.equal(x.view.getByLabelText("Edited fact text").value, "");
});

test("source, employee and client changes clear old content and fence old submit callbacks", async () => {
  const other = {
    ...employee,
    employee_id: "new-reviewer",
    name: "New reviewer",
  };
  const x = await setup({ employees: [employee, other] });
  await x.choose();
  await x.fill();
  const old = x.state.review;
  await x.act(async () =>
    x.fireEvent.change(x.view.getByLabelText("Conversation employee"), {
      target: { value: other.employee_id },
    }),
  );
  await x.waitFor(() =>
    assert.equal(
      x.state.review.preview.value?.audience.employee_id,
      other.employee_id,
    ),
  );
  assert.equal(x.view.getByLabelText("Edited fact text").value, "");
  await x.act(async () =>
    x.view.rerender(
      x.createElement(x.Harness, { currentMessage: "1".repeat(64) }),
    ),
  );
  await x.waitFor(() =>
    assert.equal(
      x.state.review.preview.value?.provenance.source_event_id,
      "1".repeat(64),
    ),
  );
  await x.act(async () =>
    old.mutation.submit(path, { fact: { source_message_id: message } }),
  );
  assert.equal(x.state.writes.length, 0);
  const oldCurrent = x.state.review;
  const denied = {
    ...x.client,
    projects: async () => {
      throw new Error("synthetic offline origin");
    },
  };
  await x.act(async () =>
    x.view.rerender(x.createElement(x.Harness, { currentClient: denied })),
  );
  await x.waitFor(() => assert.ok(x.state.review.directory.error));
  await x.act(async () => oldCurrent.mutation.submit(path, {}));
  assert.equal(x.state.writes.length, 0);
  assert.equal(x.view.queryByLabelText("Edited fact text"), null);
});

test("foreign response scope is withheld and current list revocation clears the approval preview", async () => {
  const x = await setup();
  await x.choose();
  x.state.factsStatus = 403;
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Refresh conversation facts" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.review.directory.ready, false));
  assert.equal(x.view.queryByLabelText("Edited fact text"), null);
  x.cleanup();
  const foreign = saved();
  foreign.fact.project_id = "foreign-project";
  const y = await setup({ facts: [foreign] });
  await y.act(async () => {
    y.fireEvent.change(y.view.getByLabelText("Conversation project"), {
      target: { value: projectId },
    });
    y.fireEvent.change(y.view.getByLabelText("Conversation employee"), {
      target: { value: employee.employee_id },
    });
  });
  await y.waitFor(() => assert.ok(y.state.review.facts.error));
  assert.equal(y.view.queryByText(exactText(reviewedText)), null);
  assert.equal(y.state.review.facts.value, null);
  assert.equal(y.state.writes.length, 0);
});

test("saved facts use the server cursor and reject more than the sixteen-record page contract", async () => {
  const x = await setup({ facts: [saved()], nextAfter: factId });
  await x.choose();
  const second = saved();
  second.fact.id = "55555555-5555-4555-8555-555555555555";
  second.fact.content = "Second page reviewed text";
  x.state.facts = [second];
  x.state.nextAfter = null;
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "More conversation facts" }),
    ),
  );
  await x.waitFor(() => assert.ok(x.view.getByText(second.fact.content)));
  assert.ok(
    x.state.reads.some(
      (entry) => new URL(entry.url).searchParams.get("after") === factId,
    ),
  );
  assert.equal(x.view.queryByText(exactText(reviewedText)), null);
  x.state.facts = Array.from({ length: 17 }, () => saved());
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "First conversation facts" }),
    ),
  );
  await x.waitFor(() => assert.ok(x.state.review.facts.error));
  assert.equal(x.state.review.facts.value, null);
  assert.equal(x.view.queryByText(second.fact.content), null);
});
