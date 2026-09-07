export type PreparedChoice = {
  catalog_id: string;
  employee_id: string;
  label: string;
  model: string;
  thinking: string | null;
  expected_revision_id: string | null;
  expected_lifecycle_epoch?: number;
  status: string | null;
  can_save_draft: boolean;
};
export type PreparedCatalog = {
  choices: PreparedChoice[];
  employees?: Array<
    Pick<
      PreparedChoice,
      | "employee_id"
      | "status"
      | "expected_revision_id"
      | "expected_lifecycle_epoch"
    >
  >;
  create_supported: false;
  lifecycle_supported: boolean;
};
export type ConfigurationDraft = {
  draft_id: string;
  employee_id: string;
  catalog_id: string;
  expected_revision_id: string | null;
  expected_lifecycle_epoch?: number;
  action: "adopt" | "update" | "reenable";
  model: string;
  thinking: string | null;
};
export type DraftRequest = {
  draft_id: string;
  catalog_id: string;
  expected_revision_id: string | null;
  expected_lifecycle_epoch?: number;
};
export type ManagementAction =
  | "adopt"
  | "update"
  | "retry"
  | "compensate"
  | "disable"
  | "reenable";
export type ManagementRequest = {
  idempotency_key: string;
  action: ManagementAction;
  draft_id: string | null;
  operation_id: string | null;
  expected_revision_id: string | null;
  expected_lifecycle_epoch?: number;
};
export type CommandReceipt = { command_id: string; employee_id: string };
export type ManagementCommand = {
  command_id: string;
  action: ManagementAction;
  status: "pending" | "running" | "succeeded" | "failed" | "blocked";
  attempts: number;
  operation_id: string | null;
  error_code: string | null;
  created_at: string;
  updated_at: string;
  can_retry: boolean;
  can_compensate: boolean;
  runtime_probe?: {
    state: "running" | "succeeded" | "failed";
    generation: number;
  } | null;
};
export type ManagementPage = {
  employee_id: string;
  commands: ManagementCommand[];
  expected_revision_id: string | null;
  expected_lifecycle_epoch?: number;
  status?: string | null;
  lifecycle_supported: boolean;
  lifecycle?: {
    can_disable: boolean;
    old_active_runs: number;
    pending_stops: number;
    failed_stops: number;
  };
};
