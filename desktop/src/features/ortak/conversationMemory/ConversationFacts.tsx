import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import type { ConversationFactPage } from "./types";
import { conversationPath } from "./types";
import { ConversationPublication } from "./ConversationPublication";
import type { useConversationMutation } from "./useConversationMutation";
import type { useConversationRead } from "./useConversationRead";

export function ConversationOperationStatus({
  state,
}: {
  state: ReturnType<typeof useConversationMutation>;
}) {
  return state.notice || state.pending ? (
    <Alert role="status">
      <AlertDescription className="flex flex-col gap-2">
        <p>
          {state.notice ??
            "Waiting for confirmation. The exact request is retained in this open message or project view."}
        </p>
        {state.pending ? (
          <Button variant="outline" disabled={state.busy} onClick={state.retry}>
            Retry same memory operation
          </Button>
        ) : null}
      </AlertDescription>
    </Alert>
  ) : null;
}

/** Always independent of source preview: hidden evidence must not hide Stop using. */
export function ConversationFacts({
  read,
  mutation,
  project,
  employee,
  after,
  setAfter,
  disabled = false,
}: {
  read: ReturnType<typeof useConversationRead<ConversationFactPage>>;
  mutation: ReturnType<typeof useConversationMutation>;
  project: string;
  employee: string;
  after?: string;
  setAfter: (value?: string) => void;
  disabled?: boolean;
}) {
  const blocked = disabled || mutation.busy || Boolean(mutation.pending);
  return (
    <section
      aria-label="Saved conversation facts"
      className="flex flex-col gap-3"
    >
      <h4 className="text-base font-semibold">Saved conversation facts</h4>
      <p className="text-sm text-muted-foreground">
        These approvals are separate from project memory. Publication requires
        its own confirmation. Stop using ends permission to use a fact; approval
        history and cleanup status remain stored.
      </p>
      {employee ? (
        <Button
          variant="outline"
          size="sm"
          disabled={blocked}
          onClick={read.refresh}
        >
          Refresh conversation facts
        </Button>
      ) : (
        <p className="text-sm">Choose an employee to inspect saved facts.</p>
      )}
      {read.error ? (
        <Alert variant="destructive">
          <AlertDescription>{read.error}</AlertDescription>
        </Alert>
      ) : null}
      {employee && !read.value && !read.error ? (
        <p role="status" className="text-sm">
          Checking saved conversation facts…
        </p>
      ) : null}
      {read.value ? (
        <>
          {!read.value.facts.length ? (
            <p className="text-sm">No conversation facts on this page.</p>
          ) : null}
          <ul className="flex flex-col gap-3">
            {read.value.facts.map((entry) => {
              const fact = entry.fact;
              return (
                <li
                  key={fact.id}
                  className="flex flex-col gap-2 rounded-lg border p-3 text-sm"
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge variant="secondary">
                      {fact.status === "revoked"
                        ? "Use stopped"
                        : fact.status === "expired"
                          ? "Use expired"
                          : "Reviewed"}
                    </Badge>
                    <span>Version {fact.version}</span>
                  </div>
                  {fact.source_visible ? (
                    <>
                      <p className="whitespace-pre-wrap break-words">
                        {fact.content}
                      </p>
                      <p>
                        {entry.audience?.kind === "thread"
                          ? "Only the reviewed thread"
                          : "The reviewed channel"}
                      </p>
                    </>
                  ) : (
                    <p>
                      Source evidence has changed or is no longer available.
                      Fact text and audience details are withheld. You can still
                      stop its use.
                    </p>
                  )}
                  <p className="text-xs text-muted-foreground">
                    Use until{" "}
                    <time dateTime={fact.expires_at}>
                      {new Date(fact.expires_at).toLocaleString()}
                    </time>
                  </p>
                  <details className="text-xs text-muted-foreground">
                    <summary className="cursor-pointer">
                      Approval record
                    </summary>
                    <p className="break-all">
                      Fact {fact.id} · Human {fact.approved_by}
                    </p>
                    <p>
                      Approved{" "}
                      <time dateTime={fact.approved_at}>
                        {new Date(fact.approved_at).toLocaleString()}
                      </time>
                    </p>
                  </details>
                  <ConversationPublication
                    fact={fact}
                    disabled={blocked || !read.ready}
                    submit={mutation.submit}
                  />
                  {fact.version === 1 ? (
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={blocked || !read.ready}
                      className="self-start"
                      aria-label={`Stop using conversation fact ${fact.id}`}
                      onClick={() => {
                        if (!blocked && read.ready)
                          mutation.submit(
                            `${conversationPath(project)}/${encodeURIComponent(fact.id)}/stop`,
                            {
                              expected_version: 1,
                              reason: "Human selected Stop using",
                            },
                          );
                      }}
                    >
                      Stop using
                    </Button>
                  ) : null}
                </li>
              );
            })}
          </ul>
          <div className="flex gap-2">
            {after ? (
              <Button
                variant="outline"
                size="sm"
                disabled={blocked}
                onClick={() => setAfter()}
              >
                First conversation facts
              </Button>
            ) : null}
            {read.value.next_after ? (
              <Button
                variant="outline"
                size="sm"
                disabled={blocked}
                onClick={() => setAfter(read.value?.next_after ?? undefined)}
              >
                More conversation facts
              </Button>
            ) : null}
          </div>
        </>
      ) : null}
    </section>
  );
}
