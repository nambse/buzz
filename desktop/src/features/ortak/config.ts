/** Resolve only an explicitly configured relay→API origin binding. */
export function resolveOrtakOrigin(
  raw: string | undefined,
  relayUrl: string,
): string | null {
  if (!raw || raw.length > 8192) return null;
  try {
    const bindings = JSON.parse(raw) as Record<string, unknown>;
    if (
      !bindings ||
      Array.isArray(bindings) ||
      typeof bindings !== "object" ||
      Object.keys(bindings).length > 16
    )
      return null;
    const relay = new URL(relayUrl);
    const value = bindings[relay.origin];
    if (typeof value !== "string") return null;
    const api = new URL(value);
    const loopback = ["localhost", "127.0.0.1", "[::1]"].includes(api.hostname);
    if (
      !(api.protocol === "https:" || (api.protocol === "http:" && loopback)) ||
      api.origin !== value ||
      api.username ||
      api.password
    )
      return null;
    return api.origin;
  } catch {
    return null;
  }
}
