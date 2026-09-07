import { useEffect, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import type {
  ProjectPage,
  WorkItem,
  WorkPage,
  WorkProject,
  WorkExecution,
} from "../types";

interface WorkData {
  projects: ProjectPage;
  project: WorkProject | null;
  items: WorkPage | null;
  item: WorkItem | null;
  executions: WorkExecution[];
}

/** Abort old selections and clear every private projection on authorization loss. */
export function useWorkData(
  client: OrtakClient,
  projectId: string | null,
  itemId: string | null,
  projectCursor: string | undefined,
  itemCursor: string | undefined,
  refresh: number,
  revoked: boolean,
) {
  const scope = JSON.stringify([projectId, itemId, projectCursor, itemCursor]);
  const key = JSON.stringify([scope, refresh, revoked]);
  const [state, setState] = useState<{
    key: string;
    scope: string;
    client: OrtakClient;
    data: WorkData | null;
    error: string | null;
    revoked: boolean;
    fresh: boolean;
  }>({ key, scope, client, data: null, error: null, revoked, fresh: false });
  useEffect(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    setState((previous) => ({
      key,
      scope,
      client,
      data:
        !revoked && previous.scope === scope && previous.client === client
          ? previous.data
          : null,
      error: null,
      revoked,
      fresh: false,
    }));
    if (revoked) return () => controller.abort();
    async function poll() {
      // Routine polling keeps the last successful authority until a response.
      // Explicit refreshes and failed reads already pause writes. Toggling every
      // fieldset at each timer tick would interrupt open native select menus.
      const round = new AbortController();
      const signal = AbortSignal.any([controller.signal, round.signal]);
      try {
        const [projects, project, items, item, executions] = await Promise.all([
          client.projects(signal, projectCursor),
          projectId ? client.project(projectId, signal) : null,
          projectId ? client.workItems(projectId, signal, itemCursor) : null,
          itemId ? client.workItem(itemId, signal) : null,
          itemId ? client.workExecutions(itemId, signal) : null,
        ]);
        if (controller.signal.aborted) return;
        // Never display an item if the selected project was replaced while reading.
        if (item && item.work_item.project_id !== projectId)
          throw new Error(
            "This item no longer belongs to the selected project.",
          );
        setState({
          key,
          scope,
          client,
          data: {
            projects,
            project: project?.project ?? null,
            items,
            item: item?.work_item ?? null,
            executions: executions?.executions ?? [],
          },
          error: null,
          revoked: false,
          fresh: true,
        });
        failures = 0;
        timer = setTimeout(() => void poll(), 5000);
      } catch (cause) {
        round.abort();
        if (controller.signal.aborted) return;
        const lost =
          cause instanceof OrtakApiError &&
          [401, 403, 404].includes(cause.status);
        // A failed refresh cannot continue to authorize write controls.
        setState((previous) => ({
          key,
          scope,
          client,
          data: lost ? null : previous.data,
          error:
            cause instanceof Error
              ? cause.message
              : "Work could not be loaded.",
          revoked: lost,
          fresh: false,
        }));
        failures++;
        if (!lost && failures < 5)
          timer = setTimeout(
            () => void poll(),
            Math.min(3000 * 2 ** (failures - 1), 30000),
          );
      }
    }
    void poll();
    return () => {
      controller.abort();
      if (timer) clearTimeout(timer);
    };
  }, [
    client,
    key,
    scope,
    projectId,
    itemId,
    projectCursor,
    itemCursor,
    revoked,
  ]);
  // Keep drafts mounted only inside the same private scope. A refresh token is
  // not a new scope, but cannot authorize another write until its read settles.
  if (revoked || state.scope !== scope || state.client !== client)
    return { key, data: null, error: null, revoked, fresh: false };
  return state.key === key ? state : { ...state, key, fresh: false };
}
