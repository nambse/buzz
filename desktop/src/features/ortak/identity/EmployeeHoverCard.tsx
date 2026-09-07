import type { MouseEventHandler } from "react";
import { Button } from "@/shared/ui/button";
import { PopoverContent } from "@/shared/ui/popover";
import { EmployeeIdentityBadge } from "./EmployeeIdentityBadge";
import type { OfficeEmployee } from "./directory";

/** Employee profiles open the control-plane surface without legacy runtime probes. */
export function EmployeeHoverCard({
  employee: value,
  pubkey,
  onMouseEnter,
  onMouseLeave,
  onOpen,
}: {
  employee: OfficeEmployee;
  pubkey: string;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
  onOpen?: MouseEventHandler<HTMLButtonElement>;
}) {
  return (
    <PopoverContent
      className="w-72"
      side="top"
      sideOffset={8}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      onOpenAutoFocus={(event) => event.preventDefault()}
      data-testid="ortak-employee-profile"
    >
      <div className="flex flex-col gap-3">
        <div>
          <p className="font-semibold">
            {value.employee.name ?? value.employee.employee_id}
          </p>
          <p className="text-sm text-muted-foreground">
            {value.employee.title}
          </p>
        </div>
        <div className="flex gap-2">
          <EmployeeIdentityBadge pubkey={pubkey} />
          <EmployeeIdentityBadge pubkey={pubkey} showState />
        </div>
        <p className="text-xs text-muted-foreground">
          Employee status and recorded work. Provider availability is checked
          when work starts.
        </p>
        {onOpen ? (
          <Button variant="outline" size="sm" onClick={onOpen}>
            View employee and activity
          </Button>
        ) : null}
      </div>
    </PopoverContent>
  );
}
