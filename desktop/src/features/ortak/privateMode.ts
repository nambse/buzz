/** Build-time opt-in for a desktop dedicated to the bounded private Ortak MVP. */
export const privateOrtakMode =
  import.meta.env?.VITE_ORTAK_PRIVATE_MODE === "true";

const blockedFeatures = new Set([
  "projects",
  "workflows",
  "pulse",
  "forum",
  "managed-agents",
  "channel-templates",
]);
const blockedSettings = new Set([
  "agents",
  "compute",
  "channel-templates",
  "experimental",
  "hosted-communities",
]);
const blockedCommands = new Set([
  "create_managed_agent",
  "update_managed_agent",
  "start_managed_agent",
  "start_managed_agent_runtime",
  "restart_managed_agent_runtime",
  "put_managed_agent_runtime_lifecycle",
  "reconcile_managed_agent_runtimes",
  "set_managed_agent_start_on_app_launch",
  "set_managed_agent_auto_restart",
  "create_persona",
  "update_persona",
  "update_persona_and_publish",
  "set_persona_active",
  "set_persona_shared",
  "confirm_agent_snapshot_import",
  "create_team",
  "update_team",
  "add_team_from_catalog",
  "confirm_team_snapshot_import",
  "connect_acp_runtime",
  "install_acp_runtime",
  "set_global_agent_config",
]);

/** Private mode cannot be widened by saved preview preferences. */
export function privateFeatureBlocked(feature: string) {
  return privateOrtakMode && blockedFeatures.has(feature);
}

/** Preserve human identity, appearance and Office administration settings. */
export function privateSettingsAllowed(section: string) {
  return !privateOrtakMode || !blockedSettings.has(section);
}

/** Stop legacy provisioning or gateway starts before invoking the native bridge. */
export function assertPrivateCommandAllowed(command: string) {
  if (privateOrtakMode && blockedCommands.has(command))
    throw new Error(
      "Employees are managed by the Ortak control plane in private mode.",
    );
}

/** Block direct navigation as well as hiding unavailable preview affordances. */
export function privateRouteAllowed(pathname: string) {
  return (
    !privateOrtakMode || !/^\/(projects|workflows|pulse)(\/|$)/.test(pathname)
  );
}
