import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { EmployeeMemoryForm } from "./EmployeeMemoryForm";
import { EmployeePublication } from "./EmployeePublication";
import type { useEmployeeReview } from "./useEmployeeReview";

export function EmployeeMemoryPanel({
  state,
  employeeName,
  destinationName,
  canPreview,
  channels = [],
}: {
  state: ReturnType<typeof useEmployeeReview>;
  employeeName: string;
  destinationName: string;
  canPreview: boolean;
  channels?: Array<{ id: string; name: string }>;
}) {
  const { facts, preview, mutation, blocked } = state;
  return (
    <div className="flex flex-col gap-5">
      <Alert role="status">
        <AlertDescription>
          Review and approve edited text, then publish it separately.
          Publication requires an operator-configured destination; it does not
          confirm that a run used the memory.
        </AlertDescription>
      </Alert>
      {mutation.notice ? (
        <p role="status" className="text-sm">
          {mutation.notice}
        </p>
      ) : null}
      {mutation.pending ? (
        <div className="flex flex-col gap-2">
          <p className="text-sm">
            The result of your request is not yet confirmed. Closing and
            reopening this dialog keeps the same request available to retry.
          </p>
          <Button
            type="button"
            disabled={mutation.busy}
            onClick={mutation.retry}
          >
            Retry exact request
          </Button>
        </div>
      ) : null}
      {facts.error ? (
        <Alert variant="destructive">
          <AlertDescription>{facts.error}</AlertDescription>
        </Alert>
      ) : null}
      <Button
        type="button"
        variant="outline"
        disabled={blocked}
        onClick={facts.refresh}
      >
        Refresh saved approvals
      </Button>
      {facts.ready && !facts.value?.can_approve ? (
        <p role="status" className="text-sm">
          New approvals are unavailable. Your deployment must explicitly allow
          employee-memory review and the Employee must be active. Saved metadata
          and Stop remain available with your employee access.
        </p>
      ) : null}
      {canPreview && facts.value?.can_approve ? (
        <section
          aria-label="Current sharing preview"
          className="flex flex-col gap-3"
        >
          {preview.error ? (
            <Alert variant="destructive">
              <AlertDescription>
                {preview.error} The source must be your own decided plaintext
                Office message, with both of you still members of the source and
                destination.
              </AlertDescription>
            </Alert>
          ) : null}
          <Button
            type="button"
            variant="outline"
            disabled={blocked || !facts.ready}
            onClick={preview.refresh}
          >
            Refresh sharing preview
          </Button>
          {preview.ready && preview.value ? (
            <EmployeeMemoryForm
              preview={preview.value}
              employeeName={employeeName}
              destinationName={destinationName}
              disabled={blocked || !facts.ready}
              approve={(draft) => {
                if (preview.value) state.approve(preview.value, draft);
              }}
            />
          ) : (
            <p role="status" className="text-sm">
              No current sharing preview is ready.
            </p>
          )}
        </section>
      ) : null}
      <section
        aria-label="Saved employee memory"
        className="flex flex-col gap-3"
      >
        <h3 className="font-medium">Your saved employee-memory approvals</h3>
        {facts.ready && facts.value?.facts.length === 0 ? (
          <p className="text-sm">No approvals on this page.</p>
        ) : null}
        {facts.value?.facts.map((fact) => (
          <article
            key={fact.id}
            aria-label={`${fact.kind} approval`}
            className="flex flex-col gap-2 rounded-md border p-3"
          >
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-medium">
                {fact.kind === "relationship"
                  ? "Relationship with you"
                  : "Experience"}
              </span>
              <Badge variant="outline">{fact.status}</Badge>
            </div>
            {fact.source_current && fact.audience ? (
              <>
                <p className="text-sm">
                  Destination:{" "}
                  {channels.find(
                    (channel) =>
                      channel.id === fact.audience?.destination_channel_id,
                  )?.name ?? fact.audience.destination_channel_id}
                </p>
                <p className="whitespace-pre-wrap text-sm">{fact.content}</p>
              </>
            ) : (
              <p className="text-sm text-muted-foreground">
                Source, destination and text are hidden because current access
                could not be confirmed.
                {fact.can_stop
                  ? " You can still stop this approval."
                  : " Stop is already recorded."}
              </p>
            )}
            <p className="text-xs text-muted-foreground">
              Approved{" "}
              <time dateTime={fact.approved_at}>
                {new Date(fact.approved_at).toLocaleString()}
              </time>{" "}
              · expires{" "}
              <time dateTime={fact.expires_at}>
                {new Date(fact.expires_at).toLocaleString()}
              </time>
            </p>
            <EmployeePublication
              fact={fact}
              state={state}
              employeeName={employeeName}
              destinationName={
                channels.find(
                  (channel) =>
                    channel.id === fact.audience?.destination_channel_id,
                )?.name ?? "the displayed destination"
              }
            />
            {fact.can_stop ? (
              <Button
                type="button"
                variant="outline"
                disabled={blocked || !facts.ready}
                onClick={() => state.stop(fact.id)}
              >
                Stop using{" "}
                {fact.kind === "relationship"
                  ? "relationship memory"
                  : "experience memory"}
              </Button>
            ) : null}
          </article>
        ))}
        <div className="flex gap-2">
          <Button
            type="button"
            variant="outline"
            disabled={blocked || !state.after}
            onClick={() => state.setAfter(undefined)}
          >
            First approvals page
          </Button>
          <Button
            type="button"
            variant="outline"
            disabled={blocked || !facts.ready || !facts.value?.next_after}
            onClick={() => {
              if (facts.value?.next_after)
                state.setAfter(facts.value.next_after);
            }}
          >
            Next approvals page
          </Button>
        </div>
      </section>
    </div>
  );
}
