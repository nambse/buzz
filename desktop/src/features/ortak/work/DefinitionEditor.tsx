import { useRef, useState } from "react";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import type { WorkItem, WorkProject } from "../types";
import { Field, type SubmitWork } from "./fields";

export function DefinitionEditor({
  item,
  project,
  disabled,
  submit,
}: {
  item: WorkItem;
  project: WorkProject;
  disabled: boolean;
  submit: SubmitWork;
}) {
  const [editing, setEditing] = useState(false);
  const [additions, setAdditions] = useState<string[]>([]);
  const nextAddition = useRef(0);
  const [error, setError] = useState("");
  const frozen =
    !["proposed", "ready", "in_progress", "blocked"].includes(item.state) ||
    item.criteria.some((criterion) => criterion.status !== "pending") ||
    item.approvals.some((approval) => approval.status !== "pending");
  const reason =
    project.status === "archived"
      ? "Archived projects are read-only."
      : !project.can_contribute
        ? "Your current project role cannot edit this definition."
        : frozen
          ? "Definition editing is available before review, while every acceptance criterion and approval gate is still pending. Saved review evidence is retained."
          : item.criteria.length > 16
            ? "This definition exceeds the manual editor's limit of 16 criteria."
            : null;
  return (
    <section
      aria-label="Work definition editing"
      className="flex flex-col gap-3"
    >
      {reason ? (
        <p className="text-sm text-muted-foreground">{reason}</p>
      ) : !editing ? (
        <Button
          type="button"
          variant="outline"
          disabled={disabled}
          onClick={() => setEditing(true)}
        >
          Edit work definition
        </Button>
      ) : (
        <form
          aria-label="Edit work definition"
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            if (disabled) return;
            const form = new FormData(event.currentTarget);
            const definition = {
              title: String(form.get("title")),
              description: String(form.get("description")),
              criteria: item.criteria.map((criterion) => ({
                id: criterion.id,
                text: String(form.get(`criterion-${criterion.id}`)),
              })),
              additional_criteria: additions.map((key) =>
                String(form.get(key)).trim(),
              ),
            };
            const bytes = (value: string) =>
              new TextEncoder().encode(value).length;
            if (
              (definition.title !== item.title &&
                (!definition.title.trim() || bytes(definition.title) > 200)) ||
              (definition.description !== item.description &&
                bytes(definition.description) > 8192) ||
              definition.criteria.some(
                (criterion, index) =>
                  criterion.text !== item.criteria[index].text &&
                  (!criterion.text.trim() || bytes(criterion.text) > 1024),
              ) ||
              definition.additional_criteria.some(
                (text) => !text.trim() || bytes(text) > 1024,
              )
            ) {
              setError(
                "Use a title up to 200 bytes, a description up to 8,192 bytes, and nonempty criteria up to 1,024 bytes each.",
              );
              return;
            }
            if (
              definition.title === item.title &&
              definition.description === item.description &&
              definition.criteria.every(
                (c, index) => c.text === item.criteria[index].text,
              ) &&
              additions.length === 0
            ) {
              setError("Change the definition before saving.");
              return;
            }
            setError("");
            submit(
              `/api/v1/work-items/${encodeURIComponent(item.id)}/definition`,
              "Work definition",
              {
                expected_version: item.version,
                definition: {
                  ...definition,
                  // GET text is a safe projection. An unchanged displayed value
                  // must preserve canonical text under the server's version lock.
                  title:
                    definition.title === item.title ? null : definition.title,
                  description:
                    definition.description === item.description
                      ? null
                      : definition.description,
                  criteria: definition.criteria.map((criterion, index) => ({
                    id: criterion.id,
                    text:
                      criterion.text === item.criteria[index].text
                        ? null
                        : criterion.text,
                  })),
                },
              },
            );
          }}
        >
          <fieldset disabled={disabled} className="flex flex-col gap-3">
            <legend className="mb-3 text-sm font-semibold">
              Edit work definition
            </legend>
            <Field label="Work title">
              {(id) => (
                <Input
                  id={id}
                  name="title"
                  required
                  maxLength={200}
                  defaultValue={item.title}
                />
              )}
            </Field>
            <Field label="Work description">
              {(id) => (
                <Textarea
                  id={id}
                  name="description"
                  maxLength={8192}
                  defaultValue={item.description}
                />
              )}
            </Field>
            {item.criteria.map((criterion, index) => (
              <Field
                key={criterion.id}
                label={`Acceptance criterion ${index + 1}`}
              >
                {(id) => (
                  <Textarea
                    id={id}
                    name={`criterion-${criterion.id}`}
                    required
                    maxLength={1024}
                    defaultValue={criterion.text}
                  />
                )}
              </Field>
            ))}
            {additions.map((key, index) => (
              <Field key={key} label={`New acceptance criterion ${index + 1}`}>
                {(id) => (
                  <Textarea id={id} name={key} required maxLength={1024} />
                )}
              </Field>
            ))}
            <p className="text-xs text-muted-foreground">
              Existing criteria keep their identity and order. Additions are
              saved with the same edit. Up to 16 criteria in total.
            </p>
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                variant="outline"
                disabled={item.criteria.length + additions.length >= 16}
                onClick={() => {
                  const key = `additional-${nextAddition.current++}`;
                  setAdditions((current) => [...current, key]);
                }}
              >
                Add acceptance criterion
              </Button>
              {additions.length > 0 ? (
                <Button
                  type="button"
                  variant="outline"
                  onClick={() =>
                    setAdditions((current) => current.slice(0, -1))
                  }
                >
                  Remove last unsaved criterion
                </Button>
              ) : null}
            </div>
            {error ? (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            ) : null}
            <div className="flex flex-wrap gap-2">
              <Button type="submit">Save definition</Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => {
                  setEditing(false);
                  setAdditions([]);
                  setError("");
                }}
              >
                Cancel editing
              </Button>
            </div>
          </fieldset>
        </form>
      )}
    </section>
  );
}
