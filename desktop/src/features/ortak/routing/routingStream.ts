import type { RoutingDecisionPage } from "./types";

type RoutingControl = "renew" | "retry" | "revoked";
const LIMIT = 65_536;

/** One complete current snapshot; a null decision is not recorded silence. */
export function routingPage(
  value: unknown,
  channel: string,
  message: string,
): RoutingDecisionPage {
  const page = value as RoutingDecisionPage;
  if (
    !page ||
    page.channel_id !== channel ||
    page.message_id !== message ||
    !("decision" in page) ||
    (page.decision !== null &&
      (typeof page.decision !== "object" ||
        typeof page.decision.decision_id !== "string" ||
        !["silent", "deterministic", "semantic"].includes(page.decision.mode) ||
        typeof page.decision.summary_reason !== "string" ||
        typeof page.decision.decided_at !== "string" ||
        !page.decision.scorer ||
        !Array.isArray(page.decision.recipients) ||
        page.decision.recipients.length > 32 ||
        page.decision.recipients.some(
          (recipient) =>
            !recipient ||
            typeof recipient.employee_id !== "string" ||
            !["wake", "drop"].includes(recipient.action) ||
            typeof recipient.reason !== "string" ||
            (recipient.score !== null &&
              (!Number.isFinite(recipient.score) ||
                recipient.score < 0 ||
                recipient.score > 1)) ||
            !Array.isArray(recipient.evidence) ||
            recipient.evidence.length > 8 ||
            recipient.evidence.some((label) => typeof label !== "string"),
        )))
  )
    throw new Error("Ortak returned inconsistent routing.");
  return page;
}

/** Decode one bounded frame at a time; disposal has its own absolute deadline. */
export async function consumeRoutingStream(
  response: Response,
  channel: string,
  message: string,
  signal: AbortSignal,
  receive: (page: RoutingDecisionPage) => void,
  abortTransport: () => void,
): Promise<RoutingControl> {
  if (!response.headers.get("content-type")?.startsWith("text/event-stream"))
    throw new Error("Ortak did not return a routing stream.");
  const reader = response.body?.getReader();
  if (!reader) throw new Error("Ortak returned an empty routing stream.");
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let buffer = "",
    total = 0,
    control: RoutingControl | undefined;
  let failure: unknown,
    failed = false;
  const lifetime = setTimeout(abortTransport, 55_000);
  try {
    frames: while (true) {
      signal.throwIfAborted();
      let readTimer: ReturnType<typeof setTimeout> | undefined;
      let abortRead: (() => void) | undefined;
      let result: ReadableStreamReadResult<Uint8Array>;
      try {
        result = await new Promise<ReadableStreamReadResult<Uint8Array>>(
          (resolve, reject) => {
            abortRead = () => reject(signal.reason);
            readTimer = setTimeout(
              () => reject(new Error("Routing connection timed out.")),
              12_000,
            );
            signal.addEventListener("abort", abortRead, { once: true });
            reader.read().then(resolve, reject);
          },
        );
      } finally {
        if (readTimer) clearTimeout(readTimer);
        if (abortRead) signal.removeEventListener("abort", abortRead);
      }
      signal.throwIfAborted();
      if (result.done) throw new Error("Routing connection closed.");
      total += result.value.byteLength;
      if (result.value.byteLength > 4 * LIMIT || total > 4 * 1024 * 1024)
        throw new Error("Routing exceeded the display limit.");
      buffer = (
        buffer + decoder.decode(result.value, { stream: true })
      ).replaceAll("\r\n", "\n");
      if (buffer.length > 5 * LIMIT + 1024)
        throw new Error("Routing exceeded the display limit.");
      let end = buffer.indexOf("\n\n");
      while (end !== -1) {
        const block = buffer.slice(0, end);
        buffer = buffer.slice(end + 2);
        if (new TextEncoder().encode(block).length > LIMIT + 128)
          throw new Error("Routing exceeded the display limit.");
        let event: string | undefined;
        const data: string[] = [];
        for (const line of block.split("\n")) {
          if (line.startsWith("event:")) {
            if (event !== undefined)
              throw new Error("Duplicate routing event.");
            event = line.slice(6).replace(/^ /, "");
          } else if (line.startsWith("data:"))
            data.push(line.slice(5).replace(/^ /, ""));
          else if (line && !line.startsWith(":"))
            throw new Error("Unknown routing field.");
        }
        signal.throwIfAborted();
        if (event === "control") {
          const value = JSON.parse(data.join("\n"));
          if (
            Object.keys(value).length !== 1 ||
            !["renew", "retry", "revoked"].includes(value.code)
          )
            throw new Error("Invalid routing control.");
          control = value.code;
          break frames;
        }
        if (event === "routing")
          receive(routingPage(JSON.parse(data.join("\n")), channel, message));
        else if (event === "heartbeat") {
          if (data.join("\n") !== "{}")
            throw new Error("Invalid routing heartbeat.");
        } else throw new Error("Unknown routing event.");
        end = buffer.indexOf("\n\n");
      }
      if (buffer.length > LIMIT + 1024)
        throw new Error("Routing exceeded the display limit.");
    }
  } catch (cause) {
    failed = true;
    failure = cause;
  } finally {
    clearTimeout(lifetime);
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      // Fetch abort errors its readable body. Cancel first so a normal renewal
      // does not turn into an AbortError; the cleanup deadline still owns abort.
      await Promise.race([
        reader.cancel(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error("Routing cleanup timed out.")),
            1000,
          );
        }),
      ]);
    } catch (cause) {
      if (!failed && control !== "revoked") {
        failed = true;
        failure = cause;
      }
    } finally {
      if (timer) clearTimeout(timer);
      abortTransport();
      try {
        reader.releaseLock();
      } catch (cause) {
        if (!failed && control !== "revoked") {
          failed = true;
          failure = cause;
        }
      }
    }
  }
  if (failed) throw failure;
  if (!control) throw new Error("Routing ended without control.");
  return control;
}
