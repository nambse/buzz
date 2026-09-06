import { useState } from "react";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Field } from "../work/fields";
import { conversationPath, type ConversationFact } from "./types";

/** Separate publication consent; retained cleanup remains reachable without source text. */
export function ConversationPublication({
  fact,
  disabled,
  submit,
}: {
  fact: ConversationFact["fact"];
  disabled: boolean;
  submit: (path: string, values: Record<string, unknown>) => void;
}) {
  const [confirmedFact, setConfirmed] = useState<typeof fact | null>(null);
  const confirmed = confirmedFact === fact;
  const path = `${conversationPath(fact.project_id)}/${encodeURIComponent(fact.id)}`;
  const saved = fact.export;
  if (!saved)
    return (
      <div className="flex flex-col gap-2">
        <p className="text-xs text-muted-foreground">
          No publication has been requested. Runtime use requires publication
          and a separate current operator opt-in.
        </p>
        {fact.publication_available &&
        fact.source_visible &&
        fact.status === "active" &&
        fact.version === 1 ? (
          <form
            aria-label={`Publish conversation fact ${fact.id}`}
            className="flex flex-col gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              if (disabled || !confirmed) return;
              submit(`${path}/publish`, {
                expected_version: 1,
                confirmed: true,
              });
            }}
          >
            <fieldset disabled={disabled} className="flex flex-col gap-2">
              <Field label="I approve publishing this fact to this employee’s reviewed memory for the displayed conversation audience">
                {(id) => (
                  <Input
                    id={id}
                    name="confirmed"
                    type="checkbox"
                    required
                    checked={confirmed}
                    onChange={(event) =>
                      setConfirmed(event.target.checked ? fact : null)
                    }
                    className="size-4"
                  />
                )}
              </Field>
              <p className="text-xs text-muted-foreground">
                Matching Office and Work runs may use published facts when
                current permissions and the operator’s runtime opt-in allow it.
                Publishing does not change that setting.
              </p>
              <Button
                type="submit"
                size="sm"
                variant="outline"
                disabled={disabled || !confirmed}
                className="self-start"
              >
                Publish conversation fact
              </Button>
            </fieldset>
          </form>
        ) : fact.status === "active" ? (
          <p className="text-xs text-muted-foreground">
            Publication needs current visible evidence and an available reviewed
            memory target. Stop using remains available.
          </p>
        ) : null}
      </div>
    );
  return (
    <fieldset
      aria-label={`Conversation publication status for fact ${fact.id}`}
      className="flex flex-col gap-2"
    >
      <p className="text-sm">
        {saved.erased_from_reviewed_store
          ? "Reviewed-store text removed. Approval and tombstone records remain."
          : saved.publication.state === "acknowledged"
            ? "Publication acknowledged by the reviewed store."
            : saved.publication.state === "failed"
              ? "Publication failed; acknowledgement is not confirmed."
              : "Publication queued or awaiting acknowledgement."}
      </p>
      <p className="text-xs text-muted-foreground">
        {saved.runtime_consumption_enabled
          ? "Runtime use is currently eligible for matching Office and Work runs within the reviewed audience."
          : "Runtime use is currently unavailable under the operator’s opt-in and the fact’s permissions."}
      </p>
      {!saved.erased_from_reviewed_store ? (
        <p className="text-xs text-muted-foreground">
          {saved.cleanup.state === "failed"
            ? "Reviewed-store cleanup failed; removal is not confirmed."
            : fact.status !== "active"
              ? "Use has ended. Reviewed-store removal is awaiting acknowledgement."
              : "Reviewed-store cleanup is scheduled for expiry or earlier Stop using."}
          {saved.cleanup.state === "pending" ? (
            <>
              {" "}
              Next attempt:{" "}
              <time dateTime={saved.cleanup.next_attempt_at}>
                {new Date(saved.cleanup.next_attempt_at).toLocaleString()}
              </time>
              .
            </>
          ) : null}
        </p>
      ) : null}
      {(
        [
          ["publish", saved.publication],
          ["withdraw", saved.cleanup],
        ] as const
      ).map(([action, job]) =>
        job.state === "failed" ? (
          <div key={action} className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground">
              {action === "publish" ? "Publication" : "Cleanup"}:{" "}
              {job.error_code ?? "unavailable"} · {job.attempt_count} attempts.
            </p>
            {job.retry_version >= 8 ? (
              <p className="text-xs text-muted-foreground">
                Retry limit reached. An operator must inspect the retained job.
              </p>
            ) : action === "withdraw" ||
              (fact.status === "active" && fact.source_visible) ? (
              <Button
                size="sm"
                variant="outline"
                className="self-start"
                disabled={disabled}
                aria-label={`Retry conversation ${action === "publish" ? "publication" : "reviewed-store cleanup"} for fact ${fact.id}`}
                onClick={() => {
                  if (!disabled)
                    submit(`${path}/exports/${action}/retry`, {
                      retry_version: job.retry_version,
                    });
                }}
              >
                Retry{" "}
                {action === "publish"
                  ? "publication"
                  : "reviewed-store cleanup"}
              </Button>
            ) : null}
          </div>
        ) : null,
      )}
    </fieldset>
  );
}
