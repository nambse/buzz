import { useCallback, useRef, useState } from "react";
import type { OrtakClient } from "../client";
import type { WorkProject } from "../types";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Button } from "@/shared/ui/button";
import { Field, Select } from "../work/fields";
import {
  ConversationFacts,
  ConversationOperationStatus,
} from "./ConversationFacts";
import { useConversationRead } from "./useConversationRead";
import { assertConversationExport } from "./operations";
import { useConversationMutation } from "./useConversationMutation";

/** Project-level recovery stays reachable after the original Office message disappears. */
export function ConversationMemoryPanel({
  client,
  project,
  disabled,
}: {
  client: OrtakClient;
  project: WorkProject;
  disabled: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [employee, setEmployee] = useState("");
  const [cursor, setCursor] = useState<string>();
  const [after, setAfter] = useState<string>();
  const invalidator = useRef<() => void>(() => {});
  const invalidateFacts = useRef<() => void>(() => {});
  const mutation = useConversationMutation(
    client,
    project.id,
    employee,
    project.channel_id,
    open && !disabled && project.can_review,
    () => invalidator.current(),
    "",
    () => invalidateFacts.current(),
  );
  const load = useCallback(
    async (signal: AbortSignal) => {
      const page = await client.conversationFacts(
        project.id,
        employee,
        signal,
        after,
      );
      if (
        page.facts.length > 16 ||
        page.facts.some(
          (entry) =>
            entry.fact.project_id !== project.id ||
            entry.fact.employee_id !== employee,
        )
      )
        throw new Error("conversation_fact_scope_mismatch");
      for (const entry of page.facts)
        if (entry.fact.export)
          assertConversationExport(entry.fact.export, entry.fact.id);
      return page;
    },
    [client, project.id, employee, after],
  );
  const read = useConversationRead(
    load,
    open && Boolean(employee) && !disabled && project.can_review,
    mutation.revision,
    () => invalidator.current(),
  );
  const loadEmployees = useCallback(
    (signal: AbortSignal) => client.employees(signal, cursor),
    [client, cursor],
  );
  const directory = useConversationRead(
    loadEmployees,
    open && !disabled && project.can_review,
    0,
    () => read.invalidate(),
  );
  invalidator.current = () => {
    read.invalidate();
    directory.invalidate();
  };
  invalidateFacts.current = read.invalidate;
  const blocked = disabled || mutation.busy || Boolean(mutation.pending);
  return (
    <section
      aria-label="Conversation memory recovery"
      className="flex flex-col gap-4 rounded-xl border bg-card p-5"
    >
      <h3 className="text-base font-semibold">Conversation memory</h3>
      <p className="text-sm text-muted-foreground">
        Start a new review from an Office message’s More menu. Saved
        conversation facts can be inspected and stopped here even when that
        source is unavailable.
      </p>
      <Button
        variant="outline"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        {open ? "Close conversation memory" : "Inspect conversation memory"}
      </Button>
      {open ? <ConversationOperationStatus state={mutation} /> : null}
      {!project.can_review ? (
        <p className="text-sm">
          Conversation memory requires current project review permission.
        </p>
      ) : open ? (
        <>
          {directory.error ? (
            <Alert variant="destructive">
              <AlertDescription>{directory.error}</AlertDescription>
            </Alert>
          ) : null}
          <Button
            variant="outline"
            size="sm"
            disabled={blocked}
            onClick={directory.refresh}
          >
            Refresh saved employee choices
          </Button>
          <Field label="Saved conversation employee">
            {(id) => (
              <Select
                id={id}
                value={employee}
                disabled={blocked || !directory.ready}
                onChange={(event) => {
                  setEmployee(event.target.value);
                  setAfter(undefined);
                }}
              >
                <option value="">Choose an employee</option>
                {employee &&
                !directory.value?.employees.some(
                  (e) => e.employee_id === employee,
                ) ? (
                  <option value={employee}>
                    Selected employee outside this directory page
                  </option>
                ) : null}
                {directory.value?.employees.map((entry) => (
                  <option key={entry.employee_id} value={entry.employee_id}>
                    {entry.name ?? entry.employee_id} · {entry.status}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          <div className="flex gap-2">
            {cursor ? (
              <Button
                size="sm"
                variant="outline"
                disabled={blocked}
                onClick={() => setCursor(undefined)}
              >
                First saved employee choices
              </Button>
            ) : null}
            {directory.value?.next_after ? (
              <Button
                size="sm"
                variant="outline"
                disabled={blocked}
                onClick={() =>
                  setCursor(directory.value?.next_after ?? undefined)
                }
              >
                More saved employee choices
              </Button>
            ) : null}
          </div>
          <ConversationFacts
            read={read}
            mutation={mutation}
            project={project.id}
            employee={employee}
            after={after}
            setAfter={setAfter}
            disabled={disabled}
          />
        </>
      ) : null}
    </section>
  );
}
