import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import {
  setup,
  project,
  employee,
  path,
  factId,
  reviewedText,
  exactText,
  saved,
  publication,
} from "./fixture.mjs";

const consent = /I approve publishing this fact/;
const formName = `Publish conversation fact ${factId}`;
function available() {
  const entry = saved();
  entry.fact.publication_available = true;
  return entry;
}
async function publish(x) {
  await x.act(async () =>
    x.fireEvent.click(x.view.getByRole("checkbox", { name: consent })),
  );
  await x.act(async () =>
    x.fireEvent.submit(x.view.getByRole("form", { name: formName })),
  );
  await x.waitFor(() => assert.equal(x.state.review.mutation.busy, false));
}
function assertSigned(state, write) {
  const signatures = state.signatures.filter(
    (event) =>
      event.tags.some((tag) => tag[0] === "u" && tag[1] === write.url) &&
      event.tags.some((tag) => tag[0] === "method" && tag[1] === "POST"),
  );
  assert.ok(signatures.length);
  for (const event of signatures)
    assert.deepEqual(
      event.tags.find((tag) => tag[0] === "payload"),
      ["payload", createHash("sha256").update(write.body).digest("hex")],
    );
}

test("publication requires fresh explicit consent and signs the export route with a separate receipt", async () => {
  const x = await setup({
    facts: [available()],
    exportReceipt: publication({
      publication: { ...publication().publication, state: "acknowledged" },
      runtime_consumption_enabled: true,
    }),
  });
  await x.choose();
  assert.equal(x.view.getByRole("checkbox", { name: consent }).checked, false);
  await x.act(async () =>
    x.fireEvent.submit(x.view.getByRole("form", { name: formName })),
  );
  assert.equal(x.state.writes.length, 0);
  await x.act(async () =>
    x.fireEvent.click(x.view.getByRole("checkbox", { name: consent })),
  );
  // A fresh server observation retires the old checkbox even for the same ID.
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Refresh conversation facts" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.review.facts.ready, true));
  assert.equal(x.view.getByRole("checkbox", { name: consent }).checked, false);
  await publish(x);
  assert.equal(x.state.writes.length, 1);
  const write = x.state.writes[0];
  assert.equal(new URL(write.url).pathname, `${path}/${factId}/publish`);
  const body = JSON.parse(write.body);
  assert.deepEqual(Object.keys(body).sort(), [
    "confirmed",
    "expected_version",
    "operation_id",
  ]);
  assert.equal(body.confirmed, true);
  assert.equal(body.expected_version, 1);
  assert.match(body.operation_id, /^[0-9a-f-]{36}$/);
  assertSigned(x.state, write);
  assert.equal(x.state.review.mutation.receipt, null);
  assert.equal(x.state.review.mutation.exportReceipt.export.fact_id, factId);
  await x.waitFor(() =>
    assert.ok(
      x.view.getByText("Publication acknowledged by the reviewed store."),
    ),
  );
  assert.ok(x.view.getByText(/Runtime use is currently eligible/));
  assert.ok(x.view.getByText(exactText(reviewedText)));
  assert.equal(x.view.queryByRole("button", { name: /Enable|opt-in/i }), null);
  assert.equal(
    x.view.queryByRole("checkbox", { name: /runtime|opt-in/i }),
    null,
  );
});

