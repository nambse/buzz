import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import { JSDOM } from "jsdom";
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
before(() =>
  Object.assign(globalThis, {
    document: dom.window.document,
    window: dom.window,
    HTMLElement: dom.window.HTMLElement,
    FormData: dom.window.FormData,
    IS_REACT_ACT_ENVIRONMENT: true,
  }),
);
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());
const item = {
  id: "work",
  version: 7,
  state: "in_progress",
  assignments: [{ employee_id: "cem", role: "owner", status: "active" }],
};
const project = { status: "active", can_contribute: true };
const employees = [
  { employee_id: "cem", name: "Cem", status: "disabled" },
  { employee_id: "ada", name: "Ada", status: "active" },
];

test("release remains reachable for an inactive employee absent from the directory and submits one versioned command", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, within } = await import("@testing-library/react");
  const { AssignmentPanel } = await import("./AssignmentPanel.tsx");
  const calls = [];
  const view = render(
    createElement(AssignmentPanel, {
      item,
      project,
      employees: [],
      disabled: false,
      submit: (...args) => calls.push(args),
    }),
  );
  fireEvent.click(
    view.getByText(
      "Change assignment for Employee outside this directory page",
    ),
  );
  const form = view.getByRole("form", {
    name: "Change assignment for Employee outside this directory page",
  });
  fireEvent.change(
    within(form).getByLabelText("Reason for assignment change"),
    { target: { value: "Employee unavailable" } },
  );
  fireEvent.submit(form);
  assert.deepEqual(calls, [
    [
      "/api/v1/work-items/work/assignments/cem/release",
      "Release assignment",
      { expected_version: 7, reason: "Employee unavailable" },
    ],
  ]);
  assert.equal(view.queryByRole("form", { name: "Assign employee" }), null);
});

test("replacement uses one atomic operation and excludes active unrelated assignments and inactive targets", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, within } = await import("@testing-library/react");
  const { AssignmentPanel } = await import("./AssignmentPanel.tsx");
  const calls = [];
  const selected = {
    ...item,
    assignments: [
      ...item.assignments,
      { employee_id: "other", role: "contributor", status: "active" },
    ],
  };
  const view = render(
    createElement(AssignmentPanel, {
      item: selected,
      project,
      employees: [
        ...employees,
        { employee_id: "other", name: "Already assigned", status: "active" },
      ],
      disabled: false,
      submit: (...args) => calls.push(args),
    }),
  );
  fireEvent.click(view.getByText("Change assignment for Cem"));
  const form = view.getByRole("form", { name: "Change assignment for Cem" });
  const controls = within(form);
  fireEvent.change(controls.getByLabelText("Assignment change"), {
    target: { value: "reassign" },
  });
  const options = [
    ...controls.getByLabelText("Employee from current directory page").options,
  ].map((o) => o.value);
  assert.deepEqual(options, ["", "ada"]);
  fireEvent.change(
    controls.getByLabelText("Employee from current directory page"),
    { target: { value: "ada" } },
  );
  fireEvent.change(controls.getByLabelText("Assignment role"), {
    target: { value: "contributor" },
  });
  fireEvent.change(controls.getByLabelText("Reason for assignment change"), {
    target: { value: "Hand over" },
  });
  fireEvent.submit(form);
  assert.deepEqual(calls, [
    [
      "/api/v1/work-items/work/assignments/cem/reassign",
      "Reassign employee",
      {
        expected_version: 7,
        reason: "Hand over",
        replacement_employee_id: "ada",
        role: "contributor",
      },
    ],
  ]);
});

test("assignment controls respect pending writes, current role, archive, terminal and released status", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, within } = await import("@testing-library/react");
  const { AssignmentPanel } = await import("./AssignmentPanel.tsx");
  const calls = [];
  const base = {
    item,
    project,
    employees,
    disabled: true,
    submit: (...args) => calls.push(args),
  };
  const view = render(createElement(AssignmentPanel, base));
  fireEvent.click(view.getByText("Change assignment for Cem"));
  const form = view.getByRole("form", { name: "Change assignment for Cem" });
  assert.equal(
    within(form)
      .getByRole("button", { name: "Release assignment" })
      .closest("fieldset").disabled,
    true,
  );
  fireEvent.submit(form);
  assert.equal(calls.length, 0);
  for (const override of [
    { project: { ...project, can_contribute: false } },
    { project: { ...project, status: "archived" } },
    { item: { ...item, state: "completed" } },
    { item: { ...item, state: "cancelled" } },
  ]) {
    view.rerender(
      createElement(AssignmentPanel, { ...base, ...override, disabled: false }),
    );
    assert.equal(view.queryByRole("form"), null);
  }
  view.rerender(
    createElement(AssignmentPanel, {
      ...base,
      disabled: false,
      employees: [],
      item: {
        ...item,
        assignments: [{ ...item.assignments[0], status: "released" }],
      },
    }),
  );
  assert.equal(view.queryByText(/Change assignment for/), null);
});

test("uncertain reassignment retries retain the exact operation bytes and require explicit retry", async () => {
  const { createElement } = await import("react");
  const { render, fireEvent, act, within } = await import(
    "@testing-library/react"
  );
  const { AssignmentPanel } = await import("./AssignmentPanel.tsx");
  const { useWorkMutation } = await import("./useWorkMutation.ts");
  const calls = [];
  let refreshed = 0;
  const client = {
    workMutation: async (path, body) => {
      calls.push({ path, body });
      if (calls.length === 1) throw new Error("lost confirmation");
    },
  };
  function Harness() {
    const mutation = useWorkMutation(
      client,
      () => refreshed++,
      () => {},
    );
    return createElement(
      "div",
      null,
      createElement(AssignmentPanel, {
        item,
        project,
        employees,
        disabled: mutation.busy || !!mutation.pending,
        submit: mutation.submit,
      }),
      mutation.pending
        ? createElement(
            "button",
            { onClick: mutation.retry },
            "Retry same operation",
          )
        : null,
    );
  }
  const view = render(createElement(Harness));
  fireEvent.click(view.getByText("Change assignment for Cem"));
  const form = view.getByRole("form", { name: "Change assignment for Cem" });
  const controls = within(form);
  fireEvent.change(controls.getByLabelText("Assignment change"), {
    target: { value: "reassign" },
  });
  fireEvent.change(
    controls.getByLabelText("Employee from current directory page"),
    { target: { value: "ada" } },
  );
  fireEvent.change(controls.getByLabelText("Reason for assignment change"), {
    target: { value: "Replacement" },
  });
  await act(async () => fireEvent.submit(form));
  assert.equal(calls.length, 1);
  fireEvent.submit(form);
  assert.equal(calls.length, 1);
  await act(async () =>
    fireEvent.click(view.getByRole("button", { name: "Retry same operation" })),
  );
  assert.deepEqual(calls[0], calls[1]);
  assert.ok(JSON.parse(calls[0].body).operation_id);
  assert.equal(refreshed, 1);
});
