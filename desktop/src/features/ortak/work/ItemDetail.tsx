import { useEffect, useRef } from "react";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import type { Employee, WorkItem, WorkProject, WorkExecution } from "../types";
import type { OrtakClient } from "../client";
import { ExecutionPanel } from "./ExecutionPanel";
import { DefinitionEditor } from "./DefinitionEditor";
import { AssignmentPanel } from "./AssignmentPanel";
import { DependencyPanel } from "./DependencyPanel";
import { DecompositionPanel } from "./DecompositionPanel";
import type { WorkSummary } from "../types";
import { Field, Select, type SubmitWork } from "./fields";
import { availableTransitions, stateLabel } from "./operations";

export function ItemDetail({
  item,
  project,
  employees,
  disabled,
  submit,
  client,
  executions,
  revoke,
  targets: dependencyTargets,
  selectItem,
}: {
  item: WorkItem;
  project: WorkProject;
  employees: Employee[];
  disabled: boolean;
  submit: SubmitWork;
  client: OrtakClient;
  executions: WorkExecution[];
  revoke: () => void;
  targets: WorkSummary[];
  selectItem: (id: string) => void;
}) {
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    heading.current?.focus();
  }, []);
  const path = `/api/v1/work-items/${encodeURIComponent(item.id)}`;
  const terminal = ["completed", "cancelled"].includes(item.state);
  const targets = availableTransitions(item.state, project);
  return (
    <section
      aria-label="Work item detail"
      className="flex min-w-0 flex-col gap-5 rounded-xl border bg-card p-5"
    >
      <header className="flex flex-col gap-2">
        <h3
          ref={heading}
          tabIndex={-1}
          className="break-words text-lg font-semibold outline-none"
        >
          {item.title}
        </h3>
        <div className="flex flex-wrap gap-2">
          <Badge variant="secondary">{stateLabel(item.state)}</Badge>
          <Badge variant="outline">{item.priority}</Badge>
        </div>
        <p className="text-xs text-muted-foreground">
          Saved work status · Version {item.version}. Execution state is shown
          separately below.
        </p>
      </header>
      <p className="whitespace-pre-wrap break-words text-sm">
        {item.description || "No description."}
      </p>
      <ExecutionPanel
        key={item.id}
        client={client}
        item={item}
        project={project}
        employees={employees}
        executions={executions}
        disabled={disabled}
        submit={submit}
        revoke={revoke}
      />
      <DefinitionEditor
        key={`${item.id}:${item.version}:definition`}
        item={item}
        project={project}
        disabled={disabled}
        submit={submit}
      />
      <DecompositionPanel
        key={`${item.id}:${item.version}:decomposition`}
        client={client}
        item={item}
        project={project}
        disabled={disabled}
        submit={submit}
        revoke={revoke}
        selectItem={selectItem}
      />
      {targets.length ? (
        <form
          aria-label="Change manual status"
          key={`${item.id}:${item.version}:status`}
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            const form = new FormData(event.currentTarget);
            submit(`${path}/transitions`, "Manual status", {
              expected_version: item.version,
              target: form.get("target"),
              reason: String(form.get("reason")).trim() || null,
            });
          }}
        >
          <fieldset disabled={disabled} className="flex flex-col gap-3">
            <Field label="New manual status">
              {(id) => (
                <Select id={id} name="target" required defaultValue="">
                  <option value="" disabled>
                    Choose a status
                  </option>
                  {targets.map((target) => (
                    <option key={target} value={target}>
                      {stateLabel(target)}
                    </option>
                  ))}
                </Select>
              )}
            </Field>
            <Field label="Status reason (optional)">
              {(id) => <Input id={id} name="reason" maxLength={1024} />}
            </Field>
            <Button type="submit" variant="outline">
              Save status
            </Button>
          </fieldset>
        </form>
      ) : null}
      <section aria-label="Acceptance criteria" className="flex flex-col gap-3">
        <h4 className="text-sm font-semibold">Acceptance criteria</h4>
        {!item.criteria.length ? (
          <p className="text-sm text-muted-foreground">
            No acceptance criteria recorded.
          </p>
        ) : (
          <ul className="flex flex-col gap-3">
            {item.criteria.map((criterion) => (
              <li
                key={criterion.id}
                className="flex flex-wrap items-center justify-between gap-2 text-sm"
              >
                <span className="break-words">
                  {criterion.text} · {criterion.status}
                </span>
                {project.can_review &&
                !terminal &&
                criterion.status === "pending" ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={disabled}
                    aria-label={`Accept criterion: ${criterion.text}`}
                    onClick={() =>
                      submit(
                        `${path}/criteria/${encodeURIComponent(criterion.id)}/satisfy`,
                        "Acceptance criterion",
                        { expected_version: item.version },
                      )
                    }
                  >
                    Accept criterion
                  </Button>
                ) : null}
              </li>
            ))}
          </ul>
        )}
      </section>
      <section aria-label="Review approvals" className="flex flex-col gap-3">
        <h4 className="text-sm font-semibold">Review approvals</h4>
        {!item.approvals.length ? (
          <p className="text-sm text-muted-foreground">
            No approval gates recorded.
          </p>
        ) : (
          item.approvals.map((approval) => (
            <div key={approval.id} className="flex flex-col gap-2 text-sm">
              <p>
                {approval.gate} · {approval.required ? "required" : "optional"}{" "}
                · {approval.status}
              </p>
              {approval.reason ? (
                <p className="whitespace-pre-wrap break-words text-muted-foreground">
                  {approval.reason}
                </p>
              ) : null}
              {project.can_review &&
              !terminal &&
              approval.status === "pending" ? (
                <form
                  aria-label={`Resolve ${approval.gate} approval`}
                  onSubmit={(event) => {
                    event.preventDefault();
                    const form = new FormData(event.currentTarget);
                    submit(
                      `${path}/approvals/${encodeURIComponent(approval.id)}/resolve`,
                      "Review approval",
                      {
                        expected_version: item.version,
                        decision: form.get("decision"),
                        reason: String(form.get("reason")).trim() || null,
                      },
                    );
                  }}
                >
                  <fieldset disabled={disabled} className="flex flex-col gap-3">
                    <Field label={`Decision for ${approval.gate}`}>
                      {(id) => (
                        <Select
                          id={id}
                          name="decision"
                          required
                          defaultValue=""
                        >
                          <option value="" disabled>
                            Choose a decision
                          </option>
                          <option value="approve">Approve</option>
                          <option value="reject">Reject</option>
                        </Select>
                      )}
                    </Field>
                    <Field label={`Reason for ${approval.gate} (optional)`}>
                      {(id) => <Input id={id} name="reason" maxLength={1024} />}
                    </Field>
                    <Button size="sm" variant="outline" type="submit">
                      Save approval
                    </Button>
                  </fieldset>
                </form>
              ) : null}
            </div>
          ))
        )}
        {item.state === "review" ? (
          <p className="text-xs text-muted-foreground">
            Completion requires satisfied criteria and approved required gates.
            The server also checks dependencies.
          </p>
        ) : null}
      </section>
      <AssignmentPanel
        item={item}
        project={project}
        employees={employees}
        disabled={disabled}
        submit={submit}
      />
      <DependencyPanel
        client={client}
        item={item}
        project={project}
        targets={dependencyTargets}
        disabled={disabled}
        submit={submit}
        revoke={revoke}
      />
      <details className="text-sm">
        <summary className="cursor-pointer font-medium">Saved history</summary>
        {item.history_omitted || item.history_truncated ? (
          <p className="mt-2 text-muted-foreground">
            Only the available history is shown.
          </p>
        ) : null}
        <ol className="mt-3 flex flex-col gap-2">
          {item.history.map((entry) => (
            <li key={entry.sequence} className="break-words">
              Version {entry.version} · {entry.event_type}
              {entry.to ? ` → ${stateLabel(entry.to)}` : ""}
              <time
                className="ml-2 text-xs text-muted-foreground"
                dateTime={entry.recorded_at}
              >
                {new Date(entry.recorded_at).toLocaleString()}
              </time>
            </li>
          ))}
        </ol>
      </details>
    </section>
  );
}
