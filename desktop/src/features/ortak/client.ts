import type { ReviewedFactPage, ReviewedRecall } from "./work/memoryTypes";
import {
  employeeMemoryPath,
  employeeExportPath,
  type EmployeeExportAction,
  type EmployeeExportRecord,
  type EmployeeExportReceipt,
  type EmployeeFactPage,
  type EmployeePreview,
  type EmployeePreviewRequest,
  type EmployeeReceipt,
} from "./employeeMemory/types";
import {
  conversationPath,
  type ConversationFactPage,
  type ConversationPreview,
  type ConversationPreviewRequest,
  type ConversationReceipt,
  type ConversationExportReceipt,
} from "./conversationMemory/types";
import type { RoutingDecisionPage } from "./routing/types";
import { consumeRoutingStream } from "./routing/routingStream";
import type {
  ProvisioningPage,
  ProvisioningDetail,
} from "./provisioning/types";
import type {
  PreparedCatalog,
  DraftRequest,
  ConfigurationDraft,
  ManagementRequest,
  CommandReceipt,
  ManagementPage,
} from "./provisioning/managementTypes";
import { consumeActivityStream, type ActivityFrame } from "./activityStream";
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
  WorkExecution,
  WorkDependencyPage,
  WorkDecomposition,
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

async function boundedText(response: Response, limit: number): Promise<string> {
  const reader = response.body?.getReader();
  if (!reader) throw new Error("Ortak returned an empty response.");
  const chunks: Uint8Array[] = [];
  let length = 0;
  let cleanupFailed = false;
  let cleanupError: unknown;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      length += value.byteLength;
      if (length > limit)
        throw new Error("Ortak response exceeded the display limit.");
      chunks.push(value);
    }
  } finally {
    try {
      await reader.cancel();
    } catch (cleanup) {
      cleanupFailed = true;
      cleanupError = cleanup;
    } finally {
      try {
        reader.releaseLock();
      } catch (cleanup) {
        if (!cleanupFailed) cleanupError = cleanup;
        cleanupFailed = true;
      }
    }
  }
  if (cleanupFailed) throw cleanupError;
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.length;
  }
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
}

async function boundedJson(response: Response): Promise<unknown> {
  return JSON.parse(await boundedText(response, 8 * 1024 * 1024));
}

