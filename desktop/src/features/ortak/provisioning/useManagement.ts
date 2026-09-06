import { useEffect, useRef, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import type {
  ConfigurationDraft,
  DraftRequest,
  ManagementPage,
  ManagementRequest,
  PreparedCatalog,
} from "./managementTypes";

type Pending = { employee: string } & (
  | { kind: "draft"; body: DraftRequest }
  | { kind: "command"; body: ManagementRequest }
);
type Snapshot = {
  client: OrtakClient;
  catalog: PreparedCatalog | null;
  page: ManagementPage | null;
  draft: ConfigurationDraft | null;
  error: string | null;
};
const empty = (client: OrtakClient): Snapshot => ({
  client,
  catalog: null,
  page: null,
  draft: null,
  error: null,
});

/** Every async completion belongs to a client/scope generation. Mutation retry
 * reuses the original body and idempotency key; it never invents another job. */
export function useManagement(client: OrtakClient) {
  const [snapshot, setSnapshot] = useState<Snapshot>(() => empty(client));
  const [employee, setEmployee] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [busy, setBusy] = useState(false);
  const [retryable, setRetryable] = useState(false);
  const pending = useRef<Pending | null>(null);
  const active = useRef<AbortController | null>(null);
  const owner = useRef({ client, generation: 0 });
  const blocked = useRef(false);
  const requestKey = JSON.stringify([employee, refresh]);
  const latestRead = useRef(requestKey);
  const value = snapshot.client === client ? snapshot : empty(client);

  useEffect(() => {
    owner.current = { client, generation: owner.current.generation + 1 };
    const generation = owner.current.generation;
    pending.current = null;
    blocked.current = false;
    active.current?.abort();
    setBusy(false);
    setRetryable(false);
    setEmployee(null);
    setSnapshot(empty(client));
    return () => {
      if (owner.current.generation === generation)
        owner.current.generation += 1;
      active.current?.abort();
    };
  }, [client]);

  useEffect(() => {
    latestRead.current = requestKey;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    const generation = owner.current.generation;
    setSnapshot((old) =>
      old.client === client ? { ...old, page: null } : empty(client),
    );
    async function read() {
      if (
        blocked.current ||
        controller.signal.aborted ||
        owner.current.generation !== generation ||
        latestRead.current !== requestKey
      )
        return;
      try {
        const [catalog, page] = await Promise.all([
          client.preparedEmployees(controller.signal),
          employee
            ? client.managementCommands(employee, controller.signal)
            : Promise.resolve(null),
        ]);
        if (
          controller.signal.aborted ||
          owner.current.generation !== generation ||
          latestRead.current !== requestKey
        )
          return;
        if (
          !Array.isArray(catalog.choices) ||
          catalog.choices.length > 64 ||
          catalog.create_supported !== false ||
          typeof catalog.lifecycle_supported !== "boolean" ||
          (catalog.lifecycle_supported &&
            (!Array.isArray(catalog.employees) ||
              catalog.employees.length > 64)) ||
          (page &&
            (page.employee_id !== employee ||
              !Array.isArray(page.commands) ||
              page.commands.length > 25 ||
              typeof page.lifecycle_supported !== "boolean" ||
              (page.lifecycle_supported &&
                (!Number.isSafeInteger(page.expected_lifecycle_epoch) ||
                  (page.expected_lifecycle_epoch ?? -1) < 0 ||
                  !page.lifecycle ||
                  typeof page.lifecycle.can_disable !== "boolean" ||
                  [
                    page.lifecycle.old_active_runs,
                    page.lifecycle.pending_stops,
                    page.lifecycle.failed_stops,
                  ].some(
                    (value) => !Number.isSafeInteger(value) || value < 0,
                  )))))
        ) {
          throw new Error(
            "Management response did not match the selected scope.",
          );
        }
        failures = 0;
        setSnapshot((old) => ({
          ...old,
          client,
          catalog,
          page,
          error: null,
          draft:
            old.draft &&
            catalog.choices.some(
              (choice) =>
                choice.catalog_id === old.draft?.catalog_id &&
                choice.expected_revision_id ===
                  old.draft?.expected_revision_id &&
                choice.expected_lifecycle_epoch ===
                  old.draft?.expected_lifecycle_epoch,
            )
              ? old.draft
              : null,
        }));
        timer = setTimeout(read, 5_000);
      } catch (error) {
        if (
          controller.signal.aborted ||
          owner.current.generation !== generation
        )
          return;
        const terminal =
          error instanceof OrtakApiError &&
          [401, 403, 404].includes(error.status);
        if (terminal) {
          blocked.current = true;
          owner.current.generation += 1;
          active.current?.abort();
          pending.current = null;
          setRetryable(false);
          setBusy(false);
        }
        setSnapshot((old) => ({
          ...empty(client),
          draft: terminal ? null : old.draft,
          error:
            error instanceof OrtakApiError
              ? error.message
              : "Prepared employee records are unavailable. Refresh to retry.",
        }));
        failures += 1;
        if (!terminal && failures < 5)
          timer = setTimeout(
            read,
            Math.min(30_000, 2_000 * 2 ** (failures - 1)),
          );
      }
    }
    void read();
    return () => {
      controller.abort();
      if (timer) clearTimeout(timer);
    };
  }, [client, employee, requestKey]);

  async function submit(action: Pending) {
    if (blocked.current) return;
    active.current?.abort();
    const controller = new AbortController();
    active.current = controller;
    const generation = owner.current.generation;
    pending.current = action;
    setBusy(true);
    setRetryable(false);
    setEmployee(action.employee);
    try {
      if (action.kind === "draft") {
        const draft = await client.configurationDraft(
          action.employee,
          action.body,
          controller.signal,
        );
        if (
          controller.signal.aborted ||
          owner.current.generation !== generation
        )
          return;
        if (
          draft.employee_id !== action.employee ||
          draft.draft_id !== action.body.draft_id ||
          draft.catalog_id !== action.body.catalog_id ||
          draft.expected_revision_id !== action.body.expected_revision_id ||
          (action.body.expected_lifecycle_epoch !== undefined &&
            draft.expected_lifecycle_epoch !==
              action.body.expected_lifecycle_epoch)
        )
          throw new Error("Draft did not match the saved selection.");
        setSnapshot((old) => ({ ...old, client, draft, error: null }));
      } else {
        const receipt = await client.managementCommand(
          action.employee,
          action.body,
          controller.signal,
        );
        if (
          controller.signal.aborted ||
          owner.current.generation !== generation
        )
          return;
        if (
          receipt.employee_id !== action.employee ||
          typeof receipt.command_id !== "string"
        )
          throw new Error("Command did not match the selected employee.");
        setSnapshot((old) => ({ ...old, client, draft: null, error: null }));
        setRefresh((v) => v + 1);
      }
      pending.current = null;
    } catch (error) {
      if (controller.signal.aborted || owner.current.generation !== generation)
        return;
      const definite =
        error instanceof OrtakApiError &&
        [400, 401, 403, 404, 409, 413, 422].includes(error.status);
      if (definite) pending.current = null;
      setRetryable(!definite);
      setSnapshot((old) => ({
        ...old,
        error:
          error instanceof OrtakApiError
            ? error.message
            : "The response was interrupted. Retry the same request to recover its saved result.",
      }));
      if (
        error instanceof OrtakApiError &&
        [401, 403, 404].includes(error.status)
      ) {
        blocked.current = true;
        owner.current.generation += 1;
        setBusy(false);
        setSnapshot({ ...empty(client), error: error.message });
      }
    } finally {
      if (
        owner.current.generation === generation &&
        active.current === controller
      )
        setBusy(false);
    }
  }
  return {
    ...value,
    busy,
    retryable,
    selectEmployee: (id: string) => {
      if (!busy) {
        setEmployee(id);
        setSnapshot((old) => ({ ...old, page: null }));
      }
    },
    refresh: () => {
      blocked.current = false;
      setRefresh((v) => v + 1);
    },
    saveDraft: (id: string, body: DraftRequest) =>
      submit({ employee: id, kind: "draft", body }),
    command: (id: string, body: ManagementRequest) =>
      submit({ employee: id, kind: "command", body }),
    retryRequest: () => {
      const action = pending.current;
      if (action && !busy) void submit(action);
    },
  };
}
