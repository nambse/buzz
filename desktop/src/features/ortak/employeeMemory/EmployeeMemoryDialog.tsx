import { useCallback, useMemo, useRef, useState, type ReactNode } from "react";
import { useChannelsQuery } from "@/features/channels/hooks";
import { useIdentityQuery } from "@/shared/api/hooks";
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
import { createOrtakClient } from "../client";
import { useOrtakOrigin } from "../useOrtakOrigin";
import { useConversationRead } from "../conversationMemory/useConversationRead";
import { Field, Select } from "../work/fields";
import { EmployeeMemoryPanel } from "./EmployeeMemoryPanel";
import { useEmployeeReview } from "./useEmployeeReview";
import { uuid } from "./validation";
import type { MemoryKind } from "./types";
import type { EmployeeMemoryClient } from "./useEmployeeMutation";

/** Exported presentation seam uses the same signed client and hooks as the native dialog. */
export function EmployeeMessageReview({
  client,
  actor,
  channel,
  message,
  open,
  channels,
  render,
}: {
  client: EmployeeMemoryClient;
  actor: string;
  channel: string;
  message: string;
  open: boolean;
  channels: Array<{ id: string; name: string }>;
  render?: (body: ReactNode) => ReactNode;
}) {
  const [employeeId, setEmployee] = useState("");
  const [destination, setDestination] = useState(channel);
  const [kind, setKind] = useState<MemoryKind>("experience");
  const [cursor, setCursor] = useState<string>();
  const invalidate = useRef<() => void>(() => {});
  const loadEmployees = useCallback(
    (signal: AbortSignal) => client.employees(signal, cursor),
    [client, cursor],
  );
  const employees = useConversationRead(loadEmployees, open, 0, () =>
    invalidate.current(),
  );
  const employee = employees.value?.employees.find(
    (row) => row.employee_id === employeeId,
  );
  const destinationChannel = channels.find((row) => row.id === destination);
  const state = useEmployeeReview(
    client,
    actor,
    employeeId,
    { event: message, channel },
    destinationChannel?.id ?? "",
    kind,
    open,
  );
  invalidate.current = state.invalidate;
  const blocked = state.blocked;
  const body = (
    <div className="flex flex-col gap-4">
      {employees.error ? (
        <Alert variant="destructive">
          <AlertDescription>{employees.error}</AlertDescription>
        </Alert>
      ) : null}
      <Button
        type="button"
        variant="outline"
        disabled={blocked}
        onClick={employees.refresh}
      >
        Refresh Employees
      </Button>
      <Field label="Employee">
        {(id) => (
          <Select
            id={id}
            value={employeeId}
            disabled={blocked || !employees.ready}
            onChange={(event) => {
              if (!blocked) {
                state.setAfter(undefined);
                setEmployee(event.target.value);
              }
            }}
          >
            <option value="">Choose an Employee</option>
            {employees.value?.employees.map((row) => (
              <option key={row.employee_id} value={row.employee_id}>
                {row.name ?? row.employee_id} · {row.status}
              </option>
            ))}
          </Select>
        )}
      </Field>
      <div className="flex gap-2">
        <Button
          type="button"
          variant="outline"
          disabled={blocked || !cursor}
          onClick={() => {
            setEmployee("");
            state.setAfter(undefined);
            setCursor(undefined);
          }}
        >
          First Employees page
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={blocked || !employees.value?.next_after}
          onClick={() => {
            if (employees.value?.next_after) {
              setEmployee("");
              state.setAfter(undefined);
              setCursor(employees.value.next_after);
            }
          }}
        >
          Next Employees page
        </Button>
      </div>
      <Field label="Destination channel">
        {(id) => (
          <Select
            id={id}
            value={destinationChannel ? destination : ""}
            disabled={blocked}
            onChange={(event) => {
              if (!blocked) setDestination(event.target.value);
            }}
          >
            <option value="">Choose a destination channel</option>
            {channels.map((row) => (
              <option key={row.id} value={row.id}>
                {row.name}
              </option>
            ))}
          </Select>
        )}
      </Field>
      <Field label="Employee memory kind">
        {(id) => (
          <Select
            id={id}
            value={kind}
            disabled={blocked}
            onChange={(event) => {
              if (
                !blocked &&
                (event.target.value === "experience" ||
                  event.target.value === "relationship")
              )
                setKind(event.target.value);
            }}
          >
            <option value="experience">Experience</option>
            <option value="relationship">Relationship with me</option>
          </Select>
        )}
      </Field>
      <p className="text-sm text-muted-foreground">
        The source is your selected Office message. Text starts empty; choose
        what you explicitly want to share. A relationship approval is about you,
        the signed author.
      </p>
      {employeeId ? (
        <EmployeeMemoryPanel
          state={state}
          employeeName={employee?.name ?? employeeId}
          destinationName={
            destinationChannel?.name ?? "Unavailable destination"
          }
          canPreview={Boolean(employee && destinationChannel)}
          channels={channels}
        />
      ) : null}
    </div>
  );
  return render ? render(body) : body;
}
function NativeReview({
  origin,
  actor,
  channel,
  message,
  open,
  onClose,
  restoreFocus,
}: {
  origin: string;
  actor: string;
  channel: string;
  message: string;
  open: boolean;
  onClose: () => void;
  restoreFocus: () => void;
}) {
  const client = useMemo(
    () => createOrtakClient(origin, signRelayEvent),
    [origin],
  );
  const query = useChannelsQuery({ enabled: open });
  // Directory entries are selection hints only. Preview checks the exact source,
  // destination, employee membership and configured deployment ceilings again.
  const channels = query.isError
    ? []
    : (query.data ?? []).filter(
        (row) =>
          uuid(row.id) &&
          row.isMember &&
          !row.archivedAt &&
          (!row.ttlDeadline || Date.parse(row.ttlDeadline) > Date.now()) &&
          (row.channelType === "stream" ||
            (row.channelType === "dm" &&
              row.visibility === "private" &&
              row.participantPubkeys.length === 2)),
      );
  return (
    <EmployeeMessageReview
      client={client}
      actor={actor}
      channel={channel}
      message={message}
      open={open}
      channels={channels}
      render={(body) => (
        <Dialog
          open={open}
          onOpenChange={(value) => {
            if (!value) onClose();
          }}
        >
          <DialogContent
            className="max-h-[85vh] overflow-y-auto sm:max-w-2xl"
            onCloseAutoFocus={(event) => {
              event.preventDefault();
              restoreFocus();
            }}
          >
            <DialogHeader>
              <DialogTitle>Review employee memory</DialogTitle>
              <DialogDescription>
                Explicitly review experience or your relationship with an
                Employee for one destination channel.
              </DialogDescription>
            </DialogHeader>
            {body}
          </DialogContent>
        </Dialog>
      )}
    />
  );
}
/** Current native identity/origin changes unmount all private drafts and pending callbacks. */
export function EmployeeMemoryDialog(props: {
  origin: string;
  actor: string;
  channel: string;
  message: string;
  open: boolean;
  onClose: () => void;
  restoreFocus: () => void;
}) {
  const identity = useIdentityQuery();
  const origin = useOrtakOrigin();
  if (
    identity.isError ||
    identity.data?.pubkey !== props.actor ||
    origin !== props.origin
  )
    return null;
  return (
    <NativeReview
      key={`${props.origin}:${props.actor}:${props.channel}:${props.message}`}
      {...props}
    />
  );
}
