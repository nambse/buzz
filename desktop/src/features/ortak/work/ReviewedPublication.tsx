import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Field, type SubmitWork } from "./fields";
import type { ReviewedFact } from "./memoryTypes";

/** Publication has separate human consent; cleanup remains visible after source loss. */
export function ReviewedPublication({
  fact,
  canReview,
  disabled,
  submit,
}: {
  fact: ReviewedFact;
  canReview: boolean;
  disabled: boolean;
  submit: SubmitWork;
}) {
  const path = `/api/v1/projects/${encodeURIComponent(fact.project_id)}/reviewed-memory/${encodeURIComponent(fact.id)}`;
  const saved = fact.export;
  if (!saved)
    return (
      <div className="flex flex-col gap-2">
        <p className="text-xs text-muted-foreground">
          Saved for recall preview. No publication has been requested.
        </p>
        {canReview &&
        fact.publication_available &&
        fact.source_visible &&
        fact.status === "active" ? (
          <form
            aria-label={`Publish reviewed fact ${fact.id}`}
            className="flex flex-col gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              if (
                disabled ||
                new FormData(event.currentTarget).get("confirmed") !== "on"
              )
                return;
              submit(`${path}/publish`, "Reviewed fact publication requested", {
                expected_version: fact.version,
                confirmed: true,
              });
            }}
          >
            <fieldset disabled={disabled} className="flex flex-col gap-2">
              <Field label="I approve sending this fact to the selected Honcho reviewed store for this employee and project, including selected Work runs when runtime use is enabled">
                {(id) => (
                  <Input
                    id={id}
                    name="confirmed"
                    type="checkbox"
                    required
                    className="size-4"
                  />
                )}
              </Field>
              <Button
                type="submit"
                size="sm"
                variant="outline"
                className="self-start"
              >
                Publish reviewed fact
              </Button>
            </fieldset>
          </form>
        ) : canReview && fact.status === "active" ? (
          <p className="text-xs text-muted-foreground">
            Publication requires visible evidence and a current validated memory
            target for this employee and project. Stop using remains available.
          </p>
        ) : null}
      </div>
    );
  const cleanupDue = fact.status !== "active";
  return (
    <fieldset
      className="flex flex-col gap-2"
      aria-label={`Publication status for fact ${fact.id}`}
    >
      <p className="text-sm">
        {saved.erased_from_reviewed_store
          ? "Reviewed-store text removed. Approval and tombstone records remain."
          : saved.publication.state === "acknowledged"
            ? "Publication acknowledged by the reviewed store."
            : saved.publication.state === "failed"
              ? "Publication stopped after a failed attempt."
              : "Publication queued or awaiting acknowledgement."}
      </p>
      <p className="text-xs text-muted-foreground">
        {saved.runtime_consumption_enabled
          ? "Selected Work runs for this employee and project may include this approved fact."
          : "Runtime use is not enabled for this fact under the current permissions and memory settings."}
      </p>
      {!saved.erased_from_reviewed_store ? (
        <p className="text-xs text-muted-foreground">
          {saved.cleanup.state === "failed"
            ? "Reviewed-store cleanup failed; removal is not confirmed."
            : cleanupDue
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
            {canReview &&
            job.retry_version < 8 &&
            (action === "withdraw" ||
              (fact.status === "active" && fact.source_visible)) ? (
              <Button
                size="sm"
                variant="outline"
                className="self-start"
                disabled={disabled}
                aria-label={`Retry ${action === "publish" ? "publication" : "reviewed-store cleanup"} for fact ${fact.id}`}
                onClick={() =>
                  submit(
                    `${path}/exports/${action}/retry`,
                    "Same reviewed memory operation queued",
                    { retry_version: job.retry_version },
                  )
                }
              >
                Retry{" "}
                {action === "publish"
                  ? "publication"
                  : "reviewed-store cleanup"}
              </Button>
            ) : job.retry_version >= 8 ? (
              <p className="text-xs text-muted-foreground">
                Retry limit reached. An operator must inspect the retained
                cleanup record.
              </p>
            ) : null}
          </div>
        ) : null,
      )}
    </fieldset>
  );
}
