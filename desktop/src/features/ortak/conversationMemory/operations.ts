import { conversationPath, type ConversationExport } from "./types";

type ConversationAction =
  | { kind: "approve" }
  | { kind: "stop" | "publish"; factId: string }
  | { kind: "retry"; factId: string; action: "publish" | "withdraw" };

/** Closed route dispatch keeps export acknowledgements separate from fact receipts. */
export function conversationAction(
  project: string,
  path: string,
): ConversationAction {
  const base = conversationPath(project);
  if (path === base) return { kind: "approve" };
  if (path.startsWith(`${base}/`)) {
    const match =
      /^([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\/(stop|publish|exports\/(publish|withdraw)\/retry)$/.exec(
        path.slice(base.length + 1),
      );
    if (match) {
      if (match[2] === "stop" || match[2] === "publish")
        return { kind: match[2], factId: match[1] };
      if (match[3] === "publish" || match[3] === "withdraw")
        return { kind: "retry", factId: match[1], action: match[3] };
    }
  }
  throw new Error("conversation_operation_path_mismatch");
}

/** Validate the shared export wire before either acknowledging a write or rendering it. */
export function assertConversationExport(
  value: unknown,
  factId: string,
): asserts value is ConversationExport {
  if (!value || typeof value !== "object")
    throw new Error("conversation_export_receipt_mismatch");
  const saved = value as ConversationExport;
  if (
    saved.fact_id !== factId ||
    typeof saved.erased_from_reviewed_store !== "boolean" ||
    typeof saved.runtime_consumption_enabled !== "boolean" ||
    [saved.publication, saved.cleanup].some(
      (job) =>
        !job ||
        !["pending", "acknowledged", "failed"].includes(job.state) ||
        !Number.isInteger(job.retry_version) ||
        job.retry_version < 0 ||
        job.retry_version > 8 ||
        !Number.isSafeInteger(job.attempt_count) ||
        job.attempt_count < 0 ||
        typeof job.next_attempt_at !== "string" ||
        !Number.isFinite(Date.parse(job.next_attempt_at)) ||
        (job.error_code !== null && typeof job.error_code !== "string"),
    )
  )
    throw new Error("conversation_export_receipt_mismatch");
}
