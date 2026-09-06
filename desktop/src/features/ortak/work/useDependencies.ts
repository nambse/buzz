import { useEffect, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import type { WorkDependencyPage } from "../types";

/** Current relation reads clear on every failure and stop after five attempts. */
export function useDependencies(
  client: OrtakClient,
  id: string,
  version: number,
  revoke: () => void,
) {
  const [refresh, setRefresh] = useState(0);
  const key = `${id}:${version}:${refresh}`;
  const [state, setState] = useState<{
    client: OrtakClient;
    key: string;
    data: WorkDependencyPage | null;
    error: string | null;
  }>({ client, key, data: null, error: null });
  useEffect(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    setState({ client, key, data: null, error: null });
    async function read() {
      try {
        const data = await client.workDependencies(id, controller.signal);
        if (controller.signal.aborted) return;
        if (
          data.work_item_id !== id ||
          data.work_version !== version ||
          !Array.isArray(data.dependencies) ||
          data.dependencies.length > 32
        )
          throw new Error(
            "Work changed while dependencies were read. Refresh work to retry.",
          );
        setState({ client, key, data, error: null });
        failures = 0;
        timer = setTimeout(() => void read(), 5000);
      } catch (cause) {
        if (controller.signal.aborted) return;
        setState({
          client,
          key,
          data: null,
          error:
            "Dependencies could not be read. Refresh work or retry dependencies.",
        });
        const lost =
          cause instanceof OrtakApiError &&
          [401, 403, 404].includes(cause.status);
        if (lost) {
          revoke();
          return;
        }
        failures++;
        if (failures < 5)
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
  }, [client, key, id, version, revoke]);
  const current =
    state.client === client && state.key === key
      ? state
      : { data: null, error: null };
  return { ...current, refresh: () => setRefresh((value) => value + 1) };
}
