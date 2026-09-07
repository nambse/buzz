import { useState } from "react";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import type { ProjectPage, WorkProject } from "../types";
import { Field, Select, type SubmitWork } from "./fields";

export function CreateProjectForm({
  page,
  disabled,
  submit,
}: {
  page: ProjectPage;
  disabled: boolean;
  submit: SubmitWork;
}) {
  return (
    <details className="rounded-lg border p-4">
      <summary className="cursor-pointer text-sm font-medium">
        New project
      </summary>
      <form
        aria-label="Create project"
        className="mt-4 flex flex-col gap-4"
        onSubmit={(event) => {
          event.preventDefault();
          const data = new FormData(event.currentTarget);
          submit("/api/v1/projects", "Project", {
            channel_id: data.get("channel"),
            project: {
              name: data.get("name"),
              slug: data.get("slug"),
              description: data.get("description"),
            },
          });
        }}
      >
        <fieldset disabled={disabled} className="flex flex-col gap-4">
          <Field label="Project name">
            {(id) => <Input id={id} name="name" required maxLength={200} />}
          </Field>
          <Field label="Project slug">
            {(id) => (
              <Input
                id={id}
                name="slug"
                required
                pattern="[a-z0-9]+(-[a-z0-9]+)*"
                maxLength={80}
                placeholder="release-planning"
              />
            )}
          </Field>
          <Field label="Office channel">
            {(id) => (
              <Select id={id} name="channel" required defaultValue="">
                <option value="" disabled>
                  Choose a channel
                </option>
                {page.create_channels.map((channel) => (
                  <option key={channel.id} value={channel.id}>
                    {channel.name}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          <Field label="Project description">
            {(id) => <Textarea id={id} name="description" maxLength={8192} />}
          </Field>
          <Button type="submit" disabled={!page.create_channels.length}>
            Create project
          </Button>
        </fieldset>
      </form>
    </details>
  );
}

export function CreateItemForm({
  project,
  disabled,
  submit,
  sourceMessage,
}: {
  project: WorkProject;
  disabled: boolean;
  submit: SubmitWork;
  sourceMessage?: string;
}) {
  const [error, setError] = useState<string | null>(null);
  return (
    <details
      open={sourceMessage ? true : undefined}
      className="rounded-lg border p-4"
    >
      <summary className="cursor-pointer text-sm font-medium">
        New work item
      </summary>
      <form
        aria-label="Create work item"
        className="mt-4 flex flex-col gap-4"
        onSubmit={(event) => {
          event.preventDefault();
          const data = new FormData(event.currentTarget);
          const criteria = String(data.get("criteria"))
            .split("\n")
            .map((line) => line.trim())
            .filter(Boolean);
          if (
            criteria.length > 16 ||
            criteria.some(
              (text) => new TextEncoder().encode(text).length > 1024,
            )
          ) {
            setError(
              "Use at most 16 criteria, each no longer than 1,024 bytes.",
            );
            return;
          }
          setError(null);
          submit(
            `/api/v1/projects/${encodeURIComponent(project.id)}/${sourceMessage ? "promotions" : "work-items"}`,
            sourceMessage ? "Message promotion" : "Work item",
            {
              title: data.get("title"),
              description: data.get("description"),
              priority: data.get("priority"),
              criteria,
              approvals: data.get("approval")
                ? [{ gate: "review", required: true }]
                : [],
              ...(sourceMessage ? { source_message_id: sourceMessage } : {}),
            },
          );
        }}
      >
        <fieldset disabled={disabled} className="flex flex-col gap-4">
          <Field label="Work title">
            {(id) => <Input id={id} name="title" required maxLength={200} />}
          </Field>
          <Field label="Work description">
            {(id) => <Textarea id={id} name="description" maxLength={8192} />}
          </Field>
          <Field label="Priority">
            {(id) => (
              <Select id={id} name="priority" defaultValue="normal">
                {["low", "normal", "high", "urgent"].map((priority) => (
                  <option key={priority}>{priority}</option>
                ))}
              </Select>
            )}
          </Field>
          <Field label="Acceptance criteria (one per line)">
            {(id) => <Textarea id={id} name="criteria" maxLength={12000} />}
          </Field>
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" name="approval" />
            Require reviewer approval before completion
          </label>
          {error ? (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          ) : null}
          <Button type="submit">
            {sourceMessage ? "Promote message to Work" : "Create work item"}
          </Button>
        </fieldset>
      </form>
    </details>
  );
}
