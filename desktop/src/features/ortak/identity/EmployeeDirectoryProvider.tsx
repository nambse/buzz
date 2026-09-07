import { createContext, useContext, useMemo, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { signRelayEvent } from "@/shared/api/tauri";
import { createOrtakClient } from "../client";
import { privateOrtakMode } from "../privateMode";
import { useOrtakOrigin } from "../useOrtakOrigin";
import { loadEmployeeDirectory, type EmployeeDirectory } from "./directory";

const EMPTY: EmployeeDirectory = Object.freeze({});
const DEFAULT = { entries: EMPTY, refresh: () => {} };
const Directory = createContext(DEFAULT);

/** Mounted within the community-keyed boundary; no per-employee subscriptions. */
export function EmployeeDirectoryProvider({
  children,
}: {
  children: ReactNode;
}) {
  const origin = useOrtakOrigin();
  const client = useMemo(
    () => (origin ? createOrtakClient(origin, signRelayEvent) : null),
    [origin],
  );
  const query = useQuery({
    queryKey: ["ortak-employee-identities", origin],
    enabled: privateOrtakMode && client !== null,
    queryFn: ({ signal }) => {
      if (!client) throw new Error("Office is not configured for Ortak.");
      return loadEmployeeDirectory(client, signal);
    },
    staleTime: 15_000,
    gcTime: 0,
    retry: false,
    refetchInterval: (current) => (current.state.error ? false : 15_000),
    refetchOnWindowFocus: true,
    refetchOnReconnect: true,
  });
  // A failed/forbidden refresh must not retain now-unauthorized identity metadata.
  const value =
    privateOrtakMode && origin && !query.isError
      ? (query.data ?? EMPTY)
      : EMPTY;
  const directory = useMemo(
    () => ({
      entries: value,
      refresh: () => {
        void query.refetch();
      },
    }),
    [value, query.refetch],
  );
  return <Directory.Provider value={directory}>{children}</Directory.Provider>;
}

/** Resolve identity only by verified Office key, never display name or bot flag. */
export function useOfficeEmployee(pubkey?: string | null) {
  const value = useContext(Directory);
  return pubkey ? (value.entries[pubkey.toLowerCase()] ?? null) : null;
}

/** The existing Employees Refresh action also retries a stopped identity read. */
export function useEmployeeDirectoryRefresh() {
  return useContext(Directory).refresh;
}
