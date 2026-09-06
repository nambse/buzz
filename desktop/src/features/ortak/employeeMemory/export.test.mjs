import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import {
  edited,
  exactText,
  factId,
  path,
  publication,
  saved,
  setup,
} from "./fixture.mjs";

async function publish(x) {
  await x.waitFor(() =>
    assert.ok(x.view.getByRole("form", { name: "Publish employee memory" })),
  );
  const form = x.view.getByRole("form", { name: "Publish employee memory" });
  await x.act(async () => x.fireEvent.submit(form));
  assert.equal(
    x.state.exportWrites.length,
    0,
    "publication needs separate consent",
  );
  await x.act(async () =>
    x.fireEvent.click(x.within(form).getByRole("checkbox")),
  );
  await x.act(async () => x.fireEvent.submit(form));
  await x.waitFor(() => assert.equal(x.state.exportWrites.length, 1));
}
async function refresh(x) {
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Refresh publication status" }),
    ),
  );
}

test("signed approval requires separate publication consent, shows real acknowledgment and Stop removal", async () => {
  const x = await setup();
  await x.choose();
  await x.fill();
  await x.submit();
  await x.waitFor(() => assert.ok(x.view.getByText(exactText(edited))));
  assert.equal(x.state.exportWrites.length, 0);
  await publish(x);
  const write = x.state.exportWrites[0];
  assert.equal(new URL(write.url).pathname, `${path}/${factId}/export`);
  assert.deepEqual(Object.keys(JSON.parse(write.body)).sort(), [
    "expected_version",
    "operation_id",
  ]);
  assert.equal(JSON.parse(write.body).expected_version, 1);
  const signature = x.state.signatures.find(
    (event) =>
      event.tags.some(([k, v]) => k === "u" && v === write.url) &&
      event.tags.some(([k, v]) => k === "method" && v === "POST"),
  );
  assert.deepEqual(
    signature.tags.find(([k]) => k === "payload"),
    ["payload", createHash("sha256").update(write.body).digest("hex")],
  );
  await x.waitFor(() =>
    assert.ok(
      x.view.getByText("Publication queued or awaiting acknowledgment."),
    ),
  );
  assert.equal(
    x.view.queryByText(
      "Publication acknowledged by the reviewed memory store.",
    ),
    null,
  );
  x.state.exportRecord = publication("acknowledged");
  await refresh(x);
  await x.waitFor(() =>
    assert.ok(
      x.view.getByText(
        "Publication acknowledged by the reviewed memory store.",
      ),
    ),
  );
  assert.ok(
    x.view.getByText(/Check a run’s Activity to see what it actually used/),
  );
  assert.equal(
    x.view.queryByRole("button", { name: /Recall|Enable runtime/ }),
    null,
  );
  x.state.receiptFact = x.state.facts[0];
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Stop using experience memory" }),
    ),
  );
  await x.waitFor(() =>
    assert.ok(
      x.view.getByText("Use has ended. Removal is awaiting acknowledgment."),
    ),
  );
  assert.equal(
    x.view.queryByRole("button", { name: "Publish approved memory" }),
    null,
  );
  x.state.exportRecord = publication("acknowledged", "acknowledged");
  await refresh(x);
  await x.waitFor(() =>
    assert.ok(
      x.view.getByText(
        "Removal acknowledged by the reviewed memory store. Approval history remains.",
      ),
    ),
  );
  assert.equal(
    x.state.exportWrites.length,
    1,
    "Stop uses its own signed API, not a new publication",
  );
});

