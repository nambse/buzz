import { useRef, useState } from "react";
import { useIdentityQuery } from "@/shared/api/hooks";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import type { Employee } from "../types";
import type { EmployeeMemoryClient } from "./useEmployeeMutation";
import { EmployeeMemoryPanel } from "./EmployeeMemoryPanel";
import { useEmployeeReview } from "./useEmployeeReview";

function Recovery({
  client,
  actor,
  employee,
  open,
  onClose,
  restoreFocus,
}: {
  client: EmployeeMemoryClient;
  actor: string;
  employee: Employee;
  open: boolean;
  onClose: () => void;
  restoreFocus: () => void;
}) {
  // This hook remains mounted when Radix removes its closed DialogContent.
  const state = useEmployeeReview(
    client,
    actor,
    employee.employee_id,
    null,
    "",
    "experience",
    open,
  );
  return (
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
          <DialogTitle>
            Saved memory for {employee.name ?? employee.employee_id}
          </DialogTitle>
          <DialogDescription>
            Review your saved approval metadata or stop an approval, including
            when its source is hidden. To create an approval, choose your own
            Office message.
          </DialogDescription>
        </DialogHeader>
        <EmployeeMemoryPanel
          state={state}
          employeeName={employee.name ?? employee.employee_id}
          destinationName=""
          canPreview={false}
        />
      </DialogContent>
    </Dialog>
  );
}
function CurrentRecovery(props: Omit<Parameters<typeof Recovery>[0], "actor">) {
  const identity = useIdentityQuery();
  const actor = identity.isError ? null : identity.data?.pubkey;
  return actor && /^[0-9a-f]{64}$/.test(actor) ? (
    <Recovery
      key={`${actor}:${props.employee.employee_id}`}
      {...props}
      actor={actor}
    />
  ) : null;
}
/** Recovery has no source, destination-selection, active-state or capability visibility gate. */
export function EmployeeMemoryRecovery({
  client,
  employee,
}: {
  client: EmployeeMemoryClient;
  employee: Employee;
}) {
  const [open, setOpen] = useState(false);
  const [opened, setOpened] = useState(false);
  const trigger = useRef<HTMLButtonElement | null>(null);
  return (
    <>
      <Button
        ref={trigger}
        type="button"
        size="sm"
        variant="outline"
        onClick={() => {
          setOpened(true);
          setOpen(true);
        }}
        aria-label={`Review saved memory for ${employee.name ?? employee.employee_id}`}
      >
        Saved employee memory
      </Button>
      {opened ? (
        <CurrentRecovery
          client={client}
          employee={employee}
          open={open}
          onClose={() => setOpen(false)}
          restoreFocus={() => trigger.current?.focus()}
        />
      ) : null}
    </>
  );
}
