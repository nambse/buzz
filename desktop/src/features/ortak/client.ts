import type {
  ActivityPage,
  Cancellation,
  EmployeePage,
  EmployeeWorkPage,
  RunDetailResponse,
  RunPage,
  ProjectPage,
  WorkProject,
  WorkPage,
  WorkItem,
} from "./types";

type Signer = (event: {
  kind: number;
  content: string;
  tags: string[][];
}) => Promise<unknown>;

export class OrtakApiError extends Error {
  readonly status: number;
  readonly code: string;
  constructor(status: number, code: string) {
    super(
      status === 401
        ? "Your signed session could not be verified. Refresh and try again."
        : status === 403
          ? "Your account does not have permission for this action."
          : status === 404
            ? "This item is no longer available to your account."
            : status === 409
              ? "This action conflicts with the current saved state. Refresh before choosing another action."
              : [400, 413, 422].includes(status)
                ? "The submitted values could not be accepted. Check their length and required fields."
                : "Ortak is unavailable. Your last confirmed activity remains visible.",
    );
    this.status = status;
    this.code = code;
  }
}

async function boundedJson(response: Response): Promise<unknown> {
  const reader = response.body?.getReader();
  if (!reader) throw new Error("Ortak returned an empty response.");
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      length += value.byteLength;
      if (length > 8 * 1024 * 1024)
        throw new Error("Ortak response exceeded the display limit.");
      chunks.push(value);
    }
  } finally {
    await reader.cancel();
    reader.releaseLock();
  }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return JSON.parse(new TextDecoder().decode(bytes));
}

/** Native signing stays behind the injected existing Tauri signing seam. */
export function createOrtakClient(
  origin: string,
  sign: Signer,
  transport: typeof fetch = fetch,
) {
  async function request<T>(
    path: string,
    signal: AbortSignal,
    method = "GET",
    serializedBody?: string,
  ): Promise<T> {
    const url = `${origin}${path}`;
    const body = method === "POST" ? (serializedBody ?? "{}") : undefined;
    const tags = [
      ["u", url],
      ["method", method],
      ["nonce", crypto.randomUUID()],
    ];
    if (body) {
      const hash = await crypto.subtle.digest(
        "SHA-256",
        new TextEncoder().encode(body),
      );
      tags.push([
        "payload",
        Array.from(new Uint8Array(hash), (v) =>
          v.toString(16).padStart(2, "0"),
        ).join(""),
      ]);
    }
    signal.throwIfAborted();
    const event = await sign({ kind: 27235, content: "", tags });
    signal.throwIfAborted();
    const response = await transport(url, {
      method,
      body,
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
      signal: AbortSignal.any([signal, AbortSignal.timeout(15_000)]),
      headers: {
        Authorization: `Nostr ${btoa(JSON.stringify(event))}`,
        ...(body ? { "Content-Type": "application/json" } : {}),
      },
    });
    if (!response.ok) {
      // HTTP authorization failures must clear private state even when an
      // intermediary supplies a non-JSON response body.
      await response.body?.cancel();
      throw new OrtakApiError(response.status, "request_rejected");
    }
    return (await boundedJson(response)) as T;
  }
  return {
    employeeWork: (employeeId: string, signal: AbortSignal, cursor?: string) =>
      request<EmployeeWorkPage>(
        `/api/v1/employees/${encodeURIComponent(employeeId)}/work-items?limit=25${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`,
        signal,
      ),
    projects: (signal: AbortSignal, cursor?: string) =>
      request<ProjectPage>(
        `/api/v1/projects?limit=25${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`,
        signal,
      ),
    project: (id: string, signal: AbortSignal) =>
      request<{ project: WorkProject }>(
        `/api/v1/projects/${encodeURIComponent(id)}`,
        signal,
      ),
    workItems: (project: string, signal: AbortSignal, cursor?: string) =>
      request<WorkPage>(
        `/api/v1/projects/${encodeURIComponent(project)}/work-items?limit=25${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`,
        signal,
      ),
    workItem: (id: string, signal: AbortSignal) =>
      request<{ work_item: WorkItem }>(
        `/api/v1/work-items/${encodeURIComponent(id)}`,
        signal,
      ),
    // The operation body is frozen by the Work UI before the first attempt.
    // Each manual retry signs the same bytes with a fresh authentication nonce.
    workMutation: (path: string, body: string, signal: AbortSignal) =>
      request<{ project?: WorkProject; work_item?: WorkItem }>(
        path,
        signal,
        "POST",
        body,
      ),
    employees: (signal: AbortSignal, after?: string) =>
      request<EmployeePage>(
        `/api/v1/employees?limit=25${after ? `&after=${encodeURIComponent(after)}` : ""}`,
        signal,
      ),
    runs: (signal: AbortSignal, cursor?: string) =>
      request<RunPage>(
        `/api/v1/runs?limit=25${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`,
        signal,
      ),
    detail: (id: string, signal: AbortSignal) =>
      request<RunDetailResponse>(
        `/api/v1/runs/${encodeURIComponent(id)}`,
        signal,
      ),
    events: (id: string, after: number | null, signal: AbortSignal) =>
      request<ActivityPage>(
        `/api/v1/runs/${encodeURIComponent(id)}/events?limit=100${after === null ? "" : `&after_sequence=${after}`}`,
        signal,
      ),
    cancel: (id: string, signal: AbortSignal) =>
      request<Cancellation>(
        `/api/v1/runs/${encodeURIComponent(id)}/cancel`,
        signal,
        "POST",
      ),
  };
}
export type OrtakClient = ReturnType<typeof createOrtakClient>;
