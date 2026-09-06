import { useCallback, useMemo, useRef, useState } from "react";
import { signRelayEvent } from "@/shared/api/tauri";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { createOrtakClient } from "../client";
import { Field, Select } from "../work/fields";
import { ConversationFactForm } from "./ConversationFactForm";
import { assertConversationExport } from "./operations";
import {
  ConversationFacts,
  ConversationOperationStatus,
} from "./ConversationFacts";
import { useConversationRead } from "./useConversationRead";
import {
  useConversationMutation,
  type ConversationClient,
} from "./useConversationMutation";
import {
  conversationPath,
  type AudienceKind,
  type ConversationDraft,
  type ConversationPreview,
} from "./types";

/** All asynchronous state lives above DialogContent, retaining uncertain writes on close. */
export function useConversationReview(
  client: ConversationClient,
  channel: string,
  message: string,
  open: boolean,
) {
  const [projectId, setProject] = useState("");
  const [employeeId, setEmployee] = useState("");
  const [kind, setKind] = useState<AudienceKind>("thread");
  const [projectCursor, setProjectCursor] = useState<string>();
  const [employeeCursor, setEmployeeCursor] = useState<string>();
  const [after, setAfter] = useState<string>();
  const invalidators = useRef<Array<() => void>>([]);
  const loadSelection = useCallback(
    async (signal: AbortSignal) => {
      const [projects, employees] = await Promise.all([
        client.projects(signal, projectCursor),
        client.employees(signal, employeeCursor),
      ]);
      return {
        projects: {
          ...projects,
          projects: projects.projects.filter(
            (p) => p.channel_id === channel && p.can_review,
          ),
        },
        employees,
      };
    },
    [client, channel, projectCursor, employeeCursor],
  );
  const directory = useConversationRead(loadSelection, open);
  const project = directory.value?.projects.projects.find(
    (p) => p.id === projectId,
  );
  const employee = directory.value?.employees.employees.find(
    (p) => p.employee_id === employeeId,
  );
  const loadPreview = useCallback(
    async (signal: AbortSignal) => {
      const { preview } = await client.conversationPreview(
        projectId,
        {
          employee_id: employeeId,
          source_message_id: message,
          audience: { kind },
        },
        signal,
      );
      if (
        preview.audience.format !== "ortak-reviewed-conversation-audience/1" ||
        preview.audience.project_id !== projectId ||
        preview.audience.employee_id !== employeeId ||
        preview.audience.channel_id !== channel ||
        preview.audience.kind !== kind ||
        preview.provenance.source_event_id !== message ||
        !/^[0-9a-f]{64}$/.test(preview.audience_hash) ||
        !Number.isFinite(Date.parse(preview.max_expires_at)) ||
        !Number.isFinite(Date.parse(preview.observed_at)) ||
        (preview.valid_before !== null &&
          !Number.isFinite(Date.parse(preview.valid_before)))
      )
        throw new Error("conversation_preview_scope_mismatch");
      return preview;
    },
    [client, projectId, employeeId, channel, message, kind],
  );
  const preview = useConversationRead(
    loadPreview,
    open &&
      directory.ready &&
      project?.status === "active" &&
      employee?.status === "active",
  );
  const mutation = useConversationMutation(
    client,
    projectId,
    employeeId,
    channel,
    open && directory.ready && Boolean(project?.can_review && employee),
    () => {
      for (const invalidate of invalidators.current) invalidate();
    },
    message,
    () => {
      invalidators.current[1]?.();
      invalidators.current[2]?.();
    },
  );
  const loadFacts = useCallback(
    async (signal: AbortSignal) => {
      const page = await client.conversationFacts(
        projectId,
        employeeId,
        signal,
        after,
      );
      if (
        page.facts.length > 16 ||
        page.facts.some(
          (entry) =>
            entry.fact.project_id !== projectId ||
            entry.fact.employee_id !== employeeId,
        )
      )
        throw new Error("conversation_fact_scope_mismatch");
      for (const entry of page.facts)
        if (entry.fact.export)
          assertConversationExport(entry.fact.export, entry.fact.id);
      return page;
    },
    [client, projectId, employeeId, after],
  );
  const facts = useConversationRead(
    loadFacts,
    open && directory.ready && Boolean(project && employee),
    mutation.revision,
    () => {
      for (const invalidate of invalidators.current) invalidate();
    },
  );
  invalidators.current = [
    directory.invalidate,
    preview.invalidate,
    facts.invalidate,
  ];
  const blocked = mutation.busy || Boolean(mutation.pending);
  const admission = useRef({
    preview,
    open,
    project,
    employee,
    blocked,
    message,
  });
  admission.current = { preview, open, project, employee, blocked, message };
  return {
    directory,
    project,
    employee,
    projectId,
    employeeId,
    kind,
    preview,
    facts,
    mutation,
    after,
    blocked,
    approve: (observation: ConversationPreview, draft: ConversationDraft) => {
      const now = admission.current;
      if (
        !now.open ||
        now.blocked ||
        !now.preview.ready ||
        now.preview.value !== observation ||
        now.project?.status !== "active" ||
        !now.project.can_review ||
        now.employee?.status !== "active" ||
        draft.source_message_id !== now.message ||
        draft.employee_id !== now.employee.employee_id ||
        draft.audience.kind !== observation.audience.kind ||
        draft.expected_audience_hash !== observation.audience_hash
      )
        return;
      mutation.submit(conversationPath(now.project.id), { fact: draft });
    },
    setAfter,
    setProject: (value: string) => {
      if (!blocked) {
        setProject(value);
        setAfter(undefined);
      }
    },
    setEmployee: (value: string) => {
      if (!blocked) {
        setEmployee(value);
        setAfter(undefined);
      }
    },
    setKind: (value: AudienceKind) => {
      if (!blocked) setKind(value);
    },
    projectsNext: (cursor?: string) => {
      if (!blocked) {
        setProject("");
        setProjectCursor(cursor);
      }
    },
    employeesNext: (cursor?: string) => {
      if (!blocked) {
        setEmployee("");
        setEmployeeCursor(cursor);
      }
    },
    projectCursor,
    employeeCursor,
  };
}

