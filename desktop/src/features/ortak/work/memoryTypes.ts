export type ReviewedFactSource =
  | { kind: "conversation"; message_id: string }
  | { kind: "artifact"; artifact_id: string };
export interface ReviewedFact {
  id: string;
  project_id: string;
  employee_id: string;
  source: ReviewedFactSource | null;
  source_visible: boolean;
  content: string | null;
  version: number;
  status: "active" | "expired" | "revoked";
  approved_by: string;
  approved_at: string;
  expires_at: string;
  revoked_by: string | null;
  revoked_at: string | null;
  revoke_reason: string | null;
  publication_available?: boolean;
  export?: ReviewedExport | null;
}
export interface ReviewedExportJob {
  state: "pending" | "acknowledged" | "failed";
  retry_version: number;
  attempt_count: number;
  next_attempt_at: string;
  error_code: string | null;
}
export interface ReviewedExport {
  fact_id: string;
  publication: ReviewedExportJob;
  cleanup: ReviewedExportJob;
  erased_from_reviewed_store: boolean;
  runtime_consumption_enabled: false;
}
export interface ReviewedFactPage {
  facts: ReviewedFact[];
  next_after: string | null;
}
export interface ReviewedRecall {
  facts: ReviewedFact[];
  truncated: boolean;
}
