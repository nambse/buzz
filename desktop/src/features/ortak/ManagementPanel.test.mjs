import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";
import { createOrtakClient, OrtakApiError } from "./client.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
  pretendToBeVisual: true,
});
before(() =>
  Object.assign(globalThis, {
    document: dom.window.document,
    window: dom.window,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    getComputedStyle: dom.window.getComputedStyle.bind(dom.window),
    requestAnimationFrame: dom.window.requestAnimationFrame.bind(dom.window),
    cancelAnimationFrame: dom.window.cancelAnimationFrame.bind(dom.window),
  }),
);
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());

const choice = {
  catalog_id: "catalog-1",
  employee_id: "ada",
  label: "Ada prepared profile",
  model: "gpt-6-astra",
  thinking: "max",
  expected_revision_id: null,
  status: null,
  can_save_draft: true,
};
const catalog = {
  choices: [choice],
  create_supported: false,
  lifecycle_supported: false,
};
const page = (commands = []) => ({
  employee_id: "ada",
  commands,
  expected_revision_id: null,
  lifecycle_supported: false,
});
const draft = (body) => ({
  ...body,
  employee_id: "ada",
  action: "adopt",
  model: choice.model,
  thinking: choice.thinking,
});
const request = {
  idempotency_key: "stable-request",
  action: "adopt",
  draft_id: "draft-1",
  operation_id: null,
  expected_revision_id: null,
};
const baseClient = () => ({
  preparedEmployees: async () => catalog,
  managementCommands: async () => page(),
});

test("prepared panel saves an immutable selection then admits a pending real command without claiming activation", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, act } = await import("@testing-library/react");
  const { ManagementPanel } = await import(
    "./provisioning/ManagementPanel.tsx"
  );
  let saved;
  const commands = [];
  const client = {
    ...baseClient(),
    configurationDraft: async (employee, body) => {
      assert.equal(employee, "ada");
      saved = body;
      return draft(body);
    },
    managementCommand: async (employee, body) => {
      assert.equal(employee, "ada");
      assert.equal(body.draft_id, saved.draft_id);
      assert.equal(body.action, "adopt");
      assert.deepEqual(Object.keys(body).sort(), [
        "action",
        "draft_id",
        "expected_lifecycle_epoch",
        "expected_revision_id",
        "idempotency_key",
        "operation_id",
      ]);
      commands.push({
        command_id: "command-1",
        action: "adopt",
        status: "pending",
        attempts: 0,
        operation_id: null,
        error_code: null,
        created_at: "2026-09-05T12:00:00Z",
        updated_at: "2026-09-05T12:00:00Z",
        can_retry: false,
        can_compensate: false,
      });
      return { command_id: "command-1", employee_id: employee };
    },
    managementCommands: async () => page(commands),
  };
  const view = render(createElement(ManagementPanel, { client }));
  await act(async () =>
    fireEvent.click(
      view.getByRole("button", { name: "Manage prepared employees" }),
    ),
  );
  await act(async () =>
    fireEvent.click(
      view.getByRole("button", { name: "Save draft for Ada prepared profile" }),
    ),
  );
  assert.equal(
    view.getByText("Saved draft for ada") === document.activeElement,
    true,
  );
  assert.ok(view.getAllByText(/gpt-6-astra/).length > 0);
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Check and activate" })),
  );
  assert.ok(view.getByText("pending"));
  assert.equal(
    view.queryByRole("button", { name: /^create|disable|re-enable/i }),
    null,
  );
  assert.equal(view.queryByText("active", { exact: true }), null);
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Close management" })),
  );
  assert.equal(
    document.activeElement ===
      view.getByRole("button", { name: "Manage prepared employees" }),
    true,
  );
});

test("a lost acknowledgement retries the identical command body and key", async () => {
  const { renderHook, act } = await import("@testing-library/react");
  const { useManagement } = await import("./provisioning/useManagement.ts");
  const bodies = [];
  const client = {
    ...baseClient(),
    managementCommand: async (employee, body) => {
      bodies.push(structuredClone(body));
      if (bodies.length === 1) throw new TypeError("disconnected");
      return { employee_id: employee, command_id: "retained-command" };
    },
  };
  const view = renderHook(() => useManagement(client));
  await act(async () => {});
  await act(async () => view.result.current.command("ada", request));
  assert.equal(view.result.current.retryable, true);
  await act(async () => view.result.current.retryRequest());
  assert.equal(bodies.length, 2);
  assert.deepEqual(bodies[0], bodies[1]);
  assert.equal(view.result.current.retryable, false);
});

