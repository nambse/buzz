import type { ReviewedExport, ReviewedFact } from "../work/memoryTypes";

export type AudienceKind = "thread" | "channel";
export interface ConversationAudience {
  format: "ortak-reviewed-conversation-audience/1";
  company_id: string;
  community_id: string;
  project_id: string;
  employee_id: string;
  channel_id: string;
  kind: AudienceKind;
  thread_root_event_id: string | null;
  thread_root_event_created_at: string | null;
}
export interface ConversationPreview {
  audience: ConversationAudience;
  audience_hash: string;
  provenance: {
    source_event_id: string;
    source_hash: string;
    [key: string]: unknown;
  };
  observed_at: string;
  valid_before: string | null;
  max_expires_at: string;
}
/** Conversation eligibility is a current server observation, never a UI setting. */
export type ConversationExport = Omit<
  ReviewedExport,
  "runtime_consumption_enabled"
> & { runtime_consumption_enabled: boolean };
export interface ConversationExportReceipt {
  export: ConversationExport;
}
export interface ConversationFact {
  fact: Omit<ReviewedFact, "export"> & {
    export?: ConversationExport | null;
  };
  audience: ConversationAudience | null;
  audience_hash: string | null;
  provenance: Record<string, unknown> | null;
}
export interface ConversationFactPage {
  facts: ConversationFact[];
  next_after: string | null;
}
export interface ConversationReceipt {
  fact: ConversationFact;
  created: boolean;
}
export interface ConversationPreviewRequest {
  employee_id: string;
  source_message_id: string;
  audience: { kind: AudienceKind };
}
export interface ConversationDraft extends ConversationPreviewRequest {
  expected_audience_hash: string;
  content: string;
  expires_at: string;
  reviewed: true;
}
export const conversationPath = (project: string) =>
  `/api/v1/projects/${encodeURIComponent(project)}/conversation-memory`;
