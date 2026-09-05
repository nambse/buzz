import type {
  ActivityEntry,
  ActivityPage,
  ActivityText,
  RunDetailResponse,
  RunStatus,
} from "./types";

export const DISPLAY_EVENT_LIMIT = 500;
export function isTerminal(status: RunStatus) {
  return ["completed", "failed", "cancelled"].includes(status);
}
/** Detail and events are independent snapshots; drain the detail's high-water mark. */
export function needsActivityPoll(
  detail: RunDetailResponse,
  page: ActivityPage,
  cursor: number | null,
) {
  return (
    page.has_more ||
    !isTerminal(detail.detail.run.status) ||
    detail.cancellation?.status === "pending" ||
    detail.office_delivery?.status === "pending" ||
    detail.memory?.write?.status === "pending" ||
    (detail.detail.run.last_event?.sequence ?? -1) > (cursor ?? -1)
  );
}
const text = (value?: ActivityText | string | null) =>
  typeof value === "string" ? value : (value?.text ?? "");

/** Semantic summaries over the actual Activity projection, with an honest fallback. */
export function describeActivity(entry: ActivityEntry): {
  title: string;
  detail: string;
} {
  const a = entry.activity;
  const phase = a.phase;
  switch (a.kind) {
    case "assistant_output":
      return { title: "Employee replied", detail: text(a.text) };
    case "lifecycle":
      return {
        title: `Run ${phase?.phase ?? "updated"}`,
        detail:
          text(phase?.detail ?? phase?.message ?? phase?.reason) ||
          (phase?.delivery_intent === "silent"
            ? "Finished without an Office reply."
            : ""),
      };
    case "tool_call":
      return {
        title: phase?.tool
          ? `Using ${phase.tool}`
          : `Tool ${phase?.phase ?? "updated"}`,
        detail: text(phase?.arguments ?? phase?.result ?? phase?.error),
      };
    case "terminal":
      return {
        title:
          phase?.phase === "completed"
            ? `Command finished${phase.exit_code == null ? "" : ` (exit ${phase.exit_code})`}`
            : phase?.phase === "output"
              ? "Command output"
              : "Running command",
        detail: text(phase?.command ?? phase?.chunk),
      };
    case "file_change":
      return {
        title: `${a.change ?? "Updated"} ${a.path ?? "file"}`,
        detail: text(a.summary),
      };
    case "delivery_intent":
      return {
        title:
          a.intent === "silent"
            ? "No Office reply requested"
            : "Office reply requested",
        detail: "Delivery intent is recorded; it does not confirm publication.",
      };
    case "error":
      return {
        title: a.code ? `Error: ${a.code}` : "Runtime error",
        detail: text(a.message),
      };
    case "usage":
      return { title: "Model usage recorded", detail: "" };
    default:
      return {
        title: entry.event_type.replaceAll(".", " "),
        detail: "Activity was recorded.",
      };
  }
}

/** Bind the UI cursor to dense pages. Gaps never advance or discard the cursor. */
export function appendActivity(
  existing: ActivityEntry[],
  cursor: number | null,
  page: ActivityPage,
) {
  if (page.gap)
    throw new Error(
      "Some activity is missing. Reload the timeline to resynchronize.",
    );
  let expected = cursor === null ? 0 : cursor + 1;
  const fresh: ActivityEntry[] = [];
  for (const entry of page.entries) {
    if (!Number.isSafeInteger(entry.sequence) || entry.sequence < 0)
      throw new Error("Ortak returned an invalid activity sequence.");
    if (cursor !== null && entry.sequence <= cursor) continue;
    if (entry.sequence !== expected)
      throw new Error(
        "Activity arrived out of order. Reload the timeline to resynchronize.",
      );
    fresh.push(entry);
    expected += 1;
  }
  const next = fresh.at(-1)?.sequence ?? cursor;
  if (page.next_after_sequence !== next)
    throw new Error("Ortak returned an inconsistent activity cursor.");
  return {
    entries: [...existing, ...fresh].slice(-DISPLAY_EVENT_LIMIT),
    cursor: next,
  };
}