test("uncertain publication retains exact path and bytes across close and source-hidden recovery", async () => {
  const x = await setup({ facts: [available()], status: 503 });
  await x.choose();
  await publish(x);
  assert.equal(x.state.writes.length, 1);
  const first = x.state.writes[0];
  assert.equal(
    x.state.review.mutation.pending.path,
    `${path}/${factId}/publish`,
  );
  assert.equal(
    x.view.getByRole("button", {
      name: `Stop using conversation fact ${factId}`,
    }).disabled,
    true,
  );
  await x.act(async () => {
    x.state.review.mutation.submit(`${path}/${factId}/stop`, {
      expected_version: 1,
      reason: "A second action must not replace pending publication",
    });
    x.state.review.mutation.submit(`${path}/${factId}/publish`, {
      expected_version: 1,
      confirmed: true,
    });
  });
  assert.equal(x.state.writes.length, 1);
  await x.act(async () =>
    x.view.rerender(x.createElement(x.Harness, { open: false })),
  );
  x.state.selected = { ...project, status: "archived" };
  x.state.projects = [x.state.selected];
  x.state.employees = [{ ...employee, status: "disabled" }];
  x.state.facts = [saved(true)];
  x.state.status = 200;
  await x.act(async () =>
    x.view.rerender(x.createElement(x.Harness, { open: true })),
  );
  await x.waitFor(() =>
    assert.ok(x.view.getByText(/Source evidence has changed/)),
  );
  assert.equal(x.view.queryByRole("form", { name: formName }), null);
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Retry same memory operation" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.review.mutation.pending, null));
  assert.equal(x.state.writes.length, 2);
  assert.equal(x.state.writes[1].url, first.url);
  assert.equal(x.state.writes[1].body, first.body);
  assertSigned(x.state, first);
  assert.equal(x.state.review.mutation.receipt, null);
  assert.equal(x.state.review.mutation.exportReceipt.export.fact_id, factId);
});

test("source-hidden archived withdrawal retries the retained job without preview or a second operation", async () => {
  const entry = saved(true);
  entry.fact.status = "revoked";
  entry.fact.version = 2;
  entry.fact.export = publication({
    publication: { ...publication().publication, state: "acknowledged" },
    cleanup: {
      ...publication().cleanup,
      state: "failed",
      retry_version: 2,
      attempt_count: 3,
      error_code: "remote_unavailable",
    },
  });
  const x = await setup(
    {
      facts: [entry],
      selected: { ...project, status: "archived" },
      employees: [{ ...employee, status: "disabled" }],
      status: 503,
    },
    true,
  );
  await x.choose();
  assert.ok(x.view.getByText(/cleanup failed; removal is not confirmed/));
  assert.equal(x.view.queryByRole("form", { name: formName }), null);
  assert.equal(
    x.view.queryByRole("button", { name: /Retry conversation publication/ }),
    null,
  );
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", {
        name: `Retry conversation reviewed-store cleanup for fact ${factId}`,
      }),
    ),
  );
  await x.waitFor(() => assert.ok(x.view.getByText(/Confirmation is missing/)));
  assert.equal(x.state.writes.length, 1);
  const first = x.state.writes[0];
  assert.equal(
    new URL(first.url).pathname,
    `${path}/${factId}/exports/withdraw/retry`,
  );
  assert.deepEqual(Object.keys(JSON.parse(first.body)).sort(), [
    "operation_id",
    "retry_version",
  ]);
  assert.equal(JSON.parse(first.body).retry_version, 2);
  x.state.status = 200;
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Retry same memory operation" }),
    ),
  );
  await x.waitFor(() =>
    assert.ok(
      x.view.getByText(/Retry accepted for the same reviewed-store job/),
    ),
  );
  assert.equal(x.state.writes.length, 2);
  assert.equal(x.state.writes[1].body, first.body);
  assert.equal(x.state.writes[1].url, first.url);
  assert.equal(x.state.previews.length, 0);
  assertSigned(x.state, first);
  await x.waitFor(() =>
    assert.ok(
      x.view.getByText(
        /Use has ended. Reviewed-store removal is awaiting acknowledgement/,
      ),
    ),
  );
  assert.equal(x.view.queryByText(/Reviewed-store text removed/), null);
});

test("failed publication uses its retry generation, while mismatched receipt remains unconfirmed", async () => {
  const entry = saved();
  entry.fact.export = publication({
    publication: {
      ...publication().publication,
      state: "failed",
      retry_version: 4,
      attempt_count: 3,
      error_code: "remote_unavailable",
    },
  });
  const x = await setup({ facts: [entry], exportReceipt: entry.fact.export });
  await x.choose();
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", {
        name: `Retry conversation publication for fact ${factId}`,
      }),
    ),
  );
  await x.waitFor(() => assert.ok(x.view.getByText(/Confirmation is missing/)));
  assert.equal(x.state.review.mutation.exportReceipt, null);
  assert.equal(x.state.review.mutation.receipt, null);
  const first = x.state.writes[0];
  assert.equal(
    new URL(first.url).pathname,
    `${path}/${factId}/exports/publish/retry`,
  );
  assert.equal(JSON.parse(first.body).retry_version, 4);
  x.state.exportReceipt = publication({
    publication: { ...publication().publication, retry_version: 5 },
  });
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Retry same memory operation" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.review.mutation.pending, null));
  assert.equal(x.state.writes[1].body, first.body);
  assertSigned(x.state, first);
  assert.equal(
    x.state.review.mutation.exportReceipt.export.publication.retry_version,
    5,
  );
});

