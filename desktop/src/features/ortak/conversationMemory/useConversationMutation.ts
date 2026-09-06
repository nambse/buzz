import { useCallback, useEffect, useRef, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import { workOperation, type WorkOperation } from "../work/operations";
import type { ConversationExportReceipt, ConversationReceipt } from "./types";
import { assertConversationExport, conversationAction } from "./operations";
import { lostAuthority } from "./useConversationRead";

export type ConversationClient = Pick<
  OrtakClient,
  | "project"
  | "projects"
  | "employees"
  | "conversationPreview"
  | "conversationFacts"
  | "conversationMutation"
  | "conversationExportMutation"
>;
type State = {
  pending: WorkOperation | null;
  busy: boolean;
  notice: string | null;
  receipt: ConversationReceipt | null;
  exportReceipt: ConversationExportReceipt | null;
  revision: number;
};
const initial = (): State => ({
  pending: null,
  busy: false,
  notice: null,
  receipt: null,
  exportReceipt: null,
  revision: 0,
});

/** One exact in-memory request survives uncertain I/O and dialog close/reopen. */
export function useConversationMutation(
  client: ConversationClient,
  project: string,
  employee: string,
  channel: string,
  active: boolean,
  denied: () => void,
  context = "",
  refused?: () => void,
) {
  const scope = JSON.stringify([project, employee, channel, context]);
  const latest = useRef({ client, scope, active, denied, refused });
  latest.current = { client, scope, active, denied, refused };
  const running = useRef<AbortController | null>(null);
  const retained = useRef<WorkOperation | null>(null);
  const [owned, setOwned] = useState({ client, scope, value: initial() });
  const state =
    owned.client === client && owned.scope === scope ? owned.value : initial();
  const currentState = useRef(state);
  currentState.current = state;
  const update = useCallback(
    (change: (value: State) => State) =>
      setOwned((previous) =>
        previous.client === client && previous.scope === scope
          ? { ...previous, value: change(previous.value) }
          : previous,
      ),
    [client, scope],
  );
  useEffect(() => {
    running.current?.abort();
    running.current = null;
    retained.current = null;
    setOwned({ client, scope, value: initial() });
    return () => running.current?.abort();
  }, [client, scope]);
  useEffect(() => {
    if (!active) {
      running.current?.abort();
      running.current = null;
      update((value) => ({ ...value, busy: false }));
    }
    return () => running.current?.abort();
  }, [active, update]);

  async function send(operation: WorkOperation) {
    if (
      !latest.current.active ||
      latest.current.client !== client ||
      latest.current.scope !== scope ||
      running.current ||
      !project ||
      !employee
    )
      return;
    if (
      retained.current &&
      (retained.current.body !== operation.body ||
        retained.current.path !== operation.path)
    )
      return;
    const owner = new AbortController();
    running.current = owner;
    retained.current = operation;
    const stillCurrent = () =>
      !owner.signal.aborted &&
      latest.current.active &&
      latest.current.client === client &&
      latest.current.scope === scope &&
      running.current === owner;
    update((value) => ({
      ...value,
      busy: true,
      pending: operation,
      notice: null,
      receipt: null,
      exportReceipt: null,
    }));
    try {
      const action = conversationAction(project, operation.path);
      // Recovery checks project role and employee ceiling, not the old source.
      // Archived/inactive selections can reconcile receipts and withdrawal.
      const [selected] = await Promise.all([
        client.project(project, owner.signal),
        client.conversationFacts(project, employee, owner.signal),
      ]);
      owner.signal.throwIfAborted();
      if (!stillCurrent()) return;
      if (
        selected.project.id !== project ||
        selected.project.channel_id !== channel ||
        !selected.project.can_review
      )
        throw new OrtakApiError(403, "conversation_authority_changed");
      if (action.kind === "publish" || action.kind === "retry") {
        const receipt = await client.conversationExportMutation(
          operation.path,
          operation.body,
          owner.signal,
        );
        owner.signal.throwIfAborted();
        if (!stillCurrent()) return;
        assertConversationExport(receipt.export, action.factId);
        if (action.kind === "retry") {
          const job =
            action.action === "publish"
              ? receipt.export.publication
              : receipt.export.cleanup;
          if (job.retry_version <= JSON.parse(operation.body).retry_version)
            throw new Error("conversation_export_retry_receipt_mismatch");
        }
        retained.current = null;
        update((value) => ({
          ...value,
          pending: null,
          exportReceipt: receipt,
          revision: value.revision + 1,
          notice:
            action.kind === "publish"
              ? "Publication request accepted. Check the saved publication status for acknowledgement."
              : "Retry accepted for the same reviewed-store job. Check its current acknowledgement below.",
        }));
      } else {
        const receipt = await client.conversationMutation(
          operation.path,
          operation.body,
          owner.signal,
        );
        owner.signal.throwIfAborted();
        if (!stillCurrent()) return;
        const fact = receipt.fact.fact;
        const submitted = JSON.parse(operation.body).fact;
        if (
          fact.project_id !== project ||
          fact.employee_id !== employee ||
          (action.kind === "approve" &&
            fact.source_visible &&
            (fact.source?.kind !== "conversation" ||
              fact.source.message_id !== submitted.source_message_id ||
              receipt.fact.audience_hash !==
                submitted.expected_audience_hash)) ||
          (action.kind === "stop" &&
            (action.factId !== fact.id || fact.version !== 2))
        )
          throw new Error("conversation_receipt_mismatch");
        retained.current = null;
        update((value) => ({
          ...value,
          pending: null,
          receipt,
          revision: value.revision + 1,
          notice:
            action.kind === "stop"
              ? "Use stopped. The approval record remains stored; any published text has a separate cleanup status."
              : "Conversation fact approved. Publishing it requires a separate confirmation.",
        }));
      }
    } catch (cause) {
      if (!stillCurrent()) return;
      const refused =
        cause instanceof OrtakApiError &&
        [400, 401, 403, 404, 409, 413, 422].includes(cause.status);
      if (refused) retained.current = null;
      if (lostAuthority(cause)) latest.current.denied();
      else if (refused) latest.current.refused?.();
      update((value) => ({
        ...value,
        pending: refused ? null : operation,
        receipt: null,
        exportReceipt: null,
        notice: refused
          ? "This request was refused. Refresh the audience and saved facts before another attempt."
          : "Confirmation is missing. The request may be saved. Retry this exact operation after refreshing access.",
      }));
    } finally {
      if (running.current === owner) {
        running.current = null;
        update((value) => ({ ...value, busy: false }));
      }
      owner.abort();
    }
  }
  return {
    ...state,
    submit: (path: string, values: Record<string, unknown>) => {
      if (retained.current || state.pending || state.busy || !active) return;
      try {
        conversationAction(project, path);
        void send(workOperation(path, "Conversation memory", values));
      } catch {
        update((value) => ({
          ...value,
          notice:
            "This memory action or its submitted values could not be prepared. Refresh before trying again.",
        }));
      }
    },
    retry: () => {
      if (state.pending) void send(state.pending);
    },
    clearReceipt: () => {
      if (!currentState.current.pending && !running.current)
        update((value) => ({
          ...value,
          receipt: null,
          exportReceipt: null,
          notice: null,
        }));
    },
  };
}