/** Native signing stays behind the injected existing Tauri signing seam. */
export function createOrtakClient(
  origin: string,
  sign: Signer,
  transport: typeof fetch = fetch,
) {
  async function open(
    path: string,
    signal: AbortSignal,
    method = "GET",
    serializedBody?: string,
    timeout = 15_000,
  ): Promise<Response> {
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
      signal: AbortSignal.any([signal, AbortSignal.timeout(timeout)]),
      headers: {
        Authorization: `Nostr ${btoa(JSON.stringify(event))}`,
        ...(body ? { "Content-Type": "application/json" } : {}),
      },
    });
    if (!response.ok) {
      // HTTP authorization failures must clear private state even when an
      // intermediary supplies a non-JSON response body.
      try {
        await response.body?.cancel();
      } catch {
        // The HTTP failure remains authoritative even if disposing its body
        // fails. Propagate that typed failure so revoked content is cleared.
      }
      throw new OrtakApiError(response.status, "request_rejected");
    }
    return response;
  }
  async function request<T>(
    path: string,
    signal: AbortSignal,
    method = "GET",
    body?: string,
  ): Promise<T> {
    return (await boundedJson(await open(path, signal, method, body))) as T;
  }
  async function employeeMemoryRequest<T>(
    path: string,
    signal: AbortSignal,
    method = "GET",
    body?: string,
  ): Promise<T> {
    if (body && new TextEncoder().encode(body).length > 32768)
      throw new Error("Employee memory request is too long.");
    return JSON.parse(
      await boundedText(await open(path, signal, method, body), 262144),
    ) as T;
  }
  return {
    employeeMemoryExport: async (
      employee: string,
      fact: string,
      signal: AbortSignal,
    ): Promise<EmployeeExportRecord> =>
      JSON.parse(
        await boundedText(
          await open(employeeExportPath(employee, fact), signal),
          16384,
        ),
      ) as EmployeeExportRecord,
    employeeMemoryExportMutation: async (
      employee: string,
      fact: string,
      action: EmployeeExportAction,
      body: string,
      signal: AbortSignal,
    ): Promise<EmployeeExportReceipt> => {
      if (new TextEncoder().encode(body).length > 16384)
        throw new Error("Publication request is too long.");
      return JSON.parse(
        await boundedText(
          await open(
            employeeExportPath(employee, fact, action),
            signal,
            "POST",
            body,
          ),
          16384,
        ),
      ) as EmployeeExportReceipt;
    },
    employeeMemoryPreview: (
      employee: string,
      body: EmployeePreviewRequest,
      signal: AbortSignal,
    ) =>
      employeeMemoryRequest<{ preview: EmployeePreview }>(
        `${employeeMemoryPath(employee)}/preview`,
        signal,
        "POST",
        JSON.stringify(body),
      ),
    employeeMemoryFacts: (
      employee: string,
      signal: AbortSignal,
      after?: string,
    ) =>
      employeeMemoryRequest<EmployeeFactPage>(
        `${employeeMemoryPath(employee)}${after ? `?after=${encodeURIComponent(after)}` : ""}`,
        signal,
      ),
    employeeMemoryMutation: (
      employee: string,
      fact: string | null,
      body: string,
      signal: AbortSignal,
    ) =>
      employeeMemoryRequest<EmployeeReceipt>(
        `${employeeMemoryPath(employee)}${fact ? `/${encodeURIComponent(fact)}/stop` : ""}`,
        signal,
        "POST",
        body,
      ),
    conversationPreview: (
      project: string,
      body: ConversationPreviewRequest,
      signal: AbortSignal,
    ) =>
      request<{ preview: ConversationPreview }>(
        `${conversationPath(project)}/preview`,
        signal,
        "POST",
        JSON.stringify(body),
      ),
    conversationFacts: (
      project: string,
      employee: string,
      signal: AbortSignal,
      after?: string,
    ) =>
      request<ConversationFactPage>(
        `${conversationPath(project)}?employee_id=${encodeURIComponent(employee)}${after ? `&after=${encodeURIComponent(after)}` : ""}`,
        signal,
      ),
    conversationMutation: (path: string, body: string, signal: AbortSignal) =>
      request<ConversationReceipt>(path, signal, "POST", body),
    conversationExportMutation: (
      path: string,
      body: string,
      signal: AbortSignal,
    ) => request<ConversationExportReceipt>(path, signal, "POST", body),
    routingDecision: (channel: string, message: string, signal: AbortSignal) =>
      request<RoutingDecisionPage>(
        `/api/v1/channels/${encodeURIComponent(channel)}/messages/${encodeURIComponent(message)}/routing`,
        signal,
      ),
    routingDecisionStream: async (
      channel: string,
      message: string,
      signal: AbortSignal,
      receive: (page: RoutingDecisionPage) => void,
    ) => {
      const connection = new AbortController();
      const owned = AbortSignal.any([signal, connection.signal]);
      try {
        const response = await open(
          `/api/v1/channels/${encodeURIComponent(channel)}/messages/${encodeURIComponent(message)}/routing/stream`,
          owned,
          "GET",
          undefined,
          60_000,
        );
        const control = await consumeRoutingStream(
          response,
          channel,
          message,
          owned,
          receive,
          () => connection.abort(),
        );
        if (control === "revoked")
          throw new OrtakApiError(403, "routing_revoked");
        if (control === "retry")
          throw new Error("Routing disconnected. Refresh to try again.");
      } finally {
        connection.abort();
      }
    },
    preparedEmployees: (signal: AbortSignal) =>
      request<PreparedCatalog>("/api/v1/employee-preparations", signal),
    configurationDraft: (
      employee: string,
      body: DraftRequest,
      signal: AbortSignal,
    ) =>
      request<ConfigurationDraft>(
        `/api/v1/employees/${encodeURIComponent(employee)}/configuration-drafts`,
        signal,
        "POST",
        JSON.stringify(body),
      ),
    managementCommand: (
      employee: string,
      body: ManagementRequest,
      signal: AbortSignal,
    ) =>
      request<CommandReceipt>(
        `/api/v1/employees/${encodeURIComponent(employee)}/management-commands`,
        signal,
        "POST",
        JSON.stringify(body),
      ),
    managementCommands: (employee: string, signal: AbortSignal) =>
      request<ManagementPage>(
        `/api/v1/employees/${encodeURIComponent(employee)}/management-commands`,
        signal,
      ),
    activityStream: async (
      id: string,
      after: number | null,
      signal: AbortSignal,
      receive: (frame: ActivityFrame) => void,
    ) => {
      if (after !== null && (!Number.isSafeInteger(after) || after < 0))
        throw new Error("Invalid activity cursor.");
      const response = await open(
        `/api/v1/runs/${encodeURIComponent(id)}/stream${after === null ? "" : `?after_sequence=${after}`}`,
        signal,
        "GET",
        undefined,
        60_000,
      );
      const control = await consumeActivityStream(
        response,
        id,
        signal,
        receive,
      );
      if (control === "revoked")
        throw new OrtakApiError(403, "activity_revoked");
      if (control === "resync")
        throw new OrtakApiError(409, "activity_cursor_gap");
      if (control === "retry")
        throw new Error(
          "Activity disconnected. Reconnecting from confirmed activity.",
        );
    },
    provisioning: (employeeId: string, signal: AbortSignal, cursor?: string) =>
      request<ProvisioningPage>(
        `/api/v1/employees/${encodeURIComponent(employeeId)}/provisioning?limit=25${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`,
        signal,
      ),
    provisioningOperation: (
      employeeId: string,
      operationId: string,
      signal: AbortSignal,
    ) =>
      request<ProvisioningDetail>(
        `/api/v1/employees/${encodeURIComponent(employeeId)}/provisioning/${encodeURIComponent(operationId)}`,
        signal,
      ),
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
    reviewedMemory: (
      project: string,
      employee: string,
      signal: AbortSignal,
      after?: string,
    ) =>
      request<ReviewedFactPage>(
        `/api/v1/projects/${encodeURIComponent(project)}/reviewed-memory?employee_id=${encodeURIComponent(employee)}${after ? `&after=${encodeURIComponent(after)}` : ""}`,
        signal,
      ),
    recallReviewedMemory: (
      project: string,
      employee: string,
      query: string,
      signal: AbortSignal,
    ) =>
      request<ReviewedRecall>(
        `/api/v1/projects/${encodeURIComponent(project)}/reviewed-memory/recall`,
        signal,
        "POST",
        JSON.stringify({ employee_id: employee, query }),
      ),
    workExecutions: (id: string, signal: AbortSignal) =>
      request<{ executions: WorkExecution[] }>(
        `/api/v1/work-items/${encodeURIComponent(id)}/executions`,
        signal,
      ),
    workDependencies: (id: string, signal: AbortSignal) =>
      request<WorkDependencyPage>(
        `/api/v1/work-items/${encodeURIComponent(id)}/dependencies`,
        signal,
      ),
    workDecomposition: (id: string, signal: AbortSignal) =>
      request<WorkDecomposition>(
        `/api/v1/work-items/${encodeURIComponent(id)}/decomposition`,
        signal,
      ),
    textArtifact: async (item: string, artifact: string, signal: AbortSignal) =>
      boundedText(
        await open(
          `/api/v1/work-items/${encodeURIComponent(item)}/artifacts/${encodeURIComponent(artifact)}`,
          signal,
        ),
        32768,
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
