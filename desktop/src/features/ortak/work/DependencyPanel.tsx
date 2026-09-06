import { useState } from "react";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import type { OrtakClient } from "../client";
import type { WorkItem, WorkProject, WorkSummary } from "../types";
import { Field, Select, type SubmitWork } from "./fields";
import { stateLabel } from "./operations";
import { useDependencies } from "./useDependencies";

export function DependencyPanel({
  client,
  item,
  project,
  targets,
  disabled,
  submit,
  revoke,
}: {
  client: OrtakClient;
  item: WorkItem;
  project: WorkProject;
  targets: WorkSummary[];
  disabled: boolean;
  submit: SubmitWork;
  revoke: () => void;
}) {
  const state = useDependencies(client, item.id, item.version, revoke);
  const [error, setError] = useState("");
  const editable =
    !!state.data &&
    project.status === "active" &&
    project.can_contribute &&
    !["completed", "cancelled"].includes(item.state);
  const choices = targets.filter(
    (target) =>
      target.project_id === item.project_id &&
      target.id !== item.id &&
      !state.data?.dependencies.some((edge) => edge.target?.id === target.id),
  );
  const path = `/api/v1/work-items/${encodeURIComponent(item.id)}/dependencies`;
  return (
    <section aria-label="Work dependencies" className="flex flex-col gap-3">
      <h4 className="text-sm font-semibold">Dependencies</h4>
      <p className="text-xs text-muted-foreground">
        Unfinished dependencies block execution. Removing one retains its
        history and requires a new execution request; human acceptance remains
        unchanged.
      </p>
      {state.error ? (
        <div role="alert" className="flex flex-col gap-2">
          <p className="text-sm text-destructive">{state.error}</p>
          <Button type="button" variant="outline" onClick={state.refresh}>
            Retry dependencies
          </Button>
        </div>
      ) : !state.data ? (
        <p role="status" className="text-sm text-muted-foreground">
          Loading dependencies…
        </p>
      ) : (
        <>
          {!state.data.dependencies.length ? (
            <p className="text-sm text-muted-foreground">
              No active dependencies.
            </p>
          ) : (
            <ul className="flex flex-col gap-3">
              {state.data.dependencies.map((edge, index) => (
                <li key={edge.id} className="text-sm">
                  {edge.target ? (
                    <span>
                      {edge.target.title} · {stateLabel(edge.target.state)}
                    </span>
                  ) : (
                    <span>
                      Target unavailable. Its content is hidden; this dependency
                      can still be removed.
                    </span>
                  )}
                  {editable ? (
                    <form
                      aria-label={`Remove dependency ${index + 1}`}
                      className="mt-2"
                      onSubmit={(event) => {
                        event.preventDefault();
                        if (disabled) return;
                        const reason = String(
                          new FormData(event.currentTarget).get("reason"),
                        ).trim();
                        if (
                          !reason ||
                          new TextEncoder().encode(reason).length > 1024
                        ) {
                          setError("Enter a reason up to 1,024 bytes.");
                          return;
                        }
                        setError("");
                        submit(
                          `${path}/${encodeURIComponent(edge.id)}/remove`,
                          "Remove dependency",
                          { expected_version: item.version, reason },
                        );
                      }}
                    >
                      <fieldset
                        disabled={disabled}
                        className="flex flex-col gap-2"
                      >
                        <Field
                          label={`Reason to remove dependency ${index + 1}`}
                        >
                          {(id) => (
                            <Input
                              id={id}
                              name="reason"
                              required
                              maxLength={1024}
                            />
                          )}
                        </Field>
                        <Button type="submit" size="sm" variant="outline">
                          Remove dependency {index + 1}
                        </Button>
                      </fieldset>
                    </form>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
          {editable && choices.length && state.data.dependencies.length < 32 ? (
            <form
              aria-label="Add dependency"
              onSubmit={(event) => {
                event.preventDefault();
                if (disabled) return;
                submit(path, "Add dependency", {
                  expected_version: item.version,
                  depends_on: new FormData(event.currentTarget).get("target"),
                });
              }}
            >
              <fieldset disabled={disabled} className="flex flex-col gap-2">
                <Field label="Blocker from the current work list page">
                  {(id) => (
                    <Select id={id} name="target" required defaultValue="">
                      <option value="" disabled>
                        Choose a work item
                      </option>
                      {choices.map((target) => (
                        <option key={target.id} value={target.id}>
                          {target.title}
                        </option>
                      ))}
                    </Select>
                  )}
                </Field>
                <Button type="submit" size="sm" variant="outline">
                  Add dependency
                </Button>
              </fieldset>
            </form>
          ) : null}
        </>
      )}
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      ) : null}
    </section>
  );
}
