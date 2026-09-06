import assert from "node:assert/strict";
import { after, afterEach, before } from "node:test";
import { JSDOM } from "jsdom";
import { createOrtakClient } from "../client.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
before(() => {
  for (const name of [
    "document",
    "HTMLElement",
    "HTMLInputElement",
    "Element",
    "Node",
    "Event",
    "MouseEvent",
    "FormData",
  ])
    Object.defineProperty(globalThis, name, {
      value: dom.window[name],
      configurable: true,
    });
  globalThis.window = dom.window;
  globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  // JSDOM does not implement layout; native checkbox state/events remain real.
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  dom.window.matchMedia = (media) => {
    const target = new dom.window.EventTarget();
    return {
      matches: false,
      media,
      onchange: null,
      addEventListener: target.addEventListener.bind(target),
      removeEventListener: target.removeEventListener.bind(target),
      dispatchEvent: target.dispatchEvent.bind(target),
      addListener: (listener) => target.addEventListener("change", listener),
      removeListener: (listener) =>
        target.removeEventListener("change", listener),
    };
  };
});
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());

export const channel = "22222222-2222-4222-8222-222222222222";
export const projectId = "33333333-3333-4333-8333-333333333333";
export const message = "a".repeat(64);
export const factId = "44444444-4444-4444-8444-444444444444";
export const employee = {
  employee_id: "ada-private",
  name: "Ada",
  status: "active",
  active_revision_id: "revision",
  title: "Reviewer",
};
export const project = {
  id: projectId,
  channel_id: channel,
  name: "Release plan",
  status: "active",
  can_contribute: true,
  can_review: true,
  role: "owner",
  slug: "release",
  version: 1,
};
export const path = `/api/v1/projects/${projectId}/conversation-memory`;
export const reviewedText =
  "Human edited fact, with explicit scope.\nSecond line.";
// Reviewed bytes deliberately include a newline. Testing Library's default
// whitespace normalization would make both presence and absence checks vacuous.
export const exactText = (text) => (_normalized, element) =>
  element?.tagName === "P" && element.textContent === text;
export function preview(body = {}) {
  const kind = body.audience?.kind ?? "thread";
  return {
    audience: {
      format: "ortak-reviewed-conversation-audience/1",
      community_id: "community",
      company_id: "company",
      project_id: projectId,
      employee_id: body.employee_id ?? employee.employee_id,
      channel_id: channel,
      kind,
      thread_root_event_id: kind === "thread" ? "b".repeat(64) : null,
      thread_root_event_created_at:
        kind === "thread" ? "2026-09-01T00:00:00Z" : null,
    },
    audience_hash: (kind === "thread" ? "c" : "d").repeat(64),
    provenance: {
      source_event_id: body.source_message_id ?? message,
      source_hash: "e".repeat(64),
    },
    observed_at: new Date().toISOString(),
    valid_before: new Date(Date.now() + 2 * 86400000).toISOString(),
    max_expires_at: new Date(Date.now() + 2 * 86400000).toISOString(),
  };
}
export function saved(hidden = false) {
  const observation = preview();
  return {
    fact: {
      id: factId,
      project_id: projectId,
      employee_id: employee.employee_id,
      source: hidden ? null : { kind: "conversation", message_id: message },
      source_visible: !hidden,
      content: hidden ? null : reviewedText,
      version: 1,
      status: "active",
      approved_by: "f".repeat(64),
      approved_at: new Date().toISOString(),
      expires_at: observation.max_expires_at,
      revoked_by: null,
      revoked_at: null,
      revoke_reason: null,
      publication_available: false,
      export: null,
    },
    audience: hidden ? null : observation.audience,
    audience_hash: hidden ? null : observation.audience_hash,
    provenance: hidden ? null : observation.provenance,
  };
}

export function publication(overrides = {}) {
  const job = {
    state: "pending",
    retry_version: 0,
    attempt_count: 0,
    next_attempt_at: new Date(Date.now() + 3600000).toISOString(),
    error_code: null,
  };
  return {
    fact_id: factId,
    publication: { ...job },
    cleanup: { ...job },
    erased_from_reviewed_store: false,
    runtime_consumption_enabled: false,
    ...overrides,
  };
}

