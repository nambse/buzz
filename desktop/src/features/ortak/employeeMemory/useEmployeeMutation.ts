import { useCallback, useEffect, useRef, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import { lostAuthority } from "../conversationMemory/useConversationRead";
import { assertPage, assertReceipt } from "./validation";
import { assertExport, assertExportReceipt } from "./exportValidation";
import {
  employeeMemoryPath,
  employeeExportPath,
  type EmployeeDraft,
  type EmployeeFact,
  type EmployeeExportAction,
  type MemoryOperation,
} from "./types";

export type EmployeeMemoryClient = Pick<
  OrtakClient,
  | "employees"
  | "employeeMemoryFacts"
  | "employeeMemoryPreview"
  | "employeeMemoryMutation"
  | "employeeMemoryExport"
  | "employeeMemoryExportMutation"
>;
const initial = {
  pending: null as MemoryOperation | null,
  busy: false,
  notice: null as string | null,
  revision: 0,
};

/** Retains exact command bytes in memory across close/reopen; every retry is freshly signed. */
export function useEmployeeMutation(
  client: EmployeeMemoryClient,
  employee: string,
  actor: string,
  context: string,
  active: boolean,
  denied: () => void,
) {
  const scope = JSON.stringify([employee, actor, context]);
  const current = useRef({ client, scope, active, denied });
  current.current = { client, scope, active, denied };
  const running = useRef<AbortController | null>(null);
  const retained = useRef<MemoryOperation | null>(null);
  const [owned, setOwned] = useState({ client, scope, value: initial });
  const state =
    owned.client === client && owned.scope === scope ? owned.value : initial;
  useEffect(() => {
    running.current?.abort();
    running.current = null;
    retained.current = null;
    setOwned({ client, scope, value: initial });
    return () => {
      running.current?.abort();
      running.current = null;
    };
  }, [client, scope]);
  useEffect(() => {
    if (!active) {
      running.current?.abort();
      running.current = null;
      setOwned((old) =>
        old.client === client && old.scope === scope
          ? { ...old, value: { ...old.value, busy: false } }
          : old,
      );
    }
  }, [client, scope, active]);
  const send = useCallback(
    async (operation: MemoryOperation, replay: boolean) => {
      if (
        !current.current.active ||
        current.current.client !== client ||
        current.current.scope !== scope ||
        running.current ||
        !employee ||
        !/^[0-9a-f]{64}$/.test(actor)
      )
        return;
      if (
        retained.current &&
        (retained.current.path !== operation.path ||
          retained.current.body !== operation.body)
      )
        return;
      const owner = new AbortController();
      running.current = owner;
      retained.current = operation;
      const live = () =>
        !owner.signal.aborted &&
        current.current.client === client &&
        current.current.scope === scope &&
        current.current.active;
      const update = (change: Partial<typeof initial>) => {
        if (live())
          setOwned((old) => ({
            client,
            scope,
            value: {
              ...(old.client === client && old.scope === scope
                ? old.value
                : initial),
              ...change,
            },
          }));
      };
      update({ pending: operation, busy: true, notice: null });
      try {
        // Remaining employee access is sufficient for Stop and exact receipt recovery.
        // Lost approval capability must not turn an uncertain write into a new command.
        const page = await client.employeeMemoryFacts(employee, owner.signal);
        if (!live()) return;
        assertPage(page, employee, actor);
        if (
          !replay &&
          !["stop", "retry_withdraw"].includes(operation.action) &&
          !page.can_approve
        )
          throw new OrtakApiError(403, "employee_memory_review_unavailable");
        if (operation.action === "approve" || operation.action === "stop") {
          const receipt = await client.employeeMemoryMutation(
            employee,
            operation.factId ?? null,
            operation.body,
            owner.signal,
          );
          if (!live()) return;
          assertReceipt(receipt, employee, actor, operation);
        } else {
          if (!operation.factId)
            throw new Error("A saved approval is required.");
          if (!replay) {
            const record = await client.employeeMemoryExport(
              employee,
              operation.factId,
              owner.signal,
            );
            if (!live()) return;
            assertExport(record, operation.factId);
            const expected = JSON.parse(operation.body).expected_version;
            const action =
              operation.action === "retry_withdraw" ? "withdraw" : "publish";
            if (
              operation.action === "publish"
                ? record.export !== null
                : !record.export?.jobs.some(
                    (job) =>
                      job.action === action &&
                      job.state === "failed" &&
                      job.retry_version === expected &&
                      expected < 8,
                  )
            )
              throw new OrtakApiError(409, "employee_memory_export_conflict");
          }
          const receipt = await client.employeeMemoryExportMutation(
            employee,
            operation.factId,
            operation.action,
            operation.body,
            owner.signal,
          );
          if (!live()) return;
          assertExportReceipt(receipt, operation);
        }
        retained.current = null;
        setOwned((old) => ({
          client,
          scope,
          value: {
            pending: null,
            busy: false,
            notice:
              operation.action === "stop"
                ? "Stop recorded. Approval history is retained."
                : operation.action === "approve"
                  ? "Approval saved. Publish it separately to make it available for eligible runs."
                  : operation.action === "retry_withdraw"
                    ? "Removal retry recorded. Refresh its status to check acknowledgment."
                    : "Publication request recorded. Refresh its status to check acknowledgment.",
            revision: old.value.revision + 1,
          },
        }));
      } catch (cause) {
        if (!live()) return;
        if (
          cause instanceof OrtakApiError &&
          [400, 401, 403, 404, 409, 413, 422].includes(cause.status)
        ) {
          retained.current = null;
          if (lostAuthority(cause)) current.current.denied();
          setOwned((old) => ({
            client,
            scope,
            value: {
              pending: null,
              busy: false,
              notice:
                cause.status === 409 &&
                operation.action !== "approve" &&
                operation.action !== "stop"
                  ? operation.action === "retry_withdraw"
                    ? "Removal could not be retried. Refresh its status before trying again."
                    : "Publication could not be started. Refresh its status; an operator may need to configure the employee’s destination."
                  : cause.message,
              revision: old.value.revision + (lostAuthority(cause) ? 0 : 1),
            },
          }));
        } else
          update({
            notice:
              "The result is uncertain. Retry this exact request before starting another action.",
          });
      } finally {
        update({ busy: false });
        if (running.current === owner) running.current = null;
        owner.abort();
      }
    },
    [client, scope, employee, actor],
  );
  function submit(
    action: MemoryOperation["action"],
    values: Record<string, unknown>,
    factId?: string,
  ) {
    if (
      retained.current ||
      running.current ||
      !current.current.active ||
      current.current.scope !== scope ||
      current.current.client !== client
    )
      return;
    const operationId = crypto.randomUUID();
    const body = JSON.stringify({ operation_id: operationId, ...values });
    if (new TextEncoder().encode(body).length > 32768) return;
    const path =
      action === "approve" || action === "stop"
        ? `${employeeMemoryPath(employee)}${factId ? `/${encodeURIComponent(factId)}/stop` : ""}`
        : factId
          ? employeeExportPath(employee, factId, action)
          : "";
    if (!path) return;
    void send(
      Object.freeze({ path, body, operationId, action, factId }),
      false,
    );
  }
  return {
    ...state,
    revoke: () => {
      running.current?.abort();
      running.current = null;
      retained.current = null;
      setOwned((old) => ({
        client,
        scope,
        value: {
          ...initial,
          revision:
            old.client === client && old.scope === scope
              ? old.value.revision
              : 0,
          notice: "Access changed. Refresh before continuing.",
        },
      }));
    },
    approve: (fact: EmployeeDraft) => submit("approve", { fact }),
    stop: (fact: EmployeeFact) => {
      if (fact.employee_id === employee && fact.version === 1 && fact.can_stop)
        submit("stop", { expected_version: 1 }, fact.id);
    },
    publication: (
      fact: EmployeeFact,
      action: EmployeeExportAction,
      expectedVersion: number,
    ) => {
      if (fact.employee_id !== employee) return;
      if (
        action !== "retry_withdraw" &&
        (fact.version !== 1 ||
          fact.status !== "approved" ||
          !fact.source_current ||
          Date.parse(fact.expires_at) <= Date.now())
      )
        return;
      if (
        !Number.isInteger(expectedVersion) ||
        (action === "publish"
          ? expectedVersion !== 1
          : expectedVersion < 0 || expectedVersion >= 8)
      )
        return;
      submit(action, { expected_version: expectedVersion }, fact.id);
    },
    retry: () => {
      if (retained.current) void send(retained.current, true);
    },
  };
}
