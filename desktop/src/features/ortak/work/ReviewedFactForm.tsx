import { useState } from "react";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import type { Employee, WorkItem, WorkExecution, WorkProject } from "../types";
import { Field, Select, type SubmitWork } from "./fields";
import type { ReviewedFactSource } from "./memoryTypes";

export function ReviewedFactForm({
  project,
  employee,
  item,
  executions,
  disabled,
  submit,
}: {
  project: WorkProject;
  employee: Employee;
  item: WorkItem | null;
  executions: WorkExecution[];
  disabled: boolean;
  submit: SubmitWork;
}) {
  const [selectedEvidence, setSelectedEvidence] = useState("");
  const [error, setError] = useState<string | null>(null);
  const sources: { label: string; source: ReviewedFactSource }[] = [];
  if (item?.source_message_id)
    sources.push({
      label: "Selected work's Office source",
      source: { kind: "conversation", message_id: item.source_message_id },
    });
  for (const execution of executions) {
    if (execution.employee_id === employee.employee_id && execution.artifact_id)
      sources.push({
        label: `Saved deliverable · version ${execution.execution_version}`,
        source: { kind: "artifact", artifact_id: execution.artifact_id },
      });
  }
  if (
    !project.can_review ||
    project.status !== "active" ||
    employee.status !== "active" ||
    !sources.length
  )
    return (
      <p className="text-sm text-muted-foreground">
        Approval requires an active project and employee, current review
        permission, and a selected work item with visible Office evidence or a
        saved deliverable. Existing records and Stop using remain available
        below.
      </p>
    );
  return (
    <form
      aria-label="Approve project memory"
      className="flex flex-col gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        if (disabled) return;
        const data = new FormData(event.currentTarget);
        const content = String(data.get("content") ?? "").trim();
        const selectedSource = data.get("source");
        const source = sources.find(
          (entry) => JSON.stringify(entry.source) === selectedSource,
        )?.source;
        const expiry = new Date(String(data.get("expiry")));
        const reviewed = data.get("reviewed") === "on";
        if (
          !source ||
          !reviewed ||
          !content ||
          new TextEncoder().encode(content).length > 4096 ||
          !Number.isFinite(expiry.getTime()) ||
          expiry.getTime() <= Date.now() ||
          expiry.getTime() > Date.now() + 90 * 86400000
        ) {
          setError(
            "Review the text and audience, keep the fact within 4 KiB, and choose a future expiry within 90 days.",
          );
          return;
        }
        setError(null);
        submit(
          `/api/v1/projects/${encodeURIComponent(project.id)}/reviewed-memory`,
          "Reviewed fact",
          {
            fact: {
              employee_id: employee.employee_id,
              source,
              content,
              expires_at: expiry.toISOString(),
              reviewed,
            },
          },
        );
      }}
    >
      <fieldset disabled={disabled} className="flex flex-col gap-3">
        <Field label="Evidence reviewed">
          {(id) => (
            <Select
              id={id}
              name="source"
              required
              value={
                sources.some(
                  (entry) => JSON.stringify(entry.source) === selectedEvidence,
                )
                  ? selectedEvidence
                  : ""
              }
              onChange={(event) => setSelectedEvidence(event.target.value)}
            >
              <option value="" disabled>
                Choose evidence
              </option>
              {sources.map((entry) => (
                <option
                  key={JSON.stringify(entry.source)}
                  value={JSON.stringify(entry.source)}
                >
                  {entry.label}
                </option>
              ))}
            </Select>
          )}
        </Field>
        <Field label="Edited fact">
          {(id) => (
            <Textarea
              id={id}
              name="content"
              required
              maxLength={4096}
              rows={3}
            />
          )}
        </Field>
        <p className="text-xs text-muted-foreground">
          Write only the reviewed fact you approve for{" "}
          {employee.name ?? employee.employee_id} in {project.name}. Source
          output is never copied automatically.
        </p>
        <Field label="Use until (local time)">
          {(id) => (
            <Input id={id} name="expiry" type="datetime-local" required />
          )}
        </Field>
        <Field label="I reviewed this fact, evidence, employee and project audience">
          {(id) => (
            <Input
              id={id}
              name="reviewed"
              type="checkbox"
              required
              className="size-4"
            />
          )}
        </Field>
        <Button type="submit" variant="outline" className="self-start">
          Approve fact
        </Button>
      </fieldset>
      {error ? (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}
    </form>
  );
}
