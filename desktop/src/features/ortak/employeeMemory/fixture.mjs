import assert from "node:assert/strict";
import { after, afterEach, before } from "node:test";
import { JSDOM } from "jsdom";
import { createOrtakClient } from "../client.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
  pretendToBeVisual: true,
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
    "MutationObserver",
    "CustomEvent",
  ])
    Object.defineProperty(globalThis, name, {
      value: dom.window[name],
      configurable: true,
    });
  globalThis.window = dom.window;
  globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
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

export const actor = "a".repeat(64);
export const message = "b".repeat(64);
export const channel = "11111111-1111-4111-8111-111111111111";
export const destination = "22222222-2222-4222-8222-222222222222";
export const factId = "33333333-3333-4333-8333-333333333333";
export const employee = {
  employee_id: "ada-private",
  name: "Ada",
  title: "Reviewer",
  status: "active",
  active_revision_id: "revision",
};
export const path = `/api/v1/employees/${employee.employee_id}/reviewed-memory`;
export const edited = "Explicitly edited sharing text.\nSecond line.";
export const exactText = (text) => (_normalized, element) =>
  element?.tagName === "P" && element.textContent === text;
export function preview(request = {}) {
  const until = new Date(Date.now() + 2 * 86400000).toISOString();
  return {
    employee_id: employee.employee_id,
    audience: {
      format: "ortak-reviewed-employee-audience/1",
      company_id: "44444444-4444-4444-8444-444444444444",
      destination_community_id: "55555555-5555-4555-8555-555555555555",
      destination_channel_id: request.destination_channel_id ?? channel,
      employee_id: employee.employee_id,
      kind: request.kind ?? "experience",
      human_public_key: request.human_public_key ?? null,
    },
    audience_hash: (request.kind === "relationship" ? "f" : "c").repeat(64),
    source: {
      author_public_key: actor,
      channel_id: channel,
      community_id: "55555555-5555-4555-8555-555555555555",
      event_id: request.source_event_id ?? message,
      event_created_at: "2026-09-01T00:00:00.123456Z",
      evidence_hash: "d".repeat(64),
    },
    source_hash: "e".repeat(64),
    observed_at: new Date().toISOString(),
    valid_before: until,
    max_expires_at: until,
  };
}
export function saved(hidden = false, request = {}) {
  const p = preview(request);
  return {
    id: factId,
    employee_id: employee.employee_id,
    kind: p.audience.kind,
    status: "approved",
    version: 1,
    approved_at: p.observed_at,
    expires_at: p.max_expires_at,
    revoked_at: null,
    source_current: !hidden,
    can_stop: true,
    content: hidden ? null : edited,
    audience: hidden ? null : p.audience,
    audience_hash: hidden ? null : p.audience_hash,
    source: hidden ? null : p.source,
    source_hash: hidden ? null : p.source_hash,
    provenance: hidden ? null : { format: "fixture-provenance" },
    sharing_hash: hidden ? null : "f".repeat(64),
  };
}

export function publication(
  publish = "pending",
  withdraw = "pending",
  fact = factId,
) {
  return {
    fact_id: fact,
    export: {
      target_id: "66666666-6666-4666-8666-666666666666",
      created_at: new Date().toISOString(),
      jobs: ["publish", "withdraw"].map((action, index) => {
        const state = index === 0 ? publish : withdraw;
        return {
          action,
          state,
          attempt_count:
            state === "failed" ? 20 : state === "acknowledged" ? 1 : 0,
          total_attempts:
            state === "failed" ? 20 : state === "acknowledged" ? 1 : 0,
          retry_version: 0,
          last_error_code: state === "failed" ? "service_retry" : null,
          acknowledged: state === "acknowledged",
        };
      }),
    },
  };
}

