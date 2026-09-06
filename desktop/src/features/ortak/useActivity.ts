import { useEffect, useState } from "react";
import { appendActivity } from "./activity";
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
  const [reconnecting, setReconnecting] = useState(false);
  const [accessRevoked, setAccessRevoked] = useState(false);
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
    setAccessRevoked(false);
    setConnected(false);
    setReconnecting(false);
    async function connect() {
      let received = false;
      try {
        await client.activityStream(
          runId,
          cursor,
          controller.signal,
          (frame) => {
            if (controller.signal.aborted) return;
            const next = appendActivity(retained, cursor, frame.page);
            retained = next.entries;
            cursor = next.cursor;
            setDetail(frame.detail);
            setEntries(retained);
            setConnected(true);
            setReconnecting(false);
            setError(null);
            received = true;
          },
        );
        if (controller.signal.aborted) return;
        // Only a completed authenticated lifetime resets repeated disconnects.
        // Receiving the initial replay alone must not create an endless loop.
        if (!received)
          throw new Error("Activity closed before confirming its cursor.");
        failures = 0;
        setConnected(false);
        setReconnecting(true);
        timer = setTimeout(() => void connect(), 250);
      } catch (cause) {
        if (controller.signal.aborted) return;
        setConnected(false);
        setError(
          cause instanceof Error
            ? cause.message
            : "Ortak could not load activity.",
        );
        failures += 1;
        const revoked =
          cause instanceof OrtakApiError &&
          [401, 403, 404].includes(cause.status);
        const resync = cause instanceof OrtakApiError && cause.status === 409;
        if (revoked) {
          setAccessRevoked(true);
          setEntries([]);
          setDetail(null);
        }
        const retry = !revoked && !resync && failures < 5;
        setReconnecting(retry);
        if (retry)
          timer = setTimeout(
            () => void connect(),
            Math.min(3000 * 2 ** (failures - 1), 30_000),
          );
      }
    }
    void connect();
    return () => {
      controller.abort();
      if (timer) clearTimeout(timer);
    };
  }, [client, runId, refresh]);
  return { detail, entries, error, connected, reconnecting, accessRevoked };
}
