import type { Community } from "@/features/communities/types";
import { isConfiguredOrtakRelay } from "./config";
import { privateOrtakMode } from "./privateMode";

function selectedBindings(): string | null {
  const bindings = import.meta.env?.VITE_ORTAK_API_BINDINGS_JSON;
  if (!bindings || bindings.length > 8192) return null;
  try {
    const parsed = JSON.parse(bindings);
    if (
      !parsed ||
      Array.isArray(parsed) ||
      typeof parsed !== "object" ||
      Object.keys(parsed).length !== 1
    )
      return null;
    return bindings;
  } catch {
    return null;
  }
}

/** Bootstrap and saved selection share the same single-company boundary. */
export function privateCompanyRelayAllowed(relayUrl: string): boolean {
  const bindings = selectedBindings();
  return bindings !== null && isConfiguredOrtakRelay(relayUrl, bindings);
}

/** The private app exposes the one company selected by its compiled binding. */
export function privateCompanySelected(
  communities: Community[],
  activeId: string | null,
) {
  const bindings = selectedBindings();
  if (bindings === null) return null;
  const selected = communities.filter(
    (community) =>
      community &&
      typeof community.id === "string" &&
      community.id.length > 0 &&
      typeof community.relayUrl === "string" &&
      isConfiguredOrtakRelay(community.relayUrl, bindings),
  );
  return (
    selected.find((community) => community.id === activeId) ??
    selected[0] ??
    null
  );
}

/** Saved state and deep links cannot expose a community-management operation. */
export function requireCommunityManagement() {
  if (privateOrtakMode)
    throw new Error("This app is connected to one company.");
}