export async function setup(overrides = {}, recovery = false) {
  const { createElement } = await import("react");
  const testing = await import("@testing-library/react");
  const { EmployeeMessageReview } = await import("./EmployeeMemoryDialog.tsx");
  const { EmployeeMemoryPanel } = await import("./EmployeeMemoryPanel.tsx");
  const { useEmployeeReview } = await import("./useEmployeeReview.ts");
  const state = {
    signatures: [],
    writes: [],
    reads: [],
    previews: [],
    heldPreviews: [],
    heldReads: [],
    exportReads: [],
    exportWrites: [],
    heldExports: [],
    exportReadStatus: 200,
    exportStatus: 200,
    canApprove: true,
    facts: [],
    status: 201,
    previewStatus: 200,
    factsStatus: 200,
    holdPreview: false,
    holdRead: false,
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
      const exportMatch = parsed.pathname.match(
        /^\/api\/v1\/employees\/([^/]+)\/reviewed-memory\/([^/]+)\/export(?:\/retry\/(publish|withdraw))?$/,
      );
      if (exportMatch) {
        assert.equal(decodeURIComponent(exportMatch[1]), employee.employee_id);
        const fact = exportMatch[2];
        if (init.method === "POST") {
          state.exportWrites.push({
            url,
            body: init.body,
            signal: init.signal,
          });
          const body = JSON.parse(init.body);
          const replay = state.exportWrites
            .slice(0, -1)
            .some((write) => write.body === init.body && write.url === url);
          const action = exportMatch[3];
          if (!replay) {
            if (!action)
              state.exportRecord = publication("pending", "pending", fact);
            else {
              const job = state.exportRecord.export.jobs.find(
                (row) => row.action === action,
              );
              Object.assign(job, {
                state: "pending",
                acknowledged: false,
                attempt_count: 0,
                last_error_code: null,
                retry_version: body.expected_version + 1,
              });
            }
          }
          const receipt = {
            operation_id: body.operation_id,
            created: !replay,
            result_version: action ? body.expected_version + 1 : 0,
            record: structuredClone(state.exportRecord),
          };
          return Response.json(state.exportReceipt?.(receipt) ?? receipt, {
            status: state.exportStatus,
          });
        }
        state.exportReads.push({ url, signal: init.signal });
        const value = structuredClone(
          state.exportRecord ?? { fact_id: fact, export: null },
        );
        const status = state.exportReadStatus;
        if (state.holdExport)
          await new Promise((resolve) => state.heldExports.push(resolve));
        return Response.json(value, { status });
      }
      if (parsed.pathname.endsWith("/preview")) {
        const body = JSON.parse(init.body);
        state.previews.push({ body, signal: init.signal });
        const payload = structuredClone(state.previewOverride ?? preview(body));
        const status = state.previewStatus;
        if (state.holdPreview)
          await new Promise((resolve) => state.heldPreviews.push(resolve));
        return Response.json({ preview: payload }, { status });
      }
      if (init.method === "POST") {
        state.writes.push({ url, body: init.body, signal: init.signal });
        const body = JSON.parse(init.body);
        const stop = parsed.pathname.endsWith("/stop");
        const fact = structuredClone(
          state.receiptFact ?? saved(false, body.fact),
        );
        if (stop)
          Object.assign(fact, {
            status: "stopped",
            version: 2,
            can_stop: false,
            revoked_at: new Date().toISOString(),
          });
        else {
          fact.expires_at = body.fact.expires_at;
          if (fact.source_current) fact.content = body.fact.content;
        }
        if (state.status < 300) state.facts = [fact];
        return Response.json(
          {
            operation_id: body.operation_id,
            created: state.writes.length === 1,
            effect: {
              fact_id: fact.id,
              action: stop ? "stop" : "approve",
              result_version: stop ? 2 : 1,
            },
            fact,
          },
          { status: state.status },
        );
      }
      if (parsed.pathname === "/api/v1/employees")
        return Response.json({
          employees: [employee],
          has_more: false,
          next_after: null,
        });
      assert.equal(parsed.pathname, path);
      state.reads.push({ url, signal: init.signal });
      const payload = structuredClone(
        state.page?.(parsed.searchParams.get("after")) ?? {
          can_approve: state.canApprove,
          facts: state.facts,
          next_after: state.nextAfter ?? null,
        },
      );
      const status = state.factsStatus;
      if (state.holdRead)
        await new Promise((resolve) => state.heldReads.push(resolve));
      return Response.json(payload, { status });
    },
  );
  function RecoveryHarness({
    open = true,
    currentActor = actor,
    currentClient = client,
  } = {}) {
    const review = useEmployeeReview(
      currentClient,
      currentActor,
      employee.employee_id,
      null,
      "",
      "experience",
      open,
    );
    state.review = review;
    return open
      ? createElement(EmployeeMemoryPanel, {
          state: review,
          employeeName: "Ada",
          destinationName: "",
          canPreview: false,
          channels: [
            { id: channel, name: "Source channel" },
            { id: destination, name: "Destination channel" },
          ],
        })
      : null;
  }
  function Harness({
    open = true,
    currentActor = actor,
    currentMessage = message,
    currentClient = client,
  } = {}) {
    if (recovery)
      return createElement(RecoveryHarness, {
        open,
        currentActor,
        currentClient,
      });
    return createElement(EmployeeMessageReview, {
      client: currentClient,
      actor: currentActor,
      channel,
      message: currentMessage,
      open,
      channels: [
        { id: channel, name: "Source channel" },
        { id: destination, name: "Destination channel" },
      ],
      render: (body) => (open ? body : null),
    });
  }
  const view = testing.render(createElement(Harness));
  if (!recovery)
    await testing.waitFor(() =>
      assert.equal(view.getByLabelText("Employee").disabled, false),
    );
  else
    await testing.waitFor(() => assert.equal(state.review.facts.ready, true));
  async function choose() {
    await testing.act(async () =>
      testing.fireEvent.change(view.getByLabelText("Employee"), {
        target: { value: employee.employee_id },
      }),
    );
    await testing.waitFor(() =>
      assert.ok(view.getByRole("region", { name: "Saved employee memory" })),
    );
    if (
      state.canApprove &&
      state.previewStatus === 200 &&
      !state.holdPreview &&
      !state.previewOverride
    )
      await testing.waitFor(() =>
        assert.ok(view.getByLabelText("Edited memory text")),
      );
  }
  async function fill(review = true) {
    await testing.act(async () => {
      testing.fireEvent.change(view.getByLabelText("Edited memory text"), {
        target: { value: edited },
      });
      const future = new Date(Date.now() + 3600000);
      const local = new Date(
        future.getTime() - future.getTimezoneOffset() * 60000,
      )
        .toISOString()
        .slice(0, 16);
      testing.fireEvent.change(view.getByLabelText("Approval expires"), {
        target: { value: local },
      });
    });
    if (review)
      await testing.act(async () =>
        testing.fireEvent.click(
          view.getByRole("checkbox", { name: /I explicitly approve sharing/ }),
        ),
      );
  }
  const submit = async () =>
    testing.act(async () =>
      testing.fireEvent.submit(
        view.getByRole("form", { name: "Approve employee memory" }),
      ),
    );
  return {
    ...testing,
    view,
    state,
    client,
    choose,
    fill,
    submit,
    rerender: async (props) =>
      testing.act(async () => view.rerender(createElement(Harness, props))),
  };
}
