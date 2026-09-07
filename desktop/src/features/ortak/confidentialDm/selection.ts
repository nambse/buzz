export type DmScreenMode = "ordinary" | "encrypted" | "unavailable";

/** Immutable operator display selection; native still verifies current authority.
 * A malformed configured selection never falls back to a plaintext composer. */
export function selectedDmScreen(
  raw: string,
  relay: string,
  channel: string,
): DmScreenMode {
  const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
  try {
    if (raw.length > 8192 || !uuid.test(channel)) return "unavailable";
    const bindings: unknown = JSON.parse(raw);
    if (!bindings || Array.isArray(bindings) || typeof bindings !== "object")
      return "unavailable";
    const entries = Object.entries(bindings);
    if (entries.length > 16) return "unavailable";
    for (const [origin, channels] of entries) {
      const parsed = new URL(origin);
      if (
        !/^https?:$/.test(parsed.protocol) ||
        parsed.origin !== origin ||
        !Array.isArray(channels) ||
        channels.length > 128 ||
        channels.some((id) => typeof id !== "string" || !uuid.test(id)) ||
        new Set(channels).size !== channels.length
      )
        return "unavailable";
    }
    const ws = new URL(relay);
    if (
      !/^wss?:$/.test(ws.protocol) ||
      ws.username ||
      ws.password ||
      ws.pathname !== "/" ||
      ws.search ||
      ws.hash
    )
      return "unavailable";
    ws.protocol = ws.protocol === "ws:" ? "http:" : "https:";
    const selected = entries.find(([origin]) => origin === ws.origin)?.[1] as
      | string[]
      | undefined;
    return selected?.includes(channel) ? "encrypted" : "ordinary";
  } catch {
    return "unavailable";
  }
}