export function ConversationReviewPanel({
  state,
  message,
}: {
  state: ReturnType<typeof useConversationReview>;
  message: string;
}) {
  const { directory, project, employee, preview, mutation } = state;
  return (
    <div className="flex flex-col gap-4">
      <ConversationOperationStatus state={mutation} />
      <Button
        variant="outline"
        disabled={state.blocked}
        onClick={directory.refresh}
      >
        Refresh conversation access
      </Button>
      {directory.error ? (
        <Alert variant="destructive">
          <AlertDescription>{directory.error}</AlertDescription>
        </Alert>
      ) : null}
      {!directory.value && !directory.error ? (
        <p role="status" className="text-sm">
          Loading authorized projects and employees…
        </p>
      ) : null}
      {directory.value ? (
        <>
          <Field label="Conversation project">
            {(id) => (
              <Select
                id={id}
                value={project?.id ?? ""}
                disabled={state.blocked}
                onChange={(event) => state.setProject(event.target.value)}
              >
                <option value="">Choose a bound project</option>
                {directory.value?.projects.projects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} · {p.status}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          {!directory.value.projects.projects.length ? (
            <p className="text-sm">
              No project with current review permission is listed for this
              channel.
            </p>
          ) : null}
          <div className="flex gap-2">
            {state.projectCursor ? (
              <Button
                size="sm"
                variant="outline"
                disabled={state.blocked}
                onClick={() => state.projectsNext()}
              >
                First projects
              </Button>
            ) : null}
            {directory.value.projects.next_cursor ? (
              <Button
                size="sm"
                variant="outline"
                disabled={state.blocked}
                onClick={() =>
                  state.projectsNext(
                    directory.value?.projects.next_cursor ?? undefined,
                  )
                }
              >
                Next projects
              </Button>
            ) : null}
          </div>
          <Field label="Conversation employee">
            {(id) => (
              <Select
                id={id}
                value={employee?.employee_id ?? ""}
                disabled={state.blocked}
                onChange={(event) => state.setEmployee(event.target.value)}
              >
                <option value="">Choose an employee</option>
                {directory.value?.employees.employees.map((p) => (
                  <option key={p.employee_id} value={p.employee_id}>
                    {p.name ?? p.employee_id} · {p.status}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          <div className="flex gap-2">
            {state.employeeCursor ? (
              <Button
                size="sm"
                variant="outline"
                disabled={state.blocked}
                onClick={() => state.employeesNext()}
              >
                First employees
              </Button>
            ) : null}
            {directory.value.employees.next_after ? (
              <Button
                size="sm"
                variant="outline"
                disabled={state.blocked}
                onClick={() =>
                  state.employeesNext(
                    directory.value?.employees.next_after ?? undefined,
                  )
                }
              >
                Next employees
              </Button>
            ) : null}
          </div>
          {project && employee ? (
            <>
              <Field label="Conversation audience">
                {(id) => (
                  <Select
                    id={id}
                    value={state.kind}
                    disabled={state.blocked}
                    onChange={(event) =>
                      state.setKind(event.target.value as AudienceKind)
                    }
                  >
                    <option value="thread">Only this thread</option>
                    <option value="channel">This entire channel</option>
                  </Select>
                )}
              </Field>
              <p className="text-sm text-muted-foreground">
                Thread is the default. Choosing the entire channel explicitly
                widens the reviewed audience.
              </p>
              {project.status === "active" && employee.status === "active" ? (
                <Button
                  variant="outline"
                  disabled={state.blocked}
                  onClick={preview.refresh}
                >
                  Refresh audience preview
                </Button>
              ) : (
                <p className="text-sm">
                  New approval requires an active project and employee. Saved
                  facts and Stop using remain available.
                </p>
              )}
              {preview.error ? (
                <Alert variant="destructive">
                  <AlertDescription>
                    {preview.error} Saved fact recovery below does not require
                    this source.
                  </AlertDescription>
                </Alert>
              ) : null}
              {(mutation.receipt || mutation.exportReceipt) &&
              !state.blocked &&
              project.status === "active" &&
              employee.status === "active" ? (
                <Button
                  variant="outline"
                  onClick={() => {
                    mutation.clearReceipt();
                    preview.refresh();
                  }}
                >
                  Review another conversation fact
                </Button>
              ) : null}
              {preview.value &&
              preview.ready &&
              !mutation.receipt &&
              !mutation.exportReceipt ? (
                <ConversationFactForm
                  key={`${project.id}:${employee.employee_id}:${message}:${state.kind}`}
                  preview={preview.value}
                  projectName={project.name}
                  employee={employee.employee_id}
                  employeeName={employee.name ?? employee.employee_id}
                  message={message}
                  mutation={mutation}
                  disabled={!directory.ready}
                  approve={(draft) => {
                    if (preview.value) state.approve(preview.value, draft);
                  }}
                />
              ) : null}
              <ConversationFacts
                read={state.facts}
                mutation={mutation}
                project={project.id}
                employee={employee.employee_id}
                after={state.after}
                setAfter={state.setAfter}
              />
            </>
          ) : null}
        </>
      ) : null}
    </div>
  );
}

export function ConversationMemoryDialog({
  origin,
  channel,
  message,
  open,
  onClose,
  restoreFocus,
}: {
  origin: string;
  channel: string;
  message: string;
  open: boolean;
  onClose: () => void;
  restoreFocus?: () => void;
}) {
  const client = useMemo(
    () => createOrtakClient(origin, signRelayEvent),
    [origin],
  );
  const state = useConversationReview(client, channel, message, open);
  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!value) onClose();
      }}
    >
      <DialogContent
        className="max-h-[85vh] overflow-y-auto"
        onCloseAutoFocus={(event) => {
          if (restoreFocus) {
            event.preventDefault();
            restoreFocus();
          }
        }}
      >
        <DialogHeader>
          <DialogTitle>Review conversation memory</DialogTitle>
          <DialogDescription>
            Choose the existing project, employee and conversation audience for
            this message. Write your own fact text after reviewing the server
            preview.
          </DialogDescription>
        </DialogHeader>
        <ConversationReviewPanel state={state} message={message} />
      </DialogContent>
    </Dialog>
  );
}
