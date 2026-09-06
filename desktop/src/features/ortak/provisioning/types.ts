export type ProvisioningStatus =
  | "pending"
  | "running"
  | "succeeded"
  | "failed"
  | "compensating"
  | "compensated";
export type ProvisioningOperation = {
  operation_id: string;
  employee_id: string;
  mode: "create" | "adopt" | "update";
  dry_run: boolean;
  status: ProvisioningStatus;
  current_step: string | null;
  result_revision_id: string | null;
  created_at: string;
  updated_at: string;
  finished_at: string | null;
};
export type ProvisioningPage = {
  employee_id: string;
  operations: ProvisioningOperation[];
  next_cursor: string | null;
  has_more: boolean;
  read_only: true;
};
export type ProvisioningDetail = {
  operation: ProvisioningOperation;
  runtime_probe?: {
    generation: number;
    state: "running" | "succeeded" | "failed";
    created_at: string;
    deadline: string;
    contained_at: string | null;
    error_code: string | null;
  } | null;
  steps: {
    name: string;
    state: ProvisioningStatus | "skipped";
    attempt_count: number;
    adopted_existing: boolean;
    started_at: string | null;
    finished_at: string | null;
  }[];
  read_only: true;
};
export const provisioningSteps: Record<string, string> = {
  validate_manifest: "Validate employee definition",
  reserve_employee_identity: "Reserve employee identity",
  resolve_credential_references: "Check credential references",
  ensure_runtime_profile: "Prepare runtime profile",
  validate_runtime_profile: "Validate runtime profile",
  ensure_memory_resources: "Prepare memory resources",
  ensure_office_identity: "Verify Office identity",
  publish_office_profile: "Publish Office profile",
  probe_health: "Check activation requirements",
  activate_revision: "Activate employee revision",
};
