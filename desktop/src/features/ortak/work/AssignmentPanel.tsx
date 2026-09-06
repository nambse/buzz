import { useState } from "react";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import type { Employee, WorkItem, WorkProject } from "../types";
import { Field, Select, type SubmitWork } from "./fields";

type Props = {
  item: WorkItem;
  project: WorkProject;
  employees: Employee[];
  disabled: boolean;
  submit: SubmitWork;
};
type Assignment = WorkItem["assignments"][number];
const name = (employees: Employee[], id: string) =>
  employees.find((e) => e.employee_id === id)?.name ??
  "Employee outside this directory page";
const path = (item: WorkItem) =>
  `/api/v1/work-items/${encodeURIComponent(item.id)}/assignments`;

function RoleField() {
  return (
    <Field label="Assignment role">
      {(id) => (
        <Select id={id} name="role" defaultValue="owner">
          <option value="owner">Owner</option>
          <option value="contributor">Contributor</option>
          <option value="reviewer">Reviewer</option>
        </Select>
      )}
    </Field>
  );
}
function EmployeeField({ employees }: { employees: Employee[] }) {
  return (
    <Field label="Employee from current directory page">
      {(id) => (
        <Select id={id} name="employee" required defaultValue="">
          <option value="" disabled>
            Choose an employee
          </option>
          {employees.map((e) => (
            <option key={e.employee_id} value={e.employee_id}>
              {e.name ?? "Unnamed employee"}
            </option>
          ))}
        </Select>
      )}
    </Field>
  );
}

function ChangeAssignment({
  item,
  employees,
  assignment,
  disabled,
  submit,
}: Omit<Props, "project"> & { assignment: Assignment }) {
  const [action, setAction] = useState("release");
  const [error, setError] = useState("");
  const eligible = employees.filter(
    (e) =>
      e.status === "active" &&
      (e.employee_id === assignment.employee_id ||
        !item.assignments.some(
          (a) => a.employee_id === e.employee_id && a.status === "active",
        )),
  );
  const label = name(employees, assignment.employee_id);
  return (
    <details className="mt-2">
      <summary className="cursor-pointer text-sm">
        Change assignment for {label}
      </summary>
      <form
        aria-label={`Change assignment for ${label}`}
        className="mt-3"
        onSubmit={(event) => {
          event.preventDefault();
          if (disabled) return;
          const form = new FormData(event.currentTarget);
          const reason = String(form.get("reason")).trim();
          if (!reason || new TextEncoder().encode(reason).length > 1024) {
            setError("Enter a reason up to 1,024 bytes.");
            return;
          }
          setError("");
          submit(
            `${path(item)}/${encodeURIComponent(assignment.employee_id)}/${action}`,
            action === "release" ? "Release assignment" : "Reassign employee",
            {
              expected_version: item.version,
              reason,
              ...(action === "reassign"
                ? {
                    replacement_employee_id: form.get("employee"),
                    role: form.get("role"),
                  }
                : {}),
            },
          );
        }}
      >
        <fieldset disabled={disabled} className="flex flex-col gap-3">
          <Field label="Assignment change">
            {(id) => (
              <Select
                id={id}
                value={action}
                onChange={(event) => setAction(event.target.value)}
              >
                <option value="release">Release assignment</option>
                {eligible.length ? (
                  <option value="reassign">
                    Replace employee or change role
                  </option>
                ) : null}
              </Select>
            )}
          </Field>
          {action === "reassign" ? (
            <>
              <EmployeeField employees={eligible} />
              <RoleField />
            </>
          ) : null}
          <Field label="Reason for assignment change">
            {(id) => <Input id={id} name="reason" required maxLength={1024} />}
          </Field>
          <p className="text-xs text-muted-foreground">
            Saved history and human review requirements remain. Any execution
            from the previous work version loses authority; start a new
            execution when ready.
          </p>
          {error ? (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          ) : null}
          <Button type="submit" size="sm" variant="outline">
            {action === "release" ? "Release assignment" : "Save reassignment"}
          </Button>
        </fieldset>
      </form>
    </details>
  );
}

export function AssignmentPanel({
  item,
  project,
  employees,
  disabled,
  submit,
}: Props) {
  const editable =
    project.status === "active" &&
    project.can_contribute &&
    !["completed", "cancelled"].includes(item.state);
  const eligible = employees.filter(
    (e) =>
      e.status === "active" &&
      !item.assignments.some(
        (a) => a.employee_id === e.employee_id && a.status === "active",
      ),
  );
  return (
    <section aria-label="Manual assignments" className="flex flex-col gap-3">
      <h4 className="text-sm font-semibold">Manual assignments</h4>
      {!item.assignments.length ? (
        <p className="text-sm text-muted-foreground">No visible assignments.</p>
      ) : (
        <ul className="flex flex-col gap-3 text-sm">
          {item.assignments.map((a) => (
            <li key={a.employee_id}>
              {name(employees, a.employee_id)} · {a.role} · {a.status}
              {editable && a.status === "active" ? (
                <ChangeAssignment
                  key={`${item.id}:${item.version}:${a.employee_id}`}
                  item={item}
                  employees={employees}
                  assignment={a}
                  disabled={disabled}
                  submit={submit}
                />
              ) : null}
            </li>
          ))}
        </ul>
      )}
      {editable && eligible.length ? (
        <form
          aria-label="Assign employee"
          onSubmit={(event) => {
            event.preventDefault();
            if (disabled) return;
            const form = new FormData(event.currentTarget);
            submit(path(item), "Manual assignment", {
              expected_version: item.version,
              employee_id: form.get("employee"),
              role: form.get("role"),
            });
          }}
        >
          <fieldset disabled={disabled} className="flex flex-col gap-3">
            <EmployeeField employees={eligible} />
            <RoleField />
            <Button size="sm" variant="outline" type="submit">
              Save assignment
            </Button>
          </fieldset>
        </form>
      ) : null}
    </section>
  );
}
