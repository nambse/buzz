import { useState } from "react";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import type { OrtakClient } from "../client";
import type { Employee, WorkItem, WorkExecution, WorkProject } from "../types";
import { Field, Select, type SubmitWork } from "./fields";
import { ReviewedFactForm } from "./ReviewedFactForm";
import { ReviewedRecall } from "./ReviewedRecall";
import { ReviewedPublication } from "./ReviewedPublication";
import { useReviewedMemory } from "./useReviewedMemory";

/** Project-level placement preserves recovery when a source Work item disappears. */
export function ReviewedMemoryPanel({
  client,
  project,
  employees,
  item,
  executions,
  disabled,
  refresh,
  submit,
  revoke,
}: {
  client: OrtakClient;
  project: WorkProject;
  employees: Employee[];
  item: WorkItem | null;
  executions: WorkExecution[];
  disabled: boolean;
  refresh: number;
  submit: SubmitWork;
  revoke: () => void;
}) {
  const [employeeId, setEmployeeId] = useState("");
  const [after, setAfter] = useState<string | undefined>();
  const [manualRefresh, setManualRefresh] = useState(0);
  const employee = employees.find((entry) => entry.employee_id === employeeId);
  const state = useReviewedMemory(
    client,
    project.id,
    employeeId,
    after,
    `${refresh}:${manualRefresh}`,
    revoke,
  );
  const paused = disabled || !state.fresh;
  const save: SubmitWork = (...args) => {
    if (!paused) submit(...args);
  };
  return (
    <section
      aria-label="Reviewed project memory"
      className="flex flex-col gap-4 rounded-xl border bg-card p-5"
    >
      <h4 className="text-base font-semibold">Reviewed project memory</h4>
      <p className="text-sm text-muted-foreground">
        Approve edited facts for one employee in this project. Expiry and Stop
        using end permitted use; approval records remain stored. Facts are saved
        for recall preview. Sending a fact to the selected Honcho reviewed store
        requires a separate publication approval. Runtime use additionally
        requires the operator to enable this employee and project; saving alone
        never enables use in runs.
      </p>
      <Field label="Memory audience: employee">
        {(id) => (
          <Select
            id={id}
            value={employeeId}
            onChange={(event) => {
              setEmployeeId(event.target.value);
              setAfter(undefined);
            }}
          >
            <option value="">Choose an employee</option>
            {employeeId && !employee ? (
              <option value={employeeId}>
                Selected employee outside current directory page
              </option>
            ) : null}
            {employees.map((entry) => (
              <option key={entry.employee_id} value={entry.employee_id}>
                {entry.name ?? entry.employee_id} · {entry.status}
              </option>
            ))}
          </Select>
        )}
      </Field>
      {employeeId ? (
        <Button
          variant="outline"
          className="self-start"
          onClick={() => setManualRefresh((value) => value + 1)}
        >
          Refresh reviewed memory
        </Button>
      ) : null}
      {state.error ? (
        <Alert variant="destructive">
          <AlertDescription>{state.error}</AlertDescription>
        </Alert>
      ) : null}
      {employeeId && !state.page && !state.error ? (
        <p role="status" className="text-sm">
          Checking current memory access…
        </p>
      ) : null}
      {state.page && !state.fresh ? (
        <p className="text-sm text-muted-foreground">
          Showing the last successful memory read. Changes are paused until
          memory access is refreshed; your unsaved entries are kept.
        </p>
      ) : null}
      {state.page ? (
        <>
          {employee ? (
            <ReviewedFactForm
              key={`${employeeId}:${item?.id ?? "none"}`}
              project={project}
              employee={employee}
              item={item}
              executions={executions}
              disabled={paused}
              submit={save}
            />
          ) : null}
          {!state.page.facts.length ? (
            <p className="text-sm text-muted-foreground">
              No reviewed facts in this page.
            </p>
          ) : (
            <ul className="flex flex-col gap-4">
              {state.page.facts.map((fact) => (
                <li
                  key={fact.id}
                  className="flex flex-col gap-2 rounded-lg border p-3 text-sm"
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge variant="secondary">
                      {fact.status === "revoked"
                        ? "Use stopped"
                        : fact.status === "expired"
                          ? "Use expired"
                          : "Reviewed"}
                    </Badge>
                    <span>Version {fact.version}</span>
                  </div>
                  {fact.source_visible ? (
                    <p className="whitespace-pre-wrap break-words">
                      {fact.content}
                    </p>
                  ) : (
                    <p>
                      Source evidence is no longer visible. Fact text is
                      withheld; authorized reviewers can still stop its use.
                    </p>
                  )}
                  <p className="text-xs text-muted-foreground">
                    Approved{" "}
                    <time dateTime={fact.approved_at}>
                      {new Date(fact.approved_at).toLocaleString()}
                    </time>{" "}
                    · Use until{" "}
                    <time dateTime={fact.expires_at}>
                      {new Date(fact.expires_at).toLocaleString()}
                    </time>
                  </p>
                  <details className="text-xs text-muted-foreground">
                    <summary className="cursor-pointer">
                      Approval provenance
                    </summary>
                    <p className="break-all">
                      Fact {fact.id} · Human {fact.approved_by}
                    </p>
                    {fact.source ? (
                      <p className="break-all">
                        {fact.source.kind === "conversation"
                          ? `Office message ${fact.source.message_id}`
                          : `Work artifact ${fact.source.artifact_id}`}
                      </p>
                    ) : null}
                    {fact.revoked_at ? (
                      <p>
                        Use stopped{" "}
                        <time dateTime={fact.revoked_at}>
                          {new Date(fact.revoked_at).toLocaleString()}
                        </time>
                        {fact.revoke_reason ? ` · ${fact.revoke_reason}` : ""}
                      </p>
                    ) : null}
                  </details>
                  <ReviewedPublication
                    fact={fact}
                    canReview={project.can_review}
                    disabled={paused}
                    submit={save}
                  />
                  {project.can_review && fact.version === 1 ? (
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={paused}
                      className="self-start"
                      aria-label={`Stop using fact ${fact.id}`}
                      onClick={() =>
                        save(
                          `/api/v1/projects/${encodeURIComponent(project.id)}/reviewed-memory/${encodeURIComponent(fact.id)}/stop`,
                          "Fact use stopped",
                          {
                            expected_version: fact.version,
                            reason: "Human selected Stop using",
                          },
                        )
                      }
                    >
                      Stop using
                    </Button>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
          <div className="flex flex-wrap gap-2">
            {after ? (
              <Button
                size="sm"
                variant="outline"
                onClick={() => setAfter(undefined)}
              >
                First facts
              </Button>
            ) : null}
            {state.page.next_after ? (
              <Button
                size="sm"
                variant="outline"
                onClick={() => setAfter(state.page?.next_after ?? undefined)}
              >
                More facts
              </Button>
            ) : null}
          </div>
          {project.status === "active" && employee?.status === "active" ? (
            <ReviewedRecall
              key={employeeId}
              client={client}
              project={project.id}
              employee={employeeId}
              stamp={state.stamp}
              revoke={revoke}
            />
          ) : (
            <p className="text-sm text-muted-foreground">
              Recall requires an active project and employee. Retained approvals
              remain inspectable.
            </p>
          )}
        </>
      ) : null}
    </section>
  );
}
