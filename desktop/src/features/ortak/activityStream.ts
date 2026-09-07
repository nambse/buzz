import type { ActivityPage, RunDetailResponse } from "./types";

export type ActivityFrame = { detail: RunDetailResponse; page: ActivityPage };
export type StreamControl = "renew" | "retry" | "revoked" | "resync";

/** Decode bounded SSE frames without buffering the lifetime of a connection. */
export async function consumeActivityStream(
  response: Response,
  runId: string,
  signal: AbortSignal,
  receive: (frame: ActivityFrame) => void,
): Promise<StreamControl> {
  if (!response.headers.get("content-type")?.startsWith("text/event-stream"))
    throw new Error("Ortak did not return an activity stream.");
  const reader = response.body?.getReader();
  if (!reader) throw new Error("Ortak returned an empty activity stream.");
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let buffer = "";
  let control: StreamControl | undefined;
  let failed = false;
  let failure: unknown;
  try {
    frames: while (true) {
      signal.throwIfAborted();
      const result = await new Promise<ReadableStreamReadResult<Uint8Array>>(
        (resolve, reject) => {
          const abort = () => reject(signal.reason);
          const timer = setTimeout(
            () => reject(new Error("Activity connection timed out.")),
            12_000,
          );
          signal.addEventListener("abort", abort, { once: true });
          reader
            .read()
            .then(resolve, reject)
            .finally(() => {
              clearTimeout(timer);
              signal.removeEventListener("abort", abort);
            });
        },
      );
      signal.throwIfAborted();
      if (result.done)
        throw new Error(
          "Activity connection closed. Reconnecting from confirmed activity.",
        );
      buffer = (
        buffer + decoder.decode(result.value, { stream: true })
      ).replaceAll("\r\n", "\n");
      if (buffer.length > 4 * 1024 * 1024 + 65_536)
        throw new Error("Activity exceeded the display limit.");
      let end = buffer.indexOf("\n\n");
      while (end !== -1) {
        const block = buffer.slice(0, end);
        buffer = buffer.slice(end + 2);
        if (block.length > 4 * 1024 * 1024)
          throw new Error("Activity exceeded the display limit.");
        let event = "";
        let id: string | null = null;
        const data: string[] = [];
        for (const line of block.split("\n")) {
          if (line.startsWith("event: ")) event = line.slice(7);
          else if (line.startsWith("event:")) event = line.slice(6);
          else if (line.startsWith("data:"))
            data.push(line.slice(5).replace(/^ /, ""));
          else if (line.startsWith("id:")) id = line.slice(3).replace(/^ /, "");
        }
        signal.throwIfAborted();
        if (event === "control") {
          const { code } = JSON.parse(data.join("\n"));
          if (!["renew", "retry", "revoked", "resync"].includes(code))
            throw new Error("Invalid activity control.");
          control = code as StreamControl;
          break frames;
        }
        if (event === "activity") {
          const frame = JSON.parse(data.join("\n")) as ActivityFrame;
          const cursor = frame?.page?.next_after_sequence;
          if (
            frame?.detail?.detail?.run?.run_id !== runId ||
            !Array.isArray(frame?.page?.entries) ||
            frame.page.entries.length > 25 ||
            (cursor !== null &&
              (!Number.isSafeInteger(cursor) || cursor < 0)) ||
            id !== (cursor === null ? null : String(cursor))
          )
            throw new Error("Ortak returned inconsistent activity.");
          receive(frame);
        } else if (event !== "heartbeat" && data.length > 0) {
          throw new Error("Ortak returned an unknown activity event.");
        }
        end = buffer.indexOf("\n\n");
      }
    }
  } catch (cause) {
    failed = true;
    failure = cause;
  }
  try {
    await reader.cancel();
  } catch (cause) {
    // Retain a parsing/transport failure or authoritative privacy control.
    // Cleanup failure cannot turn revocation into a content-preserving retry.
    if (!failed && control !== "revoked" && control !== "resync") {
      failed = true;
      failure = cause;
    }
  } finally {
    reader.releaseLock();
  }
  if (failed) throw failure;
  if (control === undefined) throw new Error("Activity ended without control.");
  return control;
}