export async function setup(overrides = {}, recovery = false) {
  const { createElement } = await import("react");
  const testing = await import("@testing-library/react");
  const { useConversationReview, ConversationReviewPanel } = await import(
    "./ConversationMemoryDialog.tsx"
  );
  const { ConversationMemoryPanel } = await import(
    "./ConversationMemoryPanel.tsx"
  );
  const state = {
    writes: [],
    signatures: [],
    reads: [],
    previews: [],
    heldPreviews: [],
    heldDirectories: [],
    heldWrites: [],
    status: 201,
    previewStatus: 200,
    projectStatus: 200,
    factsStatus: 200,
    selected: project,
    employees: [employee],
    facts: [],
    nextAfter: null,
    projects: [project],
    ...overrides,
  };
  const client = createOrtakClient(
    "http://127.0.0.1:3010",
    async (event) => {
      state.signatures.push(event);
      return event;
    },
    async (url, init) => {
      const parsed = new URL(url);
      state.reads.push({ url, signal: init.signal });
      if (parsed.pathname.endsWith("/preview")) {
        const body = JSON.parse(init.body);
        state.previews.push({ body, signal: init.signal });
        const payload = { preview: state.previewOverride ?? preview(body) };
        const status = state.previewStatus;
        if (state.holdPreview)
          await new Promise((resolve) => state.heldPreviews.push(resolve));
        return Response.json(payload, { status });
      }
      if (init.method === "POST") {
        state.writes.push({ url, body: init.body, signal: init.signal });
        const body = JSON.parse(init.body);
        if (
          parsed.pathname === `${path}/${factId}/publish` ||
          parsed.pathname === `${path}/${factId}/exports/publish/retry` ||
          parsed.pathname === `${path}/${factId}/exports/withdraw/retry`
        ) {
          const entry = structuredClone(state.facts[0] ?? saved());
          const exported = structuredClone(
            state.exportReceipt ?? publication(),
          );
          if (parsed.pathname.endsWith("/retry") && !state.exportReceipt) {
            const action = parsed.pathname.endsWith("/withdraw/retry")
              ? "cleanup"
              : "publication";
            exported[action].retry_version = body.retry_version + 1;
          }
          const status = state.status;
          if (state.holdWrite)
            await new Promise((resolve) => state.heldWrites.push(resolve));
          entry.fact.export = exported;
          if (status < 300) state.facts = [entry];
          return Response.json({ export: exported }, { status });
        }
        const entry = structuredClone(state.receipt ?? saved());
        if (parsed.pathname.endsWith("/stop")) {
          entry.fact.version = 2;
          entry.fact.status = "revoked";
        } else if (entry.fact.source_visible) {
          const observed = preview(body.fact);
          Object.assign(entry, {
            audience: observed.audience,
            audience_hash: body.fact.expected_audience_hash,
            provenance: observed.provenance,
          });
          Object.assign(entry.fact, {
            employee_id: body.fact.employee_id,
            content: body.fact.content,
            expires_at: body.fact.expires_at,
            source: {
              kind: "conversation",
              message_id: body.fact.source_message_id,
            },
          });
        }
        if (state.status < 300) state.facts = [entry];
        return Response.json(
          { fact: entry, created: state.writes.length === 1 },
          { status: state.status },
        );
      }
      if (parsed.pathname.endsWith("/conversation-memory"))
        return Response.json(
          { facts: state.facts, next_after: state.nextAfter },
          { status: state.factsStatus },
        );
      if (parsed.pathname === "/api/v1/projects") {
        const payload = structuredClone({
          projects: state.projects,
          next_cursor: state.projectCursor ?? null,
          create_channels: [],
          can_create_projects: false,
        });
        if (state.holdDirectory)
          await new Promise((resolve) => state.heldDirectories.push(resolve));
        return Response.json(payload);
      }
      if (parsed.pathname === "/api/v1/employees")
        return Response.json({
          employees: state.employees,
          has_more: false,
          next_after: null,
        });
      assert.equal(parsed.pathname, `/api/v1/projects/${projectId}`);
      return Response.json(
        { project: state.selected },
        { status: state.projectStatus },
      );
    },
  );
  function Harness({
    currentClient = client,
    currentMessage = message,
    open = true,
  } = {}) {
    const review = useConversationReview(
      currentClient,
      channel,
      currentMessage,
      open,
    );
    state.review = review;
    return open
      ? createElement(ConversationReviewPanel, {
          state: review,
          message: currentMessage,
        })
      : null;
  }
  const view = testing.render(
    recovery
      ? createElement(ConversationMemoryPanel, {
          client,
          project: state.selected,
          disabled: false,
        })
      : createElement(Harness),
  );
  if (!recovery)
    await testing.waitFor(() =>
      assert.equal(state.review.directory.ready, true),
    );
  else {
    await testing.act(async () =>
      testing.fireEvent.click(
        view.getByRole("button", { name: "Inspect conversation memory" }),
      ),
    );
    await testing.waitFor(() =>
      assert.equal(
        view.getByLabelText("Saved conversation employee").matches(":disabled"),
        false,
      ),
    );
  }
  async function choose() {
    await testing.act(async () => {
      testing.fireEvent.change(
        view.getByLabelText(
          recovery ? "Saved conversation employee" : "Conversation project",
        ),
        { target: { value: recovery ? employee.employee_id : projectId } },
      );
      if (!recovery)
        testing.fireEvent.change(view.getByLabelText("Conversation employee"), {
          target: { value: employee.employee_id },
        });
    });
    if (!recovery && !state.holdPreview)
      await testing.waitFor(() =>
        assert.ok(
          state.review.preview.ready ||
            state.review.preview.error ||
            state.selected.status !== "active",
        ),
      );
    await testing.waitFor(() =>
      assert.ok(
        view.queryByText("No conversation facts on this page.") ||
          view.queryByText(exactText(reviewedText)) ||
          view.queryByText(/Source evidence has changed/),
      ),
    );
  }
  async function fill(review = true) {
    await testing.act(async () => {
      testing.fireEvent.change(view.getByLabelText("Edited fact text"), {
        target: { value: reviewedText },
      });
      const local = new Date(
        Date.now() + 3600000 - new Date().getTimezoneOffset() * 60000,
      )
        .toISOString()
        .slice(0, 16);
      testing.fireEvent.change(view.getByLabelText("Use until"), {
        target: { value: local },
      });
      if (review) testing.fireEvent.click(view.getByRole("checkbox"));
    });
  }
  async function submit() {
    await testing.act(async () =>
      testing.fireEvent.submit(
        view.getByRole("form", { name: "Approve conversation fact" }),
      ),
    );
    await testing.waitFor(() =>
      assert.equal(state.review.mutation.busy, false),
    );
  }
  return {
    ...testing,
    createElement,
    Harness,
    state,
    client,
    view,
    choose,
    fill,
    submit,
  };
}
