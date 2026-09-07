import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>");
before(() =>
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  }),
);
after(() => dom.window.close());

test("execution provenance clears prior source details when current access becomes unavailable", async () => {
  const { createElement } = await import("react");
  const { render, cleanup } = await import("@testing-library/react");
  const { ExecutionContext } = await import("./ExecutionContext.tsx");
  const context = {
    status: "ready",
    artifact_id: "artifact-one",
    artifact_execution_version: 4,
    message_ids: ["selected-source-one", "selected-source-two"],
    history_limited: true,
  };
  const view = render(
    createElement(ExecutionContext, {
      execution: { reference_context: context },
    }),
  );
  assert.ok(view.getByText(/Previous deliverable: work version 4/));
  assert.ok(view.getByText(/2 conversation messages were selected/));
  assert.ok(view.getByText(/Some message text or older history was omitted/));
  assert.ok(view.getByText("selected-source-one"));
  // A stale response must not leave retained metadata visible alongside the
  // new authority status, even if that response still contains old fields.
  view.rerender(
    createElement(ExecutionContext, {
      execution: {
        reference_context: { ...context, status: "unavailable" },
      },
    }),
  );
  assert.equal(view.queryByText("selected-source-one"), null);
  assert.equal(view.queryByText(/Previous deliverable: work version 4/), null);
  assert.ok(view.getByText(/cannot be shown under current permissions/));
  view.rerender(
    createElement(ExecutionContext, { execution: { reference_context: null } }),
  );
  assert.equal(
    view.queryByRole("region", { name: "Execution references" }),
    null,
  );
  cleanup();
});
