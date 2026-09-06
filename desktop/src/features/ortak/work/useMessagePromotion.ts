import { useCallback, useEffect, useRef, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import type { ProjectPage, WorkItem } from "../types";
import { workOperation, type WorkOperation } from "./operations";

export type PromotionClient = Pick<
  OrtakClient,
  "projects" | "project" | "routingDecision" | "workMutation"
>;
type State = {
  page: ProjectPage | null;
  ready: boolean;
  revoked: boolean;
  error: string | null;
  pending: WorkOperation | null;
  result: WorkItem | null;
  busy: boolean;
  notice: string | null;
  formGeneration: number;
};
const initial = (): State => ({
  page: null,
  ready: false,
  revoked: false,
  error: null,
  pending: null,
  result: null,
  busy: false,
  notice: null,
  formGeneration: 0,
});
const denied = (cause: unknown) =>
  cause instanceof OrtakApiError && [401, 403, 404].includes(cause.status);

/** Read current source/project authority; retain only one exact uncertain write in memory. */
export function useMessagePromotion(
  client: PromotionClient,
  channel: string,
  message: string,
  open: boolean,
) {
  const identity = `${channel}:${message}`;
  const [owned, setOwned] = useState({ client, identity, value: initial() });
  const [cursor, setCursor] = useState<string>();
  const [refresh, setRefresh] = useState(0);
  const write = useRef<AbortController | null>(null);
  const readOwner = useRef<AbortController | null>(null);
  const active = useRef(false);
  const current = owned.client === client && owned.identity === identity;
  const state = current ? owned.value : initial();
  const update = useCallback(
    (change: (value: State) => State) => {
      setOwned((previous) =>
        previous.client === client && previous.identity === identity
          ? { ...previous, value: change(previous.value) }
          : previous,
      );
    },
    [client, identity],
  );
  const revoke = useCallback(() => {
    readOwner.current?.abort();
    readOwner.current = null;
    write.current?.abort();
    write.current = null;
    update((value) => ({
      ...initial(),
      revoked: true,
      error: "This message or project is no longer available to your account.",
      formGeneration: value.formGeneration + 1,
    }));
  }, [update]);
  useEffect(() => {
    write.current?.abort();
    write.current = null;
    setOwned({ client, identity, value: initial() });
    setCursor(undefined);
    return () => write.current?.abort();
  }, [client, identity]);
  useEffect(() => {
    active.current = open;
    if (!open) {
      write.current?.abort();
      write.current = null;
      update((value) => ({ ...value, ready: false, busy: false }));
    }
    return () => {
      active.current = false;
      write.current?.abort();
    };
  }, [open, update]);
  useEffect(() => {
    void refresh;
    const owner = new AbortController();
    readOwner.current = owner;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    update((value) => ({ ...value, ready: false }));
    if (!open) return () => owner.abort();
    async function read() {
      if (owner.signal.aborted) return;
      try {
        const [page, source] = await Promise.all([
          client.projects(owner.signal, cursor),
          client.routingDecision(channel, message, owner.signal),
        ]);
        if (owner.signal.aborted) return;
        if (source.message_id !== message || source.channel_id !== channel)
          throw new Error("source_response_mismatch");
        update((value) => ({
          ...value,
          page,
          ready: Boolean(source.decision),
          revoked: false,
          error: source.decision
            ? null
            : "This message has no recorded routing decision yet. Refresh after it is processed.",
        }));
        failures = 0;
        timer = setTimeout(() => void read(), 5000);
      } catch (cause) {
        if (owner.signal.aborted) return;
        if (denied(cause)) {
          revoke();
          return;
        }
        update((value) => ({
          ...value,
          ready: false,
          error:
            "Access could not be refreshed. Your draft is retained; refresh before continuing.",
        }));
        if (++failures < 5)
          timer = setTimeout(
            () => void read(),
            Math.min(3000 * 2 ** (failures - 1), 30000),
          );
      }
    }
    void read();
    return () => {
      owner.abort();
      if (readOwner.current === owner) readOwner.current = null;
      if (timer) clearTimeout(timer);
    };
  }, [client, channel, message, open, cursor, refresh, update, revoke]);

  async function send(operation: WorkOperation) {
    if (!active.current || write.current || !state.ready || state.revoked)
      return;
    const controller = new AbortController();
    write.current = controller;
    update((value) => ({
      ...value,
      busy: true,
      pending: operation,
      notice: null,
    }));
    try {
      const body = JSON.parse(operation.body);
      const projectId = operation.path.split("/")[4];
      const [selection, source] = await Promise.all([
        client.project(decodeURIComponent(projectId), controller.signal),
        client.routingDecision(channel, message, controller.signal),
      ]);
      controller.signal.throwIfAborted();
      if (
        selection.project.id !== decodeURIComponent(projectId) ||
        selection.project.channel_id !== channel ||
        !selection.project.can_contribute ||
        source.channel_id !== channel ||
        source.message_id !== message ||
        !source.decision ||
        body.source_message_id !== message
      )
        throw new OrtakApiError(403, "promotion_authority_changed");
      // Archived projects can still reconcile an already saved identical promotion.
      // New choices are restricted to active projects by the signed list below.
      const saved = await client.workMutation(
        operation.path,
        operation.body,
        controller.signal,
      );
      controller.signal.throwIfAborted();
      if (
        !saved.work_item ||
        saved.work_item.project_id !== selection.project.id ||
        saved.work_item.source_message_id !== message
      )
        throw new Error("promotion_response_mismatch");
      update((value) => ({
        ...value,
        pending: null,
        result: saved.work_item ?? null,
        notice:
          "The message is linked to saved Work. Assignment and execution remain explicit actions.",
      }));
    } catch (cause) {
      if (controller.signal.aborted) return;
      if (denied(cause)) {
        revoke();
        return;
      }
      const refused =
        cause instanceof OrtakApiError &&
        [400, 409, 413, 422].includes(cause.status);
      update((value) => ({
        ...value,
        pending: refused ? null : operation,
        ready: false,
        notice: refused
          ? "The promotion was refused or conflicts with saved Work. Refresh access and check Projects & Work before trying another definition."
          : "Confirmation is missing. This promotion may already be saved. Refresh access, then retry the same operation.",
      }));
    } finally {
      if (write.current === controller) {
        write.current = null;
        update((value) => ({ ...value, busy: false }));
      }
    }
  }
  return {
    ...state,
    cursor,
    refresh: () => setRefresh((value) => value + 1),
    page: state.page
      ? {
          ...state.page,
          projects: state.page.projects.filter(
            (project) =>
              project.channel_id === channel &&
              project.status === "active" &&
              project.can_contribute,
          ),
        }
      : null,
    setCursor: (value?: string) => {
      if (!state.pending && !state.busy) setCursor(value);
    },
    submit: (path: string, label: string, values: Record<string, unknown>) => {
      if (
        !state.ready ||
        state.pending ||
        state.result ||
        state.busy ||
        state.revoked
      )
        return;
      const project = state.page?.projects.find(
        (candidate) =>
          path ===
          `/api/v1/projects/${encodeURIComponent(candidate.id)}/promotions`,
      );
      if (
        !project ||
        project.channel_id !== channel ||
        !project.can_contribute ||
        project.status !== "active"
      )
        return;
      try {
        void send(
          workOperation(path, label, { ...values, source_message_id: message }),
        );
      } catch {
        update((value) => ({
          ...value,
          notice: "Shorten the work definition before submitting.",
        }));
      }
    },
    retry: () => {
      if (state.pending) void send(state.pending);
    },
  };
}
