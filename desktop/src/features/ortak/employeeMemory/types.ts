/** Employee-owned storage only; these DTOs confer no runtime or export grant. */
export type MemoryKind = "experience" | "relationship";
export interface EmployeeAudience {
  format: "ortak-reviewed-employee-audience/1";
  company_id: string;
  destination_community_id: string;
  destination_channel_id: string;
  employee_id: string;
  kind: MemoryKind;
  human_public_key: string | null;
}
export interface EmployeeSource {
  author_public_key: string;
  channel_id: string;
  community_id: string;
  event_id: string;
  event_created_at: string;
  evidence_hash: string;
}
export interface EmployeePreviewRequest {
  source_event_id: string;
  destination_channel_id: string;
  kind: MemoryKind;
  human_public_key: string | null;
}
export interface EmployeePreview {
  employee_id: string;
  audience: EmployeeAudience;
  audience_hash: string;
  source: EmployeeSource;
  source_hash: string;
  observed_at: string;
  valid_before: string | null;
  max_expires_at: string;
}
export interface EmployeeDraft extends EmployeePreviewRequest {
  source_event_created_at: string;
  expected_audience_hash: string;
  content: string;
  expires_at: string;
  reviewed: true;
}
export interface EmployeeFact {
  id: string;
  employee_id: string;
  kind: MemoryKind;
  status: "approved" | "expired" | "stopped";
  version: 1 | 2;
  approved_at: string;
  expires_at: string;
  revoked_at: string | null;
  source_current: boolean;
  can_stop: boolean;
  content: string | null;
  audience: EmployeeAudience | null;
  audience_hash: string | null;
  source: EmployeeSource | null;
  source_hash: string | null;
  provenance: Record<string, unknown> | null;
  sharing_hash: string | null;
}
export interface EmployeeFactPage {
  can_approve: boolean;
  facts: EmployeeFact[];
  next_after: string | null;
}
export interface EmployeeReceipt {
  operation_id: string;
  created: boolean;
  effect: {
    fact_id: string;
    action: "approve" | "stop";
    result_version: 1 | 2;
  };
  fact: EmployeeFact;
}
export const employeeMemoryPath = (employee: string) =>
  `/api/v1/employees/${encodeURIComponent(employee)}/reviewed-memory`;

export interface MemoryOperation {
  readonly path: string;
  readonly body: string;
  readonly operationId: string;
  readonly action: "approve" | "stop" | EmployeeExportAction;
  readonly factId?: string;
}

/** Separate publication commands; the server selects the current owned destination. */
export type EmployeeExportAction =
  | "publish"
  | "retry_publish"
  | "retry_withdraw";
export interface EmployeeExportJob {
  action: "publish" | "withdraw";
  state: "pending" | "acknowledged" | "failed";
  attempt_count: number;
  total_attempts: number;
  retry_version: number;
  last_error_code: string | null;
  acknowledged: boolean;
}
/** Metadata only. A publication receipt does not establish runtime use. */
export interface EmployeeExportRecord {
  fact_id: string;
  export: {
    target_id: string;
    created_at: string;
    jobs: EmployeeExportJob[];
  } | null;
}
export interface EmployeeExportReceipt {
  operation_id: string;
  created: boolean;
  result_version: number;
  record: EmployeeExportRecord;
}
export const employeeExportPath = (
  employee: string,
  fact: string,
  action: EmployeeExportAction = "publish",
) =>
  `${employeeMemoryPath(employee)}/${encodeURIComponent(fact)}/export${
    action === "retry_publish"
      ? "/retry/publish"
      : action === "retry_withdraw"
        ? "/retry/withdraw"
        : ""
  }`;
