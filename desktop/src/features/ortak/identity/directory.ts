import type { createOrtakClient } from "../client";
import type { Employee } from "../types";

export type OfficeEmployee = {
  employee: Employee;
  activity: "Working" | "Queued" | "Waiting" | null;
};
export type EmployeeDirectory = Record<string, OfficeEmployee>;
type Reader = Pick<ReturnType<typeof createOrtakClient>, "employees" | "runs">;

/** Only authenticated identity rows and visible active runs enter this projection. */
export async function loadEmployeeDirectory(
  client: Reader,
  signal: AbortSignal,
): Promise<EmployeeDirectory> {
  const employees: Employee[] = [];
  let after: string | undefined;
  const cursors = new Set<string>();
  for (let page = 0; page < 8; page += 1) {
    const result = await client.employees(signal, after);
    if (signal.aborted) throw new Error("Employee identity request retired.");
    employees.push(...result.employees);
    if (employees.length > 200)
      throw new Error("Employee directory limit exceeded.");
    if (!result.has_more) break;
    const cursor = result.next_after;
    if (!cursor || cursors.has(cursor) || page === 7)
      throw new Error("Employee directory pagination could not complete.");
    cursors.add(cursor);
    after = cursor;
  }
  const runs = await client.runs(signal);
  if (signal.aborted) throw new Error("Employee identity request retired.");
  const result: EmployeeDirectory = {};
  const employeeIds = new Set<string>();
  for (const employee of employees) {
    if (employeeIds.has(employee.employee_id))
      throw new Error("Employee identity is ambiguous.");
    employeeIds.add(employee.employee_id);
    const keys = employee.office_public_keys ?? [];
    if (keys.length > 32) throw new Error("Employee key limit exceeded.");
    const run = runs.runs.find(
      (value) =>
        value.employee_id === employee.employee_id &&
        ["running", "queued", "waiting"].includes(value.status),
    );
    const activity =
      run?.status === "running"
        ? "Working"
        : run?.status === "queued"
          ? "Queued"
          : run?.status === "waiting"
            ? "Waiting"
            : null;
    for (const key of keys) {
      if (!/^[0-9a-f]{64}$/.test(key) || result[key])
        throw new Error("Employee Office identity is ambiguous.");
      result[key] = { employee, activity };
    }
  }
  return result;
}

/** Saved Employee status is not proof that a provider or signer is available. */
export function employeeStateLabel(value: OfficeEmployee) {
  return (
    value.activity ??
    {
      draft: "Draft",
      active: "Active",
      paused: "Paused",
      disabled: "Disabled",
    }[value.employee.status]
  );
}
