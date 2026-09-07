import { useCallback, useEffect, useRef, useState } from "react";
import { OrtakApiError } from "../client";

export const lostAuthority = (cause: unknown) =>
  cause instanceof OrtakApiError && [401, 403, 404].includes(cause.status);

/** One bounded signed observation; retry is explicit, never an unbounded poll. */
export function useConversationRead<T>(
  load: ((signal: AbortSignal) => Promise<T>) | null,
  enabled: boolean,
  revision = 0,
  denied?: () => void,
) {
  const [refresh, setRefresh] = useState(0);
  const key = `${revision}:${refresh}`;
  const current = useRef({ load, enabled, key, denied });
  current.current = { load, enabled, key, denied };
  const controller = useRef<AbortController | null>(null);
  const [state, setState] = useState<{
    load: typeof load;
    key: string;
    value: T | null;
    error: string | null;
    ready: boolean;
  }>({
    load,
    key,
    value: null,
    error: null,
    ready: false,
  });
  useEffect(() => {
    const owner = new AbortController();
    controller.current = owner;
    setState({ load, key, value: null, error: null, ready: false });
    if (enabled && load) {
      void Promise.resolve()
        .then(() => {
          owner.signal.throwIfAborted();
          return load(owner.signal);
        })
        .then((value) => {
          if (
            !owner.signal.aborted &&
            current.current.load === load &&
            current.current.key === key &&
            current.current.enabled
          )
            setState({ load, key, value, error: null, ready: true });
        })
        .catch((cause: unknown) => {
          if (
            owner.signal.aborted ||
            current.current.load !== load ||
            current.current.key !== key ||
            !current.current.enabled
          )
            return;
          if (lostAuthority(cause)) current.current.denied?.();
          setState({
            load,
            key,
            value: null,
            ready: false,
            error: lostAuthority(cause)
              ? "This selection is no longer available to your account."
              : "This read could not be confirmed. Refresh to try again.",
          });
        });
    }
    return () => owner.abort();
  }, [load, enabled, key]);
  const invalidate = useCallback(() => {
    controller.current?.abort();
    setState({
      load,
      key,
      value: null,
      ready: false,
      error: "Access changed. Refresh before continuing.",
    });
  }, [load, key]);
  const visible = enabled && state.load === load && state.key === key;
  return {
    value: visible ? state.value : null,
    ready: visible && state.ready,
    error: visible ? state.error : null,
    invalidate,
    refresh: () => setRefresh((value) => value + 1),
  };
}
