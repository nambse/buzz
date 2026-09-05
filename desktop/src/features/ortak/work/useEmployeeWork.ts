import { useEffect, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import type { EmployeeWorkPage } from "../types";

/** One bounded queue page, fenced by employee and client identity on every read. */
export function useEmployeeWork(
  client: OrtakClient,
  employeeId: string,
  cursor: string | undefined,
  refresh: number,
) {
  const key = JSON.stringify([employeeId, cursor, refresh]);
  const [state, setState] = useState<{
    client: OrtakClient;
    key: string;
    page: EmployeeWorkPage | null;
    error: string | null;
  }>({ client, key, page: null, error: null });
  useEffect(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    setState({ client, key, page: null, error: null });
    async function poll() {
      try {
        const page = await client.employeeWork(
          employeeId,
          controller.signal,
          cursor,
        );
        if (controller.signal.aborted) return;
        if (
          page.employee_id !== employeeId ||
          page.work_items.length > 25 ||
          page.execution_available !== false
        )
          throw new Error(
            "The assignment response did not match this employee queue.",
          );
        setState({ client, key, page, error: null });
        failures = 0;
        timer = setTimeout(() => void poll(), 5000);
      } catch (cause) {
        if (controller.signal.aborted) return;
        const revoked =
          cause instanceof OrtakApiError &&
          [401, 403, 404].includes(cause.status);
        setState({
          client,
          key,
          page: null,
          error:
            cause instanceof OrtakApiError && cause.status === 409
              ? "The assignment queue changed while loading. Refresh assigned work to load its current state."
              : cause instanceof OrtakApiError && cause.status >= 500
                ? "Assigned work is unavailable. Refresh assigned work to try again."
                : cause instanceof Error
                  ? cause.message
                  : "Assigned work could not be loaded.",
        });
        failures++;
        if (!revoked && failures < 5)
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
  }, [client, employeeId, cursor, key]);
  return state.client === client && state.key === key
    ? state
    : { client, key, page: null, error: null };
}
