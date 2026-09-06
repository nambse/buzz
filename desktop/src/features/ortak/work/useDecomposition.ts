import { useEffect, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import type { WorkDecomposition } from "../types";

/** Current structural links clear on scope changes and stop after five failures. */
export function useDecomposition(
  client: OrtakClient,
  id: string,
  version: number,
  project: string,
  revoke: () => void,
) {
  const [refresh, setRefresh] = useState(0);
  const key = `${project}:${id}:${version}:${refresh}`;
  const [state, setState] = useState<{
    client: OrtakClient;
    key: string;
    data: WorkDecomposition | null;
    error: string | null;
  }>({ client, key, data: null, error: null });
  useEffect(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    setState({ client, key, data: null, error: null });
    async function read() {
      try {
        const data = await client.workDecomposition(id, controller.signal);
        if (controller.signal.aborted) return;
        if (
          data.work_item_id !== id ||
          data.work_version !== version ||
          !Array.isArray(data.children) ||
          data.children.length > 32 ||
          [...data.children, ...(data.parent ? [data.parent] : [])].some(
            (entry) => entry.id === id || entry.project_id !== project,
          ) ||
          new Set(data.children.map((entry) => entry.id)).size !==
            data.children.length
        )
          throw new Error("Work changed while its structural links were read.");
        setState({ client, key, data, error: null });
        failures = 0;
        timer = setTimeout(() => void read(), 5000);
      } catch (cause) {
        if (controller.signal.aborted) return;
        setState({
          client,
          key,
          data: null,
          error: "Work links could not be read. Refresh work or retry links.",
        });
        if (
          cause instanceof OrtakApiError &&
          [401, 403, 404].includes(cause.status)
        ) {
          revoke();
          return;
        }
        if (++failures < 5)
          timer = setTimeout(
            () => void read(),
            Math.min(3000 * 2 ** (failures - 1), 30000),
          );
      }
    }
    void read();
    return () => {
      controller.abort();
      if (timer) clearTimeout(timer);
    };
  }, [client, key, id, version, project, revoke]);
  const current =
    state.client === client && state.key === key
      ? state
      : { data: null, error: null };
  return { ...current, refresh: () => setRefresh((value) => value + 1) };
}
