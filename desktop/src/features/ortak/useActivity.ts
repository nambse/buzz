import { useEffect, useState } from "react";
import { appendActivity, needsActivityPoll } from "./activity";
import { OrtakApiError, type OrtakClient } from "./client";
import type { ActivityEntry, RunDetailResponse } from "./types";

export function useRunActivity(
  client: OrtakClient,
  runId: string,
  refresh: number,
) {
  const [detail, setDetail] = useState<RunDetailResponse | null>(null);
  const [entries, setEntries] = useState<ActivityEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [connected, setConnected] = useState(false);
  useEffect(() => {
    // A manual reload deliberately starts a new cursor generation.
    void refresh;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let cursor: number | null = null;
    let retained: ActivityEntry[] = [];
    let failures = 0;
    setEntries([]);
    setDetail(null);
    setError(null);
    setConnected(false);
    async function poll() {
      const round = new AbortController();
      const signal = AbortSignal.any([controller.signal, round.signal]);
      try {
        const [nextDetail, page] = await Promise.all([
          client.detail(runId, signal),
          client.events(runId, cursor, signal),
        ]);
        if (controller.signal.aborted) return;
        const next = appendActivity(retained, cursor, page);
        retained = next.entries;
        cursor = next.cursor;
        setDetail(nextDetail);
        setEntries(retained);
        setConnected(true);
        setError(null);
        failures = 0;
        // Drain a bounded page at a time; terminal detail is not enough to stop
        // until all already-durable events have been consumed.
        if (needsActivityPoll(nextDetail, page, cursor)) {
          timer = setTimeout(() => void poll(), page.has_more ? 250 : 2500);
        }
      } catch (cause) {
        round.abort();
        if (controller.signal.aborted) return;
        setConnected(false);
        setError(
          cause instanceof Error
            ? cause.message
            : "Ortak could not load activity.",
        );
        failures += 1;
        // Authorization changes discard cached private data immediately.
        const revoked =
          cause instanceof OrtakApiError &&
          [401, 403, 404].includes(cause.status);
        if (revoked) {
          setEntries([]);
          setDetail(null);
        }
        if (!revoked && failures < 5)
          timer = setTimeout(
            () => void poll(),
            Math.min(3000 * 2 ** (failures - 1), 30_000),
          );
      }
    }
    void poll();
    return () => {
      controller.abort();
      if (timer) clearTimeout(timer);
    };
  }, [client, runId, refresh]);
  return { detail, entries, error, connected };
}
