import assert from "node:assert/strict";
import test from "node:test";
import { loadEmployeeDirectory, employeeStateLabel } from "./directory.ts";
const key = "ab".repeat(32);
const ada = {
  employee_id: "ada",
  name: "Ada",
  title: "Planner",
  status: "active",
  office_public_keys: [key],
};
const page = (employees, more = false, cursor = null) => ({
  employees,
  has_more: more,
  next_after: cursor,
});
const signal = () => new AbortController().signal;

test("server identities survive model/key changes and active state never claims provider availability", async () => {
  const calls = [];
  const client = {
    employees: async (_signal, after) => {
      calls.push(after);
      return after ? page([ada]) : page([], true, "first");
    },
    runs: async () => ({ runs: [{ employee_id: "ada", status: "completed" }] }),
  };
  let result = await loadEmployeeDirectory(client, signal());
  assert.deepEqual(calls, [undefined, "first"]);
  assert.equal(employeeStateLabel(result[key]), "Active");
  client.runs = async () => ({
    runs: [{ employee_id: "ada", status: "running" }],
  });
  result = await loadEmployeeDirectory(client, signal());
  assert.equal(employeeStateLabel(result[key]), "Working");
  client.employees = async () => page([]);
  result = await loadEmployeeDirectory(client, signal());
  assert.deepEqual(result, {});
});

test("ambiguous keys, repeated cursors and abort never yield authoritative partial identity", async () => {
  const client = {
    employees: async () => page([ada, { ...ada, employee_id: "bora" }]),
    runs: async () => ({ runs: [] }),
  };
  await assert.rejects(loadEmployeeDirectory(client, signal()), /ambiguous/);
  let reads = 0;
  client.employees = async () => {
    reads++;
    return page([], true, "same");
  };
  await assert.rejects(loadEmployeeDirectory(client, signal()), /pagination/);
  assert.equal(reads, 2);
  const controller = new AbortController();
  client.employees = async () => {
    controller.abort();
    return page([ada]);
  };
  await assert.rejects(
    loadEmployeeDirectory(client, controller.signal),
    /retired/,
  );
});

test("a visible active run cannot create an employee missing from the authorized directory", async () => {
  const value = await loadEmployeeDirectory(
    {
      employees: async () => page([]),
      runs: async () => ({ runs: [{ employee_id: "ada", status: "running" }] }),
    },
    signal(),
  );
  assert.deepEqual(value, {});
});
