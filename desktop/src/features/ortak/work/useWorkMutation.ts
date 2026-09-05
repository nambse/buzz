import { useCallback, useEffect, useRef, useState } from "react";
import { OrtakApiError, type OrtakClient } from "../client";
import { workOperation, type WorkOperation } from "./operations";

/** A single explicit write at a time, retaining uncertain attempts across selection changes. */
export function useWorkMutation(
  client: OrtakClient,
  refresh: () => void,
  revoke: () => void,
) {
  const [pending, setPending] = useState<WorkOperation | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const inFlight = useRef(false);
  const lifetime = useRef(new AbortController());
  const generation = useRef(0);
  const pause = useCallback(() => {
    if (inFlight.current)
      setNotice(
        "A write was interrupted before confirmation. Refresh access, then retry the same operation to check its saved result.",
      );
    lifetime.current.abort();
    lifetime.current = new AbortController();
    generation.current++;
    inFlight.current = false;
    setBusy(false);
  }, []);
  useEffect(() => {
    // Client identity carries the configured origin; never retain another origin’s write.
    void client;
    setPending(null);
    setBusy(false);
    setNotice(null);
    inFlight.current = false;
    generation.current++;
    const controller = new AbortController();
    lifetime.current = controller;
    return () => {
      controller.abort();
      lifetime.current.abort();
      generation.current++;
    };
  }, [client]);

  async function send(operation: WorkOperation) {
    if (inFlight.current) return;
    inFlight.current = true;
    const signal = lifetime.current.signal;
    const attempt = ++generation.current;
    setBusy(true);
    setPending(operation);
    setNotice(null);
    try {
      await client.workMutation(operation.path, operation.body, signal);
      if (signal.aborted) return;
      setPending(null);
      setNotice(`${operation.label} saved.`);
      refresh();
    } catch (cause) {
      if (signal.aborted) return;
      const status = cause instanceof OrtakApiError ? cause.status : null;
      if (status && [400, 401, 403, 404, 409, 413, 422].includes(status)) {
        setPending(null);
        setNotice(
          status === 409
            ? "The saved state or permissions changed. Review the refreshed item before choosing another action."
            : cause instanceof Error
              ? cause.message
              : "The action was refused.",
        );
        if ([401, 403, 404].includes(status)) revoke();
        else refresh();
      } else {
        setNotice(
          `Confirmation is missing for “${operation.label}”. It may already be saved. Retry the same operation to check safely.`,
        );
      }
    } finally {
      if (attempt === generation.current) {
        inFlight.current = false;
        if (!signal.aborted) setBusy(false);
      }
    }
  }
  function submit(
    path: string,
    label: string,
    values: Record<string, unknown>,
  ) {
    if (pending || inFlight.current) return;
    try {
      void send(workOperation(path, label, values));
    } catch (cause) {
      setNotice(cause instanceof Error ? cause.message : "Invalid form.");
    }
  }
  return {
    pending,
    busy,
    notice,
    submit,
    pause,
    retry: () => pending && void send(pending),
  };
}