test("client replacement aborts draft requests and rejects late private results", async () => {
  const { renderHook, act } = await import("@testing-library/react");
  const { useManagement } = await import("./provisioning/useManagement.ts");
  let finish;
  let signal;
  const old = {
    ...baseClient(),
    configurationDraft: (_employee, _body, incoming) => {
      signal = incoming;
      return new Promise((resolve) => {
        finish = resolve;
      });
    },
  };
  const next = {
    ...baseClient(),
    preparedEmployees: async () => ({ ...catalog, choices: [] }),
  };
  const view = renderHook(({ client }) => useManagement(client), {
    initialProps: { client: old },
  });
  await act(async () => {});
  const body = {
    draft_id: "draft-1",
    catalog_id: choice.catalog_id,
    expected_revision_id: null,
  };
  await act(async () => {
    void view.result.current.saveDraft("ada", body);
  });
  view.rerender({ client: next });
  assert.equal(view.result.current.draft, null);
  assert.equal(signal.aborted, true);
  await act(async () => finish(draft(body)));
  assert.equal(view.result.current.draft, null);
  assert.equal(view.result.current.catalog.choices.length, 0);
});

test("mutation revocation clears private data and fences an overlapping successful read", async () => {
  const { renderHook, act } = await import("@testing-library/react");
  const { useManagement } = await import("./provisioning/useManagement.ts");
  let finishRead;
  let reading = 0;
  const client = {
    ...baseClient(),
    preparedEmployees: async () => {
      if (++reading === 1) return catalog;
      return new Promise((resolve) => {
        finishRead = resolve;
      });
    },
    managementCommand: async () => {
      throw new OrtakApiError(403, "forbidden");
    },
  };
  const view = renderHook(() => useManagement(client));
  await act(async () => {});
  await act(async () => view.result.current.refresh());
  assert.equal(typeof finishRead, "function");
  await act(async () => view.result.current.command("ada", request));
  assert.equal(view.result.current.catalog, null);
  assert.equal(view.result.current.draft, null);
  assert.equal(view.result.current.retryable, false);
  await act(async () => finishRead(catalog));
  assert.equal(view.result.current.catalog, null);
});

test("management reads stop after five failures and explicit refresh restarts recovery", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const { renderHook, act } = await import("@testing-library/react");
  const { useManagement } = await import("./provisioning/useManagement.ts");
  let calls = 0;
  const client = {
    ...baseClient(),
    preparedEmployees: async () => {
      calls++;
      throw new OrtakApiError(503, "unavailable");
    },
  };
  const view = renderHook(() => useManagement(client));
  await act(async () => {});
  for (const delay of [2000, 4000, 8000, 16000, 300000])
    await act(async () => context.mock.timers.tick(delay));
  assert.equal(calls, 5);
  await act(async () => view.result.current.refresh());
  assert.equal(calls, 6);
});

test("signed client binds exact immutable bodies, while each network retry receives a new NIP98 nonce", async () => {
  const signed = [];
  const fetched = [];
  const client = createOrtakClient(
    "https://api.example",
    async (event) => {
      signed.push(event);
      return event;
    },
    async (url, init) => {
      fetched.push({ url, init });
      return Response.json({});
    },
  );
  const signal = new AbortController().signal;
  await client.managementCommand("ada", request, signal);
  await client.managementCommand("ada", request, signal);
  assert.equal(fetched[0].init.body, fetched[1].init.body);
  assert.notEqual(
    Object.fromEntries(signed[0].tags).nonce,
    Object.fromEntries(signed[1].tags).nonce,
  );
  for (let index = 0; index < fetched.length; index++) {
    const tags = Object.fromEntries(signed[index].tags);
    assert.equal(tags.u, fetched[index].url);
    assert.equal(tags.method, "POST");
    const hash = await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode(fetched[index].init.body),
    );
    assert.equal(tags.payload, Buffer.from(hash).toString("hex"));
    assert.equal(fetched[index].init.credentials, "omit");
  }
});

test("lifecycle disable remains available after catalog retirement and shows pending separately from stopped", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, act } = await import("@testing-library/react");
  const { ManagementPanel } = await import(
    "./provisioning/ManagementPanel.tsx"
  );
  const employee = {
    employee_id: "ada",
    status: "active",
    expected_revision_id: "revision-1",
    expected_lifecycle_epoch: 0,
  };
  let pending = false;
  const bodies = [];
  const client = {
    preparedEmployees: async () => ({
      choices: [],
      employees: [employee],
      create_supported: false,
      lifecycle_supported: true,
    }),
    managementCommands: async () => ({
      ...employee,
      commands: pending
        ? [
            {
              command_id: "disable-1",
              action: "disable",
              status: "pending",
              attempts: 0,
              operation_id: null,
              error_code: null,
              created_at: "2026-09-05T12:00:00Z",
              updated_at: "2026-09-05T12:00:00Z",
              can_retry: false,
              can_compensate: false,
            },
          ]
        : [],
      lifecycle_supported: true,
      lifecycle: {
        can_disable: !pending && employee.status === "active",
        old_active_runs: employee.status === "disabled" ? 2 : 0,
        pending_stops: employee.status === "disabled" ? 2 : 0,
        failed_stops: 0,
      },
    }),
    managementCommand: async (id, body) => {
      assert.equal(id, "ada");
      bodies.push(body);
      pending = true;
      return { employee_id: id, command_id: "disable-1" };
    },
  };
  const view = render(createElement(ManagementPanel, { client }));
  await act(async () =>
    fireEvent.click(
      view.getByRole("button", { name: "Manage prepared employees" }),
    ),
  );
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Manage ada · active" })),
  );
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Disable ada" })),
  );
  assert.equal(bodies.length, 1);
  assert.deepEqual(
    { ...bodies[0], idempotency_key: "generated" },
    {
      idempotency_key: "generated",
      action: "disable",
      draft_id: null,
      operation_id: null,
      expected_revision_id: "revision-1",
      expected_lifecycle_epoch: 0,
    },
  );
  assert.ok(view.getByText("pending"));
  assert.match(view.container.textContent, /Last saved status: active/);
  assert.equal(view.queryByRole("button", { name: "Disable ada" }), null);
  employee.status = "disabled";
  employee.expected_lifecycle_epoch = 1;
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Refresh management" })),
  );
  assert.match(view.container.textContent, /Last saved status: disabled/);
  assert.match(view.container.textContent, /Earlier runs still active: 2/);
  assert.match(view.container.textContent, /Pending stops: 2/);
});