test("uncertain publication replays identical bytes after close, source loss and Stop with a fresh signature", async () => {
  const x = await setup({ facts: [saved()], exportStatus: 503 }, true);
  await publish(x);
  await x.waitFor(() =>
    assert.equal(
      x.view.getByRole("button", { name: "Retry exact request" }).disabled,
      false,
    ),
  );
  const first = x.state.exportWrites[0];
  await x.rerender({ open: false });
  x.state.canApprove = false;
  x.state.facts = [
    {
      ...saved(true),
      status: "stopped",
      version: 2,
      can_stop: false,
      revoked_at: new Date().toISOString(),
    },
  ];
  x.state.exportRecord = publication("acknowledged", "acknowledged");
  x.state.exportStatus = 200;
  x.state.exportReceipt = (receipt) => ({
    ...receipt,
    operation_id: "88888888-8888-4888-8888-888888888888",
  });
  await x.rerender({ open: true });
  await x.waitFor(() =>
    assert.ok(x.view.getByText(/New approvals are unavailable/)),
  );
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Retry exact request" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.exportWrites.length, 2));
  await x.waitFor(() =>
    assert.equal(
      x.view.getByRole("button", { name: "Retry exact request" }).disabled,
      false,
    ),
  );
  // A mismatched successful response must retain the original request.
  x.state.exportReceipt = undefined;
  await x.act(async () =>
    x.fireEvent.click(
      x.view.getByRole("button", { name: "Retry exact request" }),
    ),
  );
  await x.waitFor(() => assert.equal(x.state.exportWrites.length, 3));
  assert.equal(x.state.exportWrites[1].body, first.body);
  assert.equal(x.state.exportWrites[1].url, first.url);
  assert.equal(x.state.exportWrites[2].body, first.body);
  assert.equal(x.state.exportWrites[2].url, first.url);
  const signatures = x.state.signatures.filter(
    (event) =>
      event.tags.some(([k, v]) => k === "u" && v === first.url) &&
      event.tags.some(([k, v]) => k === "method" && v === "POST"),
  );
  assert.equal(signatures.length, 3);
  assert.notDeepEqual(
    signatures[0].tags.find(([k]) => k === "nonce"),
    signatures[1].tags.find(([k]) => k === "nonce"),
  );
  await x.waitFor(() =>
    assert.equal(
      x.view.queryByRole("button", { name: "Retry exact request" }),
      null,
    ),
  );
  assert.equal(x.view.queryByText(exactText(edited)), null);
  assert.equal(
    x.view.queryByRole("button", { name: "Publish approved memory" }),
    null,
  );
});

test("source-hidden removal retry uses the retained version and malformed metadata cannot claim success", async () => {
  const record = publication("acknowledged", "failed");
  record.export.jobs[1].retry_version = 3;
  const x = await setup(
    { facts: [saved(true)], canApprove: false, exportRecord: record },
    true,
  );
  await x.waitFor(() =>
    assert.ok(x.view.getByRole("button", { name: "Retry removal" })),
  );
  assert.equal(x.state.previews.length, 0);
  assert.equal(x.view.queryByText(exactText(edited)), null);
  await x.act(async () =>
    x.fireEvent.click(x.view.getByRole("button", { name: "Retry removal" })),
  );
  await x.waitFor(() => assert.equal(x.state.exportWrites.length, 1));
  assert.equal(
    new URL(x.state.exportWrites[0].url).pathname,
    `${path}/${factId}/export/retry/withdraw`,
  );
  assert.equal(JSON.parse(x.state.exportWrites[0].body).expected_version, 3);
  await x.waitFor(() =>
    assert.equal(
      x.view.queryByRole("button", { name: "Retry exact request" }),
      null,
    ),
  );
  for (const corrupt of [
    (value) => {
      value.fact_id = "77777777-7777-4777-8777-777777777777";
    },
    (value) => {
      value.export.jobs[1].action = "publish";
    },
    (value) => {
      value.export.jobs[1].acknowledged = false;
    },
    (value) => {
      value.export.content = "must never be projected";
    },
  ]) {
    x.state.exportRecord = publication("acknowledged", "acknowledged");
    corrupt(x.state.exportRecord);
    await refresh(x);
    await x.waitFor(() =>
      assert.ok(
        x.view.getByText(
          "This read could not be confirmed. Refresh to try again.",
        ),
      ),
    );
    assert.equal(
      x.view.queryByText(/Removal acknowledged by the reviewed memory store/),
      null,
    );
    assert.equal(x.view.queryByRole("button", { name: "Retry removal" }), null);
    assert.ok(
      x.view.getByRole("button", { name: "Stop using experience memory" }),
      "a failed metadata read cannot hide Stop recovery",
    );
  }
});

test("publication preflight revocation fences held status reads and rejects stale callbacks", async () => {
  const x = await setup({ facts: [saved()] }, true);
  await x.waitFor(() =>
    assert.ok(x.view.getByRole("form", { name: "Publish employee memory" })),
  );
  const prior = x.state.review;
  const fact = prior.facts.value.facts[0];
  x.state.exportRecord = publication("acknowledged");
  x.state.holdExport = true;
  await refresh(x);
  await x.waitFor(() => assert.equal(x.state.heldExports.length, 1));
  const held = x.state.exportReads.at(-1);
  x.state.holdExport = false;
  x.state.factsStatus = 403;
  await x.act(async () => prior.publication(fact, "publish", 1));
  await x.waitFor(() => assert.equal(x.state.review.facts.ready, false));
  await x.act(async () => x.state.heldExports.shift()());
  assert.equal(held.signal.aborted, true);
  assert.equal(x.state.exportWrites.length, 0);
  assert.equal(x.view.queryByText(exactText(edited)), null);
  assert.equal(
    x.view.queryByText(
      "Publication acknowledged by the reviewed memory store.",
    ),
    null,
  );
  await x.act(async () => prior.publication(fact, "publish", 1));
  assert.equal(x.state.exportWrites.length, 0);
});
