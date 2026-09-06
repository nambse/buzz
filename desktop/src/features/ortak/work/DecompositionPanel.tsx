import { useState } from "react";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import type { OrtakClient } from "../client";
import type { WorkItem, WorkProject } from "../types";
import { Field, Select, type SubmitWork } from "./fields";
import { stateLabel } from "./operations";
import { useDecomposition } from "./useDecomposition";

export function DecompositionPanel({
  client,
  item,
  project,
  disabled,
  submit,
  revoke,
  selectItem,
}: {
  client: OrtakClient;
  item: WorkItem;
  project: WorkProject;
  disabled: boolean;
  submit: SubmitWork;
  revoke: () => void;
  selectItem: (id: string) => void;
}) {
  const state = useDecomposition(
    client,
    item.id,
    item.version,
    item.project_id,
    revoke,
  );
  const [error, setError] = useState<string | null>(null);
  const parent = state.data?.parent;
  const editable =
    !!state.data &&
    project.status === "active" &&
    project.can_contribute &&
    !["completed", "cancelled"].includes(item.state);
  return (
    <section aria-label="Parent and child work" className="flex flex-col gap-3">
      <h4 className="text-sm font-semibold">Parent and child work</h4>
      <p className="text-xs text-muted-foreground">
        Each child has its own definition, assignments and human acceptance.
        Completing a child does not complete its parent. Add a dependency when
        work must block other work. Each item supports up to 32 direct children,
        within eight levels.
      </p>
      {state.error ? (
        <div role="alert" className="flex flex-col gap-2">
          <p className="text-sm text-destructive">{state.error}</p>
          <Button type="button" variant="outline" onClick={state.refresh}>
            Retry work links
          </Button>
        </div>
      ) : !state.data ? (
        <p role="status" className="text-sm text-muted-foreground">
          Loading work links…
        </p>
      ) : (
        <>
          {parent ? (
            <Button
              type="button"
              variant="outline"
              className="h-auto whitespace-normal text-left"
              onClick={() => selectItem(parent.id)}
              disabled={disabled}
            >
              Parent: {parent.title} · {stateLabel(parent.state)}
            </Button>
          ) : null}
          {state.data.children.length ? (
            <ul className="flex flex-col gap-2">
              {state.data.children.map((child) => (
                <li key={child.id}>
                  <Button
                    type="button"
                    variant="outline"
                    className="h-auto whitespace-normal text-left"
                    onClick={() => selectItem(child.id)}
                    disabled={disabled}
                  >
                    Child: {child.title} · {stateLabel(child.state)}
                  </Button>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-muted-foreground">
              No child work is currently visible.
            </p>
          )}
          {editable ? (
            <details
              className="rounded-lg border p-3"
              key={`${item.id}:${item.version}`}
            >
              <summary className="cursor-pointer text-sm font-medium">
                Create child work
              </summary>
              <form
                aria-label="Create child work"
                className="mt-3"
                onSubmit={(event) => {
                  event.preventDefault();
                  if (disabled) return;
                  const form = new FormData(event.currentTarget);
                  const title = String(form.get("title") ?? "").trim();
                  const description = String(form.get("description") ?? "");
                  const criteria = String(form.get("criteria") ?? "")
                    .split("\n")
                    .map((line) => line.trim())
                    .filter(Boolean);
                  const bytes = (value: string) =>
                    new TextEncoder().encode(value).length;
                  if (
                    !title ||
                    bytes(title) > 200 ||
                    bytes(description) > 8192 ||
                    criteria.length > 16 ||
                    criteria.some((value) => bytes(value) > 1024)
                  ) {
                    setError(
                      "Use a title up to 200 bytes, a description up to 8,192 bytes and at most 16 criteria of 1,024 bytes each.",
                    );
                    return;
                  }
                  setError(null);
                  submit(
                    `/api/v1/work-items/${encodeURIComponent(item.id)}/children`,
                    "Create child work",
                    {
                      expected_version: item.version,
                      child: {
                        title,
                        description,
                        priority: form.get("priority"),
                        criteria,
                        approvals: form.get("approval")
                          ? [{ gate: "review", required: true }]
                          : [],
                      },
                    },
                  );
                }}
              >
                <fieldset disabled={disabled} className="flex flex-col gap-3">
                  <Field label="Child title">
                    {(id) => (
                      <Input id={id} name="title" required maxLength={200} />
                    )}
                  </Field>
                  <Field label="Child description">
                    {(id) => (
                      <Textarea id={id} name="description" maxLength={8192} />
                    )}
                  </Field>
                  <Field label="Child priority">
                    {(id) => (
                      <Select id={id} name="priority" defaultValue="normal">
                        {["low", "normal", "high", "urgent"].map((priority) => (
                          <option key={priority}>{priority}</option>
                        ))}
                      </Select>
                    )}
                  </Field>
                  <Field label="Child acceptance criteria (one per line)">
                    {(id) => (
                      <Textarea id={id} name="criteria" maxLength={12000} />
                    )}
                  </Field>
                  <label className="flex items-center gap-2 text-sm">
                    <input type="checkbox" name="approval" defaultChecked />
                    Require child reviewer approval
                  </label>
                  {error ? (
                    <p role="alert" className="text-sm text-destructive">
                      {error}
                    </p>
                  ) : null}
                  <Button type="submit" variant="outline">
                    Create child
                  </Button>
                </fieldset>
              </form>
            </details>
          ) : null}
        </>
      )}
    </section>
  );
}
