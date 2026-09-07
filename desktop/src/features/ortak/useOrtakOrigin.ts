import { useEffect, useState } from "react";
import { getRelayHttpUrl } from "@/shared/api/tauri";
import { resolveOrtakOrigin } from "./config";

/** AppReady's community-key remount resets this local state on company changes. */
export function useOrtakOrigin() {
  const raw = import.meta.env.VITE_ORTAK_API_BINDINGS_JSON;
  const [origin, setOrigin] = useState<string | null | undefined>(
    raw ? undefined : null,
  );
  useEffect(() => {
    if (!raw) return;
    let active = true;
    void getRelayHttpUrl()
      .then((relay) => {
        if (active) setOrigin(resolveOrtakOrigin(raw, relay));
      })
      .catch(() => {
        if (active) setOrigin(null);
      });
    return () => {
      active = false;
    };
  }, []);
  return origin;
}
