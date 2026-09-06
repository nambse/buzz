import { useCallback, useState } from "react";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Button } from "@/shared/ui/button";
import { Checkbox } from "@/shared/ui/checkbox";
import { Field } from "../work/fields";
import { useConversationRead } from "../conversationMemory/useConversationRead";
import type { EmployeeFact, EmployeeExportRecord } from "./types";
import type { useEmployeeReview } from "./useEmployeeReview";

/** Publication is separate consent; metadata and removal recovery need no source text. */
export function EmployeePublication({
  fact,
  state,
  employeeName,
  destinationName,
}: {
  fact: EmployeeFact;
  state: ReturnType<typeof useEmployeeReview>;
  employeeName: string;
  destinationName: string;
}) {
  const { readExport, mutation } = state;
  const load = useCallback(
    (signal: AbortSignal) => readExport(fact.id, signal),
    [readExport, fact.id],
  );
  const status = useConversationRead(
    load,
    state.facts.ready,
    mutation.revision,
    state.invalidate,
  );
  const [consent, setConsent] = useState<{
    fact: EmployeeFact;
    record: EmployeeExportRecord;
  } | null>(null);
  const confirmed =
    consent !== null &&
    consent.fact === fact &&
    consent.record === status.value;
  const disabled = state.blocked || !state.facts.ready || !status.ready;
  const canPublish =
    state.facts.value?.can_approve &&
    fact.status === "approved" &&
    fact.version === 1 &&
    fact.source_current &&
    Date.parse(fact.expires_at) > Date.now();
  const saved = status.value?.export;
  const publication = saved?.jobs.find((job) => job.action === "publish");
  const removal = saved?.jobs.find((job) => job.action === "withdraw");
  return (
    <section
      aria-label="Employee memory publication"
      className="flex flex-col gap-2"
    >
      {status.error ? (
        <Alert variant="destructive">
          <AlertDescription>{status.error}</AlertDescription>
        </Alert>
      ) : null}
      <Button
        type="button"
        size="sm"
        variant="outline"
        className="self-start"
        disabled={state.blocked || !state.facts.ready}
        onClick={status.refresh}
      >
        Refresh publication status
      </Button>
      {!status.ready && !status.error ? (
        <p role="status" className="text-sm">
          Checking publication status…
        </p>
      ) : null}
      {status.ready && status.value && !saved ? (
        <>
          <p className="text-sm">No publication has been requested.</p>
          {canPublish ? (
            <form
              aria-label="Publish employee memory"
              className="flex flex-col gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                if (!disabled && confirmed && canPublish)
                  state.publication(fact, "publish", 1);
              }}
            >
              <Field
                label={`I approve publishing this text to ${employeeName} for ${destinationName}`}
              >
                {(id) => (
                  <Checkbox
                    id={id}
                    required
                    checked={confirmed}
                    disabled={disabled}
                    onCheckedChange={(checked) =>
                      setConsent(
                        checked === true && status.value
                          ? { fact, record: status.value }
                          : null,
                      )
                    }
                  />
                )}
              </Field>
              <Button
                type="submit"
                size="sm"
                variant="outline"
                className="self-start"
                disabled={disabled || !confirmed}
              >
                Publish approved memory
              </Button>
            </form>
          ) : (
            <p className="text-xs text-muted-foreground">
              New publication requires a current approval, visible source and
              permission to review this employee’s memory. Stop remains
              available.
            </p>
          )}
        </>
      ) : null}
      {status.ready && saved && publication && removal ? (
        <>
          <p role="status" className="text-sm">
            {removal.acknowledged
              ? "Removal acknowledged by the reviewed memory store. Approval history remains."
              : publication.acknowledged
                ? "Publication acknowledged by the reviewed memory store."
                : publication.state === "failed"
                  ? "Publication failed; acknowledgment is not confirmed."
                  : "Publication queued or awaiting acknowledgment."}
          </p>
          {!removal.acknowledged ? (
            <p className="text-xs text-muted-foreground">
              {removal.state === "failed"
                ? "Removal failed; cleanup is not confirmed."
                : fact.status !== "approved" ||
                    Date.parse(fact.expires_at) <= Date.now()
                  ? "Use has ended. Removal is awaiting acknowledgment."
                  : "Removal is scheduled for expiry or earlier Stop using."}
            </p>
          ) : null}
          {saved.jobs.map((job) =>
            job.state === "failed" ? (
              <div key={job.action} className="flex flex-col gap-2">
                {job.retry_version >= 8 ? (
                  <p className="text-xs text-muted-foreground">
                    The {job.action === "publish" ? "publication" : "removal"}{" "}
                    retry limit was reached. An operator must check its status.
                  </p>
                ) : job.action === "withdraw" || canPublish ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    className="self-start"
                    disabled={disabled}
                    onClick={() => {
                      if (!disabled)
                        state.publication(
                          fact,
                          job.action === "publish"
                            ? "retry_publish"
                            : "retry_withdraw",
                          job.retry_version,
                        );
                    }}
                  >
                    Retry {job.action === "publish" ? "publication" : "removal"}
                  </Button>
                ) : null}
              </div>
            ) : null,
          )}
        </>
      ) : null}
      <p className="text-xs text-muted-foreground">
        Eligible Office and message-promoted Work runs may use published memory
        when the operator has enabled this destination and permissions remain
        current. Check a run’s Activity to see what it actually used.
      </p>
    </section>
  );
}
