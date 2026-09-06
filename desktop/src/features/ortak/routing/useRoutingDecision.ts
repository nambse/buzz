import { useEffect, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import { routingPage } from "./routingStream";
import type { RoutingDecisionPage } from "./types";

type Client = Pick<OrtakClient, "routingDecisionStream">;

/** Signed current snapshots, bounded reconnect and effect-owned transport. */
export function useRoutingDecision(
  client: Client,
  channel: string,
  message: string,
  refresh: number,
) {
  const [state, setState] = useState<{
    client: Client;
    identity: string;
    refresh: number;
    page: RoutingDecisionPage | null;
    error: string | null;
    checkedAt: number | null;
    retrying: boolean;
    connected: boolean;
  } | null>(null);
  const identity = `${channel}:${message}`;
  useEffect(() => {
    const owner = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let active: AbortController | undefined;
    let attempt = 0,
      failures = 0;
    setState(null);
    async function connect() {
      const generation = ++attempt;
      const connection = new AbortController();
      active = connection;
      const signal = AbortSignal.any([owner.signal, connection.signal]);
      let received = false;
      const current = () => !signal.aborted && generation === attempt;
      try {
        await client.routingDecisionStream(
          channel,
          message,
          signal,
          (value) => {
            if (!current()) return;
            const page = routingPage(value, channel, message);
            received = true;
            setState({
              client,
              identity,
              refresh,
              page,
              error: null,
              checkedAt: Date.now(),
              retrying: false,
              connected: true,
            });
          },
        );
        if (!current()) return;
        if (!received)
          throw new Error("Routing renewed without a current snapshot.");
        failures = 0;
        setState((previous) =>
          previous
            ? { ...previous, connected: false, retrying: true }
            : previous,
        );
        // A normal45s renewal signs a new subscription and re-reads current data.
        timer = setTimeout(() => void connect(), 1000);
      } catch (cause) {
        if (!current()) return;
        failures += 1;
        const denied =
          cause instanceof OrtakApiError &&
          [401, 403, 404].includes(cause.status);
        const retrying = !denied && failures < 5;
        // Even after a valid frame, a failed connection clears private evidence.
        // A frame does not reset the consecutive failed-connection budget.
        setState({
          client,
          identity,
          refresh,
          page: null,
          checkedAt: null,
          connected: false,
          retrying,
          error:
            cause instanceof OrtakApiError && denied
              ? cause.message
              : "Routing could not be checked. Refresh to try again.",
        });
        if (retrying)
          timer = setTimeout(
            () => void connect(),
            Math.min(3000 * 2 ** (failures - 1), 30_000),
          );
      } finally {
        connection.abort();
      }
    }
    void connect();
    return () => {
      owner.abort();
      active?.abort();
      if (timer) clearTimeout(timer);
    };
  }, [client, channel, message, identity, refresh]);
  return state?.identity === identity &&
    state.client === client &&
    state.refresh === refresh
    ? state
    : null;
}
