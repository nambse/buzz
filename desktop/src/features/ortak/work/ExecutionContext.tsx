import type { WorkExecution } from "../types";

export function ExecutionContext({ execution }: { execution: WorkExecution }) {
  const context = execution.reference_context;
  if (!context) return null;
  return (
    <section aria-label="Execution references" className="flex flex-col gap-2">
      <h5 className="text-sm font-semibold">Execution references</h5>
      {context.status === "unavailable" ? (
        <p className="text-sm text-muted-foreground">
          These references cannot be shown under current permissions. Starting
          and delivery each require a fresh check of source access.
        </p>
      ) : (
        <>
          <p className="text-sm text-muted-foreground">
            {context.artifact_execution_version !== null
              ? `Previous deliverable: work version ${context.artifact_execution_version}.`
              : "No previous deliverable was selected."}{" "}
            {context.status === "pending"
              ? "Conversation references will be checked before the employee starts."
              : `${context.message_ids.length} conversation messages were selected.`}{" "}
            {context.history_limited
              ? "Some message text or older history was omitted to fit the context limit."
              : ""}
          </p>
          {context.message_ids.length > 0 ? (
            <details className="text-xs text-muted-foreground">
              <summary>Selected message references</summary>
              <ul className="mt-2 flex flex-col gap-1">
                {context.message_ids.map((id) => (
                  <li key={id} className="break-all">
                    {id}
                  </li>
                ))}
              </ul>
            </details>
          ) : null}
        </>
      )}
    </section>
  );
}
