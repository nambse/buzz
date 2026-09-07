export type RoutingDecisionPage = {
  channel_id: string;
  message_id: string;
  decision: null | {
    decision_id: string;
    mode: "silent" | "semantic" | "deterministic";
    summary_reason: string;
    policy_version: string | null;
    decided_at: string;
    scorer: {
      adapter: string | null;
      model: string | null;
      reasoning_effort: string | null;
      prompt_version: string | null;
      version: string | null;
      latency_ms: number | null;
      cache_hit: boolean | null;
      failure_code: string | null;
      input_tokens: number | null;
      output_tokens: number | null;
      total_tokens: number | null;
    };
    recipients: {
      employee_id: string;
      action: "wake" | "drop";
      reason: string;
      score: number | null;
      evidence: string[];
    }[];
    recipients_truncated: boolean;
  };
};