test("re-enable requires the new prepared draft and sends the exact disabled epoch and revision", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, act } = await import("@testing-library/react");
  const { ManagementPanel } = await import(
    "./provisioning/ManagementPanel.tsx"
  );
  const disabled = {
    ...choice,
    status: "disabled",
    expected_revision_id: "revision-1",
    expected_lifecycle_epoch: 3,
  };
  const mutations = [];
  const client = {
    preparedEmployees: async () => ({
      choices: [disabled],
      employees: [disabled],
      create_supported: false,
      lifecycle_supported: true,
    }),
    managementCommands: async () => ({
      employee_id: "ada",
      status: "disabled",
      expected_revision_id: "revision-1",
      expected_lifecycle_epoch: 3,
      commands: [],
      lifecycle_supported: true,
      lifecycle: {
        can_disable: false,
        old_active_runs: 0,
        pending_stops: 0,
        failed_stops: 0,
      },
    }),
    configurationDraft: async (employee_id, body) => {
      assert.equal(body.expected_lifecycle_epoch, 3);
      assert.equal(body.expected_revision_id, "revision-1");
      mutations.push(["draft", body]);
      return {
        ...body,
        employee_id,
        action: "reenable",
        model: choice.model,
        thinking: choice.thinking,
      };
    },
    managementCommand: async (employee_id, body) => {
      mutations.push(["command", body]);
      return { employee_id, command_id: "reenable-1" };
    },
  };
  const view = render(createElement(ManagementPanel, { client }));
  await act(async () =>
    fireEvent.click(
      view.getByRole("button", { name: "Manage prepared employees" }),
    ),
  );
  assert.equal(
    view.queryByRole("button", { name: "Check and re-enable" }),
    null,
  );
  await act(async () =>
    fireEvent.click(
      view.getByRole("button", { name: "Save draft for Ada prepared profile" }),
    ),
  );
  assert.match(
    view.container.textContent,
    /stays disabled until every fresh health check passes/,
  );
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Check and re-enable" })),
  );
  assert.equal(mutations[1][1].draft_id, mutations[0][1].draft_id);
  assert.equal(mutations[1][1].action, "reenable");
  assert.equal(mutations[1][1].expected_lifecycle_epoch, 3);
  assert.match(view.container.textContent, /Last saved status: disabled/);
});

test("a newer employee lifecycle clears an already saved draft instead of presenting stale activation", async () => {
  const { renderHook, act } = await import("@testing-library/react");
  const { useManagement } = await import("./provisioning/useManagement.ts");
  const selected = {
    ...choice,
    expected_revision_id: "revision-1",
    expected_lifecycle_epoch: 2,
    status: "disabled",
  };
  const client = {
    preparedEmployees: async () => ({
      choices: [selected],
      employees: [selected],
      create_supported: false,
      lifecycle_supported: true,
    }),
    managementCommands: async () => ({
      employee_id: "ada",
      status: "disabled",
      expected_revision_id: "revision-1",
      expected_lifecycle_epoch: selected.expected_lifecycle_epoch,
      commands: [],
      lifecycle_supported: true,
      lifecycle: {
        can_disable: false,
        old_active_runs: 0,
        pending_stops: 0,
        failed_stops: 0,
      },
    }),
    configurationDraft: async (employee_id, body) => ({
      ...body,
      employee_id,
      action: "reenable",
      model: choice.model,
      thinking: choice.thinking,
    }),
  };
  const view = renderHook(() => useManagement(client));
  await act(async () => {});
  await act(async () =>
    view.result.current.saveDraft("ada", {
      draft_id: "saved-before-disable",
      catalog_id: choice.catalog_id,
      expected_revision_id: "revision-1",
      expected_lifecycle_epoch: 2,
    }),
  );
  assert.equal(view.result.current.draft.expected_lifecycle_epoch, 2);
  selected.expected_lifecycle_epoch = 3;
  await act(async () => view.result.current.refresh());
  assert.equal(view.result.current.draft, null);
});
