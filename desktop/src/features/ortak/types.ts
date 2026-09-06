import type { ConversationAudience } from "./conversationMemory/types";
import type { EmployeeAudience } from "./employeeMemory/types";

export type RunStatus =
  | "queued"
  | "running"
  | "waiting"
  | "completed"
  | "failed"
  | "cancelled";
export type Employee = {
  employee_id: string;
  name: string | null;
  title: string | null;
  status: "draft" | "active" | "paused" | "disabled";
  active_revision_id: string | null;
};
export type EmployeePage = {
  can_view_provisioning?: boolean;
  can_execute_provisioning?: boolean;
  employees: Employee[];
  has_more: boolean;
  next_after: string | null;
};
export type RunHeader = {
  run_id: string;
  employee_id: string;
  status: RunStatus;
  outcome: { kind: string; delivery_intent?: string; code?: string };
  timing: {
    queued_at: string;
    started_at: string | null;
    finished_at: string | null;
    updated_at: string;
  };
  provenance: {
    routing_decision_id: string | null;
    message_id: string | null;
    work_item_id?: string | null;
  };
  last_event: { sequence: number } | null;
};
export type RunPage = {
  runs: RunHeader[];
  has_more: boolean;
  next_cursor: string | null;
};
export type Cancellation = {
  request_id: string;
  run_id: string;
  status: "pending" | "acknowledged" | "failed";
  requested_at: string;
};
export type RunDetailResponse = {
  detail: {
    run: RunHeader;
    error_message: string | null;
    cancel_reason: string | null;
  };
  cancellation: Cancellation | null;
  can_request_cancel: boolean;
  memory?: RunMemory;
  work_output?: {
    status: "pending" | "materialized" | "failed";
    artifact_id: string | null;
    work_item_id: string;
    error_code: string | null;
  } | null;
  office_delivery: {
    status: "pending" | "delivered" | "failed";
    error_code: string | null;
    delivered_at: string | null;
  } | null;
};
export type ActivityText = {
  text: string;
  redacted?: boolean;
  truncated?: boolean;
};
export type RunMemory = {
  scope:
    | "run_scratch"
    | "run_scratch_and_reviewed_project"
    | "run_scratch_and_reviewed_conversation"
    | "run_scratch_and_reviewed_employee";
  run_id: string;
  reviewed?: {
    fact_id: string;
    approval_id: string;
    approved_by: string;
    expires_at: string;
    current: boolean;
    content: ActivityText | null;
    audience_kind?: "project" | "conversation" | "employee";
    audience?: ConversationAudience | EmployeeAudience | null;
  }[];
  recall: {
    status: "not_prepared" | "prepared";
    prepared_at: string | null;
    truncated: boolean;
    withheld?: boolean;
    records: {
      record_ref: string;
      content: ActivityText;
      source: string;
      recorded_at: string;
    }[];
  };
  write: {
    status: "pending" | "acknowledged" | "failed";
    error_code: string | null;
    attempts: number;
    next_attempt_at: string | null;
    content: ActivityText;
    withheld?: boolean;
    source: string;
    recorded_at: string;
    receipt: { reference: string; written: number } | null;
    acknowledged_at: string | null;
  } | null;
};
export type Activity = {
  kind: string;
  text?: ActivityText;
  path?: string;
  change?: string;
  summary?: ActivityText;
  message?: ActivityText;
  code?: string;
  intent?: string;
  phase?: {
    phase: string;
    reason?: string | ActivityText;
    detail?: ActivityText;
    message?: ActivityText;
    tool?: string;
    arguments?: ActivityText;
    result?: ActivityText;
    error?: ActivityText;
    command?: ActivityText;
    chunk?: ActivityText;
    exit_code?: number | null;
    delivery_intent?: string;
  };
};
export type ActivityEntry = {
  sequence: number;
  event_type: string;
  occurred_at: string;
  activity: Activity;
  redacted: boolean;
  truncated: boolean;
};
export type ActivityPage = {
  entries: ActivityEntry[];
  next_after_sequence: number | null;
  has_more: boolean;
  gap: { expected: number; found: number } | null;
};

export type WorkState =
  | "proposed"
  | "ready"
  | "in_progress"
  | "blocked"
  | "review"
  | "completed"
  | "cancelled";
export type WorkPriority = "low" | "normal" | "high" | "urgent";
export interface WorkProject {
  id: string;
  slug: string;
  name: string;
  description?: string;
  status: "active" | "archived";
  version: number;
  channel_id: string;
  role: "owner" | "contributor" | "reviewer" | "viewer";
  can_contribute: boolean;
  can_review: boolean;
}
export interface ProjectPage {
  projects: WorkProject[];
  next_cursor: string | null;
  can_create_projects: boolean;
  create_channels: { id: string; name: string }[];
}
export interface WorkSummary {
  source_message_id?: string | null;
  id: string;
  project_id: string;
  title: string;
  priority: WorkPriority;
  state: WorkState;
  version: number;
}
export interface WorkPage {
  work_items: WorkSummary[];
  next_cursor: string | null;
}
export interface WorkDependencyPage {
  work_item_id: string;
  work_version: number;
  dependencies: { id: string; target: WorkSummary | null }[];
}
export interface WorkDecomposition {
  work_item_id: string;
  work_version: number;
  parent: WorkSummary | null;
  children: WorkSummary[];
}
export interface WorkItem extends WorkSummary {
  description: string;
  criteria: {
    id: string;
    position: number;
    text: string;
    status: "pending" | "satisfied";
  }[];
  approvals: {
    id: string;
    gate: string;
    required: boolean;
    status: "pending" | "approved" | "rejected";
    reason: string | null;
  }[];
  assignments: {
    employee_id: string;
    role: "owner" | "contributor" | "reviewer";
    status: "active" | "released";
  }[];
  history: {
    sequence: number;
    version: number;
    event_type: string;
    recorded_at: string;
    from?: WorkState;
    to?: WorkState;
  }[];
  history_omitted: boolean;
  history_truncated: boolean;
  execution_available: boolean;
}

export interface EmployeeWorkPage {
  employee_id: string;
  work_items: (WorkSummary & {
    assignment_role: "owner" | "contributor" | "reviewer";
  })[];
  next_cursor: string | null;
  execution_available: boolean;
}

export interface WorkExecution {
  run_id: string;
  employee_id: string;
  execution_version: number;
  status: RunStatus;
  artifact_id: string | null;
  output_code: string | null;
  reconciled: boolean;
}
