import { useEffect, useRef, useState } from "react";
import { Button } from "@/shared/ui/button";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { OrtakApiError, type OrtakClient } from "../client";
import { RunPanel } from "../RunPanel";
import type { Employee, WorkExecution, WorkItem, WorkProject } from "../types";
import { Field, Select, type SubmitWork } from "./fields";

export function ExecutionPanel({
  client,
  item,
  project,
  employees,
  executions,
  disabled,
  submit,
  revoke,
}: {
  client: OrtakClient;
  item: WorkItem;
  project: WorkProject;
  employees: Employee[];
  executions: WorkExecution[];
  disabled: boolean;
  submit: SubmitWork;
  revoke: () => void;
}) {
  const [selected, setSelected] = useState<string | null>(null);
  const [artifact, setArtifact] = useState<{ id: string; text: string } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const controller = useRef<AbortController | null>(null);
  useEffect(() => () => controller.current?.abort(), []);
  const active = executions.some((execution) => !execution.reconciled);
  const assigned = employees.filter(
    (employee) =>
      employee.status === "active" &&
      item.assignments.some(
        (assignment) =>
          assignment.employee_id === employee.employee_id &&
          assignment.status === "active" &&
          assignment.role !== "reviewer",
      ),
  );
  const definitionReady =
    ["ready", "in_progress"].includes(item.state) &&
    item.criteria.every((criterion) => criterion.status === "pending") &&
    item.approvals.every((approval) => approval.status === "pending");
  const canStart =
    project.can_contribute &&
    project.status === "active" &&
    definitionReady &&
    !active &&
    assigned.length > 0;
  const execution =
    executions.find((entry) => entry.run_id === selected) ?? executions[0];
  async function loadArtifact(id: string) {
    controller.current?.abort();
    const attempt = new AbortController();
    controller.current = attempt;
    setArtifact(null);
    setError(null);
    setLoading(true);
    try {
      const text = await client.textArtifact(item.id, id, attempt.signal);
      if (!attempt.signal.aborted) setArtifact({ id, text });
    } catch (cause) {
      if (attempt.signal.aborted) return;
      setError(
        cause instanceof Error
          ? cause.message
          : "The deliverable could not be loaded. Try opening it again.",
      );
      if (
        cause instanceof OrtakApiError &&
        [401, 403, 404].includes(cause.status)
      )
        revoke();
    } finally {
      if (!attempt.signal.aborted) setLoading(false);
    }
  }
  return (
    <section aria-label="Employee execution" className="flex flex-col gap-3">
      <h4 className="text-sm font-semibold">Employee execution</h4>
      <p className="text-sm text-muted-foreground">
        Start the saved assignment explicitly. A complete text deliverable moves
        this item to review; acceptance criteria and approval gates still
        require a human decision.
      </p>
      {canStart ? (
        <form
          aria-label="Start employee execution"
          onSubmit={(event) => {
            event.preventDefault();
            submit(
              `/api/v1/work-items/${encodeURIComponent(item.id)}/executions`,
              "Execution request",
              {
                expected_version: item.version,
                employee_id: new FormData(event.currentTarget).get("employee"),
              },
            );
          }}
        >
          <fieldset disabled={disabled} className="flex flex-col gap-3">
            <Field label="Assigned employee to execute">
              {(id) => (
                <Select
                  id={id}
                  name="employee"
                  defaultValue={assigned[0]?.employee_id}
                  required
                >
                  {assigned.map((employee) => (
                    <option
                      key={employee.employee_id}
                      value={employee.employee_id}
                    >
                      {employee.name ?? "Unnamed employee"}
                    </option>
                  ))}
                </Select>
              )}
            </Field>
            <Button type="submit" className="self-start">
              Start execution
            </Button>
          </fieldset>
        </form>
      ) : (
        <p className="text-sm text-muted-foreground">
          {active
            ? "An execution is queued, active, or settling its result. Editing the definition or status stops that execution; use its activity controls to request cancellation."
            : !project.can_contribute
              ? "Starting requires current project contribution permission."
              : project.status !== "active"
                ? "This project is archived. Existing activity and deliverables remain readable."
                : !definitionReady
                  ? "Starting requires ready or in progress work with unresolved human review decisions. Existing activity remains available below."
                  : "Assign an active owner or contributor from the employee directory before starting."}
        </p>
      )}
      {executions.length ? (
        <>
          <Field label="Saved execution">
            {(id) => (
              <Select
                id={id}
                value={execution?.run_id ?? ""}
                onChange={(event) => {
                  controller.current?.abort();
                  setLoading(false);
                  setArtifact(null);
                  setError(null);
                  setSelected(event.target.value);
                }}
              >
                {executions.map((entry) => (
                  <option key={entry.run_id} value={entry.run_id}>
                    Version {entry.execution_version} · {entry.status} ·{" "}
                    {employees.find(
                      (employee) => employee.employee_id === entry.employee_id,
                    )?.name ?? entry.employee_id}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          <p className="text-xs text-muted-foreground">
            The most recent 20 visible executions are shown. Work text output
            does not publish an Office reply or request a post-artifact memory
            write.
          </p>
          {execution?.artifact_id ? (
            <Button
              variant="outline"
              className="self-start"
              disabled={loading}
              onClick={() => void loadArtifact(execution.artifact_id as string)}
            >
              {loading ? "Loading deliverable…" : "Open text deliverable"}
            </Button>
          ) : execution?.reconciled ? (
            <p role="status" className="text-sm">
              No deliverable was saved for this execution.{" "}
              {execution.output_code ? `Result: ${execution.output_code}.` : ""}
            </p>
          ) : null}
          {error ? (
            <Alert variant="destructive">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : null}
          {artifact && artifact.id === execution?.artifact_id ? (
            <section
              aria-label="Saved text deliverable"
              className="rounded-lg border p-4"
            >
              <h5 className="mb-3 text-sm font-semibold">
                Saved text deliverable
              </h5>
              <pre className="whitespace-pre-wrap break-words font-sans text-sm">
                {artifact.text}
              </pre>
            </section>
          ) : null}
          {execution ? (
            <RunPanel
              key={execution.run_id}
              client={client}
              runId={execution.run_id}
              employeeName={
                employees.find(
                  (employee) => employee.employee_id === execution.employee_id,
                )?.name ?? "Assigned employee"
              }
              onAccessRevoked={revoke}
            />
          ) : null}
        </>
      ) : (
        <p className="text-sm text-muted-foreground">
          No employee execution has been requested.
        </p>
      )}
    </section>
  );
}
