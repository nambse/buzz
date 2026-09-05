import { useEffect, useRef, useState } from "react";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Skeleton } from "@/shared/ui/skeleton";
import type { OrtakClient } from "../client";
import type { Employee } from "../types";
import { CreateItemForm, CreateProjectForm } from "./CreateForms";
import { ItemDetail } from "./ItemDetail";
import { stateLabel } from "./operations";
import { useWorkData } from "./useWorkData";
import { useWorkMutation } from "./useWorkMutation";

export function WorkScreen({
  client,
  employees,
  accessRevoked = false,
}: {
  client: OrtakClient;
  employees: Employee[];
  accessRevoked?: boolean;
}) {
  const projectHeading = useRef<HTMLHeadingElement>(null);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [itemId, setItemId] = useState<string | null>(null);
  const [projectCursor, setProjectCursor] = useState<string | undefined>();
  const [itemCursor, setItemCursor] = useState<string | undefined>();
  const [refresh, setRefresh] = useState(0);
  const [revoked, setRevoked] = useState(false);
  const reload = () => setRefresh((value) => value + 1);
  const state = useWorkData(
    client,
    projectId,
    itemId,
    projectCursor,
    itemCursor,
    refresh,
    revoked || accessRevoked,
  );
  const mutation = useWorkMutation(client, reload, () => setRevoked(true));
  const data = state.data;
  const disabled = mutation.busy || mutation.pending !== null;
  useEffect(() => {
    if (data?.project?.id && !itemId) projectHeading.current?.focus();
  }, [data?.project?.id, itemId]);
  useEffect(() => {
    if (state.revoked || accessRevoked) mutation.pause();
  }, [state.revoked, accessRevoked, mutation.pause]);
  return (
    <section
      aria-label="Projects and manual work"
      className="flex flex-col gap-4 pt-4"
    >
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold">Projects &amp; Work</h2>
          <p className="text-sm text-muted-foreground">
            Plan, assign, and review manual work. These actions do not start
            employee execution.
          </p>
        </div>
        <Button
          size="sm"
          variant="outline"
          disabled={accessRevoked}
          onClick={() => {
            setRevoked(false);
            if (state.revoked) {
              setProjectId(null);
              setItemId(null);
            }
            reload();
          }}
        >
          Refresh work
        </Button>
      </header>
      {mutation.notice && !state.revoked && !accessRevoked ? (
        <Alert role="status">
          <AlertDescription className="flex flex-col gap-3">
            <p>{mutation.notice}</p>
            {mutation.pending ? (
              <Button
                className="self-start"
                size="sm"
                variant="outline"
                disabled={mutation.busy || !data}
                onClick={mutation.retry}
              >
                Retry same operation
              </Button>
            ) : null}
          </AlertDescription>
        </Alert>
      ) : null}
      {mutation.busy ? (
        <p role="status" className="text-sm text-muted-foreground">
          Saving manual work…
        </p>
      ) : null}
      {state.error || state.revoked || accessRevoked ? (
        <Alert variant="destructive">
          <AlertDescription>
            {state.error ?? "Work access must be refreshed before continuing."}
          </AlertDescription>
        </Alert>
      ) : null}
      {!data && !state.error && !state.revoked && !accessRevoked ? (
        <Skeleton className="h-32 w-full" />
      ) : null}
      {data ? (
        <div className="grid items-start gap-5 lg:grid-cols-[minmax(12rem,1fr)_minmax(0,3fr)]">
          <section aria-label="Projects" className="flex flex-col gap-3">
            {data.projects.can_create_projects ? (
              <CreateProjectForm
                page={data.projects}
                disabled={disabled}
                submit={mutation.submit}
              />
            ) : null}
            {!data.projects.projects.length ? (
              <p className="text-sm text-muted-foreground">
                No projects are available to this account.
              </p>
            ) : null}
            {data.projects.projects.map((project) => (
              <Button
                key={project.id}
                variant={project.id === projectId ? "secondary" : "outline"}
                className="h-auto flex-col items-start gap-1 whitespace-normal py-3 text-left"
                aria-pressed={project.id === projectId}
                onClick={() => {
                  setProjectId(project.id);
                  setItemId(null);
                  setItemCursor(undefined);
                }}
              >
                <span className="break-words">{project.name}</span>
                <span className="text-xs text-muted-foreground">
                  {project.role} · {project.status}
                </span>
              </Button>
            ))}
            <div className="flex flex-wrap gap-2">
              {projectCursor ? (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => setProjectCursor(undefined)}
                >
                  First projects
                </Button>
              ) : null}
              {data.projects.next_cursor ? (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    setProjectCursor(data.projects.next_cursor ?? undefined)
                  }
                >
                  More projects
                </Button>
              ) : null}
            </div>
          </section>
          {data.project ? (
            <section
              aria-label="Project detail"
              className="flex min-w-0 flex-col gap-4"
            >
              <header>
                <h3
                  ref={projectHeading}
                  tabIndex={-1}
                  className="break-words text-lg font-semibold outline-none"
                >
                  {data.project.name}
                </h3>
                <p className="whitespace-pre-wrap break-words text-sm text-muted-foreground">
                  {data.project.description}
                </p>
              </header>
              {data.project.status === "active" &&
              data.project.can_contribute ? (
                <CreateItemForm
                  key={data.project.id}
                  project={data.project}
                  disabled={disabled}
                  submit={mutation.submit}
                />
              ) : null}
              <div className="grid items-start gap-4 xl:grid-cols-[minmax(10rem,1fr)_minmax(0,2fr)]">
                <section
                  aria-label="Work items"
                  className="flex flex-col gap-3"
                >
                  {!data.items?.work_items.length ? (
                    <p className="text-sm text-muted-foreground">
                      No work items in this page.
                    </p>
                  ) : null}
                  {data.items?.work_items.map((item) => (
                    <Button
                      key={item.id}
                      variant={item.id === itemId ? "secondary" : "outline"}
                      className="h-auto flex-col items-start gap-2 whitespace-normal py-3 text-left"
                      aria-pressed={item.id === itemId}
                      onClick={() => setItemId(item.id)}
                    >
                      <span className="break-words">{item.title}</span>
                      <Badge variant="secondary">
                        {stateLabel(item.state)}
                      </Badge>
                    </Button>
                  ))}
                  <div className="flex flex-wrap gap-2">
                    {itemCursor ? (
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => setItemCursor(undefined)}
                      >
                        First work items
                      </Button>
                    ) : null}
                    {data.items?.next_cursor ? (
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() =>
                          setItemCursor(data.items?.next_cursor ?? undefined)
                        }
                      >
                        More work items
                      </Button>
                    ) : null}
                  </div>
                </section>
                {data.item ? (
                  <ItemDetail
                    key={data.item.id}
                    item={data.item}
                    project={data.project}
                    employees={employees}
                    disabled={disabled || data.project.status !== "active"}
                    submit={mutation.submit}
                  />
                ) : (
                  <p className="text-sm text-muted-foreground">
                    Select a work item to review its saved state.
                  </p>
                )}
              </div>
            </section>
          ) : (
            <p className="text-sm text-muted-foreground">
              Select a project to view its work.
            </p>
          )}
        </div>
      ) : null}
    </section>
  );
}
