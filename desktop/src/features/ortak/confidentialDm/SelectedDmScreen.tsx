import { useEffect, useState, type ReactNode } from "react";
import { useIdentityQuery } from "@/shared/api/hooks";
import { getRelayWsUrl } from "@/shared/api/tauri";
import { ConfidentialDm } from "./ConfidentialDm";
import { selectedDmScreen } from "./selection";

/** Sits above all ordinary timeline, target-event and draft hooks. */
export function SelectedDmScreen({
  channelId,
  selection,
  children,
}: {
  channelId: string;
  selection: string;
  children: ReactNode;
}) {
  const identity = useIdentityQuery();
  const [relay, setRelay] = useState<string | null | undefined>();
  useEffect(() => {
    let current = true;
    void getRelayWsUrl()
      .then((value) => {
        if (current) setRelay(value);
      })
      .catch(() => {
        if (current) setRelay(null);
      });
    return () => {
      current = false;
    };
  }, []);
  if (relay === undefined)
    return (
      <p role="status" className="p-4 text-sm">
        Opening conversation…
      </p>
    );
  const mode = relay
    ? selectedDmScreen(selection, relay, channelId)
    : "unavailable";
  if (mode === "ordinary") return children;
  if (mode === "unavailable")
    return (
      <p role="alert" className="p-4 text-sm">
        This conversation's privacy settings could not be loaded. Reopen the
        community to retry.
      </p>
    );
  const human = identity.isError ? null : identity.data?.pubkey;
  return (
    <ConfidentialDm
      selected={relay && human ? { channelId, human, relay } : null}
      employeeName="Employee"
    />
  );
}
