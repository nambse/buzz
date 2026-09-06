import { useMemo, useState } from "react";
import { Link } from "@tanstack/react-router";
import type { TimelineMessage } from "@/features/messages/types";
import { signRelayEvent } from "@/shared/api/tauri";
import { Alert, AlertDescription } from "@/shared/ui/alert";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { DropdownMenuGroup, DropdownMenuItem } from "@/shared/ui/dropdown-menu";
import { Skeleton } from "@/shared/ui/skeleton";
import { createOrtakClient } from "../client";
import { useOrtakOrigin } from "../useOrtakOrigin";
import { canReadMessageRouting } from "../routing/MessageRoutingAction";
import { CreateItemForm } from "./CreateForms";
import { Field, Select } from "./fields";
import { useMessagePromotion } from "./useMessagePromotion";

/** Message identity is a source reference only; the server derives its channel and author. */
export function MessagePromotionAction({
  message,
  channel,
  onSelect,
}: {
  message: TimelineMessage;
  channel?: string | null;
  onSelect: (origin: string) => void;
}) {
  const origin = useOrtakOrigin();
  return origin && canReadMessageRouting(message, channel) ? (
    <DropdownMenuGroup>
      <DropdownMenuItem onSelect={() => onSelect(origin)}>
        Promote to Work
      </DropdownMenuItem>
    </DropdownMenuGroup>
  ) : null;
}

export function MessagePromotionPanel({
  state,
  message,
  openWork,
}: {
  state: ReturnType<typeof useMessagePromotion>;
  message: string;
  openWork: (project: string, item: string) => React.ReactNode;
}) {
  const [projectId, setProjectId] = useState("");
  const project = state.page?.projects.find((item) => item.id === projectId);
  const disabled =
    !state.ready ||
    state.busy ||
    Boolean(state.pending) ||
    Boolean(state.result);
  return (
    <section
      aria-label="Promote Office message"
      className="flex flex-col gap-4"
    >
      <Button
        variant="outline"
        size="sm"
        disabled={state.busy}
        onClick={state.refresh}
      >
        Refresh promotion access
      </Button>
      {state.error ? (
        <Alert variant="destructive">
          <AlertDescription>{state.error}</AlertDescription>
        </Alert>
      ) : null}
      {state.notice || state.pending ? (
        <Alert role="status">
          <AlertDescription className="flex flex-col gap-3">
            <p>
              {state.notice ??
                "This attempt is awaiting confirmation. Its exact request is retained while this message view stays mounted."}
            </p>
            {state.pending ? (
              <Button
                variant="outline"
                disabled={state.busy || !state.ready}
                onClick={state.retry}
              >
                Retry same promotion
              </Button>
            ) : null}
          </AlertDescription>
        </Alert>
      ) : null}
      {state.result ? openWork(state.result.project_id, state.result.id) : null}
      {!state.page && !state.error ? (
        <Skeleton className="h-24 w-full" />
      ) : null}
      {state.page && !state.result ? (
        <>
          <Field label="Work project">
            {(id) => (
              <Select
                id={id}
                value={project?.id ?? ""}
                disabled={disabled}
                onChange={(event) => setProjectId(event.target.value)}
              >
                <option value="">Choose a project</option>
                {state.page?.projects.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          {!state.page.projects.length ? (
            <p className="text-sm text-muted-foreground">
              No active project you can contribute to on this Office channel is
              listed on this page.
            </p>
          ) : null}
          <div className="flex gap-2">
            {state.cursor ? (
              <Button
                variant="outline"
                size="sm"
                disabled={disabled}
                onClick={() => state.setCursor()}
              >
                First projects
              </Button>
            ) : null}
            {state.page.next_cursor ? (
              <Button
                variant="outline"
                size="sm"
                disabled={disabled}
                onClick={() =>
                  state.setCursor(state.page?.next_cursor ?? undefined)
                }
              >
                Next projects
              </Button>
            ) : null}
          </div>
          {project ? (
            <CreateItemForm
              key={project.id}
              project={project}
              disabled={disabled}
              sourceMessage={message}
              submit={state.submit}
            />
          ) : null}
        </>
      ) : null}
    </section>
  );
}

/** Keep uncertain request state above DialogContent so closing/reopening preserves it. */
export function MessagePromotionDialog({
  origin,
  channel,
  message,
  open,
  onClose,
  restoreFocus,
}: {
  origin: string;
  channel: string;
  message: string;
  open: boolean;
  onClose: () => void;
  restoreFocus?: () => void;
}) {
  const client = useMemo(
    () => createOrtakClient(origin, signRelayEvent),
    [origin],
  );
  const state = useMessagePromotion(client, channel, message, open);
  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!value) onClose();
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
          <DialogTitle>Promote message to Work</DialogTitle>
          <DialogDescription>
            Link this Office message to a work definition in an authorized
            project. You can assign and start it from Projects &amp; Work.
          </DialogDescription>
        </DialogHeader>
        <MessagePromotionPanel
          key={`${origin}:${channel}:${message}:${state.cursor ?? ""}:${state.formGeneration}`}
          state={state}
          message={message}
          openWork={(project, item) => (
            <Button asChild>
              <Link
                to="/agents"
                search={{ workProject: project, workItem: item }}
                onClick={onClose}
              >
                Open saved Work
              </Link>
            </Button>
          )}
        />
      </DialogContent>
    </Dialog>
  );
}