test("foreign export acknowledgements and unknown paths cannot be mistaken for Stop receipts", async () => {
  const x = await setup({
    facts: [available()],
    exportReceipt: publication({
      fact_id: "55555555-5555-4555-8555-555555555555",
    }),
  });
  await x.choose();
  await x.act(async () => {
    x.state.review.mutation.submit(`${path}/${factId}/enable`, {});
    x.state.review.mutation.submit(`${path}/${factId}/publish?other=1`, {});
  });
  assert.equal(x.state.writes.length, 0);
  await publish(x);
  assert.ok(x.state.review.mutation.pending);
  assert.equal(x.state.review.mutation.receipt, null);
  assert.equal(x.state.review.mutation.exportReceipt, null);
  assert.ok(x.view.getByText(/Confirmation is missing/));
  assert.equal(x.view.queryByText(/Use stopped\. The approval/), null);
});

test("late export response is fenced by context and rejected current access clears private status", async () => {
  const x = await setup({ facts: [available()], holdWrite: true });
  await x.choose();
  await x.act(async () =>
    x.fireEvent.click(x.view.getByRole("checkbox", { name: consent })),
  );
  await x.act(async () =>
    x.fireEvent.submit(x.view.getByRole("form", { name: formName })),
  );
  await x.waitFor(() => assert.equal(x.state.heldWrites.length, 1));
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
  await x.act(async () => x.state.heldWrites.shift()());
  assert.equal(x.state.writes[0].signal.aborted, true);
  assert.equal(x.state.review.mutation.exportReceipt, null);
  assert.equal(x.state.review.mutation.receipt, null);
  assert.equal(x.view.queryByText(/Publication request accepted/), null);
  x.cleanup();
  const y = await setup({ facts: [available()] });
  await y.choose();
  y.state.projectStatus = 403;
  await publish(y);
  await y.waitFor(() => assert.equal(y.state.review.directory.ready, false));
  assert.equal(y.state.writes.length, 0);
  assert.equal(y.view.queryByText(exactText(reviewedText)), null);
  assert.equal(y.view.queryByRole("form", { name: formName }), null);
  assert.equal(y.state.review.mutation.pending, null);
});

test("malformed export list is withheld and exhausted retry has no write affordance", async () => {
  const entry = saved();
  entry.fact.export = publication({ runtime_consumption_enabled: "true" });
  const x = await setup({ facts: [entry] });
  await x.act(async () => {
    x.fireEvent.change(x.view.getByLabelText("Conversation project"), {
      target: { value: project.id },
    });
    x.fireEvent.change(x.view.getByLabelText("Conversation employee"), {
      target: { value: employee.employee_id },
    });
  });
  await x.waitFor(() => assert.ok(x.state.review.facts.error));
  assert.equal(x.view.queryByText(exactText(reviewedText)), null);
  assert.equal(x.state.review.facts.value, null);
  x.cleanup();
  const exhausted = saved(true);
  exhausted.fact.export = publication({
    cleanup: { ...publication().cleanup, state: "failed", retry_version: 8 },
  });
  const y = await setup({ facts: [exhausted], previewStatus: 403 });
  await y.choose();
  assert.ok(y.view.getByText(/Retry limit reached/));
  assert.equal(
    y.view.queryByRole("button", { name: /Retry conversation/ }),
    null,
  );
  assert.ok(
    y.view.getByRole("button", {
      name: `Stop using conversation fact ${factId}`,
    }),
  );
  assert.equal(y.state.writes.length, 0);
});
