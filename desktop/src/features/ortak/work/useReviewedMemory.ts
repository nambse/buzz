import { useEffect, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import type { ReviewedFactPage } from "./memoryTypes";

/** Current-authority pages; stop on the first failed refresh until manual retry. */
export function useReviewedMemory(
  client: OrtakClient,
  project: string,
  employee: string,
  after: string | undefined,
  refresh: number | string,
  revoke: () => void,
) {
  const scope = JSON.stringify([project, employee, after]);
  const key = JSON.stringify([scope, refresh]);
  const [state, setState] = useState<{
    key: string;
    scope: string;
    client: OrtakClient;
    page: ReviewedFactPage | null;
    error: string | null;
    stamp: number;
    fresh: boolean;
  }>({ key, scope, client, page: null, error: null, stamp: 0, fresh: false });
  useEffect(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    setState((previous) => ({
      key,
      scope,
      client,
      page:
        previous.scope === scope && previous.client === client
          ? previous.page
          : null,
      error: null,
      stamp: previous.stamp + 1,
      fresh: false,
    }));
    if (!employee) return () => controller.abort();
    async function read() {
      setState((previous) => ({
        ...previous,
        stamp: previous.stamp + 1,
      }));
      try {
        const page = await client.reviewedMemory(
          project,
          employee,
          controller.signal,
          after,
        );
        if (controller.signal.aborted) return;
        setState((previous) => ({
          key,
          scope,
          client,
          page,
          error: null,
          stamp: previous.stamp + 1,
          fresh: true,
        }));
        timer = setTimeout(() => void read(), 5000);
      } catch (cause) {
        if (controller.signal.aborted) return;
        const lost =
          cause instanceof OrtakApiError &&
          [401, 403, 404].includes(cause.status);
        setState((previous) => ({
          key,
          scope,
          client,
          page: lost ? null : previous.page,
          error:
            cause instanceof Error
              ? cause.message
              : "Reviewed memory is unavailable. Refresh to try again.",
          stamp: previous.stamp + 1,
          fresh: false,
        }));
        if (lost) revoke();
      }
    }
    void read();
    return () => {
      controller.abort();
      if (timer) clearTimeout(timer);
    };
  }, [client, project, employee, after, scope, key, revoke]);
  if (state.scope !== scope || state.client !== client)
    return { key, client, page: null, error: null, stamp: 0, fresh: false };
  return state.key === key
    ? state
    : { ...state, key, stamp: state.stamp + 1, fresh: false };
}
