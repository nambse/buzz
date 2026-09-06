import { useMemo, useState } from "react";
import { signRelayEvent } from "@/shared/api/tauri";
import { Alert, AlertDescription, AlertTitle } from "@/shared/ui/alert";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Skeleton } from "@/shared/ui/skeleton";
import { createOrtakClient, type OrtakClient } from "../client";
import { useRoutingDecision } from "./useRoutingDecision";

function label(value: string) {
  const known: Record<string, string> = {
    no_relevant_employee: "No employee met the relevance threshold",
    semantic_match: "Relevant employees selected within the routing limits",
    semantic_scorer_timed_out:
      "Scoring reached its time limit; no employee was selected",
    semantic_scorer_unavailable:
      "Scoring was unavailable; no employee was selected",
    semantic_scorer_disabled: "Semantic scoring is not enabled",
    below_semantic_threshold: "Below the relevance threshold",
    recipient_limit_reached: "The recipient limit was reached",
    request_timeout: "The scoring request timed out",
  };
  return known[value] ?? value.replaceAll("_", " ");
}

export function RoutingDecisionPanel({
  client,
  channel,
  message,
}: {
  client: Pick<OrtakClient, "routingDecisionStream">;
  channel: string;
  message: string;
}) {
  const [refresh, setRefresh] = useState(0);
  const state = useRoutingDecision(client, channel, message, refresh);
  const decision = state?.page?.decision;
  return (
    <section aria-label="Recorded routing" className="flex flex-col gap-4">
      <Button
        variant="outline"
        size="sm"
        onClick={() => setRefresh((v) => v + 1)}
      >
        Refresh routing
      </Button>
      {!state ? <Skeleton className="h-24 w-full" /> : null}
      {state?.error ? (
        <Alert variant="destructive">
          <AlertTitle>Routing could not be checked</AlertTitle>
          <AlertDescription>
            {state.error}{" "}
            {state.retrying
              ? "Retrying shortly."
              : "Use Refresh routing to check again."}
          </AlertDescription>
        </Alert>
      ) : null}
      {state?.page && !decision ? (
        <Alert role="status">
          <AlertTitle>No routing decision is recorded</AlertTitle>
          <AlertDescription>
            This message has no confirmed routing outcome yet. This does not
            mean a decision to stay silent was made.
          </AlertDescription>
        </Alert>
      ) : null}
      {decision ? (
        <>
          <div className="flex flex-col gap-2">
            <Badge variant="secondary">{decision.mode}</Badge>
            <p>
              {decision.mode === "silent"
                ? "No employee was dispatched by this decision."
                : "This decision selected employees for dispatch."}
            </p>
            <p className="text-sm">{label(decision.summary_reason)}</p>
            <p className="text-xs text-muted-foreground">
              Recorded {new Date(decision.decided_at).toLocaleString()} · Policy{" "}
              {decision.policy_version ?? "not recorded"}
            </p>
          </div>
          {decision.scorer.adapter ? (
            <dl className="grid grid-cols-2 gap-2 text-sm">
              <dt>Scorer</dt>
              <dd>{decision.scorer.adapter}</dd>
              <dt>Model / thinking</dt>
              <dd>
                {decision.scorer.model ?? "not recorded"} /{" "}
                {decision.scorer.reasoning_effort ?? "not recorded"}
              </dd>
              <dt>Scorer / prompt version</dt>
              <dd>
                {decision.scorer.version ?? "not recorded"} /{" "}
                {decision.scorer.prompt_version ?? "not recorded"}
              </dd>
              <dt>Latency</dt>
              <dd>
                {decision.scorer.latency_ms === null
                  ? "not recorded"
                  : `${decision.scorer.latency_ms} ms`}
              </dd>
              <dt>Input / output tokens</dt>
              <dd>
                {decision.scorer.input_tokens ?? "not recorded"} /{" "}
                {decision.scorer.output_tokens ?? "not recorded"}
              </dd>
              <dt>Cached evidence</dt>
              <dd>
                {decision.scorer.cache_hit === null
                  ? "not recorded"
                  : decision.scorer.cache_hit
                    ? "Yes"
                    : "No"}
              </dd>
              {decision.scorer.failure_code ? (
                <>
                  <dt>Scoring outcome</dt>
                  <dd>{label(decision.scorer.failure_code)}</dd>
                </>
              ) : null}
            </dl>
          ) : null}
          <p className="text-sm text-muted-foreground">
            Only candidates your current account can inspect are shown. Scores
            explain relevance; routing rules decide dispatch.
          </p>
          <ul
            className="flex flex-col gap-3"
            aria-label="Visible routing candidates"
          >
            {decision.recipients.map((recipient) => (
              <li key={recipient.employee_id} className="flex flex-col gap-1">
                <span>
                  {recipient.employee_id} ·{" "}
                  {recipient.action === "wake" ? "Selected" : "Not selected"}
                </span>
                <span className="text-sm">
                  {label(recipient.reason)}
                  {recipient.score !== null
                    ? ` · Score ${recipient.score.toFixed(2)}`
                    : " · Not scored"}
                </span>
                {recipient.evidence.length ? (
                  <span className="text-xs text-muted-foreground">
                    {recipient.evidence.map(label).join(", ")}
                  </span>
                ) : null}
              </li>
            ))}
          </ul>
          {!decision.recipients.length ? (
            <p>No candidate details are available to this account.</p>
          ) : null}
          {decision.recipients_truncated ? (
            <p>Showing the first 32 visible candidates.</p>
          ) : null}
        </>
      ) : null}
      {state?.checkedAt ? (
        <p className="text-xs text-muted-foreground">
          Last snapshot {new Date(state.checkedAt).toLocaleTimeString()}.
          {state.connected
            ? " Live routing connected."
            : " Reconnecting to routing."}
        </p>
      ) : null}
    </section>
  );
}

export function RoutingDecisionDialog({
  origin,
  channel,
  message,
  onClose,
  restoreFocus,
}: {
  origin: string;
  channel: string;
  message: string;
  onClose: () => void;
  restoreFocus?: () => void;
}) {
  const client = useMemo(
    () => createOrtakClient(origin, signRelayEvent),
    [origin],
  );
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent
        className="max-h-[85vh] overflow-y-auto"
        onCloseAutoFocus={(event) => {
          if (restoreFocus) {
            event.preventDefault();
            restoreFocus();
          }
        }}
      >
        <DialogHeader>
          <DialogTitle>Message routing</DialogTitle>
          <DialogDescription>
            The saved routing outcome and currently authorized candidate
            details.
          </DialogDescription>
        </DialogHeader>
        <RoutingDecisionPanel
          key={`${origin}:${channel}:${message}`}
          client={client}
          channel={channel}
          message={message}
        />
      </DialogContent>
    </Dialog>
  );
}
