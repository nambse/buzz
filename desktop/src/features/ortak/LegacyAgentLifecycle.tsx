import { useManagedAgentRuntimeReconciliation } from "@/features/agents/useManagedAgentRuntimeReconciliation";
import { useAutoRestartPolicy } from "@/features/agents/lib/useAutoRestartPolicy";

/** Mounted only by retained legacy desktop mode; Ortak workers own runtime starts. */
export function LegacyAgentLifecycle({
  communities,
}: {
  communities: readonly { relayUrl: string }[];
}) {
  useManagedAgentRuntimeReconciliation(communities);
  useAutoRestartPolicy();
  return null;
}
