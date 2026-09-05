import { useEffect, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import type { ProjectPage, WorkItem, WorkPage, WorkProject } from "../types";

interface WorkData {
  projects: ProjectPage;
  project: WorkProject | null;
  items: WorkPage | null;
  item: WorkItem | null;
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
  const key = JSON.stringify([
    projectId,
    itemId,
    projectCursor,
    itemCursor,
    refresh,
    revoked,
  ]);
  const [state, setState] = useState<{
    key: string;
    data: WorkData | null;
    error: string | null;
    revoked: boolean;
  }>({ key, data: null, error: null, revoked });
  useEffect(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    setState({ key, data: null, error: null, revoked });
    if (revoked) return () => controller.abort();
    async function poll() {
      const round = new AbortController();
      const signal = AbortSignal.any([controller.signal, round.signal]);
      try {
        const [projects, project, items, item] = await Promise.all([
          client.projects(signal, projectCursor),
          projectId ? client.project(projectId, signal) : null,
          projectId ? client.workItems(projectId, signal, itemCursor) : null,
          itemId ? client.workItem(itemId, signal) : null,
        ]);
        if (controller.signal.aborted) return;
        // Never display an item if the selected project was replaced while reading.
        if (item && item.work_item.project_id !== projectId)
          throw new Error(
            "This item no longer belongs to the selected project.",
          );
        setState({
          key,
          data: {
            projects,
            project: project?.project ?? null,
            items,
            item: item?.work_item ?? null,
          },
          error: null,
          revoked: false,
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
        setState({
          key,
          data: null,
          error:
            cause instanceof Error
              ? cause.message
              : "Work could not be loaded.",
          revoked: lost,
        });
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
  }, [client, key, projectId, itemId, projectCursor, itemCursor, revoked]);
  return state.key === key ? state : { key, data: null, error: null, revoked };
}
